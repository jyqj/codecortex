//! Incremental indexer pipeline.
//!
//! Phase 1: Scan → Vec<ScannedFile>
//! Phase 2: Diff (mtime+size fast-skip → hash confirm) → Vec<PendingFile>
//! Phase 3: Parallel parse (rayon) → Vec<IndexedFile>
//! Phase 4: Symbol resolution (cross-file)
//! Phase 5: Embedding
//! Phase 6: Batch write to SQLite
//! Phase 7: Post-processing (test edges, communities, frameworks)
//! Phase 8: Git co-change analysis

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use crate::memory_budget::MemoryBudget;
use cc_db::index_db::{FileState, FileWriteUnit, IndexDb};
use cc_model::edge::ResolutionKind;
use cc_model::parse::ParseOutcome;
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

/// Intermediate result for Phase 1+2 (scan and diff).
struct ScanDiffResult {
    files_scanned: usize,
    files_added: usize,
    files_updated: usize,
    files_skipped: usize,
    existing: HashMap<String, FileState>,
    scanned_paths: HashSet<String>,
    to_remove: Vec<String>,
    to_parse: Vec<PendingFile>,
}

/// Intermediate result for Phase 3 (parse).
struct ParseResult {
    write_units: Vec<FileWriteUnit>,
    parse_errors: Vec<String>,
    files_to_parse: usize,
    used_parallel: bool,
}

/// Intermediate result for Phase 4 (resolve).
pub(crate) struct ResolveResult {
    pub(crate) hierarchy_edges: Vec<cc_model::edge::SemanticEdgeRecord>,
}

/// Intermediate result for Phase 6 (write).
pub(crate) struct WriteResult {
    pub(crate) write_units: Vec<FileWriteUnit>,
    pub(crate) config_units: Vec<FileWriteUnit>,
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

    pub fn build_auto_index(
        &self,
        project_path: &Path,
        full: bool,
        file_limit: usize,
    ) -> CcResult<IndexReport> {
        self.build_index_internal(project_path, full, Some(file_limit))
    }

    fn build_index_internal(
        &self,
        project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
    ) -> CcResult<IndexReport> {
        let start = std::time::Instant::now();

        // Phase 1+2: Scan and diff
        let scan_result = self.phase_scan_and_diff(project_path, full, auto_file_limit)?;

        // Phase 3: Parallel parse
        let parse_result = self.phase_parse(project_path, scan_result.to_parse)?;

        let mut write_units = parse_result.write_units;

        // Phase 3.5+3.6: Dirty propagation and reload
        let mut actions = self.build_actions_map(
            &write_units,
            &scan_result.existing,
            &scan_result.scanned_paths,
        );
        let dirty_count = if full {
            0
        } else {
            self.run_dirty_propagation(&mut actions, &write_units)?
        };
        self.phase_dirty_reload(
            &mut write_units,
            &actions,
            &scan_result.existing,
            dirty_count,
        )?;

        // Phase 3.7+3.8: Framework enrichment and C/C++ include resolution
        let fw_context = self.phase_framework_enrichment(project_path, &mut write_units)?;

        // Phase 4: Symbol resolution (all sub-phases)
        let resolve_result = self.phase_resolve(
            project_path,
            full,
            &mut write_units,
            &scan_result.to_remove,
            &fw_context,
        )?;

        // Count statistics
        let mut symbols_total = 0;
        let mut chunks_total = 0;
        for unit in &write_units {
            symbols_total += unit.outcome.symbols.len();
            chunks_total += unit.outcome.chunks.len();
        }
        let route_nodes = self.collect_route_nodes(&write_units);

        // Phase 6: Batch write (dual path)
        let write_result = self.phase_write(
            project_path,
            full,
            write_units,
            &actions,
            &scan_result.to_remove,
            &route_nodes,
            &resolve_result.hierarchy_edges,
        )?;

        // Phase 7: Post-processing (test edges, dispatch synthesis, communities)
        self.phase_postprocess(
            project_path,
            full,
            &write_result.write_units,
            &write_result.config_units,
            &scan_result.to_remove,
        )?;

        // Phase 8-11: Analysis (git cochange, infra, resolver feedback, ADR)
        self.phase_analysis(project_path, &write_result.write_units, &route_nodes)?;

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(IndexReport {
            files_scanned: scan_result.files_scanned,
            files_added: scan_result.files_added,
            files_updated: scan_result.files_updated,
            files_removed: scan_result.to_remove.len(),
            files_skipped: scan_result.files_skipped,
            symbols_total,
            chunks_total,
            parse_errors: parse_result.parse_errors,
            elapsed_ms: elapsed,
            files_parsed: parse_result.files_to_parse,
            used_parallel_parse: parse_result.used_parallel,
        })
    }

    // ── Phase helper structs ────────────────────────────────────────────

    /// Phase 1+2: Scan files and compute diff against existing DB state.
    fn phase_scan_and_diff(
        &self,
        _project_path: &Path,
        full: bool,
        auto_file_limit: Option<usize>,
    ) -> CcResult<ScanDiffResult> {
        // Phase 1: Scan
        let scanned = self.scanner.scan();
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
            HashMap::new()
        } else {
            self.db.get_file_state()?
        };
        let scanned_paths: std::collections::HashSet<String> =
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
                        let content = std::fs::read(&file.abs_path).ok()?;
                        let hash = format!("{:x}", Sha256::digest(&content));
                        if hash == old.content_hash {
                            (hash, FileAction::Skip)
                        } else {
                            (hash, FileAction::Update)
                        }
                    }
                    None => {
                        let content = std::fs::read(&file.abs_path).ok()?;
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

        // Files to remove
        let to_remove: Vec<String> = existing
            .keys()
            .filter(|p| !scanned_paths.contains(p.as_str()))
            .cloned()
            .collect();

        // Filter and sort non-skip files for parsing (large files first)
        let mut to_parse: Vec<PendingFile> = pending
            .into_iter()
            .filter(|pf| !matches!(pf.action, FileAction::Skip))
            .collect();
        to_parse.sort_by(|a, b| b.scanned.size.cmp(&a.scanned.size));

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
    fn phase_parse(
        &self,
        project_path: &Path,
        to_parse: Vec<PendingFile>,
    ) -> CcResult<ParseResult> {
        let mut parse_errors = Vec::new();

        // Pre-compute Cargo workspace alias map for Rust crate import resolution
        let workspace_aliases = crate::resolver::resolve_cargo_workspace(project_path);

        let parse_one = |pf: &PendingFile| -> Result<FileWriteUnit, (String, String)> {
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

            Ok(FileWriteUnit {
                rel_path,
                language,
                content_hash,
                mtime,
                size,
                outcome,
            })
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
                let batch_results: Vec<Result<FileWriteUnit, (String, String)>> =
                    pool.install(|| batch.par_iter().map(&parse_one).collect());

                for result in batch_results {
                    match result {
                        Ok(unit) => write_units.push(unit),
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
                    Ok(unit) => write_units.push(unit),
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
    fn build_actions_map(
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

        for dirty_path in &dirty_files {
            let edges = self.db.load_file_edges_for_reresolve(dirty_path)?;
            // Build a FileWriteUnit from DB data with resolution fields cleared
            let mut call_edges = edges.call_edges;
            for edge in &mut call_edges {
                edge.callee_symbol_uid = None;
                edge.resolution_kind = ResolutionKind::Unresolved;
            }
            let mut symbol_refs = edges.symbol_refs;
            for sr in &mut symbol_refs {
                sr.target_symbol_uid = None;
                sr.target_symbol_id = None;
                sr.resolution_kind = ResolutionKind::Unresolved;
            }
            let mut route_edges = edges.route_edges;
            for re in &mut route_edges {
                re.handler_symbol_id = None;
                re.handler_symbol_uid = None;
            }

            // Retrieve file metadata from existing state for mtime/hash
            let (content_hash, mtime, size) = if let Some(state) = existing.get(dirty_path) {
                (state.content_hash.clone(), state.mtime, state.size)
            } else {
                (String::new(), 0.0, 0u64)
            };

            let outcome = ParseOutcome {
                symbols: edges.symbols,
                imports: edges.imports,
                call_edges,
                symbol_refs,
                semantic_edges: edges.semantic_edges,
                dispatch_sites: edges.dispatch_sites,
                route_edges,
                ..Default::default()
            };

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
                    let src = imp.import_string.to_lowercase();
                    let import_checks: &[(&str, &str)] = &[
                        ("express", "express"),
                        ("fastify", "fastify"),
                        ("react", "react"),
                        ("nestjs", "@nestjs/common"),
                        ("nestjs", "@nestjs/core"),
                        ("django", "django"),
                        ("flask", "flask"),
                        ("fastapi", "fastapi"),
                        ("gin", "github.com/gin-gonic/gin"),
                        ("echo", "github.com/labstack/echo"),
                        ("fiber", "github.com/gofiber/fiber"),
                        ("chi", "github.com/go-chi/chi"),
                        ("gorilla", "github.com/gorilla/mux"),
                        ("net_http", "net/http"),
                        ("spring", "org.springframework"),
                        ("koa", "koa"),
                        ("axum", "axum"),
                        ("actix", "actix-web"),
                        ("actix", "actix_web"),
                        ("rocket", "rocket"),
                    ];
                    for &(fw_key, marker) in import_checks {
                        if src.contains(marker) {
                            let entry = repo_fws.entry(fw_key.to_string()).or_insert(0.0);
                            *entry = (*entry + 0.4).min(0.95);
                        }
                    }
                }
            }

            // Build file-level framework map from write_unit imports
            let mut file_fw_map: HashMap<String, Vec<(String, f64)>> = HashMap::new();
            for unit in write_units.iter() {
                let mut file_fws: HashMap<String, f64> = HashMap::new();
                for imp in &unit.outcome.imports {
                    let src = imp.import_string.to_lowercase();
                    let import_checks: &[(&str, &str)] = &[
                        ("express", "express"),
                        ("fastify", "fastify"),
                        ("react", "react"),
                        ("nestjs", "@nestjs/common"),
                        ("nestjs", "@nestjs/core"),
                        ("django", "django"),
                        ("flask", "flask"),
                        ("fastapi", "fastapi"),
                        ("gin", "github.com/gin-gonic/gin"),
                        ("echo", "github.com/labstack/echo"),
                        ("fiber", "github.com/gofiber/fiber"),
                        ("chi", "github.com/go-chi/chi"),
                        ("gorilla", "github.com/gorilla/mux"),
                        ("net_http", "net/http"),
                        ("spring", "org.springframework"),
                        ("koa", "koa"),
                        ("axum", "axum"),
                        ("actix", "actix-web"),
                        ("actix", "actix_web"),
                        ("rocket", "rocket"),
                    ];
                    for &(fw_key, marker) in import_checks {
                        if src.contains(marker) {
                            let entry = file_fws.entry(fw_key.to_string()).or_insert(0.0);
                            *entry = (*entry + 0.4).min(0.95);
                        }
                    }
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
            let go_router_keys = ["gin", "echo", "fiber", "chi", "gorilla", "net_http"];
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
            if let Ok(compile_targets) = self.db.infra_nodes_by_kind("compile_target") {
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
