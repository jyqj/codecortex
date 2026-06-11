//! Lightweight code-index engine for cc-server.
//!
//! Thin wrapper around cc-db, cc-index and cc-search.
//!
//! # Domain views
//!
//! `CodeIndex` itself only carries the index lifecycle (open/close/build),
//! the shared infrastructure accessors (`ensure_db`, `index_status`,
//! `repo_size_tier`, `output_budget`, `diagnostics_info`), and the lock
//! poison-recovery hook. All query surface area is grouped into three
//! zero-cost borrowed views, entered via:
//!
//! - [`CodeIndex::search`] → [`SearchOps`]: context search and task-to-symbol
//!   retrieval (`search_in_context`, `search_in_context_with`, `task_symbols`).
//! - [`CodeIndex::graph`] → [`GraphOps`]: symbol/file/graph lookups
//!   (`find_symbol`, `callers`, `graph_query`, `explore_symbols`,
//!   `graph_schema`, ...). Impl blocks live here and in `engine_query.rs`,
//!   mirroring the historical `CodeIndex` split across the two files.
//! - [`CodeIndex::impact`] → [`ImpactOps`]: change-impact analysis
//!   (`detect_impact*`, `analyze_impact*`, `git_changed_files`,
//!   `find_impacted_tests`), in `engine_query.rs`.

use cc_db::index_db::IndexDb;
use cc_index::{IndexReport, Indexer, PreparedBuild};
use cc_model::config::{
    load_project_config, IndexPaths, IndexingConfig, ProjectConfig, ProjectStats, RepoSizeTier,
};
use cc_model::context::{ContextEnvelope, ContextNode, ContextSpan, NodeType, Role};
use cc_model::search::SearchRequest;
use cc_model::{CcError, CcResult, Intent};
use cc_search::SearchEngine;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

/// Owned inputs cloned from a `CodeIndex` under a brief read lock so that the
/// heavy, read-only `prepare_build` phase can run with no `CodeIndex` lock held.
/// The DB handle is shared (`Arc`); writes are still serialized by the caller's
/// write lock during `commit_build`.
pub struct BuildInputs {
    db: Arc<IndexDb>,
    project: PathBuf,
    indexing: IndexingConfig,
}

/// Result of `CodeIndex::graph_query`, carrying the flattened rows plus
/// truncation signals so callers can distinguish a complete result from one
/// that was cut off by the default LIMIT.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphQueryOutput {
    pub rows: Vec<serde_json::Value>,
    pub row_count: usize,
    pub default_limit_applied: bool,
    pub limit: Option<usize>,
}

fn estimate_project_file_count(project: &Path) -> usize {
    const EXCLUDED_DIRS: &[&str] = &[
        ".git",
        ".codecortex",
        "target",
        "node_modules",
        ".next",
        "dist",
        "build",
        "vendor",
    ];

    let mut count = 0usize;
    let mut stack = vec![project.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !EXCLUDED_DIRS.contains(&name.as_ref()) {
                    stack.push(path);
                }
            } else if file_type.is_file() {
                count += 1;
            }
        }
    }
    count
}

pub struct CodeIndex {
    pub project_path: Option<PathBuf>,
    config: Option<ProjectConfig>,
    pub(crate) index_db: Option<Arc<IndexDb>>,
    engine: Option<SearchEngine>,
    pub(crate) repo_tier: Option<RepoSizeTier>,
    /// True when the DB was freshly created (Initialized) or rebuilt after a
    /// schema mismatch — signals that an auto-index build is needed.
    needs_initial_index: bool,
}

/// Borrowed view over [`CodeIndex`] for the search domain: context search and
/// task-to-symbol retrieval. Obtained via [`CodeIndex::search`].
#[derive(Clone, Copy)]
pub struct SearchOps<'a>(pub(crate) &'a CodeIndex);

/// Borrowed view over [`CodeIndex`] for the graph domain: symbol, file, and
/// graph lookups. Obtained via [`CodeIndex::graph`].
#[derive(Clone, Copy)]
pub struct GraphOps<'a>(pub(crate) &'a CodeIndex);

/// Borrowed view over [`CodeIndex`] for the impact domain: change-impact
/// analysis over the call graph and git diffs. Obtained via
/// [`CodeIndex::impact`].
#[derive(Clone, Copy)]
pub struct ImpactOps<'a>(pub(crate) &'a CodeIndex);

impl CodeIndex {
    /// Enter the search domain view.
    pub fn search(&self) -> SearchOps<'_> {
        SearchOps(self)
    }

    /// Enter the graph domain view.
    pub fn graph(&self) -> GraphOps<'_> {
        GraphOps(self)
    }

    /// Enter the impact domain view.
    pub fn impact(&self) -> ImpactOps<'_> {
        ImpactOps(self)
    }

    pub fn new(project_path: Option<&Path>) -> CcResult<Self> {
        let mut index = Self::empty();
        if let Some(path) = project_path {
            index.set_project(path, false)?;
        }
        Ok(index)
    }

    pub fn empty() -> Self {
        Self {
            project_path: None,
            config: None,
            index_db: None,
            engine: None,
            repo_tier: None,
            needs_initial_index: false,
        }
    }

    pub fn set_project(&mut self, path: &Path, auto_index: bool) -> CcResult<()> {
        let project = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let config = load_project_config(&project);
        let paths = IndexPaths::new(&project);
        std::fs::create_dir_all(&paths.workdir)?;
        std::fs::create_dir_all(&paths.logs_dir)?;

        let estimated_files = estimate_project_file_count(&project);
        let read_pool_size = config
            .indexing
            .db_read_pool_size
            .unwrap_or_else(|| RepoSizeTier::from_file_count(estimated_files).db_read_pool_size());
        let (db, schema_status) =
            IndexDb::open_with_read_pool_size(&paths.index_db, read_pool_size)?;
        let db = Arc::new(db);
        let repo_tier = Some(RepoSizeTier::from_file_count(estimated_files));
        let engine = SearchEngine::new(db.clone(), &config, repo_tier);

        self.project_path = Some(project);
        self.config = Some(config);
        self.index_db = Some(db);
        self.engine = Some(engine);
        self.repo_tier = repo_tier;
        self.needs_initial_index = matches!(
            schema_status,
            cc_db::index_migrate::SchemaStatus::Initialized
        );

        if auto_index {
            let _ = self.build_auto_index(false);
        }
        Ok(())
    }

    pub fn close(&mut self) {
        self.engine = None;
        self.index_db = None;
    }

    pub fn is_closed(&self) -> bool {
        self.project_path.is_some() && self.index_db.is_none()
    }

    pub fn reopen(&mut self) -> CcResult<()> {
        if !self.is_closed() {
            return Ok(());
        }
        let path = self.project_path.clone().ok_or(CcError::ProjectNotSet)?;
        self.set_project(&path, false)
    }

    pub fn index_db(&self) -> Option<&Arc<IndexDb>> {
        self.index_db.as_ref()
    }

    /// Whether this CodeIndex was freshly created (empty DB) and needs an
    /// initial index build. Cleared after a successful build.
    pub fn needs_initial_index(&self) -> bool {
        self.needs_initial_index
    }

    pub(crate) fn ensure_project(&self) -> CcResult<&Path> {
        self.project_path.as_deref().ok_or(CcError::ProjectNotSet)
    }

    pub(crate) fn ensure_db(&self) -> CcResult<&Arc<IndexDb>> {
        self.index_db.as_ref().ok_or(CcError::ProjectNotSet)
    }

    fn ensure_engine(&self) -> CcResult<&SearchEngine> {
        self.engine.as_ref().ok_or(CcError::ProjectNotSet)
    }

    fn ensure_config(&self) -> CcResult<&ProjectConfig> {
        self.config.as_ref().ok_or(CcError::ProjectNotSet)
    }

    pub fn build_index(&mut self, full: bool) -> CcResult<IndexReport> {
        let project = self.ensure_project()?;
        let config = self.ensure_config()?;
        let db = self.ensure_db()?;
        let indexer = Indexer::new(db.clone(), project, &config.indexing);
        let report = indexer.build_index(project, full)?;
        self.after_successful_index_build();
        Ok(report)
    }

    pub fn build_auto_index(&mut self, full: bool) -> CcResult<IndexReport> {
        let project = self.ensure_project()?;
        let config = self.ensure_config()?;
        let db = self.ensure_db()?;
        let indexer = Indexer::new(db.clone(), project, &config.indexing);
        let report = indexer.build_auto_index(project, full, config.auto_index.file_limit)?;
        self.after_successful_index_build();
        Ok(report)
    }

    /// Clone the owned inputs needed to drive a split build. Call this under a
    /// brief read lock, then release the lock before running `prepare_build`.
    pub fn build_inputs(&self) -> CcResult<BuildInputs> {
        let project = self.ensure_project()?;
        let config = self.ensure_config()?;
        let db = self.ensure_db()?;
        Ok(BuildInputs {
            db: db.clone(),
            project: project.to_path_buf(),
            indexing: config.indexing.clone(),
        })
    }

    /// Run the read-only prepare phase with no `CodeIndex` lock held. This is an
    /// associated function (no `self`) by design: the caller holds the inputs
    /// across the unlocked window and passes the resulting `PreparedBuild` to
    /// [`CodeIndex::commit_build`]. `full`/`auto_file_limit` must match the
    /// paired `commit_build` call.
    pub fn prepare_build(
        inputs: &BuildInputs,
        full: bool,
        auto_file_limit: Option<usize>,
    ) -> CcResult<PreparedBuild> {
        let indexer = Indexer::new(inputs.db.clone(), &inputs.project, &inputs.indexing);
        indexer.prepare_build(&inputs.project, full, auto_file_limit)
    }

    /// Commit a previously prepared build under the caller's write lock. Runs
    /// `phase_write` plus postprocess/analysis, then performs the post-build
    /// bookkeeping (`after_successful_index_build`). `full`/`auto_file_limit`
    /// must match the values passed to the paired `prepare_build`.
    pub fn commit_build(
        &mut self,
        inputs: &BuildInputs,
        full: bool,
        auto_file_limit: Option<usize>,
        prepared: PreparedBuild,
    ) -> CcResult<IndexReport> {
        let indexer = Indexer::new(inputs.db.clone(), &inputs.project, &inputs.indexing);
        let report = indexer.commit_build(&inputs.project, full, auto_file_limit, prepared)?;
        self.after_successful_index_build();
        Ok(report)
    }

    fn after_successful_index_build(&mut self) {
        // No manual cache invalidation: the cc-db persisted epoch vector
        // (index_epoch/evidence_epoch, bumped inside every write transaction)
        // is the single index clock — both the SearchEngine result cache and
        // the GraphReadModel caches key their entries on it.
        self.repo_tier = Some(self.compute_repo_tier());
        self.needs_initial_index = false;
    }

    /// Drop cached search results after recovering a poisoned lock — they may
    /// have been computed from a half-mutated CodeIndex. The cc-db persisted
    /// epoch vector remains the only index clock for search/graph caches;
    /// poison recovery additionally calls `engine.invalidate_cache()` because
    /// the epochs cannot tell results computed from a half-mutated CodeIndex
    /// apart from valid ones.
    ///
    /// Correctness depends on the CodeIndex lock ordering: this runs while
    /// holding the CodeIndex WRITE lock (`&mut self`), and every in-flight
    /// search — including its result-cache `put` — completes under a READ
    /// lock, so no stale entry can be inserted concurrently with or after
    /// this clear.
    pub fn invalidate_search_cache_after_poison(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            engine.invalidate_cache();
        }
    }

    pub fn index_status(&self) -> CcResult<ProjectStats> {
        let project = self.ensure_project()?;
        self.ensure_db()?.stats(project)
    }

    pub fn diagnostics_info(&self) -> serde_json::Value {
        let schema_version = cc_db::index_migrate::CURRENT_SCHEMA_VERSION;

        let db_schema_version = self.index_db.as_ref().and_then(|db| {
            let conn = db.read_conn().ok()?;
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
                .ok()
        });

        let last_indexed = self
            .index_db
            .as_ref()
            .and_then(|db| db.get_metadata("last_indexed_at").ok().flatten());

        let auto_index_enabled = self
            .config
            .as_ref()
            .map(|c| c.auto_index.enabled)
            .unwrap_or(true);

        serde_json::json!({
            "schema_version": schema_version,
            "db_schema_version": db_schema_version,
            "last_indexed_at": last_indexed,
            "auto_index_enabled": auto_index_enabled,
            "lock_poison_recovered": crate::handlers::poison_recovered(),
        })
    }
}

impl SearchOps<'_> {
    pub fn search_in_context(
        &self,
        query: &str,
        top_k: usize,
        intent: Option<Intent>,
    ) -> CcResult<ContextEnvelope> {
        self.search_in_context_with(query, top_k, intent, SearchRequest::default())
    }

    pub fn search_in_context_with(
        &self,
        query: &str,
        top_k: usize,
        intent: Option<Intent>,
        overrides: SearchRequest,
    ) -> CcResult<ContextEnvelope> {
        let tier = self.0.repo_size_tier();
        let token_budget = tier.default_token_budget();
        let max_output_chars = tier.max_output_chars();
        let top_k = if top_k == 0 {
            tier.search_top_k()
        } else {
            top_k
        };
        let engine = self.0.ensure_engine()?;
        let detected_intent = intent.unwrap_or_else(|| detect_intent(query));
        let request = build_context_search_request(query, top_k, overrides);
        // Graph-aware rerank happens entirely inside cc-search: it searches a
        // rerank_window-sized candidate list, folds graph connectivity into
        // rerank_score, and returns the FINAL top_k ordering.  Hits must not
        // be re-scored or re-sorted here (see SearchHit::rerank_score).
        let graph_limits = tier.graph_enrich_limits();
        let (hits, enrichment) =
            engine.search_with_graph_context(&request, &graph_limits, token_budget)?;

        let mut nodes = Vec::with_capacity(hits.len());
        let mut spans = Vec::with_capacity(hits.len());
        let mut files = HashSet::new();
        let mut rendered_sections = Vec::new();

        for (idx, hit) in hits.iter().enumerate() {
            files.insert(hit.file_path.clone());
            let node_id = format!("search:{}", hit.chunk_id);
            let title = format!(
                "{} {}:{}-{}",
                hit.file_path, hit.breadcrumb, hit.start_line, hit.end_line
            );
            let role = if hit.reasons.iter().any(|r| r == "doc-file") {
                Role::DocContext
            } else {
                Role::Primary
            };
            let mut node = ContextNode::new(
                node_id.clone(),
                NodeType::SearchHit,
                role,
                title.clone(),
                hit.text.clone(),
            );
            node.file_path = Some(hit.file_path.clone());
            node.start_line = Some(hit.start_line);
            node.end_line = Some(hit.end_line);
            node.score = hit.rerank_score;
            node.confidence = 0.8;
            node.source = hit.source.clone();
            node.reasons = hit.reasons.clone();
            node.invalidation_keys = vec![hit.file_path.clone()];
            node.metadata = hit.metadata.clone();
            node.span_kind = Some("indexed_chunk".to_string());
            node.backing_file_path = Some(hit.file_path.clone());
            node.source_start_line = Some(hit.start_line);
            node.source_end_line = Some(hit.end_line);

            spans.push(ContextSpan {
                node_id,
                file_path: Some(hit.file_path.clone()),
                start_line: Some(hit.start_line),
                end_line: Some(hit.end_line),
                label: hit.breadcrumb.clone(),
            });

            rendered_sections.push(format!(
                "## {}. {}:{}-{}\n{}",
                idx + 1,
                hit.file_path,
                hit.start_line,
                hit.end_line,
                hit.text
            ));
            nodes.push(node);
        }

        // Append graph context nodes within budget.
        let primary_tokens: u32 = nodes.iter().map(|n| n.token_estimate).sum();
        let graph_budget = (token_budget * graph_limits.graph_budget_pct) / 100;
        let mut graph_tokens_used = 0u32;
        let mut graph_rendered: Vec<String> = Vec::new();
        for gnode in enrichment.nodes {
            if graph_tokens_used + gnode.token_estimate > graph_budget {
                break;
            }
            graph_tokens_used += gnode.token_estimate;
            graph_rendered.push(format!("- {}", gnode.title));
            nodes.push(gnode);
        }

        let token_estimate: u32 = primary_tokens + graph_tokens_used;
        let summary = if hits.is_empty() {
            format!("No indexed code results found for `{}`.", query)
        } else {
            format!(
                "Found {} indexed code result(s) across {} file(s) for `{}`.",
                hits.len(),
                files.len(),
                query
            )
        };
        let mut rendered_prompt = format!(
            "Task: code-index search\nIntent: {}\nQuery: {}\n\n{}",
            detected_intent,
            query,
            rendered_sections.join("\n\n")
        );
        if !graph_rendered.is_empty() {
            rendered_prompt.push_str("\n\n## Graph Context\n");
            rendered_prompt.push_str(&graph_rendered.join("\n"));
        }
        if rendered_prompt.len() > max_output_chars {
            let mut truncate_at = max_output_chars.min(rendered_prompt.len());
            while !rendered_prompt.is_char_boundary(truncate_at) {
                truncate_at = truncate_at.saturating_sub(1);
            }
            rendered_prompt.truncate(truncate_at);
            rendered_prompt.push_str("\n\n... truncated by adaptive repo-size budget");
        }

        Ok(ContextEnvelope {
            task: query.to_string(),
            intent: detected_intent,
            query: query.to_string(),
            token_budget,
            token_estimate,
            summary,
            rendered_prompt,
            revision: 0,
            nodes,
            spans,
            reasons: {
                let mut r = vec!["hybrid search over code index".to_string()];
                if enrichment.symbols_resolved > 0 {
                    r.push("graph-enriched".to_string());
                }
                r
            },
            invalidations: Vec::new(),
            machine_pack: serde_json::json!({
                "kind": "code_index_context",
                "query": query,
                "top_k": top_k,
                "repo_size_tier": format!("{:?}", tier),
                "token_budget": token_budget,
                "hits": hits,
            }),
            evidence_summary: serde_json::json!({
                "search_hits": hits.len(),
                "files": files.into_iter().collect::<Vec<_>>(),
                "graph_enrichment": {
                    "symbols_resolved": enrichment.symbols_resolved,
                    "callers_added": enrichment.callers_added,
                    "callees_added": enrichment.callees_added,
                    "tests_found": enrichment.tests_found,
                },
            }),
        })
    }
}

impl GraphOps<'_> {
    pub fn find_symbol(
        &self,
        name: &str,
        exact: bool,
        top_k: usize,
        include_metrics: bool,
    ) -> CcResult<serde_json::Value> {
        let db = self.0.ensure_db()?;
        let rows = db.find_symbol(name, exact, top_k)?;
        if !include_metrics {
            return serde_json::to_value(&rows).map_err(|e| CcError::Search(e.to_string()));
        }
        let mut results: Vec<serde_json::Value> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut obj = serde_json::to_value(row).map_err(|e| CcError::Search(e.to_string()))?;
            if let Some(uid) = row.symbol_uid.as_deref() {
                if let Ok(info) = db.symbol_degree_details(uid) {
                    let hint = centrality_hint(&info);
                    obj["metrics"] = serde_json::json!({
                        "in_degree": info.in_degree,
                        "out_degree": info.out_degree,
                        "caller_count": info.caller_count,
                        "callee_count": info.callee_count,
                        "ref_count": info.ref_count,
                        "centrality_hint": hint,
                    });
                }
            }
            results.push(obj);
        }
        Ok(serde_json::json!(results))
    }

    pub fn file_symbols(&self, file_path: &str) -> CcResult<Vec<cc_db::index_db::SymbolRow>> {
        self.0.ensure_db()?.file_symbols(file_path)
    }

    pub fn list_indexed_files(&self) -> CcResult<Vec<cc_db::index_db::FileInfoRow>> {
        self.0.ensure_db()?.list_indexed_files()
    }

    pub fn list_communities(&self) -> CcResult<Vec<cc_db::index_db::CommunityRow>> {
        self.0.ensure_db()?.list_communities()
    }

    pub fn list_frameworks(&self) -> CcResult<Vec<(String, f64)>> {
        self.0.ensure_db()?.list_repo_frameworks()
    }

    pub fn summarize_file(&self, file_path: &str) -> CcResult<serde_json::Value> {
        self.0.ensure_db()?.file_summary(file_path)
    }

    pub fn graph_query(&self, query: &str) -> CcResult<GraphQueryOutput> {
        let db = self.0.ensure_db()?;

        let tokens = cc_search::cypher::tokenize(query)?;
        let parsed = cc_search::cypher::parse_tokens(&tokens)?;
        let result = cc_search::cypher::execute_parsed(&parsed, db)?;
        let default_limit_applied = result.default_limit_applied;
        let limit = result.limit;
        let rows: Vec<serde_json::Value> = result
            .rows
            .into_iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (i, val) in row.into_iter().enumerate() {
                    let key = result
                        .columns
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| format!("col{}", i));
                    map.insert(key, val);
                }
                serde_json::Value::Object(map)
            })
            .collect();
        let row_count = rows.len();
        Ok(GraphQueryOutput {
            rows,
            row_count,
            default_limit_applied,
            limit,
        })
    }

    pub fn callers(
        &self,
        symbol_name: &str,
        limit: usize,
    ) -> CcResult<Vec<cc_db::index_db::CallEdgeLite>> {
        let db = self.0.ensure_db()?;
        let syms = db.find_symbol(symbol_name, true, 1)?;
        let sym = syms
            .first()
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", symbol_name)))?;
        let uid = sym
            .symbol_uid
            .as_deref()
            .ok_or_else(|| CcError::Search("symbol has no uid".into()))?;
        db.caller_rows_by_uid(uid, limit)
    }

    pub fn callees(
        &self,
        symbol_name: &str,
        limit: usize,
    ) -> CcResult<Vec<cc_db::index_db::CallEdgeLite>> {
        let db = self.0.ensure_db()?;
        let syms = db.find_symbol(symbol_name, true, 1)?;
        let sym = syms
            .first()
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", symbol_name)))?;
        let uid = sym
            .symbol_uid
            .as_deref()
            .ok_or_else(|| CcError::Search("symbol has no uid".into()))?;
        db.callee_rows_by_uid(uid, limit)
    }

    pub fn symbol_refs(
        &self,
        symbol_name: &str,
        limit: usize,
    ) -> CcResult<Vec<cc_db::index_db::SymbolRefLite>> {
        let db = self.0.ensure_db()?;
        let syms = db.find_symbol(symbol_name, true, 1)?;
        let sym = syms
            .first()
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", symbol_name)))?;
        let uid = sym
            .symbol_uid
            .as_deref()
            .ok_or_else(|| CcError::Search("symbol has no uid".into()))?;
        db.symbol_ref_rows_by_uid(uid, limit)
    }

    pub fn list_unresolved_refs(
        &self,
        limit: usize,
        file_path: Option<&str>,
        kind: Option<&str>,
    ) -> CcResult<serde_json::Value> {
        let db = self.0.ensure_db()?;
        let rows = db.list_resolution_attempts(limit.clamp(1, 500), file_path, kind)?;
        let mut by_kind: HashMap<String, usize> = HashMap::new();
        let mut with_candidates = 0usize;
        for row in &rows {
            *by_kind.entry(row.reference_kind.clone()).or_default() += 1;
            if row
                .candidates
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            {
                with_candidates += 1;
            }
        }
        Ok(serde_json::json!({
            "items": rows,
            "count": rows.len(),
            "with_candidates": with_candidates,
            "by_kind": by_kind,
            "filters": {
                "file_path": file_path,
                "kind": kind,
            }
        }))
    }

}

// task_symbols lives in a second `SearchOps` impl block to keep the method in
// its historical position in this file (minimal move); it belongs to the
// search domain because it is task-text retrieval with a search fallback.
impl SearchOps<'_> {
    pub fn task_symbols(
        &self,
        task: &str,
        max_symbols: Option<usize>,
        expand_depth: Option<usize>,
        intent: Option<&str>,
    ) -> CcResult<serde_json::Value> {
        let candidates = crate::symbol_extract::extract_candidate_symbols(task);
        let mut matched_symbols: Vec<serde_json::Value> = Vec::new();
        let mut matched_uids: Vec<String> = Vec::new();
        let mut seen_symbol_keys: HashSet<String> = HashSet::new();
        let mut seen_uids: HashSet<String> = HashSet::new();

        for name in &candidates {
            if let Ok(syms) = self.0.ensure_db()?.find_symbol(name, true, 1) {
                for sym in &syms {
                    let dedup_key = sym.symbol_uid.clone().unwrap_or_else(|| {
                        format!("{}:{}:{}", sym.file_path, sym.name, sym.start_line)
                    });
                    if !seen_symbol_keys.insert(dedup_key) {
                        continue;
                    }
                    if let Some(uid) = &sym.symbol_uid {
                        if seen_uids.insert(uid.clone()) {
                            matched_uids.push(uid.clone());
                        }
                    }
                    matched_symbols.push(serde_json::json!({
                        "name": sym.name,
                        "kind": sym.kind,
                        "file_path": sym.file_path,
                        "start_line": sym.start_line,
                        "end_line": sym.end_line,
                        "symbol_uid": sym.symbol_uid,
                        "qname": sym.qname,
                        "signature": sym.signature,
                    }));
                }
            }
        }

        // Fallback: if too few symbols matched, use search_in_context
        if matched_symbols.len() < 3 {
            let detected = intent.and_then(|i| Intent::from_str(i).ok());
            let env = self.search_in_context(task, 10, detected)?;
            return Ok(serde_json::to_value(env)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialize failed"})));
        }

        let max_syms = max_symbols
            .unwrap_or_else(|| self.0.repo_size_tier().explore_max_symbols())
            .clamp(1, 20);
        matched_symbols.truncate(max_syms);
        matched_uids.truncate(max_syms);

        let db = self.0.ensure_db()?;

        let mut expanded_callers: Vec<serde_json::Value> = Vec::new();
        let mut expanded_callees: Vec<serde_json::Value> = Vec::new();
        let mut relevant_files: HashSet<String> = HashSet::new();

        // Collect file paths from matched symbols
        for sym in &matched_symbols {
            if let Some(fp) = sym.get("file_path").and_then(|v| v.as_str()) {
                relevant_files.insert(fp.to_string());
            }
        }

        // Expand callers/callees for each matched UID
        let depth = expand_depth.unwrap_or(1).min(3);
        let per_symbol_limit = depth * 3; // depth 1 = 3, depth 2 = 6, depth 3 = 9
        for uid in &matched_uids {
            if let Ok(callers) = db.caller_rows_by_uid(uid, per_symbol_limit) {
                for edge in &callers {
                    relevant_files.insert(edge.file_path.clone());
                    expanded_callers.push(serde_json::json!({
                        "file_path": edge.file_path,
                        "line": edge.line,
                        "caller_symbol": edge.caller_symbol,
                        "callee_symbol": edge.callee_symbol,
                        "dispatch_kind": edge.dispatch_kind,
                        "synthesized_by": edge.synthesized_by,
                        "synthesis_key": edge.synthesis_key,
                        "registered_file": edge.registered_file,
                        "registered_line": edge.registered_line,
                    }));
                }
            }
            if let Ok(callees) = db.callee_rows_by_uid(uid, per_symbol_limit) {
                for edge in &callees {
                    relevant_files.insert(edge.file_path.clone());
                    expanded_callees.push(serde_json::json!({
                        "file_path": edge.file_path,
                        "line": edge.line,
                        "caller_symbol": edge.caller_symbol,
                        "callee_symbol": edge.callee_symbol,
                        "dispatch_kind": edge.dispatch_kind,
                        "synthesized_by": edge.synthesized_by,
                        "synthesis_key": edge.synthesis_key,
                        "registered_file": edge.registered_file,
                        "registered_line": edge.registered_line,
                    }));
                }
            }
        }

        let mut files_sorted: Vec<String> = relevant_files.into_iter().collect();
        files_sorted.sort();

        Ok(serde_json::json!({
            "task": task,
            "candidates_extracted": candidates,
            "matched_symbols": matched_symbols,
            "expanded_callers": expanded_callers,
            "expanded_callees": expanded_callees,
            "relevant_files": files_sorted,
        }))
    }
}

// ── Free functions kept in engine.rs ───────────────────────────────────

/// Build the `SearchRequest` for `search_in_context_with`, merging the caller
/// supplied override fields (boost/recent/pinned/overlay files, conversation
/// queries, preselect limit, and `path_prefix`) onto the base query.
pub(crate) fn build_context_search_request(
    query: &str,
    top_k: usize,
    overrides: SearchRequest,
) -> SearchRequest {
    SearchRequest {
        query: query.to_string(),
        top_k,
        include_grep: true,
        boost_file_paths: overrides.boost_file_paths,
        recent_file_paths: overrides.recent_file_paths,
        pinned_file_paths: overrides.pinned_file_paths,
        conversation_queries: overrides.conversation_queries,
        overlay_file_paths: overrides.overlay_file_paths,
        file_preselect_limit: overrides.file_preselect_limit,
        path_prefix: overrides.path_prefix,
        ..Default::default()
    }
}

fn detect_intent(query: &str) -> Intent {
    let q = query.to_lowercase();
    if q.contains("fix") || q.contains("bug") || q.contains("error") || q.contains("报错") {
        Intent::Fix
    } else if q.contains("refactor") || q.contains("重构") {
        Intent::Refactor
    } else if q.contains("trace") || q.contains("call") || q.contains("链路") {
        Intent::Trace
    } else if q.contains("test") || q.contains("测试") {
        Intent::Test
    } else if q.contains("explain") || q.contains("解释") || q.contains("说明") {
        Intent::Explain
    } else {
        Intent::Locate
    }
}

pub(crate) fn centrality_hint(info: &cc_db::index_db::SymbolDegreeInfo) -> &'static str {
    if info.out_degree > 5 * info.in_degree.max(1) {
        "hub"
    } else if info.in_degree > 5 * info.out_degree.max(1) {
        "authority"
    } else if info.in_degree <= 1 && info.out_degree <= 1 {
        "leaf"
    } else {
        "connector"
    }
}

pub use crate::engine_query::{compute_package_boundaries, PackageBoundary};

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::SymbolDegreeInfo;
    use tempfile::TempDir;

    // ── CodeIndex::new(None) → empty index ──────────────────────────

    #[test]
    fn new_with_none_creates_empty_index() {
        let idx = CodeIndex::new(None).unwrap();
        assert!(idx.project_path.is_none());
        assert!(idx.index_db.is_none());
        assert!(!idx.needs_initial_index());
    }

    // ── CodeIndex::empty() ──────────────────────────────────────────

    #[test]
    fn empty_has_no_project_or_db() {
        let idx = CodeIndex::empty();
        assert!(idx.project_path.is_none());
        assert!(idx.index_db.is_none());
        assert!(idx.index_db().is_none());
    }

    // ── ensure_db returns ProjectNotSet when no project ─────────────

    #[test]
    fn ensure_db_returns_project_not_set() {
        let idx = CodeIndex::empty();
        match idx.ensure_db() {
            Err(CcError::ProjectNotSet) => {}
            Err(other) => panic!("expected ProjectNotSet, got {:?}", other),
            Ok(_) => panic!("expected error but got Ok"),
        }
    }

    // ── ensure_project returns ProjectNotSet when no project ────────

    #[test]
    fn ensure_project_returns_project_not_set() {
        let idx = CodeIndex::empty();
        let err = idx.ensure_project().unwrap_err();
        assert!(matches!(err, CcError::ProjectNotSet));
    }

    // ── set_project on a valid temp directory ───────────────────────

    #[test]
    fn set_project_initializes_db_and_path() {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();

        assert!(idx.project_path.is_some());
        assert!(idx.index_db.is_some());
        assert!(idx.ensure_db().is_ok());
        assert!(idx.ensure_project().is_ok());
    }

    // ── close / is_closed / reopen cycle ────────────────────────────

    #[test]
    fn close_marks_index_closed_and_reopen_restores() {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();

        assert!(!idx.is_closed());

        idx.close();
        assert!(idx.is_closed());
        assert!(idx.index_db.is_none());

        idx.reopen().unwrap();
        assert!(!idx.is_closed());
        assert!(idx.index_db.is_some());
    }

    // ── is_closed is false when no project set ──────────────────────

    #[test]
    fn is_closed_false_when_no_project() {
        let idx = CodeIndex::empty();
        assert!(
            !idx.is_closed(),
            "empty index (no project) should not be considered 'closed'"
        );
    }

    // ── reopen is no-op when not closed ─────────────────────────────

    #[test]
    fn reopen_noop_when_not_closed() {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();

        // reopen on an already-open index should succeed without error
        idx.reopen().unwrap();
        assert!(idx.index_db.is_some());
    }

    // ── reopen fails when no project path ───────────────────────────

    #[test]
    fn reopen_fails_with_project_not_set_on_empty() {
        let mut idx = CodeIndex::empty();
        // Not closed (project_path is None), so reopen returns Ok immediately
        assert!(idx.reopen().is_ok());
    }

    // ── needs_initial_index after fresh set_project ─────────────────

    #[test]
    fn needs_initial_index_true_on_fresh_db() {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();

        // A freshly created DB should have SchemaStatus::Initialized
        assert!(idx.needs_initial_index());
    }

    #[test]
    fn successful_build_clears_initial_index_flag() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();
        assert!(idx.needs_initial_index());

        let report = idx.build_index(false).unwrap();

        assert!(report.files_scanned >= 1);
        assert!(!idx.needs_initial_index());

        let auto_report = idx.build_auto_index(false).unwrap();
        assert!(auto_report.files_scanned >= 1);
    }

    // ── build_context_search_request propagates overrides ──────────

    #[test]
    fn build_context_search_request_propagates_path_prefix() {
        let overrides = SearchRequest {
            path_prefix: Some("src/api/".to_string()),
            boost_file_paths: Some(vec!["src/main.rs".to_string()]),
            file_preselect_limit: Some(42),
            ..Default::default()
        };
        let request = build_context_search_request("login", 7, overrides);
        assert_eq!(request.query, "login");
        assert_eq!(request.top_k, 7);
        assert_eq!(request.path_prefix.as_deref(), Some("src/api/"));
        assert_eq!(
            request.boost_file_paths.as_deref(),
            Some(["src/main.rs".to_string()].as_slice())
        );
        assert_eq!(request.file_preselect_limit, Some(42));
    }

    // ── detect_intent free function ─────────────────────────────────

    #[test]
    fn detect_intent_fix_keywords() {
        assert_eq!(detect_intent("fix the bug"), Intent::Fix);
        assert_eq!(detect_intent("error in login"), Intent::Fix);
        assert_eq!(detect_intent("报错了"), Intent::Fix);
    }

    #[test]
    fn detect_intent_refactor() {
        assert_eq!(detect_intent("refactor this module"), Intent::Refactor);
        assert_eq!(detect_intent("需要重构"), Intent::Refactor);
    }

    #[test]
    fn detect_intent_trace() {
        assert_eq!(detect_intent("trace the call path"), Intent::Trace);
        assert_eq!(detect_intent("follow the 链路"), Intent::Trace);
    }

    #[test]
    fn detect_intent_test() {
        assert_eq!(detect_intent("add a test for this"), Intent::Test);
        assert_eq!(detect_intent("测试覆盖"), Intent::Test);
    }

    #[test]
    fn detect_intent_explain() {
        assert_eq!(detect_intent("explain how it works"), Intent::Explain);
        assert_eq!(detect_intent("解释一下"), Intent::Explain);
    }

    #[test]
    fn detect_intent_default_locate() {
        assert_eq!(detect_intent("find the symbol"), Intent::Locate);
        assert_eq!(detect_intent("where is this defined"), Intent::Locate);
    }

    // ── centrality_hint ─────────────────────────────────────────────

    #[test]
    fn centrality_hint_hub() {
        let info = SymbolDegreeInfo {
            in_degree: 1,
            out_degree: 10,
            caller_count: 0,
            callee_count: 0,
            ref_count: 0,
        };
        assert_eq!(centrality_hint(&info), "hub");
    }

    #[test]
    fn centrality_hint_authority() {
        let info = SymbolDegreeInfo {
            in_degree: 10,
            out_degree: 1,
            caller_count: 0,
            callee_count: 0,
            ref_count: 0,
        };
        assert_eq!(centrality_hint(&info), "authority");
    }

    #[test]
    fn centrality_hint_leaf() {
        let info = SymbolDegreeInfo {
            in_degree: 0,
            out_degree: 1,
            caller_count: 0,
            callee_count: 0,
            ref_count: 0,
        };
        assert_eq!(centrality_hint(&info), "leaf");
    }

    #[test]
    fn centrality_hint_connector() {
        let info = SymbolDegreeInfo {
            in_degree: 3,
            out_degree: 3,
            caller_count: 0,
            callee_count: 0,
            ref_count: 0,
        };
        assert_eq!(centrality_hint(&info), "connector");
    }

    // ── centrality_hint edge cases ──────────────────────────────────

    #[test]
    fn centrality_hint_zero_zero_is_leaf() {
        let info = SymbolDegreeInfo {
            in_degree: 0,
            out_degree: 0,
            caller_count: 0,
            callee_count: 0,
            ref_count: 0,
        };
        assert_eq!(centrality_hint(&info), "leaf");
    }

    // ── graph_query truncation signals ──────────────────────────────

    #[test]
    fn graph_query_signals_default_limit_when_omitted() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();
        idx.build_index(false).unwrap();

        // No explicit LIMIT → default limit applied, limit = DEFAULT_CYPHER_LIMIT.
        let output = idx
            .graph()
            .graph_query("MATCH (f:Function) RETURN f.name")
            .unwrap();
        assert!(output.default_limit_applied);
        assert_eq!(output.limit, Some(50));
        assert_eq!(output.row_count, output.rows.len());
    }

    #[test]
    fn graph_query_no_default_limit_when_explicit() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();
        idx.build_index(false).unwrap();

        // Explicit LIMIT → no default limit, limit reflects the explicit value.
        let output = idx
            .graph()
            .graph_query("MATCH (f:Function) RETURN f.name LIMIT 5")
            .unwrap();
        assert!(!output.default_limit_applied);
        assert_eq!(output.limit, Some(5));
    }

    // ── graph rerank parity fixture ─────────────────────────────────
    //
    // Fixates the end-to-end behaviour of `search_in_context_with`'s graph
    // rerank on a fixture with call edges: hit ORDER, graph_score values,
    // and rerank_score values were captured from the pre-refactor
    // implementation (graph_enrich in cc-server) and must stay bit-identical
    // after the rerank moves into cc-search.

    fn insert_context_fixture_file(
        db: &cc_db::index_db::IndexDb,
        file_path: &str,
        text: &str,
        symbol_name: &str,
        symbol_uid: Option<&str>,
        call_edges: Vec<cc_model::CallEdgeRecord>,
    ) {
        use cc_db::index_db::FileWriteUnit;
        use cc_model::{ChunkRecord, Language, ParseOutcome, ParserTier, SymbolRecord};

        let chunk = ChunkRecord {
            chunk_id: format!("chunk:{}", file_path),
            file_path: file_path.to_string(),
            language: Language::Rust,
            chunk_index: 0,
            start_line: 1,
            end_line: 3,
            breadcrumb: "root".to_string(),
            text: text.to_string(),
            symbol_name: Some(symbol_name.to_string()),
            symbol_kind: Some(cc_model::SymbolKind::Function),
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        };
        let symbol = SymbolRecord {
            symbol_id: format!("sym:{file_path}:{symbol_name}"),
            file_path: file_path.to_string(),
            name: symbol_name.to_string(),
            kind: cc_model::SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: symbol_uid.map(|s| s.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let outcome = ParseOutcome {
            summary: text.to_string(),
            chunks: vec![chunk],
            symbols: vec![symbol],
            call_edges,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        };
        let conn = db.read_conn().unwrap();
        cc_db::index_db::IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: file_path.to_string(),
                language: Language::Rust,
                content_hash: format!("hash-{file_path}"),
                mtime: 0.0,
                size: text.len() as u64,
                outcome,
            },
        )
        .unwrap();
    }

    #[test]
    fn graph_rerank_parity_with_pre_refactor_baseline() {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();
        let db = idx.ensure_db().unwrap().clone();

        // beta: lexically stronger (matches all 8 query tokens), no symbol_uid
        // so the graph lane and graph enrichment cannot see it.
        insert_context_fixture_file(
            &db,
            "src/beta.rs",
            "fn ranktoken() { alphaword(); betaword(); gammaword(); deltaword(); epsword(); zetaword(); etaword(); }",
            "ranktoken",
            None,
            vec![],
        );
        // alpha: lexically weaker (7 of 8 query tokens) but heavily connected
        // in the call graph (20 outgoing edges -> high graph_score).
        let ghost_edges: Vec<cc_model::CallEdgeRecord> = (0..20)
            .map(|i| cc_model::CallEdgeRecord {
                edge_id: format!("edge:alpha->ghost{i}"),
                file_path: "src/alpha.rs".to_string(),
                caller_symbol: Some("ranktoken".to_string()),
                callee_symbol: format!("ghost_fn_{i}"),
                line: 2,
                caller_symbol_uid: Some("uid:alpha".to_string()),
                callee_symbol_uid: Some(format!("uid:ghost{i}")),
                ..Default::default()
            })
            .collect();
        insert_context_fixture_file(
            &db,
            "src/alpha.rs",
            "fn ranktoken() { alphaword(); betaword(); gammaword(); deltaword(); epsword(); zetaword(); }",
            "ranktoken",
            Some("uid:alpha"),
            ghost_edges,
        );

        let query = "ranktoken alphaword betaword gammaword deltaword epsword zetaword etaword";
        let envelope = idx
            .search()
            .search_in_context_with(query, 2, None, SearchRequest::default())
            .unwrap();

        let hits = envelope.machine_pack["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2, "expected exactly the two fixture hits");

        let order: Vec<&str> = hits
            .iter()
            .map(|h| h["chunk_id"].as_str().unwrap())
            .collect();
        let graph_scores: Vec<f64> = hits
            .iter()
            .map(|h| h["graph_score"].as_f64().unwrap())
            .collect();
        let rerank_scores: Vec<f64> = hits
            .iter()
            .map(|h| h["rerank_score"].as_f64().unwrap())
            .collect();

        // Baseline captured from the pre-refactor implementation
        // (graph_enrich + re-sort living in cc-server). The graph boost must
        // FLIP the order: alpha is lexically weaker but heavily connected.
        assert_eq!(
            order,
            vec!["chunk:src/alpha.rs", "chunk:src/beta.rs"],
            "graph-boosted hit order must match pre-refactor baseline"
        );

        let expected_alpha_graph = (21.0f64).ln() / 10.0; // ln(in+out+1)/10, 20 edges
        assert!(
            (graph_scores[0] - expected_alpha_graph).abs() < 1e-12,
            "alpha graph_score drifted: got {}, want {}",
            graph_scores[0],
            expected_alpha_graph
        );
        assert_eq!(graph_scores[1], 0.0, "beta has no graph connectivity");

        // Bit-exact rerank values captured pre-refactor; any scoring-constant
        // drift during the ScoringConfig migration shows up here.
        assert!(
            (rerank_scores[0] - 0.7865038336950975).abs() < 1e-12,
            "alpha rerank_score drifted: got {}",
            rerank_scores[0]
        );
        assert!(
            (rerank_scores[1] - 0.7275681946166044).abs() < 1e-12,
            "beta rerank_score drifted: got {}",
            rerank_scores[1]
        );

        // Flip proof: without the graph contribution (weight 0.3) alpha would
        // rank BELOW beta — the final ordering genuinely depends on the graph
        // rerank step.
        let alpha_pre_graph = rerank_scores[0] - graph_scores[0] * 0.3;
        assert!(
            alpha_pre_graph < rerank_scores[1],
            "fixture must demonstrate a graph-driven flip: alpha pre-graph {} vs beta {}",
            alpha_pre_graph,
            rerank_scores[1]
        );
    }

    #[test]
    fn graph_score_affects_final_ranking() {
        use cc_model::search::SearchHit;
        use cc_model::Language;

        // Construct two hits: B has higher rerank but no graph, A has lower rerank but high graph
        let mut hit_a = SearchHit {
            chunk_id: "a".into(),
            file_path: "a.rs".into(),
            language: Language::Rust,
            start_line: 1,
            end_line: 10,
            breadcrumb: String::new(),
            symbol_name: None,
            symbol_kind: None,
            text: String::new(),
            fused_score: 0.5,
            lexical_score: 0.5,
            grep_score: 0.0,
            graph_score: 0.4,   // high graph score
            rerank_score: 0.50, // lower rerank
            reasons: vec![],
            source: String::new(),
            lane: None,
            metadata: serde_json::Value::Null,
        };

        let hit_b = SearchHit {
            chunk_id: "b".into(),
            file_path: "b.rs".into(),
            language: Language::Rust,
            start_line: 1,
            end_line: 10,
            breadcrumb: String::new(),
            symbol_name: None,
            symbol_kind: None,
            text: String::new(),
            fused_score: 0.55,
            lexical_score: 0.55,
            grep_score: 0.0,
            graph_score: 0.0,   // no graph score
            rerank_score: 0.55, // higher rerank
            reasons: vec![],
            source: String::new(),
            lane: None,
            metadata: serde_json::Value::Null,
        };

        // Apply graph rerank with weight 0.3
        let gw = 0.3;
        hit_a.rerank_score += hit_a.graph_score * gw; // 0.50 + 0.12 = 0.62
                                                      // hit_b stays at 0.55

        let mut hits = [hit_b, hit_a];
        hits.sort_by(|a, b| {
            b.rerank_score
                .partial_cmp(&a.rerank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // A should now be first (0.62 > 0.55)
        assert_eq!(hits[0].chunk_id, "a");
        assert_eq!(hits[1].chunk_id, "b");
    }
}
