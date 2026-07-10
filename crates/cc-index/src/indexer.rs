//! Incremental indexer pipeline.
//!
//! Phase 1: Scan → Vec<ScannedFile>
//! Phase 2: Diff (mtime+size fast-skip → hash confirm) → Vec<PendingFile>
//! Phase 3: Parallel parse (rayon) → Vec<IndexedFile>
//! Phase 4: Symbol resolution (cross-file)
//! Phase 5: Batch write to SQLite
//! Phase 6: Post-processing (test edges, communities, frameworks)
//! Phase 7: Git co-change analysis

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::dirty_reload_policy::parse_outcome_from_reloaded_edges;
use crate::memory_budget::MemoryBudget;
use cc_db::index_db::{FileState, FileWriteUnit, IndexDb};
use cc_model::{CcError, CcResult, Language};

use crate::framework_registry;
use cc_parsers::import_resolver::resolve_import;
use cc_parsers::ParserRegistry;

use crate::scanner::{ScannedFile, Scanner};

/// Minimum number of files to justify rayon parallel overhead.
/// Below this threshold, sequential iteration is used.
pub(crate) const MIN_FILES_FOR_PARALLEL: usize = 50;
/// Best-effort safeguard: emit a warning when parsing a single file takes too long.
const SLOW_PARSE_WARN_MS: u128 = 1_500;

/// Read a file for the incremental scan, returning `None` (with a traced
/// warning) on failure instead of silently dropping it. A file that vanished
/// between the directory scan and the read (concurrent delete / rename race)
/// is skipped at debug level; any other IO error (permission, encoding,
/// transient) is warned so the file isn't silently omitted from the index.
/// Returning `None` lets the caller's `filter_map` skip that one file
/// without aborting the whole parallel scan.
fn read_for_scan(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(content) => Some(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "file vanished during scan, skipping");
            None
        }
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "failed to read file during incremental scan, skipping"
            );
            None
        }
    }
}

/// What to do with a scanned file.
#[derive(Debug, Clone, Copy)]
pub enum FileAction {
    Add,
    Update,
    Skip,
    /// 文件本身未变，但依赖的导出变了，需要重新解析引用
    DirtyResolveOnly,
}

/// Phase 2 output: file with diff decision.
#[derive(Debug, Clone)]
pub struct PendingFile {
    pub scanned: ScannedFile,
    pub content_hash: String,
    pub action: FileAction,
}

/// Index report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexReport {
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_removed: usize,
    pub files_skipped: usize,
    pub symbols_total: usize,
    pub chunks_total: usize,
    pub parse_errors: Vec<String>,
    pub elapsed_ms: u64,
    /// Number of files actually parsed (Add + Update, excluding Skip).
    pub files_parsed: usize,
    /// Whether the parallel parse path was taken.
    pub used_parallel_parse: bool,
    /// How incremental dirty propagation ended — `normal`, `partial_closure`,
    /// `budget_exceeded`, or `disabled`. `None` for full builds, where
    /// propagation does not apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirty_propagation: Option<crate::dirty_closure::DirtyPropagationStatus>,
    /// Per-phase timing breakdown (all in milliseconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_timing: Option<PhaseTiming>,
    /// Build-side decision envelope: signature-gate decisions and degrade
    /// notes for the postprocess/analysis passes — the build-side counterpart
    /// of `GraphExplain`. `None` when nothing noteworthy happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_explain: Option<cc_model::BuildExplain>,
}

/// Per-phase timing breakdown in milliseconds.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PhaseTiming {
    pub scan_diff_ms: u64,
    pub parse_ms: u64,
    pub resolve_ms: u64,
    pub write_ms: u64,
    pub postprocess_ms: u64,
    pub analysis_ms: u64,
}

pub struct Indexer {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) parsers: ParserRegistry,
    pub(crate) scanner: Scanner,
    pub(crate) parse_timeout_micros: Option<u64>,
    pub(crate) dirty_propagation: bool,
    pub(crate) dirty_propagation_max_files: usize,
    pub(crate) memory_budget: MemoryBudget,
    pub(crate) max_concurrent_parse: Option<usize>,
    pub(crate) use_direct_writer: bool,
    pub(crate) dispatch_synthesis: bool,
    pub(crate) event_fanout_cap: usize,
    pub(crate) event_denylist: Vec<String>,
}

/// Which files the scan/diff phase examines.
#[derive(Debug, Clone, Default)]
pub enum BuildScope {
    /// Walk the whole project tree (manual builds, full rebuilds,
    /// auto-index).
    #[default]
    FullTree,
    /// Diff only the event-reported paths (watcher increments): `changed`
    /// paths re-enter the scanner through the exact same ignore/type/size
    /// filters (pruned walk); `removed` paths are dropped from the index if
    /// present. Ignored for full builds.
    Targeted(TargetedChanges),
}

/// The watcher-reported change set driving a [`BuildScope::Targeted`] build.
#[derive(Debug, Clone, Default)]
pub struct TargetedChanges {
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

/// Intermediate result for Phase 1+2 (scan and diff).
pub(crate) struct ScanDiffResult {
    pub(crate) files_scanned: usize,
    pub(crate) files_added: usize,
    pub(crate) files_updated: usize,
    pub(crate) files_skipped: usize,
    /// Shared snapshot of the persisted file state (see cc-db's
    /// `file_state_cache`): a cache hit hands out the same allocation, so
    /// holding it across the build costs one `Arc`.
    pub(crate) existing: std::sync::Arc<HashMap<String, FileState>>,
    pub(crate) scanned_paths: HashSet<String>,
    pub(crate) to_remove: Vec<String>,
    pub(crate) to_parse: Vec<PendingFile>,
}

/// Intermediate result for Phase 3 (parse).
pub(crate) struct ParseResult {
    pub(crate) write_units: Vec<FileWriteUnit>,
    pub(crate) parse_errors: Vec<String>,
    pub(crate) files_to_parse: usize,
    pub(crate) used_parallel: bool,
}

/// Intermediate result for Phase 4 (resolve).
pub(crate) struct ResolveResult {
    pub(crate) hierarchy_edges: Vec<cc_model::edge::SemanticEdgeRecord>,
    /// The resolution catalog plus its seed-token basis, carried to the
    /// commit so the post-write fold can park it for the next build
    /// (see `resolver::catalog_cache`). `None` when this build is not
    /// cache-eligible (full build, empty batch, no aggregate baseline).
    pub(crate) catalog_carry: Option<crate::resolver::catalog_cache::CatalogCarry>,
}

/// Intermediate result for Phase 6 (write).
pub(crate) struct WriteResult {
    pub(crate) write_units: Vec<FileWriteUnit>,
    pub(crate) config_units: Vec<FileWriteUnit>,
    /// The incremental batch's in-transaction `symbols_seed` token span;
    /// `None` on the full-rebuild path.
    pub(crate) seed_tokens: Option<cc_db::index_db::SeedTokenSpan>,
}

impl Indexer {
    pub fn new(
        db: Arc<IndexDb>,
        project_path: &Path,
        config: &cc_model::config::IndexingConfig,
    ) -> Self {
        Self {
            db,
            parsers: ParserRegistry::new(),
            scanner: Scanner::new(project_path, config),
            parse_timeout_micros: config.parse_timeout_micros,
            dirty_propagation: config.dirty_propagation,
            dirty_propagation_max_files: config.dirty_propagation_max_files,
            memory_budget: MemoryBudget::new(config.memory_budget_fraction),
            max_concurrent_parse: config.max_concurrent_parse,
            use_direct_writer: config.use_direct_writer,
            dispatch_synthesis: config.dispatch_synthesis,
            event_fanout_cap: config.event_fanout_cap,
            event_denylist: config.event_denylist.clone(),
        }
    }

    pub fn build_index(&self, project_path: &Path, full: bool) -> CcResult<IndexReport> {
        self.build_index_internal(project_path, full, None)
    }

    fn build_index_internal(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
    ) -> CcResult<IndexReport> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit).execute(self, project_path)
    }

    /// Read-only half of a build (scan/diff → parse → resolve → snapshot).
    ///
    /// Performs no index writes, so callers may run it without holding an index
    /// write lock and then hand the result to [`Indexer::commit_build`] under a
    /// brief write lock. `full`/`auto_file_limit` must match the values passed to
    /// the paired `commit_build` call so both halves share the same build mode.
    /// `scope` selects the scan walk (see [`BuildScope`]); commit stages are
    /// scope-independent.
    pub fn prepare_build(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        scope: BuildScope,
    ) -> CcResult<crate::build_plan::PreparedBuild> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit)
            .with_scope(scope)
            .prepare(self, project_path)
    }

    /// Write half of a build, consuming the [`PreparedBuild`] produced by
    /// [`Indexer::prepare_build`]. Composes the three commit stages inline
    /// (single write-lock behavior). Must be called with the same `full`/
    /// `auto_file_limit` used for the matching `prepare_build`.
    pub fn commit_build(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        prepared: crate::build_plan::PreparedBuild,
    ) -> CcResult<IndexReport> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit).commit(
            self,
            project_path,
            prepared,
        )
    }

    /// Stage 1 of a staged commit: generation guard + `phase_write`, under
    /// the caller's write lock. The returned [`WrittenBuild`] is the
    /// transport into the lock-free [`Indexer::compute_build_postprocess`].
    /// `full`/`auto_file_limit` must match across the staged calls (and the
    /// originating `prepare_build`). See `build_plan` for the staging
    /// contract, including the build-gate requirement.
    pub fn commit_build_write(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        prepared: crate::build_plan::PreparedBuild,
    ) -> CcResult<crate::build_plan::WrittenBuild> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit).commit_write(
            self,
            project_path,
            prepared,
        )
    }

    /// Stage 2 of a staged commit: postprocess/analysis compute. Reads the
    /// committed state through the read pool only — safe to run with no index
    /// lock held while the caller keeps holding the build gate.
    pub fn compute_build_postprocess(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        written: crate::build_plan::WrittenBuild,
    ) -> CcResult<crate::build_plan::StagedPostprocess> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit).compute_postprocess(
            self,
            project_path,
            written,
        )
    }

    /// Stage 3 of a staged commit: apply the staged deltas (short DB
    /// transactions) under the caller's write lock and produce the report.
    pub fn apply_build_postprocess(
        &self,
        full: bool,
        auto_file_limit: Option<usize>,
        staged: crate::build_plan::StagedPostprocess,
    ) -> CcResult<IndexReport> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit)
            .apply_postprocess(self, staged)
    }

    // ── Phase helper structs ────────────────────────────────────────────

    /// Phase 1+2: Scan files and compute diff against existing DB state.
    ///
    /// `scope` selects the walk: `FullTree` scans the whole project;
    /// `Targeted` (incremental only) prunes the walk to the event-reported
    /// paths, replacing the O(repo) tree walk with an O(events) probe while
    /// keeping every diff decision (mtime+size fast path, hash confirm,
    /// `CODECORTEX_STRICT_HASH`) identical.
    pub(crate) fn phase_scan_and_diff(
        &self,
        _project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        scope: &BuildScope,
    ) -> CcResult<ScanDiffResult> {
        let targeted = match scope {
            BuildScope::Targeted(changes) if !full => Some(changes),
            _ => None,
        };

        // Phase 1: Scan
        let scanned = match targeted {
            None => self.scanner.scan(),
            Some(changes) => self.scanner.scan_paths(&changes.changed),
        };
        let files_scanned = scanned.len();
        if let Some(limit) = auto_file_limit {
            if files_scanned > limit {
                return Err(CcError::Config(format!(
                    "auto-index skipped: indexable file count {} exceeds auto_index.file_limit {}",
                    files_scanned, limit
                )));
            }
        }

        // Phase 2: Diff
        let existing = if full {
            std::sync::Arc::new(HashMap::new())
        } else {
            self.db.reads().get_file_state()?
        };
        let yielded_paths: std::collections::HashSet<String> =
            scanned.iter().map(|f| f.rel_path.clone()).collect();
        let strict_hash = std::env::var("CODECORTEX_STRICT_HASH")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        let pending: Vec<PendingFile> = scanned
            .into_par_iter()
            .filter_map(|file| {
                let (hash, action) = match existing.get(&file.rel_path) {
                    Some(old)
                        if !strict_hash
                            && (file.mtime - old.mtime).abs() < 0.001
                            && file.size == old.size =>
                    {
                        // Fast path: unchanged mtime + size means we can avoid reading and
                        // hashing the file during incremental scans. The size guard catches
                        // common same-mtime edits on coarse-grained filesystems.
                        (old.content_hash.clone(), FileAction::Skip)
                    }
                    Some(old) => {
                        let content = match read_for_scan(&file.abs_path) {
                            Some(c) => c,
                            None => return None,
                        };
                        let hash = format!("{:x}", Sha256::digest(&content));
                        if hash == old.content_hash {
                            (hash, FileAction::Skip)
                        } else {
                            (hash, FileAction::Update)
                        }
                    }
                    None => {
                        let content = match read_for_scan(&file.abs_path) {
                            Some(c) => c,
                            None => return None,
                        };
                        (format!("{:x}", Sha256::digest(&content)), FileAction::Add)
                    }
                };
                Some(PendingFile {
                    scanned: file,
                    content_hash: hash,
                    action,
                })
            })
            .collect();

        let files_added = pending
            .iter()
            .filter(|p| matches!(p.action, FileAction::Add))
            .count();
        let files_updated = pending
            .iter()
            .filter(|p| matches!(p.action, FileAction::Update))
            .count();
        let files_skipped = pending
            .iter()
            .filter(|p| matches!(p.action, FileAction::Skip))
            .count();

        // Removals + the "present after this build" set. A full-tree walk
        // derives removals from absence; a targeted diff derives them from
        // the event list — a removed event whose file is indexed, or a
        // changed file that vanished / stopped being indexable (oversized,
        // ignored) before the scan reached it.
        let (scanned_paths, to_remove) = match targeted {
            None => {
                let to_remove: Vec<String> = existing
                    .keys()
                    .filter(|p| !yielded_paths.contains(p.as_str()))
                    .cloned()
                    .collect();
                (yielded_paths, to_remove)
            }
            Some(changes) => {
                let mut removed: HashSet<String> = changes
                    .removed
                    .iter()
                    .filter(|p| existing.contains_key(p.as_str()))
                    .cloned()
                    .collect();
                for path in &changes.changed {
                    if existing.contains_key(path.as_str()) && !yielded_paths.contains(path) {
                        removed.insert(path.clone());
                    }
                }
                // Dirty propagation must still see every present file (Skip
                // actions are its promotion candidates), so reconstruct the
                // present set from the persisted state instead of a walk.
                let mut present: HashSet<String> = existing
                    .keys()
                    .filter(|p| !removed.contains(p.as_str()))
                    .cloned()
                    .collect();
                present.extend(yielded_paths.iter().cloned());
                (present, removed.into_iter().collect())
            }
        };

        // Filter and sort non-skip files for parsing (large files first)
        let mut to_parse: Vec<PendingFile> = pending
            .into_iter()
            .filter(|pf| !matches!(pf.action, FileAction::Skip))
            .collect();
        to_parse.sort_by_key(|pf| std::cmp::Reverse(pf.scanned.size));

        Ok(ScanDiffResult {
            files_scanned,
            files_added,
            files_updated,
            files_skipped,
            existing,
            scanned_paths,
            to_remove,
            to_parse,
        })
    }

    /// Phase 3: Parallel (or sequential) parsing of pending files.
    pub(crate) fn phase_parse(
        &self,
        project_path: &Path,
        to_parse: Vec<PendingFile>,
    ) -> CcResult<ParseResult> {
        let mut parse_errors = Vec::new();

        // Pre-compute Cargo workspace alias map for Rust crate import resolution
        let workspace_aliases = crate::resolver::resolve_cargo_workspace(project_path);

        let parse_one = |pf: &PendingFile| -> Result<(FileWriteUnit, String), (String, String)> {
            let rel_path = pf.scanned.rel_path.clone();
            let abs_path = pf.scanned.abs_path.clone();
            let language = pf.scanned.language;
            let content_hash = pf.content_hash.clone();
            let mtime = pf.scanned.mtime;
            let size = pf.scanned.size;
            let parse_started = std::time::Instant::now();

            let content = std::fs::read_to_string(&abs_path)
                .map_err(|e| (rel_path.clone(), e.to_string()))?;
            let mut outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.parsers.parse_with_timeout(
                    &rel_path,
                    &content,
                    language,
                    self.parse_timeout_micros,
                )
            }))
            .unwrap_or_else(|panic_info| {
                let msg = panic_info
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
                    .or_else(|| panic_info.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic");
                Err(CcError::Other(format!(
                    "parser panic for {}: {}",
                    rel_path, msg
                )))
            })
            .map_err(|e| (rel_path.clone(), e.to_string()))?;

            for import in &mut outcome.imports {
                import.resolved_path =
                    resolve_import(project_path, &rel_path, &import.import_string);

                // Fallback: resolve Rust workspace crate imports
                if import.resolved_path.is_none()
                    && language == Language::Rust
                    && !workspace_aliases.is_empty()
                {
                    import.resolved_path = crate::resolver::resolve_rust_workspace_import(
                        &import.import_string,
                        &workspace_aliases,
                    );
                }
            }

            let elapsed_ms = parse_started.elapsed().as_millis();
            if elapsed_ms >= SLOW_PARSE_WARN_MS {
                tracing::warn!(
                    file = %rel_path,
                    elapsed_ms,
                    size_bytes = size,
                    "slow parse detected"
                );
            }

            Ok((
                FileWriteUnit {
                    rel_path,
                    language,
                    content_hash,
                    mtime,
                    size,
                    outcome,
                },
                content,
            ))
        };

        const DEFAULT_BATCH_SIZE: usize = 200;

        let files_to_parse = to_parse.len();
        let used_parallel = files_to_parse >= MIN_FILES_FOR_PARALLEL;

        let mut write_units: Vec<FileWriteUnit> = Vec::with_capacity(files_to_parse);

        if used_parallel {
            // Build a controlled thread pool capped by max_concurrent_parse config.
            let num_threads = self
                .max_concurrent_parse
                .unwrap_or_else(rayon::current_num_threads);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .map_err(|e| CcError::Other(format!("rayon pool: {}", e)))?;

            let mut offset = 0;
            while offset < to_parse.len() {
                // Refresh RSS and compute pressure-aware batch size.
                self.memory_budget.refresh();
                let batch_size = self
                    .memory_budget
                    .suggested_batch_size(to_parse.len() - offset, DEFAULT_BATCH_SIZE);
                let end = (offset + batch_size).min(to_parse.len());
                let batch = &to_parse[offset..end];

                // Parallel parse for this batch.
                let batch_results: Vec<Result<(FileWriteUnit, String), (String, String)>> =
                    pool.install(|| batch.par_iter().map(&parse_one).collect());

                for result in batch_results {
                    match result {
                        Ok((unit, _source)) => {
                            write_units.push(unit);
                        }
                        Err((file, error)) => {
                            tracing::warn!(file = %file, error = %error, "parse error");
                            parse_errors.push(format!("{}: {}", file, error));
                        }
                    }
                }

                tracing::debug!(
                    batch = offset / DEFAULT_BATCH_SIZE + 1,
                    files = end - offset,
                    total = to_parse.len(),
                    rss_mb = self.memory_budget.current_rss() / 1_048_576,
                    "parse batch complete"
                );

                offset = end;
            }
        } else {
            // Small file count: sequential processing.
            for pf in &to_parse {
                match parse_one(pf) {
                    Ok((unit, _source)) => {
                        write_units.push(unit);
                    }
                    Err((file, error)) => {
                        tracing::warn!(file = %file, error = %error, "parse error");
                        parse_errors.push(format!("{}: {}", file, error));
                    }
                }
            }
        }

        Ok(ParseResult {
            write_units,
            parse_errors,
            files_to_parse,
            used_parallel,
        })
    }

    /// Build the actions map from write_units and scanned paths for dirty propagation.
    pub(crate) fn build_actions_map(
        &self,
        write_units: &[FileWriteUnit],
        existing: &HashMap<String, FileState>,
        scanned_paths: &HashSet<String>,
    ) -> HashMap<String, FileAction> {
        let mut actions: HashMap<String, FileAction> = HashMap::new();
        // All parsed files are Add or Update
        for unit in write_units {
            if existing.contains_key(&unit.rel_path) {
                actions.insert(unit.rel_path.clone(), FileAction::Update);
            } else {
                actions.insert(unit.rel_path.clone(), FileAction::Add);
            }
        }
        // All scanned but not parsed files are Skip
        for path in scanned_paths {
            if !actions.contains_key(path) {
                actions.insert(path.clone(), FileAction::Skip);
            }
        }
        actions
    }

    /// Phase 3.6: Load dirty files' edge data for re-resolution.
    pub(crate) fn phase_dirty_reload(
        &self,
        project_path: &Path,
        write_units: &mut Vec<FileWriteUnit>,
        actions: &HashMap<String, FileAction>,
        existing: &HashMap<String, FileState>,
        dirty_count: usize,
    ) -> CcResult<()> {
        let dirty_files: Vec<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::DirtyResolveOnly))
            .map(|(p, _)| p.clone())
            .collect();

        // Dirty files are not re-parsed, but the tree may have shifted under
        // them (a dependency removed, renamed, or newly shadowing) — so their
        // import `resolved_path`s are recomputed from the current filesystem,
        // mirroring `phase_parse`. Skipping this would leave IMPORTS edges (and
        // the resolution context they feed) pointing at files that moved or
        // vanished. The Cargo alias map is cheap and read once for the batch.
        let workspace_aliases = crate::resolver::resolve_cargo_workspace(project_path);

        for dirty_path in &dirty_files {
            let edges = self.db.reads().load_file_edges_for_reresolve(dirty_path)?;

            // Retrieve file metadata from existing state for mtime/hash
            let (content_hash, mtime, size) = if let Some(state) = existing.get(dirty_path) {
                (state.content_hash.clone(), state.mtime, state.size)
            } else {
                (String::new(), 0.0, 0u64)
            };

            // The conversion clears potentially-stale resolution state per
            // the central policy declared in `dirty_reload_policy`; its
            // complete destructuring keeps every reload field policed.
            let mut outcome = parse_outcome_from_reloaded_edges(edges);

            // Re-resolve import targets against the current tree. `resolve_import`
            // keys off the from-file/import-string (unchanged for a dirty file),
            // so a moved/removed dependency now yields the correct new path or
            // `None`. Language is `Unknown` on reload, so gate the Rust
            // workspace fallback on the extension instead.
            let is_rust = dirty_path.ends_with(".rs");
            for import in &mut outcome.imports {
                import.resolved_path =
                    resolve_import(project_path, dirty_path, &import.import_string);
                if import.resolved_path.is_none() && is_rust && !workspace_aliases.is_empty() {
                    import.resolved_path = crate::resolver::resolve_rust_workspace_import(
                        &import.import_string,
                        &workspace_aliases,
                    );
                }
            }

            write_units.push(FileWriteUnit {
                rel_path: dirty_path.clone(),
                language: Language::Unknown,
                content_hash,
                mtime,
                size,
                outcome,
            });
        }

        if dirty_count > 0 {
            tracing::info!(
                dirty_loaded = dirty_files.len(),
                "dirty propagation: loaded edge data for re-resolution"
            );
        }

        Ok(())
    }

    /// Phase 3.7+3.8: Framework resolver enrichment and C/C++ include resolution.
    pub(crate) fn phase_framework_enrichment(
        &self,
        project_path: &Path,
        write_units: &mut [FileWriteUnit],
    ) -> CcResult<crate::framework_resolvers::ProjectFrameworkContext> {
        // Phase 3.7: Framework resolver enrichment (before resolution)
        let fw_context = {
            let pkg_markers = framework_registry::check_package_markers(project_path);
            let mut repo_fws: HashMap<String, f64> = HashMap::new();
            for (fw_key, conf) in &pkg_markers {
                repo_fws.insert(fw_key.clone(), *conf);
            }
            for unit in write_units.iter() {
                for imp in &unit.outcome.imports {
                    score_import_markers(&imp.import_string, &mut repo_fws);
                }
            }

            // Build file-level framework map from write_unit imports
            let mut file_fw_map: HashMap<String, Vec<(String, f64)>> = HashMap::new();
            for unit in write_units.iter() {
                let mut file_fws: HashMap<String, f64> = HashMap::new();
                for imp in &unit.outcome.imports {
                    score_import_markers(&imp.import_string, &mut file_fws);
                }
                if !file_fws.is_empty() {
                    file_fw_map.insert(unit.rel_path.clone(), file_fws.into_iter().collect());
                }
            }

            let ctx = crate::framework_resolvers::ProjectFrameworkContext {
                repo_frameworks: repo_fws.iter().map(|(k, v)| (k.clone(), *v)).collect(),
                file_frameworks: file_fw_map,
            };

            let registry = crate::framework_resolvers::default_registry();
            // go_router resolver 的激活 key 集合（canonical "gin" + 其 detection
            // 别名族 echo/fiber/chi/gorilla/net_http）现在从 framework_taxonomy 派生，
            // 取代此前并行硬编码的 `go_router_keys`。激活逻辑（`&& has_go_router`，
            // 见下方 filter）保持不变 —— 仅 key 集合的来源收口到 taxonomy 单一声明。
            let go_router_keys: Vec<&'static str> =
                crate::framework_resolvers::taxon_for_key("gin")
                    .map(|taxon| crate::framework_resolvers::canonical_aliases(taxon).collect())
                    .expect("gin taxon must exist in framework_taxonomy");
            let has_go_router = ctx
                .repo_frameworks
                .iter()
                .any(|(k, _)| go_router_keys.contains(&k.as_str()));

            let active = registry.active_resolvers(&ctx);
            if !active.is_empty() || has_go_router {
                let keys: Vec<&str> = active.iter().map(|r| r.framework_key()).collect();
                tracing::info!(resolvers = ?keys, has_go_router, "pre-resolve framework enrichment active");

                let all_resolvers = registry.all_resolvers();
                for unit in write_units.iter_mut() {
                    let lang = unit.language;
                    let applicable: Vec<&dyn crate::framework_resolvers::FrameworkResolver> =
                        all_resolvers
                            .iter()
                            .filter(|r| {
                                r.languages().contains(&lang)
                                    && (ctx.has_framework(r.framework_key())
                                        || (r.framework_key() == "gin" && has_go_router))
                            })
                            .map(|r| r.as_ref())
                            .collect();

                    if applicable.is_empty() {
                        continue;
                    }

                    // Read source on demand only for framework-relevant files.
                    // The file was just read during Phase 3 parsing, so the OS
                    // page cache is still warm and this re-read is cheap.
                    let full_path = project_path.join(&unit.rel_path);
                    let source = match std::fs::read_to_string(&full_path) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    for resolver in &applicable {
                        resolver.enrich_file(
                            &unit.rel_path,
                            &source,
                            lang,
                            &mut unit.outcome,
                            &ctx,
                        );
                    }
                }
            }

            ctx
        };

        // Phase 3.8: Resolve C/C++ includes using compile_commands.json include_dirs
        {
            if let Ok(compile_targets) = self.db.reads().infra_nodes_by_kind("compile_target") {
                let include_map: HashMap<String, Vec<String>> = compile_targets
                    .iter()
                    .filter_map(|node| {
                        let dirs = node
                            .properties
                            .get("include_dirs")?
                            .as_array()?
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>();
                        if dirs.is_empty() {
                            return None;
                        }
                        Some((node.name.clone(), dirs))
                    })
                    .collect();

                if !include_map.is_empty() {
                    let mut resolved_count = 0usize;
                    for unit in write_units.iter_mut() {
                        if !matches!(
                            unit.language,
                            cc_model::Language::C | cc_model::Language::Cpp
                        ) {
                            continue;
                        }
                        let dirs = match include_map.get(&unit.rel_path) {
                            Some(d) => d,
                            None => continue,
                        };
                        for imp in &mut unit.outcome.imports {
                            if imp.resolved_path.is_some() {
                                continue;
                            }
                            if imp.is_namespace {
                                continue;
                            }
                            for dir in dirs {
                                let candidate = project_path.join(dir).join(&imp.import_string);
                                if candidate.exists() {
                                    imp.resolved_path = Some(
                                        candidate
                                            .strip_prefix(project_path)
                                            .unwrap_or(&candidate)
                                            .to_string_lossy()
                                            .to_string(),
                                    );
                                    resolved_count += 1;
                                    break;
                                }
                            }
                        }
                    }
                    if resolved_count > 0 {
                        tracing::info!(
                            resolved_count,
                            "resolved C/C++ includes via compile_commands.json"
                        );
                    }
                }
            }
        }

        Ok(fw_context)
    }
}

/// Accumulate framework confidence scores from a single import string using the
/// authoritative [`framework_registry::import_marker_table`]. Each framework
/// whose marker substring is present gains +0.4 (capped at 0.95). This is the
/// single source of truth for import-based framework detection during Phase 3.7
/// enrichment — it replaces two previously-inlined copies of the marker table
/// that had drifted out of sync with the registry.
fn score_import_markers(import_string: &str, scores: &mut HashMap<String, f64>) {
    let src = import_string.to_lowercase();
    for &(fw_key, markers) in framework_registry::import_marker_table() {
        if markers
            .iter()
            .any(|marker| src.contains(&marker.to_lowercase()))
        {
            let entry = scores.entry(fw_key.to_string()).or_insert(0.0);
            *entry = (*entry + 0.4).min(0.95);
        }
    }
}

#[cfg(test)]
mod import_marker_dedup_tests {
    use super::*;

    /// Guard: the Phase 3.7 enrichment path (`score_import_markers`) and the
    /// framework-registry detection path must consult the *same* authoritative
    /// marker table, so a framework added/edited in one place is honoured by the
    /// other. We assert this by checking that every framework key in the
    /// registry table is detectable by `score_import_markers` using that same
    /// table's own markers — proving there is no second, divergent copy.
    #[test]
    fn enrichment_uses_authoritative_marker_table() {
        let table = framework_registry::import_marker_table();
        assert!(!table.is_empty());

        for &(fw_key, markers) in table {
            // Use the first marker as a representative import string.
            let import = markers[0];
            let mut scores: HashMap<String, f64> = HashMap::new();
            score_import_markers(import, &mut scores);
            assert!(
                scores.contains_key(fw_key),
                "framework `{fw_key}` (marker `{import}`) must be detected via the \
                 shared import_marker_table()"
            );
        }
    }

    /// Sanity: a framework that exists only in the authoritative table (e.g.
    /// `nextjs`, added after the old inline copies) is now detected by the
    /// enrichment path, confirming the inline copies are truly gone.
    #[test]
    fn detects_framework_absent_from_old_inline_table() {
        let mut scores: HashMap<String, f64> = HashMap::new();
        score_import_markers("next/server", &mut scores);
        assert!(
            scores.contains_key("nextjs"),
            "nextjs was missing from the removed inline tables; it must now be \
             detected via the authoritative table"
        );
    }
}

#[cfg(test)]
mod dirty_reload_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use cc_model::edge::SemanticRelation;
    use tempfile::TempDir;

    fn setup_indexer() -> (TempDir, Indexer) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("dirty.db")).unwrap().0);
        let cfg = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), tmp.path(), &cfg);
        (tmp, indexer)
    }

    /// End-to-end behavior check: dirty reload clears potentially-stale
    /// cross-file target UIDs on resolver-resolved semantic edges while
    /// leaving hierarchy edges untouched. The rule itself is declared in
    /// `crate::dirty_reload_policy` (see its unit tests for per-category
    /// assertions).
    #[test]
    fn dirty_reload_clears_stale_semantic_target_uids() {
        let (tmp, indexer) = setup_indexer();
        let conn = crate::test_seed::seed_conn(&indexer.db);
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/a.py','Python','h',1.0,1,'2024-01-01');\
             INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line) \
                 VALUES('se-inh','src/a.py','Child','uChild','Base','uBase-STALE','inherits',1);\
             INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line) \
                 VALUES('se-def','src/a.py','a.py','uFile','Child','uChild','defines',1);",
        )
        .unwrap();
        drop(conn);

        let mut write_units: Vec<FileWriteUnit> = Vec::new();
        let mut actions: HashMap<String, FileAction> = HashMap::new();
        actions.insert("src/a.py".to_string(), FileAction::DirtyResolveOnly);
        let existing: HashMap<String, FileState> = HashMap::new();

        indexer
            .phase_dirty_reload(tmp.path(), &mut write_units, &actions, &existing, 1)
            .unwrap();

        assert_eq!(write_units.len(), 1);
        let edges = &write_units[0].outcome.semantic_edges;
        let inherits = edges
            .iter()
            .find(|e| e.relation_kind == SemanticRelation::Inherits)
            .expect("inherits edge loaded");
        assert_eq!(
            inherits.target_symbol_uid, None,
            "stale cross-file target UID must be cleared for re-resolution"
        );
        let defines = edges
            .iter()
            .find(|e| e.relation_kind == SemanticRelation::Defines)
            .expect("defines edge loaded");
        assert_eq!(
            defines.target_symbol_uid.as_deref(),
            Some("uChild"),
            "hierarchy edges are regenerated each run and must not be touched"
        );
    }
}
