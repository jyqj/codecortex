//! Lightweight code-index engine for cc-server.
//!
//! Thin wrapper around cc-db, cc-index and cc-search.

use cc_db::index_db::IndexDb;
use cc_index::{IndexReport, Indexer};
use cc_model::config::{
    load_project_config, IndexPaths, ProjectConfig, ProjectStats, RepoSizeTier,
};
use cc_model::context::{ContextEnvelope, ContextNode, ContextSpan, NodeType, Role};
use cc_model::impact::ImpactReport;
use cc_model::search::SearchRequest;
use cc_model::{CcError, CcResult, Intent};
use cc_search::SearchEngine;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::impact::ImpactAnalyzer;

pub struct CodeIndex {
    pub project_path: Option<PathBuf>,
    config: Option<ProjectConfig>,
    index_db: Option<Arc<IndexDb>>,
    engine: Option<SearchEngine>,
    repo_tier: Option<RepoSizeTier>,
}

impl CodeIndex {
    pub fn new(project_path: Option<&Path>) -> CcResult<Self> {
        let mut index = Self {
            project_path: None,
            config: None,
            index_db: None,
            engine: None,
            repo_tier: None,
        };
        if let Some(path) = project_path {
            index.set_project(path, false)?;
        }
        Ok(index)
    }

    pub fn set_project(&mut self, path: &Path, auto_index: bool) -> CcResult<()> {
        let project = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let config = load_project_config(&project);
        let paths = IndexPaths::new(&project);
        std::fs::create_dir_all(&paths.workdir)?;
        std::fs::create_dir_all(&paths.logs_dir)?;

        let db = Arc::new(IndexDb::open(&paths.index_db)?);
        let engine = SearchEngine::new(db.clone(), &config);

        self.project_path = Some(project);
        self.config = Some(config);
        self.index_db = Some(db);
        self.engine = Some(engine);
        self.repo_tier = None;

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

    fn ensure_project(&self) -> CcResult<&Path> {
        self.project_path.as_deref().ok_or(CcError::ProjectNotSet)
    }

    fn ensure_db(&self) -> CcResult<&Arc<IndexDb>> {
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
        Ok(report)
    }

    pub fn build_auto_index(&mut self, full: bool) -> CcResult<IndexReport> {
        let project = self.ensure_project()?;
        let config = self.ensure_config()?;
        let db = self.ensure_db()?;
        let indexer = Indexer::new(db.clone(), project, &config.indexing);
        let report = indexer.build_auto_index(project, full, config.auto_index.file_limit)?;
        self.repo_tier = None;
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
            let mut node = ContextNode::new(
                node_id.clone(),
                NodeType::SearchHit,
                Role::Primary,
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
    ) -> CcResult<Vec<cc_db::index_db::SymbolRow>> {
        self.ensure_db()?.find_symbol(name, exact, top_k)
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
        match cc_search::cypher::cypher_query(query, db) {
            Ok(result) => {
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
            Err(_) => cc_search::GraphQueryEngine::new(db.clone()).execute(query),
        }
    }

    #[allow(dead_code)]
    pub fn cypher_query(&self, query: &str) -> CcResult<cc_search::cypher::CypherResult> {
        cc_search::cypher::cypher_query(query, self.ensure_db()?)
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

    pub fn trace_path(&self, from: &str, to: &str, max_depth: usize) -> CcResult<Vec<Vec<String>>> {
        let db = self.ensure_db()?;
        let from_uid = db
            .find_symbol(from, true, 1)?
            .first()
            .and_then(|s| s.symbol_uid.clone())
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", from)))?;
        let to_uid = db
            .find_symbol(to, true, 1)?
            .first()
            .and_then(|s| s.symbol_uid.clone())
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", to)))?;

        let uid_names = db.symbol_names_by_uid()?;
        let all_edges = db.call_uid_edges()?;
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (caller, callee) in &all_edges {
            adj.entry(caller.clone()).or_default().push(callee.clone());
        }

        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back(vec![from_uid.clone()]);
        visited.insert(from_uid);
        let mut paths = Vec::new();

        while let Some(path) = queue.pop_front() {
            if path.len() > max_depth + 1 {
                break;
            }
            let current = path.last().expect("path has at least one uid");
            if *current == to_uid {
                let named: Vec<String> = path
                    .iter()
                    .map(|uid| uid_names.get(uid).cloned().unwrap_or_else(|| uid.clone()))
                    .collect();
                paths.push(named);
                continue;
            }
            if let Some(neighbors) = adj.get(current) {
                for next in neighbors {
                    if !visited.contains(next) {
                        visited.insert(next.clone());
                        let mut new_path = path.clone();
                        new_path.push(next.clone());
                        queue.push_back(new_path);
                    }
                }
            }
        }
        Ok(paths)
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
        let rows = db.list_resolution_attempts(limit.max(1).min(500), file_path, kind)?;
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
            if let Ok(syms) = self.find_symbol(name, true, 1) {
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
            .max(1)
            .min(20);
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

    pub fn detect_impact(&self, changed_files: &[String]) -> CcResult<ImpactReport> {
        ImpactAnalyzer::new(self.ensure_db()?.clone()).analyze(changed_files, 3)
    }

    pub fn analyze_impact(&self, base_ref: Option<&str>) -> CcResult<ImpactReport> {
        let project = self.ensure_project()?;
        let analyzer = ImpactAnalyzer::new(self.ensure_db()?.clone())
            .with_project_root(project.display().to_string());
        let changed = analyzer.git_changed_files(base_ref);
        analyzer.analyze(&changed, 3)
    }

    pub fn repo_size_tier(&mut self) -> RepoSizeTier {
        if let Some(tier) = self.repo_tier {
            return tier;
        }
        let count = self.index_status().map(|s| s.indexed_files).unwrap_or(0);
        let tier = RepoSizeTier::from_file_count(count);
        self.repo_tier = Some(tier);
        tier
    }

    pub fn explore_symbols(
        &mut self,
        names: &[String],
        max_callers: Option<usize>,
        max_callees: Option<usize>,
        include_source: bool,
        include_relations: bool,
    ) -> CcResult<serde_json::Value> {
        let capped = if names.len() > 10 {
            &names[..10]
        } else {
            names
        };
        let tier = self.repo_size_tier();
        let caller_limit = max_callers.unwrap_or(tier.explore_max_symbols());
        let callee_limit = max_callees.unwrap_or(tier.explore_max_symbols());
        let max_src_chars = tier.max_source_chars_per_symbol();

        let mut results = Vec::with_capacity(capped.len());

        for name in capped {
            let db = self.ensure_db()?;
            // Exact match first, fuzzy fallback
            let mut syms = db.find_symbol(name, true, 3)?;
            if syms.is_empty() {
                syms = db.find_symbol(name, false, 3)?;
            }
            if syms.is_empty() {
                results.push(serde_json::json!({
                    "query": name,
                    "error": "symbol not found",
                }));
                continue;
            }
            if syms.len() > 1 {
                let candidates: Vec<serde_json::Value> = syms
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "name": s.name,
                            "kind": s.kind,
                            "file_path": s.file_path,
                            "qname": s.qname,
                        })
                    })
                    .collect();
                results.push(serde_json::json!({
                    "query": name,
                    "candidates": candidates,
                }));
                continue;
            }

            let sym = &syms[0];
            let uid = match sym.symbol_uid.as_deref() {
                Some(u) => u,
                None => {
                    results.push(serde_json::json!({
                        "query": name,
                        "error": "symbol has no uid",
                    }));
                    continue;
                }
            };

            let mut entry = serde_json::json!({
                "query": name,
                "symbol": {
                    "name": sym.name,
                    "kind": sym.kind,
                    "file_path": sym.file_path,
                    "start_line": sym.start_line,
                    "end_line": sym.end_line,
                    "qname": sym.qname,
                    "signature": sym.signature,
                },
            });

            // Callers
            let callers = self.ensure_db()?.caller_rows_by_uid(uid, caller_limit)?;
            let callers_json: Vec<serde_json::Value> = callers
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "caller": c.caller_symbol,
                        "file_path": c.file_path,
                        "line": c.line,
                        "dispatch_kind": c.dispatch_kind,
                        "synthesized_by": c.synthesized_by,
                        "synthesis_key": c.synthesis_key,
                        "registered_file": c.registered_file,
                        "registered_line": c.registered_line,
                    })
                })
                .collect();
            entry["callers"] = serde_json::json!(callers_json);

            // Callees
            let callees = self.ensure_db()?.callee_rows_by_uid(uid, callee_limit)?;
            let callees_json: Vec<serde_json::Value> = callees
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "callee": c.callee_symbol,
                        "file_path": c.file_path,
                        "line": c.line,
                        "dispatch_kind": c.dispatch_kind,
                        "synthesized_by": c.synthesized_by,
                        "synthesis_key": c.synthesis_key,
                        "registered_file": c.registered_file,
                        "registered_line": c.registered_line,
                    })
                })
                .collect();
            entry["callees"] = serde_json::json!(callees_json);

            // Source — use strict path guard to only read indexed files
            if include_source {
                if let (Some(project), Some(db)) =
                    (self.project_path.as_ref(), self.index_db.as_ref())
                {
                    match crate::path_guard::resolve_indexed_path_strict(
                        project,
                        &sym.file_path,
                        db,
                    ) {
                        Ok(full_path) => {
                            if let Ok(content) = std::fs::read_to_string(&full_path) {
                                let lines: Vec<&str> = content.lines().collect();
                                let start = (sym.start_line as usize).saturating_sub(1);
                                let end = (sym.end_line as usize).min(lines.len());
                                if start < end {
                                    let mut source = lines[start..end].join("\n");
                                    if source.len() > max_src_chars {
                                        let mut truncate_at = max_src_chars.min(source.len());
                                        while !source.is_char_boundary(truncate_at) {
                                            truncate_at = truncate_at.saturating_sub(1);
                                        }
                                        source.truncate(truncate_at);
                                        source.push_str("\n// ... truncated");
                                    }
                                    entry["source"] = serde_json::json!(source);
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }

            // Semantic relations
            if include_relations {
                let db = self.ensure_db()?;
                let mut relations = Vec::new();
                if let Ok(edges) = db.query_semantic_edges(Some(uid), None, None) {
                    for edge in &edges {
                        relations.push(serde_json::json!({
                            "direction": "outgoing",
                            "relation": format!("{:?}", edge.relation_kind),
                            "target": edge.target_symbol,
                            "target_uid": edge.target_symbol_uid,
                            "file_path": edge.file_path,
                            "line": edge.line,
                        }));
                    }
                }
                if let Ok(edges) = db.query_semantic_edges(None, Some(uid), None) {
                    for edge in &edges {
                        relations.push(serde_json::json!({
                            "direction": "incoming",
                            "relation": format!("{:?}", edge.relation_kind),
                            "source": edge.source_symbol,
                            "source_uid": edge.source_symbol_uid,
                            "file_path": edge.file_path,
                            "line": edge.line,
                        }));
                    }
                }
                entry["relations"] = serde_json::json!(relations);
            }

            results.push(entry);
        }

        Ok(serde_json::json!(results))
    }

    /// Read exact source for one symbol by short name or qualified name.
    ///
    /// This is the LLM-friendly single-step path: it avoids the usual
    /// find_symbol -> read_file -> slice dance and preserves indexed path
    /// boundaries via `path_guard`.
    pub fn get_symbol_source(
        &mut self,
        symbol: &str,
        exact: bool,
        include_line_numbers: bool,
        max_chars: Option<usize>,
    ) -> CcResult<serde_json::Value> {
        let project = self.ensure_project()?.to_path_buf();
        let db = self.ensure_db()?.clone();
        let tier = self.repo_size_tier();
        let max_src_chars = max_chars.unwrap_or_else(|| tier.max_source_chars_per_symbol());

        let rows = if exact {
            db.query_json(
                "SELECT name, kind, file_path, container, start_line, end_line, qname, signature, symbol_uid
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1
                 ORDER BY CASE WHEN qname = ?1 THEN 0 WHEN name = ?1 THEN 1 ELSE 2 END, file_path, start_line
                 LIMIT 8",
                &[symbol.to_string()],
            )?
        } else {
            let pat = format!("%{}%", symbol);
            db.query_json(
                "SELECT name, kind, file_path, container, start_line, end_line, qname, signature, symbol_uid
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1 OR qname LIKE ?2 OR name LIKE ?2
                 ORDER BY CASE WHEN qname = ?1 THEN 0 WHEN name = ?1 THEN 1 WHEN qname LIKE ?2 THEN 2 ELSE 3 END, file_path, start_line
                 LIMIT 8",
                &[symbol.to_string(), pat],
            )?
        };

        if rows.is_empty() {
            return Ok(serde_json::json!({
                "query": symbol,
                "error": "symbol not found",
                "exact": exact,
            }));
        }
        if rows.len() > 1 {
            return Ok(serde_json::json!({
                "query": symbol,
                "error": "ambiguous symbol",
                "candidates": rows,
            }));
        }

        let row = &rows[0];
        let file_path = row
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CcError::Other("symbol row missing file_path".to_string()))?;
        let start_line = row.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
        let end_line = row
            .get("end_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(start_line as u64) as u32;

        let full_path = crate::path_guard::resolve_indexed_path_strict(&project, file_path, &db)
            .map_err(CcError::Other)?;
        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| CcError::Other(format!("read source: {}", e)))?;
        let source = slice_lines(
            &content,
            start_line,
            end_line,
            include_line_numbers,
            max_src_chars,
        );

        Ok(serde_json::json!({
            "query": symbol,
            "symbol": row,
            "source": source,
            "line_numbered": include_line_numbers,
            "truncated": source.contains("... truncated"),
        }))
    }

    #[allow(dead_code)]
    pub fn compute_package_boundaries(
        &self,
        all_edges: &[(String, String)],
    ) -> CcResult<Vec<PackageBoundary>> {
        compute_package_boundaries(self.ensure_db()?, all_edges)
    }
}

fn slice_lines(
    content: &str,
    start_line: u32,
    end_line: u32,
    include_line_numbers: bool,
    max_chars: usize,
) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = (start_line as usize).saturating_sub(1).min(lines.len());
    let end = (end_line as usize).min(lines.len());
    let mut source = if start < end {
        if include_line_numbers {
            lines[start..end]
                .iter()
                .enumerate()
                .map(|(idx, line)| format!("{:>5} | {}", start + idx + 1, line))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            lines[start..end].join("\n")
        }
    } else {
        String::new()
    };
    if source.len() > max_chars {
        let mut truncate_at = max_chars.min(source.len());
        while !source.is_char_boundary(truncate_at) {
            truncate_at = truncate_at.saturating_sub(1);
        }
        source.truncate(truncate_at);
        source.push_str("\n// ... truncated");
    }
    source
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

#[derive(Debug, Clone, Serialize)]
pub struct PackageBoundary {
    pub from_package: String,
    pub to_package: String,
    pub call_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageLayer {
    pub package: String,
    pub layer: String,
    pub reason: String,
    pub fan_in: u32,
    pub fan_out: u32,
}

fn extract_package(file_path: &str) -> String {
    let skip = ["src", "lib", "app", "internal", "pkg", "cmd"];
    let parts: Vec<&str> = file_path.split('/').collect();
    for (idx, part) in parts.iter().enumerate() {
        if idx < parts.len() - 1 && !skip.contains(part) && !part.is_empty() {
            return (*part).to_string();
        }
    }
    parts
        .first()
        .filter(|p| !p.is_empty())
        .unwrap_or(&"root")
        .to_string()
}

pub fn compute_package_boundaries(
    db: &IndexDb,
    all_edges: &[(String, String)],
) -> CcResult<Vec<PackageBoundary>> {
    let uid_rows = db.query_json(
        "SELECT symbol_uid, file_path FROM symbols WHERE symbol_uid IS NOT NULL",
        &[],
    )?;
    let mut uid_to_file: HashMap<String, String> = HashMap::new();
    for row in &uid_rows {
        let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
        let fp = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
        if !uid.is_empty() && !fp.is_empty() {
            uid_to_file.insert(uid.to_string(), fp.to_string());
        }
    }

    let mut pkg_counts: HashMap<(String, String), u32> = HashMap::new();
    for (caller_uid, callee_uid) in all_edges {
        let from_fp = match uid_to_file.get(caller_uid) {
            Some(fp) => fp.as_str(),
            None => continue,
        };
        let to_fp = match uid_to_file.get(callee_uid) {
            Some(fp) => fp.as_str(),
            None => continue,
        };
        let from_pkg = extract_package(from_fp);
        let to_pkg = extract_package(to_fp);
        if from_pkg != to_pkg {
            *pkg_counts.entry((from_pkg, to_pkg)).or_insert(0) += 1;
        }
    }

    let mut boundaries: Vec<PackageBoundary> = pkg_counts
        .into_iter()
        .map(|((from, to), count)| PackageBoundary {
            from_package: from,
            to_package: to,
            call_count: count,
        })
        .collect();
    boundaries.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    boundaries.truncate(10);
    Ok(boundaries)
}

#[allow(dead_code)]
pub fn compute_package_layers(
    db: &IndexDb,
    boundaries: &[PackageBoundary],
) -> CcResult<Vec<PackageLayer>> {
    let mut fan_in_map: HashMap<String, u32> = HashMap::new();
    let mut fan_out_map: HashMap<String, u32> = HashMap::new();
    let mut all_packages: HashSet<String> = HashSet::new();

    for boundary in boundaries {
        *fan_out_map
            .entry(boundary.from_package.clone())
            .or_insert(0) += boundary.call_count;
        *fan_in_map.entry(boundary.to_package.clone()).or_insert(0) += boundary.call_count;
        all_packages.insert(boundary.from_package.clone());
        all_packages.insert(boundary.to_package.clone());
    }

    let route_rows = db.query_json("SELECT DISTINCT file_path FROM route_nodes", &[])?;
    let mut pkgs_with_routes: HashSet<String> = HashSet::new();
    for row in &route_rows {
        if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
            pkgs_with_routes.insert(extract_package(fp));
        }
    }

    let entry_rows = db.query_json(
        "SELECT DISTINCT file_path FROM symbols WHERE name = 'main' AND kind IN ('function', 'method')",
        &[],
    )?;
    let mut pkgs_with_entry: HashSet<String> = HashSet::new();
    for row in &entry_rows {
        if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
            pkgs_with_entry.insert(extract_package(fp));
        }
    }

    let mut layers = Vec::new();
    for pkg in &all_packages {
        let fan_in = *fan_in_map.get(pkg).unwrap_or(&0);
        let fan_out = *fan_out_map.get(pkg).unwrap_or(&0);
        let has_routes = pkgs_with_routes.contains(pkg);
        let has_entry = pkgs_with_entry.contains(pkg);

        let (layer, reason) = if has_entry && fan_out > 0 && fan_in == 0 {
            ("entry", "has entry point, outbound-only")
        } else if has_routes {
            ("api", "contains route definitions")
        } else if fan_in > fan_out && fan_in > 3 {
            ("core", "high fan-in, depended upon by many packages")
        } else if fan_out == 0 && fan_in > 0 {
            ("leaf", "no outbound cross-package calls")
        } else if fan_in == 0 && fan_out > 0 {
            ("entry", "outbound-only, no inbound cross-package calls")
        } else {
            ("internal", "mixed or balanced fan-in/fan-out")
        };

        layers.push(PackageLayer {
            package: pkg.clone(),
            layer: layer.to_string(),
            reason: reason.to_string(),
            fan_in,
            fan_out,
        });
    }
    layers.sort_by(|a, b| {
        b.fan_in
            .cmp(&a.fan_in)
            .then_with(|| b.fan_out.cmp(&a.fan_out))
    });
    Ok(layers)
}
