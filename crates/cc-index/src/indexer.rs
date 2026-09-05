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
/// Upper bound on the total bytes of source text retained in memory to avoid
/// re-reading files across scan → parse → framework enrichment. Beyond this
/// budget the pipeline falls back to per-phase reads (the pre-optimization
/// behavior), keeping peak RSS bounded on huge full builds.
const SOURCE_RETAIN_MAX_TOTAL_BYTES: usize = 128 * 1024 * 1024;

/// Content hash used by the scan/diff confirm step (hex-encoded blake3, same
/// 64-hex-char width as the previous SHA-256 encoding). The schema version was
/// bumped alongside the algorithm switch, so pre-blake3 databases rebuild via
/// the standard rebuild-on-mismatch policy instead of comparing hashes across
/// algorithms.
pub(crate) fn content_hash_hex(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

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

/// Event-scoped build hint: the `/`-normalized rel paths a watcher tick
/// observed as changed (created/modified) or removed since the last build.
/// An incremental prepare carrying a non-empty scope stats/hashes only these
/// paths instead of walking the whole tree; every fallback to the full walk
/// (first build, oversized event set, dot-path events that can change
/// admission rules) is decided inside the scan/diff phase.
#[derive(Debug, Clone, Default)]
pub struct BuildScope {
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl BuildScope {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

/// Above this many distinct event paths a scoped scan falls back to the full
/// tree walk: bulk operations (branch checkout, generated-code refresh,
/// watcher overflow backfill) touch enough of the tree that one shared walk
/// is both cheaper and safer than hundreds of point stats.
const SCOPED_SCAN_MAX_EVENTS: usize = 512;

/// Signature-gate hints derived from an event scope. A scoped build runs no
/// tree walk, so the walk-manifest consumers (config-linker and infra
/// signature gates) would each fall back to their own walk — costing more
/// than the scoped scan saved. When the event set provably contains no
/// candidate of a consumer's input class, that consumer's signature cannot
/// have changed (the scope's trust model: only the event paths changed on
/// disk since the last build), so the gate may reuse its recorded signature
/// without walking.
///
/// A flag is only `true` when EVERY event path exists as a regular file
/// whose name cannot classify as a candidate of that input class. Missing
/// paths (removals — indistinguishable from removed directories whose
/// subtree may have held candidates) and directories clear both flags.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScopeSignatureHints {
    /// No event path can be a config-linker candidate
    /// (`config_linker::is_config_path`).
    pub(crate) config_files_unaffected: bool,
    /// No event path can be an infra candidate
    /// (`infra_pass::may_be_infra_candidate_name` — a name-only superset of
    /// the discovery classifier, so the content sniff is never needed).
    pub(crate) infra_files_unaffected: bool,
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
    /// Source text captured by the diff hash-confirm read, so Phase 3 parse
    /// does not re-read the file from disk. `None` when the fast path skipped
    /// the read, the content was not valid UTF-8, or the retention budget
    /// (see [`SOURCE_RETAIN_MAX_TOTAL_BYTES`]) was exhausted.
    pub content: Option<Arc<str>>,
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
    /// Parse-phase rayon pool, built once and reused across builds (thread
    /// spawn/teardown per build was measurable on watcher-tick cadence).
    parse_pool: std::sync::OnceLock<rayon::ThreadPool>,
}

/// Intermediate result for Phase 1+2 (scan and diff).
pub(crate) struct ScanDiffResult {
    pub(crate) files_scanned: usize,
    pub(crate) files_added: usize,
    pub(crate) files_updated: usize,
    pub(crate) files_skipped: usize,
    /// Pre-batch file-state snapshot, shared with the cross-build cache on
    /// the `IndexDb` handle (`Arc`: the cache advances copy-on-write, so
    /// this build's view is stable).
    pub(crate) existing: Arc<HashMap<String, FileState>>,
    pub(crate) scanned_paths: HashSet<String>,
    pub(crate) to_remove: Vec<String>,
    pub(crate) to_parse: Vec<PendingFile>,
    /// The shared walk manifest for the config/infra signature consumers.
    /// `None` when this build did not walk the tree (event-scoped prepare),
    /// in which case those consumers fall back to their own walks.
    pub(crate) walk_manifest: Option<Arc<crate::scanner::WalkManifest>>,
    /// Event-scope signature hints (`Some` only for event-scoped scans):
    /// lets the config/infra signature gates skip their fallback walks when
    /// the event set provably contains no candidate of their input class.
    pub(crate) scope_hints: Option<ScopeSignatureHints>,
}

/// Intermediate result for Phase 3 (parse).
pub(crate) struct ParseResult {
    pub(crate) write_units: Vec<FileWriteUnit>,
    pub(crate) parse_errors: Vec<String>,
    pub(crate) files_to_parse: usize,
    pub(crate) used_parallel: bool,
    /// Source text of successfully parsed files, keyed by rel path, so
    /// framework enrichment can avoid re-reading from disk. Bounded by
    /// [`SOURCE_RETAIN_MAX_TOTAL_BYTES`]; enrichment falls back to a
    /// filesystem read for paths not present.
    pub(crate) sources: HashMap<String, Arc<str>>,
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
            parse_pool: std::sync::OnceLock::new(),
        }
    }

    /// The shared parse-phase rayon pool, built on first use with the
    /// configured `max_concurrent_parse` cap and reused across builds.
    fn parse_pool(&self) -> CcResult<&rayon::ThreadPool> {
        if let Some(pool) = self.parse_pool.get() {
            return Ok(pool);
        }
        let num_threads = self
            .max_concurrent_parse
            .unwrap_or_else(rayon::current_num_threads);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .map_err(|e| CcError::Other(format!("rayon pool: {e}")))?;
        // A concurrent initializer winning the race is fine: use theirs.
        let _ = self.parse_pool.set(pool);
        Ok(self.parse_pool.get().expect("parse pool just initialized"))
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
    pub fn prepare_build(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
    ) -> CcResult<crate::build_plan::PreparedBuild> {
        self.prepare_build_scoped(project_path, full, auto_file_limit, None)
    }

    /// [`Indexer::prepare_build`] with an event-scoped hint: when `scope`
    /// carries watcher events, the scan/diff phase stats/hashes only those
    /// paths instead of walking the whole tree (with documented safety
    /// fallbacks — see [`BuildScope`]). Commit-side semantics are unchanged;
    /// the same `full`/`auto_file_limit` pairing rules apply.
    pub fn prepare_build_scoped(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        scope: Option<&BuildScope>,
    ) -> CcResult<crate::build_plan::PreparedBuild> {
        crate::build_plan::IndexBuildPlan::new(full, auto_file_limit).prepare_scoped(
            self,
            project_path,
            scope,
        )
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
    /// An incremental build carrying a non-empty [`BuildScope`] takes the
    /// event-scoped path (stat/hash only the event paths); everything else —
    /// full builds, scope-less manual/auto builds, and scoped builds whose
    /// event set trips a safety fallback — runs the shared tree walk.
    pub(crate) fn phase_scan_and_diff(
        &self,
        _project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
        scope: Option<&BuildScope>,
    ) -> CcResult<ScanDiffResult> {
        if !full {
            if let Some(scope) = scope.filter(|s| !s.is_empty()) {
                if let Some(result) = self.scoped_scan_and_diff(scope, auto_file_limit)? {
                    return Ok(result);
                }
                tracing::debug!("scoped scan fell back to the full tree walk");
            }
        }

        // Phase 1: Scan (single shared walk: indexable set + manifest for the
        // config/infra signature consumers).
        let (scanned, walk_manifest) = self.scanner.scan_with_manifest();
        let walk_manifest = Some(Arc::new(walk_manifest));
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
            Arc::new(HashMap::new())
        } else {
            self.db.reads().get_file_state()?
        };
        let yielded_paths: std::collections::HashSet<String> =
            scanned.iter().map(|f| f.rel_path.clone()).collect();
        let pending = self.diff_scanned_files(scanned, &existing);

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

        // Files to remove: DB paths the walk did not yield (full-tree
        // walk — the event-scoped path derives removals from the event set
        // inside `scoped_scan_and_diff`).
        let to_remove: Vec<String> = existing
            .keys()
            .filter(|p| !yielded_paths.contains(p.as_str()))
            .cloned()
            .collect();
        let scanned_paths = yielded_paths;

        Ok(ScanDiffResult {
            files_scanned,
            files_added,
            files_updated,
            files_skipped,
            existing,
            scanned_paths,
            to_remove,
            to_parse: Self::pending_to_parse(pending),
            walk_manifest,
            scope_hints: None,
        })
    }

    /// Event-scoped Phase 1+2: stat/hash only the scope's event paths and
    /// diff them against the full DB file state. Returns `Ok(None)` when the
    /// scope cannot safely describe the build, so the caller falls back to
    /// the full tree walk:
    /// - a dot-path event (`.gitignore`, `.codecortex.json`, `.env`, …) can
    ///   change admission rules or config-linker inputs for OTHER files;
    /// - an oversized event set (bulk checkout / overflow backfill) makes
    ///   one shared walk cheaper and safer than hundreds of point stats;
    /// - an empty DB file state means this is effectively a first build.
    ///
    /// Correctness notes: `existing` is still the FULL file-state snapshot,
    /// so the dirty closure can promote any importer in the repo, and
    /// `scanned_paths` covers every surviving DB file so promoted importers
    /// exist in the actions map as `Skip`. Removals are derived strictly from
    /// the event set: an event path (or an indexed file under an event
    /// directory prefix) that the scoped scan did not re-admit.
    fn scoped_scan_and_diff(
        &self,
        scope: &BuildScope,
        auto_file_limit: Option<usize>,
    ) -> CcResult<Option<ScanDiffResult>> {
        let mut event_paths: Vec<String> = Vec::new();
        let mut event_set: HashSet<String> = HashSet::new();
        for raw in scope.changed.iter().chain(scope.removed.iter()) {
            let rel = raw.replace('\\', "/");
            let rel = rel.trim_matches('/');
            if rel.is_empty() {
                continue;
            }
            if rel.split('/').any(|c| c.starts_with('.')) {
                tracing::debug!(path = %raw, "scoped scan: dot-path event may change admission rules");
                return Ok(None);
            }
            if event_set.insert(rel.to_string()) {
                event_paths.push(rel.to_string());
            }
        }
        if event_paths.is_empty() || event_paths.len() > SCOPED_SCAN_MAX_EVENTS {
            return Ok(None);
        }

        let existing = crate::indexer_phases::time_step("scan_diff", "scoped_file_state", || {
            self.db.reads().get_file_state()
        })?;
        if existing.is_empty() {
            // Effectively a first build: the event set cannot describe the
            // whole tree.
            return Ok(None);
        }

        let (admitted, pending) =
            crate::indexer_phases::time_step("scan_diff", "scoped_stat", || {
                let scanned = self.scanner.scan_paths(&event_paths);
                let admitted: HashSet<String> =
                    scanned.iter().map(|f| f.rel_path.clone()).collect();
                let pending = self.diff_scanned_files(scanned, &existing);
                (admitted, pending)
            });

        // Removals: an event path that is indexed but no longer admitted, or
        // an indexed file under an event directory prefix (a removed/renamed
        // directory arrives as one dir-level event) that was not re-admitted.
        let dir_prefixes: Vec<String> = event_paths
            .iter()
            .filter(|p| !admitted.contains(p.as_str()))
            .map(|p| format!("{p}/"))
            .collect();
        let to_remove: Vec<String> = existing
            .keys()
            .filter(|path| {
                !admitted.contains(path.as_str())
                    && (event_set.contains(path.as_str())
                        || dir_prefixes
                            .iter()
                            .any(|prefix| path.starts_with(prefix.as_str())))
            })
            .cloned()
            .collect();
        let removed_set: HashSet<&str> = to_remove.iter().map(|p| p.as_str()).collect();

        // The actions universe: every surviving DB file is a Skip candidate
        // (dirty propagation may promote any of them), plus the admitted
        // event files (covers adds).
        let mut scanned_paths: HashSet<String> =
            crate::indexer_phases::time_step("scan_diff", "scoped_universe", || {
                existing
                    .keys()
                    .filter(|p| !removed_set.contains(p.as_str()))
                    .cloned()
                    .collect()
            });
        scanned_paths.extend(admitted);

        let files_scanned = scanned_paths.len();
        if let Some(limit) = auto_file_limit {
            if files_scanned > limit {
                return Err(CcError::Config(format!(
                    "auto-index skipped: indexable file count {files_scanned} exceeds auto_index.file_limit {limit}"
                )));
            }
        }

        let files_added = pending
            .iter()
            .filter(|p| matches!(p.action, FileAction::Add))
            .count();
        let files_updated = pending
            .iter()
            .filter(|p| matches!(p.action, FileAction::Update))
            .count();
        // Keep the report invariant scanned == added + updated + skipped:
        // files outside the event set were never touched and count as
        // skipped, exactly as an unscoped incremental would classify them.
        let files_skipped = files_scanned.saturating_sub(files_added + files_updated);

        let scope_hints = self.classify_scope_events(&event_paths);

        tracing::debug!(
            events = event_paths.len(),
            added = files_added,
            updated = files_updated,
            removed = to_remove.len(),
            config_files_unaffected = scope_hints.config_files_unaffected,
            infra_files_unaffected = scope_hints.infra_files_unaffected,
            "event-scoped scan/diff (no tree walk)"
        );

        Ok(Some(ScanDiffResult {
            files_scanned,
            files_added,
            files_updated,
            files_skipped,
            existing,
            scanned_paths,
            to_remove,
            to_parse: Self::pending_to_parse(pending),
            // No tree walk ran: config/infra signature consumers either use
            // the scope hints (no candidate events → recorded signature
            // reusable) or fall back to their own walks behind their gates.
            walk_manifest: None,
            scope_hints: Some(scope_hints),
        }))
    }

    /// Derive [`ScopeSignatureHints`] from a scoped scan's event paths: a
    /// consumer's flag stays `true` only when every event path exists as a
    /// regular file whose NAME cannot classify as one of its candidates.
    /// Missing paths (removals — could have been directories holding
    /// candidates) and directories conservatively clear both flags. Cost is
    /// at most one `stat` per event path (≤ [`SCOPED_SCAN_MAX_EVENTS`]),
    /// versus the tree walk each consumer would otherwise run.
    fn classify_scope_events(&self, event_paths: &[String]) -> ScopeSignatureHints {
        let mut hints = ScopeSignatureHints {
            config_files_unaffected: true,
            infra_files_unaffected: true,
        };
        for rel in event_paths {
            let is_regular_file = std::fs::metadata(self.scanner.project_path().join(rel))
                .map(|md| md.is_file())
                .unwrap_or(false);
            if !is_regular_file {
                return ScopeSignatureHints::default();
            }
            let file_name = rel.rsplit('/').next().unwrap_or(rel);
            if crate::config_linker::is_config_path(Path::new(rel)) {
                hints.config_files_unaffected = false;
            }
            if crate::infra_pass::may_be_infra_candidate_name(file_name) {
                hints.infra_files_unaffected = false;
            }
        }
        hints
    }

    /// Shared Phase-2 diff: mtime+size fast-skip → blake3 hash confirm, with
    /// source-text retention for files that will be parsed (see
    /// [`PendingFile::content`]).
    fn diff_scanned_files(
        &self,
        scanned: Vec<ScannedFile>,
        existing: &HashMap<String, FileState>,
    ) -> Vec<PendingFile> {
        let strict_hash = std::env::var("CODECORTEX_STRICT_HASH")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        // Shared retention budget for source text carried from the hash-confirm
        // read into Phase 3 parse (see `PendingFile::content`).
        let retain_budget = std::sync::atomic::AtomicUsize::new(0);
        let retain_source = |content: Vec<u8>| -> Option<Arc<str>> {
            let len = content.len();
            let prev = retain_budget.fetch_add(len, std::sync::atomic::Ordering::Relaxed);
            if prev + len > SOURCE_RETAIN_MAX_TOTAL_BYTES {
                return None;
            }
            match String::from_utf8(content) {
                Ok(s) => Some(Arc::from(s)),
                Err(_) => None,
            }
        };

        scanned
            .into_par_iter()
            .filter_map(|file| {
                let (hash, action, content) = match existing.get(&file.rel_path) {
                    Some(old)
                        if !strict_hash
                            && (file.mtime - old.mtime).abs() < 0.001
                            && file.size == old.size =>
                    {
                        // Fast path: unchanged mtime + size means we can avoid reading and
                        // hashing the file during incremental scans. The size guard catches
                        // common same-mtime edits on coarse-grained filesystems.
                        (old.content_hash.clone(), FileAction::Skip, None)
                    }
                    Some(old) => {
                        let content = match read_for_scan(&file.abs_path) {
                            Some(c) => c,
                            None => return None,
                        };
                        let hash = content_hash_hex(&content);
                        if hash == old.content_hash {
                            (hash, FileAction::Skip, None)
                        } else {
                            (hash, FileAction::Update, retain_source(content))
                        }
                    }
                    None => {
                        let content = match read_for_scan(&file.abs_path) {
                            Some(c) => c,
                            None => return None,
                        };
                        let hash = content_hash_hex(&content);
                        (hash, FileAction::Add, retain_source(content))
                    }
                };
                Some(PendingFile {
                    scanned: file,
                    content_hash: hash,
                    action,
                    content,
                })
            })
            .collect()
    }

    /// Filter and sort non-skip files for parsing (large files first).
    fn pending_to_parse(pending: Vec<PendingFile>) -> Vec<PendingFile> {
        let mut to_parse: Vec<PendingFile> = pending
            .into_iter()
            .filter(|pf| !matches!(pf.action, FileAction::Skip))
            .collect();
        to_parse.sort_by(|a, b| b.scanned.size.cmp(&a.scanned.size));
        to_parse
    }

    /// Phase 3: Parallel (or sequential) parsing of pending files.
    pub(crate) fn phase_parse(
        &self,
        project_path: &Path,
        mut to_parse: Vec<PendingFile>,
    ) -> CcResult<ParseResult> {
        let mut parse_errors = Vec::new();

        // Pre-compute Cargo workspace alias map for Rust crate import resolution
        let workspace_aliases = crate::resolver::resolve_cargo_workspace(project_path);

        let parse_one = |pf: &PendingFile| -> Result<(FileWriteUnit, Arc<str>), (String, String)> {
            let rel_path = pf.scanned.rel_path.clone();
            let abs_path = pf.scanned.abs_path.clone();
            let language = pf.scanned.language;
            let content_hash = pf.content_hash.clone();
            let mtime = pf.scanned.mtime;
            let size = pf.scanned.size;
            let parse_started = std::time::Instant::now();

            // Reuse the source captured by the scan hash-confirm read; fall
            // back to a filesystem read when it was not retained.
            let content: Arc<str> = match pf.content.clone() {
                Some(c) => c,
                None => Arc::from(
                    std::fs::read_to_string(&abs_path)
                        .map_err(|e| (rel_path.clone(), e.to_string()))?,
                ),
            };
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

        /// Per-file parse result: the write unit plus its source text (kept
        /// for enrichment), or `(rel_path, error)` on failure.
        type ParseOneResult = Result<(FileWriteUnit, Arc<str>), (String, String)>;

        let files_to_parse = to_parse.len();
        let used_parallel = files_to_parse >= MIN_FILES_FOR_PARALLEL;

        let mut write_units: Vec<FileWriteUnit> = Vec::with_capacity(files_to_parse);
        let mut sources: HashMap<String, Arc<str>> = HashMap::new();
        // Enrichment-side retention budget: sources beyond the cap are dropped
        // and re-read from disk on demand during framework enrichment.
        let mut retained_bytes = 0usize;
        let mut retain =
            |sources: &mut HashMap<String, Arc<str>>, rel_path: &str, source: Arc<str>| {
                if retained_bytes + source.len() <= SOURCE_RETAIN_MAX_TOTAL_BYTES {
                    retained_bytes += source.len();
                    sources.insert(rel_path.to_string(), source);
                }
            };

        if used_parallel {
            // Reuse the cached thread pool capped by max_concurrent_parse config.
            let pool = self.parse_pool()?;

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
                let batch_results: Vec<ParseOneResult> =
                    pool.install(|| batch.par_iter().map(&parse_one).collect());

                for result in batch_results {
                    match result {
                        Ok((unit, source)) => {
                            retain(&mut sources, &unit.rel_path, source);
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

                // The scan-carried content has served its purpose for this
                // batch; drop it so peak memory stays bounded to one batch
                // plus the enrichment retention budget.
                for pf in &mut to_parse[offset..end] {
                    pf.content = None;
                }

                offset = end;
            }
        } else {
            // Small file count: sequential processing.
            for pf in &to_parse {
                match parse_one(pf) {
                    Ok((unit, source)) => {
                        retain(&mut sources, &unit.rel_path, source);
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
            sources,
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
    ///
    /// `sources` carries the parse-phase file contents (bounded, see
    /// [`ParseResult::sources`]); files absent from it are read from disk.
    pub(crate) fn phase_framework_enrichment(
        &self,
        project_path: &Path,
        write_units: &mut [FileWriteUnit],
        sources: &HashMap<String, Arc<str>>,
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

                    // Prefer the parse-phase source carried in memory; fall
                    // back to an on-demand read (page cache still warm from
                    // Phase 3) when it was not retained.
                    let source: Arc<str> = match sources.get(&unit.rel_path) {
                        Some(s) => Arc::clone(s),
                        None => {
                            let full_path = project_path.join(&unit.rel_path);
                            match std::fs::read_to_string(&full_path) {
                                Ok(s) => Arc::from(s),
                                Err(_) => continue,
                            }
                        }
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
