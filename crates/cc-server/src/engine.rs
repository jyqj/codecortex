//! Lightweight code-index engine for cc-server.
//!
//! Thin wrapper around cc-db, cc-index and cc-search.

use cc_db::index_db::IndexDb;
use cc_index::{IndexReport, Indexer};
use cc_model::config::{
    load_project_config, IndexPaths, ProjectConfig, ProjectStats, RepoSizeTier,
};
use cc_model::context::{ContextEnvelope, ContextNode, ContextSpan, NodeType, Role};
use cc_model::search::SearchRequest;
use cc_model::{CcError, CcResult, Intent};
use cc_search::SearchEngine;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

impl CodeIndex {
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

        let (db, schema_status) = IndexDb::open(&paths.index_db)?;
        let db = Arc::new(db);
        let engine = SearchEngine::new(db.clone(), &config);

        self.project_path = Some(project);
        self.config = Some(config);
        self.index_db = Some(db);
        self.engine = Some(engine);
        self.repo_tier = None;
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
        self.repo_tier = None;
        self.needs_initial_index = false;
        Ok(report)
    }

    pub fn build_auto_index(&mut self, full: bool) -> CcResult<IndexReport> {
        let project = self.ensure_project()?;
        let config = self.ensure_config()?;
        let db = self.ensure_db()?;
        let indexer = Indexer::new(db.clone(), project, &config.indexing);
        let report = indexer.build_auto_index(project, full, config.auto_index.file_limit)?;
        self.repo_tier = None;
        self.needs_initial_index = false;
        Ok(report)
    }

    pub fn index_status(&self) -> CcResult<ProjectStats> {
        let project = self.ensure_project()?;
        self.ensure_db()?.stats(project)
    }

    pub fn search_in_context(
        &mut self,
        query: &str,
        top_k: usize,
        intent: Option<Intent>,
    ) -> CcResult<ContextEnvelope> {
        self.search_in_context_with(query, top_k, intent, SearchRequest::default())
    }

    pub fn search_in_context_with(
        &mut self,
        query: &str,
        top_k: usize,
        intent: Option<Intent>,
        overrides: SearchRequest,
    ) -> CcResult<ContextEnvelope> {
        let tier = self.repo_size_tier();
        let token_budget = tier.default_token_budget();
        let max_output_chars = tier.max_output_chars();
        let top_k = if top_k == 0 {
            tier.search_top_k()
        } else {
            top_k
        };
        let engine = self.ensure_engine()?;
        let detected_intent = intent.unwrap_or_else(|| detect_intent(query));
        let request = SearchRequest {
            query: query.to_string(),
            top_k,
            include_grep: true,
            boost_file_paths: overrides.boost_file_paths,
            recent_file_paths: overrides.recent_file_paths,
            pinned_file_paths: overrides.pinned_file_paths,
            file_preselect_limit: overrides.file_preselect_limit,
            ..Default::default()
        };
        let hits = engine.search(&request)?;

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

        let token_estimate: u32 = nodes.iter().map(|n| n.token_estimate).sum();
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
            reasons: vec!["hybrid search over code index".to_string()],
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
            }),
        })
    }

    pub fn find_symbol(
        &self,
        name: &str,
        exact: bool,
        top_k: usize,
        include_metrics: bool,
    ) -> CcResult<serde_json::Value> {
        let db = self.ensure_db()?;
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
        self.ensure_db()?.file_symbols(file_path)
    }

    pub fn list_indexed_files(&self) -> CcResult<Vec<cc_db::index_db::FileInfoRow>> {
        self.ensure_db()?.list_indexed_files()
    }

    pub fn list_communities(&self) -> CcResult<Vec<cc_db::index_db::CommunityRow>> {
        self.ensure_db()?.list_communities()
    }

    pub fn list_frameworks(&self) -> CcResult<Vec<(String, f64)>> {
        self.ensure_db()?.list_repo_frameworks()
    }

    pub fn summarize_file(&self, file_path: &str) -> CcResult<serde_json::Value> {
        self.ensure_db()?.file_summary(file_path)
    }

    pub fn graph_query(&self, query: &str) -> CcResult<Vec<serde_json::Value>> {
        let db = self.ensure_db()?;

        // Try the new Cypher parser first. Only fall back to the legacy
        // GraphQueryEngine when the *parser* cannot handle the syntax
        // (tokenize or parse failure). Execution errors (SQL translation,
        // runtime, validation) are propagated directly — falling back on
        // those would silently hide real bugs and bypass safety checks in
        // the new engine.
        let tokens = match cc_search::cypher::tokenize(query) {
            Ok(t) => t,
            Err(_) => return cc_search::GraphQueryEngine::new(db.clone()).execute(query),
        };
        let has_union = tokens.iter().any(|t| {
            matches!(
                t,
                cc_search::cypher::Token::Union | cc_search::cypher::Token::UnionAll
            )
        });
        let parse_ok = if has_union {
            cc_search::cypher::parse_union(&tokens).is_ok()
        } else {
            cc_search::cypher::parse(&tokens).is_ok()
        };
        if !parse_ok {
            return cc_search::GraphQueryEngine::new(db.clone()).execute(query);
        }

        // Parse succeeded — execute with the new engine and propagate errors.
        let result = cc_search::cypher::cypher_query(query, db)?;
        let maps = result
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
        Ok(maps)
    }

    pub fn callers(
        &self,
        symbol_name: &str,
        limit: usize,
    ) -> CcResult<Vec<cc_db::index_db::CallEdgeLite>> {
        let db = self.ensure_db()?;
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
        let db = self.ensure_db()?;
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
        let db = self.ensure_db()?;
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
        let db = self.ensure_db()?;
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

    pub fn find_impacted_tests(&self, files: &[String]) -> CcResult<Vec<String>> {
        self.ensure_db()?.find_impacted_tests(files)
    }

    pub fn task_symbols(
        &mut self,
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
            if let Ok(syms) = self.ensure_db()?.find_symbol(name, true, 1) {
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
            let detected = intent.and_then(|i| match i {
                "fix" => Some(Intent::Fix),
                "refactor" => Some(Intent::Refactor),
                "trace" => Some(Intent::Trace),
                "locate" => Some(Intent::Locate),
                "test" => Some(Intent::Test),
                "explain" => Some(Intent::Explain),
                _ => None,
            });
            let env = self.search_in_context(task, 10, detected)?;
            return Ok(serde_json::to_value(env)
                .unwrap_or_else(|_| serde_json::json!({"error": "serialize failed"})));
        }

        let max_syms = max_symbols
            .unwrap_or_else(|| self.repo_size_tier().explore_max_symbols())
            .clamp(1, 20);
        matched_symbols.truncate(max_syms);
        matched_uids.truncate(max_syms);

        let db = self.ensure_db()?;

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

// NOTE: The following items have been moved to engine_query.rs:
// - detect_impact, analyze_impact, repo_size_tier, output_budget
// - explore_symbols, get_symbol_source, compute_package_boundaries (method)
// - graph_schema, compute_edge_provenance, compute_runtime_evidence
// - slice_lines, PackageBoundary, PackageLayer, extract_package
// - compute_package_boundaries (free fn), compute_package_layers

// Re-export types that were previously defined here
pub use crate::engine_query::{compute_package_boundaries, compute_package_layers};
pub use crate::engine_query::{PackageBoundary, PackageLayer};

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
}
