//! Incremental indexer pipeline.
//!
//! Phase 1: Scan → Vec<ScannedFile>
//! Phase 2: Diff (mtime fast-skip → hash confirm) → Vec<PendingFile>
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
use cc_db::index_db::{FileWriteUnit, IndexDb, SymbolTargetRow};
use cc_model::edge::{ResolutionKind, RouteNodeRecord};
use cc_model::parse::ParseOutcome;
use cc_model::symbol::SymbolRefRecord;
use cc_model::{CcError, CcResult, Language, ParserTier, StableId};
use cc_parsers::import_resolver::resolve_import;
use cc_parsers::ParserRegistry;

use crate::community::{build_community_labels, louvain_communities};
use crate::config_linker::{extract_config_links, ConfigLinkKind};
use crate::framework_registry;
use crate::resolver::{ResolutionContext, SymbolCatalog};
use crate::scanner::{ScannedFile, Scanner};

/// Minimum number of files to justify rayon parallel overhead.
/// Below this threshold, sequential iteration is used.
const MIN_FILES_FOR_PARALLEL: usize = 50;
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
    db: Arc<IndexDb>,
    parsers: ParserRegistry,
    scanner: Scanner,
    parse_timeout_micros: Option<u64>,
    dirty_propagation: bool,
    dirty_propagation_max_files: usize,
    memory_budget: MemoryBudget,
    max_concurrent_parse: Option<usize>,
    use_direct_writer: bool,
    dispatch_synthesis: bool,
    event_fanout_cap: usize,
    event_denylist: Vec<String>,
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

        let pending: Vec<PendingFile> = scanned
            .into_par_iter()
            .filter_map(|file| {
                let content = std::fs::read(&file.abs_path).ok()?;
                let hash = format!("{:x}", Sha256::digest(&content));

                let action = match existing.get(&file.rel_path) {
                    None => FileAction::Add,
                    Some((old_hash, old_mtime)) => {
                        // Fast mtime check
                        if (file.mtime - old_mtime).abs() < 0.001 {
                            return Some(PendingFile {
                                scanned: file,
                                content_hash: hash,
                                action: FileAction::Skip,
                            });
                        }
                        if &hash == old_hash {
                            return Some(PendingFile {
                                scanned: file,
                                content_hash: hash,
                                action: FileAction::Skip,
                            });
                        }
                        FileAction::Update
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

        // Phase 3: Parse (parallel for large batches, sequential for small)
        //
        // Optimization: sort non-skip files by size descending before dispatch.
        // Large files are dispatched first so that rayon's work-stealing doesn't
        // leave a single slow job as the tail, reducing overall wall-clock time.
        let mut parse_errors = Vec::new();
        let mut symbols_total = 0;
        let mut chunks_total = 0;

        let mut to_parse: Vec<PendingFile> = pending
            .into_iter()
            .filter(|pf| !matches!(pf.action, FileAction::Skip))
            .collect();
        to_parse.sort_by(|a, b| b.scanned.size.cmp(&a.scanned.size));

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
            let mut outcome = self
                .parsers
                .parse_with_timeout(&rel_path, &content, language, self.parse_timeout_micros)
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

            // Per-file memory reclaim — bound peak RSS during parallel parsing
            self.parsers.per_file_reclaim();

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

        // Log session-level allocation summary
        let alloc_summary = self.parsers.alloc_summary();
        if alloc_summary.peak_bytes > 0 {
            tracing::info!(
                peak_mb = alloc_summary.peak_bytes / 1_048_576,
                total_allocs = alloc_summary.alloc_count,
                total_frees = alloc_summary.free_count,
                "tree-sitter allocation summary"
            );
        }

        // Phase 3.5: Dirty propagation — detect export changes and mark importers
        // Build an actions map from pending decisions for dirty propagation to mutate.
        let mut actions: HashMap<String, FileAction> = HashMap::new();
        // All parsed files are Add or Update
        for unit in &write_units {
            // Determine original action: if existing DB had this file, it's Update; otherwise Add
            if existing.contains_key(&unit.rel_path) {
                actions.insert(unit.rel_path.clone(), FileAction::Update);
            } else {
                actions.insert(unit.rel_path.clone(), FileAction::Add);
            }
        }
        // All scanned but not parsed files are Skip
        for path in &scanned_paths {
            if !actions.contains_key(path) {
                actions.insert(path.clone(), FileAction::Skip);
            }
        }

        let dirty_count = if full {
            0
        } else {
            self.run_dirty_propagation(&mut actions, &write_units)?
        };

        // Phase 3.6: Load dirty files' edge data for re-resolution
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
            let (content_hash, mtime, size) = if let Some((hash, mt)) = existing.get(dirty_path) {
                (hash.clone(), *mt, 0u64)
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

        // Phase 3.7: Framework resolver enrichment (before resolution)
        // Run framework resolvers on write_units so that their route_edges
        // flow through Phase 4c resolution and collect_route_nodes.
        // Build ProjectFrameworkContext from package markers + write_unit imports
        // (DB has not been written yet, so we cannot query it).
        //
        // fw_context is declared at this scope so Phase 4d can reuse it.
        let fw_context = {
            let pkg_markers = framework_registry::check_package_markers(project_path);
            // Also scan write_unit imports for framework signals
            let mut repo_fws: HashMap<String, f64> = HashMap::new();
            for (fw_key, conf) in &pkg_markers {
                repo_fws.insert(fw_key.clone(), *conf);
            }
            for unit in &write_units {
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
            for unit in &write_units {
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
                for unit in &mut write_units {
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
                    for unit in &mut write_units {
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

        // Phase 4: Symbol resolution (cross-file)
        let resolver_excluded_files: Vec<String> = write_units
            .iter()
            .map(|u| u.rel_path.clone())
            .chain(to_remove.iter().cloned())
            .collect();
        let persisted_symbols = if full {
            Vec::new()
        } else {
            self.db
                .resolver_seed_symbols_excluding(&resolver_excluded_files)?
        };

        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&persisted_symbols);
        for unit in &write_units {
            catalog.add_symbols(&unit.outcome.symbols);
        }

        let resolution_contexts: Vec<ResolutionContext> = write_units
            .iter()
            .map(|unit| SymbolCatalog::build_resolution_context(&unit.outcome, &unit.rel_path))
            .collect();

        // Phase 4a: Resolve semantic edge UIDs and backfill base_types/implements
        // This must happen BEFORE building the TypeCatalog so that is_subtype()
        // has real data to work with.
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units
                .par_iter_mut()
                .zip(resolution_contexts.par_iter())
                .for_each(|(unit, context)| {
                    let file_path = unit.rel_path.clone();
                    catalog.resolve_semantic_edges_and_backfill_with_context(
                        &file_path,
                        &mut unit.outcome,
                        context,
                    );
                });
        } else {
            for (unit, context) in write_units.iter_mut().zip(resolution_contexts.iter()) {
                let file_path = unit.rel_path.clone();
                catalog.resolve_semantic_edges_and_backfill_with_context(
                    &file_path,
                    &mut unit.outcome,
                    context,
                );
            }
        }

        // Phase 4a-2: Derive USES_TYPE edges from type annotations
        // Must happen after backfill (which may update base_types/implements),
        // before TypeCatalog construction.
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units.par_iter_mut().for_each(|unit| {
                let file_path = unit.rel_path.clone();
                catalog.derive_uses_type_edges(&file_path, &mut unit.outcome);
            });
        } else {
            for unit in &mut write_units {
                let file_path = unit.rel_path.clone();
                catalog.derive_uses_type_edges(&file_path, &mut unit.outcome);
            }
        }

        // Phase 4b: Build TypeCatalog for type-aware method dispatch resolution
        // Now symbols have base_types/implements populated from semantic_edges.
        let all_symbols: Vec<_> = persisted_symbols
            .iter()
            .cloned()
            .chain(
                write_units
                    .iter()
                    .flat_map(|u| u.outcome.symbols.iter().cloned()),
            )
            .collect();
        catalog.build_type_catalog(&all_symbols);

        // Phase 4b-1: Feed type_assigns into TypeCatalog for variable type inference
        catalog.add_type_assigns_from_outcomes(&write_units);

        // Phase 4b-2: Generate hierarchy edges (Defines, DefinesMethod, ContainsFile)
        let file_paths: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let hierarchy_edges = crate::hierarchy::generate_hierarchy_edges(&all_symbols, &file_paths);

        drop(all_symbols);

        // Phase 4c: Resolve call edges, symbol refs, route edges
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units
                .par_iter_mut()
                .zip(resolution_contexts.par_iter())
                .for_each(|(unit, context)| {
                    let file_path = unit.rel_path.clone();
                    catalog.resolve_outcome_with_context(&file_path, &mut unit.outcome, context);
                });
        } else {
            for (unit, context) in write_units.iter_mut().zip(resolution_contexts.iter()) {
                let file_path = unit.rel_path.clone();
                catalog.resolve_outcome_with_context(&file_path, &mut unit.outcome, context);
            }
        }

        // Phase 4d: Cross-file framework resolution (post-catalog)
        // Now that the SymbolCatalog is fully built, resolvers can look up
        // handler symbols across files (e.g. Express sub-router mounting,
        // FastAPI include_router prefixes, NestJS handler UID binding).
        {
            let registry = crate::framework_resolvers::default_registry();
            let active = registry.active_resolvers(&fw_context);
            if !active.is_empty() {
                let mut owned_pairs: Vec<(String, ParseOutcome)> = write_units
                    .iter()
                    .map(|u| (u.rel_path.clone(), u.outcome.clone()))
                    .collect();
                for resolver in &active {
                    resolver.resolve_cross_file(&catalog, &mut owned_pairs, &fw_context);
                }
                // Merge back any changes from cross-file resolution
                for (i, (_, outcome)) in owned_pairs.into_iter().enumerate() {
                    if i < write_units.len() {
                        let unit = &mut write_units[i];
                        if outcome.route_edges.len() > unit.outcome.route_edges.len() {
                            unit.outcome.route_edges = outcome.route_edges;
                        }
                        if outcome.call_edges.len() > unit.outcome.call_edges.len() {
                            unit.outcome.call_edges = outcome.call_edges;
                        }
                    }
                }
            }
        }

        // Count statistics
        for unit in &write_units {
            symbols_total += unit.outcome.symbols.len();
            chunks_total += unit.outcome.chunks.len();
        }
        let files_removed = to_remove.len();
        let route_nodes = self.collect_route_nodes(&write_units);

        // Separate dirty write units from normal ones before Phase 6 write.
        // DirtyResolveOnly files participated in Phase 4 resolution but must
        // NOT go through replace_files_batch (which would delete and rewrite
        // chunks, FTS data, etc. that we did not reload).  They are persisted
        // via replace_reresolved_edges_only, which only updates edge tables
        // (symbols, imports, call_edges, symbol_refs, semantic_edges,
        // dispatch_sites, route_edges) without touching files/chunks/FTS/
        // route_nodes/literals/scopes.
        let dirty_set: HashSet<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::DirtyResolveOnly))
            .map(|(p, _)| p.clone())
            .collect();
        let (dirty_write_units, normal_write_units): (Vec<_>, Vec<_>) = write_units
            .into_iter()
            .partition(|u| dirty_set.contains(&u.rel_path));

        // Phase 6: Batch write — dual path
        let config_units = if full {
            // Full rebuild: temp-db + atomic swap
            // All main data (files, route_nodes, config_units, metadata)
            // are written inside the temp-db closure.
            // (Dirty propagation is skipped for full builds, so
            //  dirty_write_units is always empty here.)
            if self.use_direct_writer {
                match self.write_full_snapshot_via_direct_writer(
                    project_path,
                    &normal_write_units,
                    &route_nodes,
                ) {
                    Ok(config_units) => {
                        tracing::info!("full rebuild completed via direct writer");
                        config_units
                    }
                    Err(e) => {
                        tracing::warn!(
                            err = %e,
                            "direct writer failed, falling back to standard rebuild"
                        );
                        self.write_full_snapshot_via_temp_db(
                            project_path,
                            &normal_write_units,
                            &route_nodes,
                        )?
                    }
                }
            } else {
                self.write_full_snapshot_via_temp_db(
                    project_path,
                    &normal_write_units,
                    &route_nodes,
                )?
            }
        } else {
            // Incremental: keep existing path
            if !to_remove.is_empty() {
                self.db.remove_files_batch(&to_remove)?;
            }
            if !normal_write_units.is_empty() {
                self.db.replace_files_batch(&normal_write_units)?;
            }
            if !dirty_write_units.is_empty() {
                self.db.replace_reresolved_edges_only(&dirty_write_units)?;
            }
            if !route_nodes.is_empty() {
                self.db.insert_route_nodes_batch(&route_nodes)?;
            }

            let config_units = self.build_config_link_units(project_path)?;
            if !config_units.is_empty() {
                self.db.replace_files_batch(&config_units)?;
            }

            // Update metadata (for incremental only; full path sets it inside temp-db)
            let now = chrono::Utc::now().to_rfc3339();
            self.db.set_metadata("last_indexed_at", &now)?;
            self.db.set_metadata("index_version", "1.0.0")?;

            config_units
        };

        // Reassemble write_units for downstream phases that need the full list
        let write_units: Vec<FileWriteUnit> = normal_write_units
            .into_iter()
            .chain(dirty_write_units)
            .collect();

        // Write hierarchy edges (appends to semantic_edges, does not replace)
        if !hierarchy_edges.is_empty() {
            self.db.insert_semantic_edges_batch(&hierarchy_edges)?;
            tracing::info!(count = hierarchy_edges.len(), "generated hierarchy edges");
        }

        // Post-processing passes run on the live DB after both paths
        self.persist_frameworks(project_path, &write_units)?;

        // Phase 7: Post-processing — rebuild test edges for changed files
        let mut changed_paths: Vec<String> =
            write_units.iter().map(|u| u.rel_path.clone()).collect();
        changed_paths.extend(config_units.iter().map(|u| u.rel_path.clone()));
        changed_paths.extend(to_remove.iter().cloned());
        if full {
            self.db.rebuild_test_edges()?;
        } else if !changed_paths.is_empty() {
            self.db.rebuild_test_edges_for_files(&changed_paths)?;
        }

        // Phase 7b: Dynamic dispatch synthesis (event emitter → handler)
        // NOTE: synthesis must run BEFORE community detection so that
        //       synthetic edges are included in the community graph.
        if self.dispatch_synthesis {
            let synthesis_config = crate::dispatch_synthesis::SynthesisConfig {
                enabled: true,
                event_fanout_cap: self.event_fanout_cap,
                generic_event_denylist: if self.event_denylist.is_empty() {
                    crate::dispatch_synthesis::SynthesisConfig::default().generic_event_denylist
                } else {
                    self.event_denylist.iter().cloned().collect()
                },
            };
            let mut stats = crate::dispatch_synthesis::run_event_emitter_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            if stats.event_emitter_edges > 0 {
                tracing::info!(
                    edges = stats.event_emitter_edges,
                    skipped_generic = stats.skipped_generic,
                    skipped_fanout = stats.skipped_fanout,
                    "event emitter synthesis complete"
                );
            }

            // Phase 7c: JSX component synthesis
            let jsx_count = crate::dispatch_synthesis::run_jsx_synthesis(&self.db)?;
            stats.jsx_edges = jsx_count;
            if jsx_count > 0 {
                tracing::info!(edges = jsx_count, "JSX component synthesis complete");
            }

            // Phase 7d: State setter synthesis
            let setter_count = crate::dispatch_synthesis::run_state_setter_synthesis(&self.db)?;
            stats.setter_edges = setter_count;
            if setter_count > 0 {
                tracing::info!(edges = setter_count, "state setter synthesis complete");
            }

            // Phase 7e: Field-backed observer synthesis
            let observer_count = crate::dispatch_synthesis::run_field_observer_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            stats.field_observer_edges = observer_count;
            if observer_count > 0 {
                tracing::info!(edges = observer_count, "field observer synthesis complete");
            }

            // Phase 7f: React re-render chain synthesis
            let rerender_count =
                crate::dispatch_synthesis::run_react_rerender_chain_synthesis(&self.db)?;
            stats.react_rerender_edges = rerender_count;
            if rerender_count > 0 {
                tracing::info!(
                    edges = rerender_count,
                    "React re-render chain synthesis complete"
                );
            }

            // Phase 7g: Vue template synthesis (child components + event handlers)
            let vue_count = crate::dispatch_synthesis::run_vue_template_synthesis(&self.db)?;
            if vue_count > 0 {
                tracing::info!(edges = vue_count, "Vue template synthesis complete");
            }

            // Phase 7h: Interface/abstract method dispatch synthesis
            let interface_count = crate::dispatch_synthesis::run_interface_dispatch_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            if interface_count > 0 {
                tracing::info!(
                    edges = interface_count,
                    "Interface dispatch synthesis complete"
                );
            }
        } else {
            // If synthesis was enabled in a previous run and is disabled now,
            // proactively remove stale synthetic edges so callers/callees and
            // communities reflect the current config without requiring a full
            // rebuild.
            let removed_event = self.db.delete_synthetic_call_edges("event_emitter")?;
            let removed_setter = self.db.delete_synthetic_call_edges("react_state_setter")?;
            let removed_jsx = self.db.delete_synthetic_semantic_edges("synth:jsx:")?;
            let removed_observer = self.db.delete_synthetic_call_edges("field_observer")?;
            let removed_rerender = self.db.delete_synthetic_call_edges("react_rerender")?;
            let removed_vue_semantic = self.db.delete_synthetic_semantic_edges("synth:vue:")?;
            let removed_vue_handler = self.db.delete_synthetic_call_edges("vue_event_handler")?;
            let removed_interface = self.db.delete_synthetic_call_edges("interface_dispatch")?;
            if removed_event > 0
                || removed_setter > 0
                || removed_jsx > 0
                || removed_observer > 0
                || removed_rerender > 0
                || removed_vue_semantic > 0
                || removed_vue_handler > 0
                || removed_interface > 0
            {
                tracing::info!(
                    event_edges = removed_event,
                    setter_edges = removed_setter,
                    jsx_edges = removed_jsx,
                    observer_edges = removed_observer,
                    rerender_edges = removed_rerender,
                    vue_semantic_edges = removed_vue_semantic,
                    vue_handler_edges = removed_vue_handler,
                    interface_edges = removed_interface,
                    "dispatch synthesis disabled; removed stale synthetic edges"
                );
            }
        }

        self.rebuild_communities()?;

        // Phase 8: Git co-change analysis
        self.analyze_git_cochanges(project_path)?;

        // Phase 9: Infrastructure pass
        let (mut infra_nodes, mut infra_edges) = crate::infra_pass::run_infra_pass(project_path);
        if !infra_nodes.is_empty() || !infra_edges.is_empty() {
            // Bind infra nodes to code symbols before persisting
            let bind_symbols: Vec<_> = write_units
                .iter()
                .flat_map(|u| u.outcome.symbols.iter().cloned())
                .collect();
            crate::infra_pass::bind_infra_to_symbols(&mut infra_nodes, &bind_symbols);

            // Match binding target URLs to known route nodes
            crate::infra_pass::match_bindings_to_routes(&mut infra_edges, &route_nodes);

            self.db.replace_infra_data(&infra_nodes, &infra_edges)?;
            let bound_count = infra_nodes
                .iter()
                .filter(|n| n.bound_symbol_uid.is_some())
                .count();
            let binding_count = infra_edges
                .iter()
                .filter(|e| {
                    matches!(
                        e.kind,
                        cc_model::infra::InfraEdgeKind::BindsTopic
                            | cc_model::infra::InfraEdgeKind::ConsumesQueue
                    )
                })
                .count();
            tracing::info!(
                nodes = infra_nodes.len(),
                edges = infra_edges.len(),
                bound = bound_count,
                bindings = binding_count,
                "indexed infra graph"
            );
        }

        // Phase 10: Resolver quality feedback.  This table is consumed by LLM
        // tools to show unresolved references together with likely candidates,
        // making resolver gaps visible without re-scanning raw files.
        let unresolved_count = self.db.rebuild_resolution_attempts()?;
        if unresolved_count > 0 {
            tracing::info!(
                count = unresolved_count,
                "rebuilt unresolved reference backlog"
            );
        }

        // Phase 11: Architecture Decision Records (ADR) indexing
        {
            let adr_dirs = [
                "docs/adr",
                "docs/decisions",
                "doc/architecture/decisions",
                "doc/adr",
            ];
            let mut adr_docs = Vec::new();

            for dir in &adr_dirs {
                let adr_path = project_path.join(dir);
                if adr_path.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&adr_path) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().is_some_and(|e| e == "md") {
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    // Extract MADR-format header
                                    let mut title = None;
                                    let mut status = None;
                                    let mut date = None;
                                    for line in content.lines().take(20) {
                                        if title.is_none() && line.starts_with("# ") {
                                            title = Some(line.trim_start_matches("# ").to_string());
                                        }
                                        if line.to_lowercase().starts_with("status:") {
                                            status = Some(
                                                line.split(':')
                                                    .nth(1)
                                                    .unwrap_or("")
                                                    .trim()
                                                    .to_string(),
                                            );
                                        }
                                        if line.to_lowercase().starts_with("date:") {
                                            date = Some(
                                                line.split(':')
                                                    .nth(1)
                                                    .unwrap_or("")
                                                    .trim()
                                                    .to_string(),
                                            );
                                        }
                                    }
                                    if let Some(t) = title {
                                        let rel = path
                                            .strip_prefix(project_path)
                                            .unwrap_or(&path)
                                            .to_string_lossy()
                                            .to_string();
                                        adr_docs.push(serde_json::json!({
                                            "file": rel,
                                            "title": t,
                                            "status": status,
                                            "date": date,
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if !adr_docs.is_empty() {
                tracing::info!(count = adr_docs.len(), "indexed ADR documents");
                let _ = self.db.set_metadata(
                    "adr_documents",
                    &serde_json::to_string(&adr_docs).unwrap_or_default(),
                );
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        Ok(IndexReport {
            files_scanned,
            files_added,
            files_updated,
            files_removed,
            files_skipped,
            symbols_total,
            chunks_total,
            parse_errors,
            elapsed_ms: elapsed,
            files_parsed: files_to_parse,
            used_parallel_parse: used_parallel,
        })
    }

    fn collect_route_nodes(&self, write_units: &[FileWriteUnit]) -> Vec<RouteNodeRecord> {
        let mut route_nodes = Vec::new();
        for unit in write_units {
            for route in &unit.outcome.route_edges {
                route_nodes.push(RouteNodeRecord {
                    route_id: StableId::edge_id(
                        "route_node",
                        &route.file_path,
                        route.line,
                        route.start_col,
                    ),
                    file_path: route.file_path.clone(),
                    route_path: route.route_path.clone(),
                    method: route.method.clone(),
                    handler_symbol_uid: route.handler_symbol_uid.clone(),
                    handler_name: route.handler_name.clone(),
                    framework: route.framework.clone(),
                    line: route.line,
                    end_line: route.end_line,
                    normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                        &route.route_path,
                    )),
                    confidence: route.confidence,
                    parser_tier: route.parser_tier,
                });
            }
        }
        route_nodes
    }

    /// Pure function: build config link units from pre-collected snapshot data.
    /// Does not query the database, suitable for use inside temp-db write closure.
    fn build_config_link_units_from_snapshot(
        project_path: &Path,
        symbol_targets: Vec<SymbolTargetRow>,
        indexed_files: &[String],
    ) -> CcResult<Vec<FileWriteUnit>> {
        let mut known_symbols = HashSet::new();
        let mut qname_lookup: HashMap<String, (String, Option<String>, String)> = HashMap::new();
        let mut basename_lookup: HashMap<String, Vec<(String, Option<String>, String)>> =
            HashMap::new();
        for sym in symbol_targets {
            if let Some(qname) = sym.qname.clone() {
                known_symbols.insert(qname.clone());
                qname_lookup.insert(
                    qname,
                    (
                        sym.symbol_id.clone(),
                        sym.symbol_uid.clone(),
                        sym.file_path.clone(),
                    ),
                );
            }
            basename_lookup.entry(sym.name.clone()).or_default().push((
                sym.symbol_id,
                sym.symbol_uid,
                sym.file_path,
            ));
        }

        let known_files: HashSet<String> = indexed_files.iter().cloned().collect();
        let mut file_basename_lookup: HashMap<String, Vec<String>> = HashMap::new();
        for file in indexed_files {
            if let Some(base) = Path::new(file).file_name().and_then(|n| n.to_str()) {
                file_basename_lookup
                    .entry(base.to_string())
                    .or_default()
                    .push(file.clone());
            }
        }
        let links = extract_config_links(project_path, &known_symbols, &known_files)?;
        if links.is_empty() {
            return Ok(Vec::new());
        }

        let mut grouped: HashMap<String, Vec<_>> = HashMap::new();
        for link in links {
            grouped
                .entry(link.config_file.clone())
                .or_default()
                .push(link);
        }

        let mut units = Vec::new();
        for (config_file, links) in grouped {
            let abs_path = project_path.join(&config_file);
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let metadata = match abs_path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mut symbol_refs = Vec::new();
            for link in &links {
                let (
                    target_symbol_id,
                    target_symbol_uid,
                    target_file_path,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                ) = match link.link_kind {
                    ConfigLinkKind::ModulePath => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_module_exact".to_string(),
                            )
                        } else {
                            let tail = link
                                .referenced_value
                                .rsplit('.')
                                .next()
                                .unwrap_or(&link.referenced_value);
                            match basename_lookup.get(tail) {
                                Some(candidates) if candidates.len() == 1 => {
                                    let (sid, suid, fpath) = &candidates[0];
                                    (
                                        Some(sid.clone()),
                                        suid.clone(),
                                        Some(fpath.clone()),
                                        ResolutionKind::Heuristic,
                                        link.confidence,
                                        "config_module_suffix".to_string(),
                                    )
                                }
                                _ => (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                ),
                            }
                        }
                    }
                    ConfigLinkKind::FilePath => {
                        let resolved_path = if known_files.contains(&link.referenced_value) {
                            Some(link.referenced_value.clone())
                        } else {
                            Path::new(&link.referenced_value)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .and_then(|base| file_basename_lookup.get(base))
                                .filter(|paths| paths.len() == 1)
                                .and_then(|paths| paths.first().cloned())
                        };
                        match resolved_path {
                            Some(path) => (
                                None,
                                None,
                                Some(path),
                                if known_files.contains(&link.referenced_value) {
                                    ResolutionKind::Exact
                                } else {
                                    ResolutionKind::Heuristic
                                },
                                link.confidence,
                                if known_files.contains(&link.referenced_value) {
                                    "config_file_exact".to_string()
                                } else {
                                    "config_file_basename".to_string()
                                },
                            ),
                            None => (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            ),
                        }
                    }
                    ConfigLinkKind::DependencyImport => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_dependency_exact".to_string(),
                            )
                        } else if let Some(candidates) = basename_lookup.get(&link.referenced_value)
                        {
                            if candidates.len() == 1 {
                                let (sid, suid, fpath) = &candidates[0];
                                (
                                    Some(sid.clone()),
                                    suid.clone(),
                                    Some(fpath.clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_symbol".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else if let Some(paths) = file_basename_lookup.get(&link.referenced_value)
                        {
                            if paths.len() == 1 {
                                (
                                    None,
                                    None,
                                    Some(paths[0].clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_file".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else {
                            (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            )
                        }
                    }
                };

                symbol_refs.push(SymbolRefRecord {
                    ref_id: StableId::ref_id(&config_file, &link.referenced_value, link.line, 0),
                    file_path: config_file.clone(),
                    symbol_name: link.referenced_value.clone(),
                    container: Some(link.config_key.clone()),
                    ref_kind: match link.link_kind {
                        ConfigLinkKind::ModulePath => "config_module".to_string(),
                        ConfigLinkKind::FilePath => "config_file".to_string(),
                        ConfigLinkKind::DependencyImport => "config_dependency".to_string(),
                    },
                    line: link.line,
                    column: 0,
                    target_symbol_id,
                    target_file_path,
                    target_symbol_uid,
                    ref_name: Some(link.referenced_value.clone()),
                    scope_id: None,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                    ref_end_line: Some(link.line),
                    ref_end_col: None,
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: link.confidence.max(0.70),
                });
            }

            let excerpt: String = links
                .iter()
                .take(6)
                .map(|link| format!("{} -> {}", link.config_key, link.referenced_value))
                .collect::<Vec<_>>()
                .join("; ");

            let outcome = ParseOutcome {
                summary: format!(
                    "Configuration file with {} code link(s){}",
                    symbol_refs.len(),
                    if excerpt.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", excerpt)
                    }
                ),
                symbol_refs,
                parser_tier: ParserTier::Heuristic,
                parser_confidence: 0.85,
                ..Default::default()
            };

            let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            units.push(FileWriteUnit {
                rel_path: config_file,
                language: Language::Unknown,
                content_hash,
                mtime,
                size: metadata.len(),
                outcome,
            });
        }

        Ok(units)
    }

    /// Compatibility wrapper: queries DB for symbol targets / file paths,
    /// then delegates to the pure function.
    fn build_config_link_units(&self, project_path: &Path) -> CcResult<Vec<FileWriteUnit>> {
        let symbol_targets = self.db.list_symbol_targets()?;
        let indexed_files = self.db.list_file_paths()?;
        Self::build_config_link_units_from_snapshot(project_path, symbol_targets, &indexed_files)
    }

    /// Collect symbol targets from write_units for config link snapshot.
    fn collect_symbol_targets(write_units: &[FileWriteUnit]) -> Vec<SymbolTargetRow> {
        let mut targets = Vec::new();
        for unit in write_units {
            for s in &unit.outcome.symbols {
                targets.push(SymbolTargetRow {
                    symbol_id: s.symbol_id.clone(),
                    symbol_uid: s.symbol_uid.clone(),
                    name: s.name.clone(),
                    qname: s.qname.clone(),
                    file_path: s.file_path.clone(),
                });
            }
        }
        targets
    }

    /// Write all index data via temp-db + atomic swap (full rebuild only).
    /// All main data (files, route_nodes, config_units, metadata) is written
    /// inside the temp-db transaction. Post-processing passes (frameworks,
    /// communities, test_edges, git co-changes, infra) run after the swap
    /// against the live DB.
    fn write_full_snapshot_via_temp_db(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<Vec<FileWriteUnit>> {
        // Pre-collect snapshot data for config links before entering temp-db closure.
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
        )?;

        let now = chrono::Utc::now().to_rfc3339();

        // Clone config_units for return value (originals move into the closure).
        let config_units_ret = config_units.clone();

        self.db.rebuild_with_temp_db(|conn| {
            // Write main file data
            for unit in write_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write route nodes
            for r in route_nodes {
                IndexDb::insert_route_node_into(conn, r)?;
            }

            // Write config link units
            for unit in &config_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write metadata
            IndexDb::set_metadata_on(conn, "last_indexed_at", &now)?;
            IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

            Ok(())
        })?;

        Ok(config_units_ret)
    }

    /// Write all index data via DirectWriter (high-speed path) + atomic swap.
    /// Same data flow as `write_full_snapshot_via_temp_db` but uses aggressive
    /// PRAGMAs (journal OFF, synchronous OFF, 64KB pages) for faster writes.
    fn write_full_snapshot_via_direct_writer(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<Vec<FileWriteUnit>> {
        // Pre-collect snapshot data (same as temp_db path)
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
        )?;

        let now = chrono::Utc::now().to_rfc3339();
        let config_units_ret = config_units.clone();

        self.db.rebuild_with_direct_writer(|conn| {
            // Write main file data
            for unit in write_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write route nodes
            for r in route_nodes {
                IndexDb::insert_route_node_into(conn, r)?;
            }

            // Write config link units
            for unit in &config_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write metadata
            IndexDb::set_metadata_on(conn, "last_indexed_at", &now)?;
            IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

            Ok(())
        })?;

        Ok(config_units_ret)
    }

    fn persist_frameworks(
        &self,
        project_path: &Path,
        _write_units: &[FileWriteUnit],
    ) -> CcResult<()> {
        framework_registry::detect_and_persist_frameworks(&self.db, project_path)
    }

    fn analyze_git_cochanges(&self, project_path: &Path) -> CcResult<()> {
        match crate::git_cochange::analyze_cochanges(project_path, 2, 0.2, 500) {
            Ok(co_changes) => {
                if !co_changes.is_empty() {
                    self.db.insert_co_change_edges_batch(&co_changes)?;
                    tracing::info!(count = co_changes.len(), "indexed git co-change edges");
                }
            }
            Err(err) => {
                // Non-fatal: git may not be available or the project may not be a git repo
                tracing::warn!(error = %err, "skipping git co-change analysis");
            }
        }
        Ok(())
    }

    fn rebuild_communities(&self) -> CcResult<()> {
        let edges = self.db.call_uid_edges()?;
        let assignments = louvain_communities(&edges, 20);
        let symbol_names = self.db.symbol_names_by_uid()?;
        let labels = build_community_labels(&assignments, &symbol_names);
        self.db.update_communities(&assignments, &labels)
    }

    /// Dirty propagation: detect export signature changes and mark importers
    /// as `DirtyResolveOnly` so their cross-file references get re-resolved
    /// against the updated symbol catalog.
    fn run_dirty_propagation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
    ) -> CcResult<usize> {
        if !self.dirty_propagation {
            return Ok(0);
        }

        // Step 1: Collect all Add/Update files (the ones that were freshly parsed)
        let changed_files: Vec<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::Add | FileAction::Update))
            .map(|(p, _)| p.clone())
            .collect();

        if changed_files.is_empty() {
            return Ok(0);
        }

        // Step 2: Compare old vs new export fingerprints to find files whose
        //         public API surface actually changed.
        let mut export_changed_files = Vec::new();
        for file_path in &changed_files {
            let old_fp = self.db.get_export_fingerprint(file_path)?;
            let new_fp = Self::compute_new_export_fingerprint(write_units, file_path);
            if old_fp != new_fp {
                export_changed_files.push(file_path.clone());
            }
        }

        if export_changed_files.is_empty() {
            return Ok(0);
        }

        // Step 3: Find all files that import at least one of the changed files
        let importers = self.db.find_importers_of(&export_changed_files)?;

        // Step 4: Count how many Skip files would be promoted. If the count
        //         exceeds the configured limit, bail out to avoid runaway
        //         propagation (the user should do a full rebuild instead).
        let dirty_count = importers
            .iter()
            .filter(|p| matches!(actions.get(*p), Some(FileAction::Skip)))
            .count();

        if dirty_count > self.dirty_propagation_max_files {
            tracing::warn!(
                dirty_count,
                max = self.dirty_propagation_max_files,
                "dirty propagation: too many affected files, skipping (consider full rebuild)"
            );
            return Ok(0);
        }

        // Step 5: Promote Skip → DirtyResolveOnly
        let mut marked = 0;
        for importer in importers {
            if let Some(action) = actions.get_mut(&importer) {
                if matches!(action, FileAction::Skip) {
                    *action = FileAction::DirtyResolveOnly;
                    marked += 1;
                }
            }
        }

        if marked > 0 {
            tracing::info!(
                marked,
                export_changed = export_changed_files.len(),
                "dirty propagation: marked files for re-resolution"
            );
        }

        Ok(marked)
    }

    /// Compute the export fingerprint from freshly-parsed write_units.
    ///
    /// The algorithm matches `IndexDb::get_export_fingerprint()`:
    ///   1. Select exported symbols (export_name IS NOT NULL or is_default_export)
    ///   2. Format each as "uid|name|signature|export_name"
    ///   3. Sort by uid (first field)
    ///   4. Join with "\n" and hash with blake3
    fn compute_new_export_fingerprint(
        write_units: &[FileWriteUnit],
        file_path: &str,
    ) -> Option<String> {
        let unit = write_units.iter().find(|u| u.rel_path == file_path)?;
        let mut parts: Vec<String> = unit
            .outcome
            .symbols
            .iter()
            .filter(|s| s.export_name.is_some() || s.is_default_export)
            .map(|s| {
                format!(
                    "{}|{}|{}|{}",
                    s.symbol_uid.as_deref().unwrap_or(""),
                    s.name,
                    s.signature.as_deref().unwrap_or(""),
                    s.export_name.as_deref().unwrap_or(""),
                )
            })
            .collect();
        // Sort by the uid prefix (whole string sort gives the same result
        // because uid is the first field, matching the DB's ORDER BY symbol_uid).
        parts.sort();

        if parts.is_empty() {
            return None;
        }

        let combined = parts.join("\n");
        Some(blake3::hash(combined.as_bytes()).to_hex().to_string())
    }
}
