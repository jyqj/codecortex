//! Advanced query and analysis methods for `CodeIndex`.
//!
//! Split from `engine.rs` for maintainability. Contains impact analysis,
//! explore/graph_schema, symbol source retrieval, and package boundary analysis.

use cc_db::index_db::IndexDb;
use cc_model::config::{OutputBudget, RepoSizeTier};
use cc_model::impact::ImpactReport;
use cc_model::{CcError, CcResult};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::impact::ImpactAnalyzer;

use super::engine::{centrality_hint, CodeIndex};

impl CodeIndex {
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

    pub fn repo_size_tier(&self) -> RepoSizeTier {
        if let Some(tier) = self.repo_tier {
            return tier;
        }
        let count = self.index_status().map(|s| s.indexed_files).unwrap_or(0);
        RepoSizeTier::from_file_count(count)
    }

    /// Return an adaptive output budget for the given handler name.
    pub fn output_budget(&self, handler: &str) -> OutputBudget {
        self.repo_size_tier().output_budget(handler)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn explore_symbols(
        &mut self,
        names: &[String],
        max_callers: Option<usize>,
        max_callees: Option<usize>,
        include_source: bool,
        include_relations: bool,
        include_metrics: bool,
        outline: bool,
        max_source_per_file: Option<usize>,
    ) -> CcResult<serde_json::Value> {
        let capped = if names.len() > 10 {
            &names[..10]
        } else {
            names
        };
        let tier = self.repo_size_tier();
        let caller_limit = max_callers.unwrap_or(tier.explore_max_symbols());
        let callee_limit = max_callees.unwrap_or(tier.explore_max_symbols());
        let max_src_chars = max_source_per_file.unwrap_or(tier.max_source_chars_per_symbol());

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
                if outline {
                    // Outline mode: return signature + child symbol names instead of full source
                    let mut outline_parts = Vec::new();
                    if let Some(ref sig) = sym.signature {
                        outline_parts.push(sig.clone());
                    }
                    // Query child symbols via parent_symbol_id
                    if let Ok(db) = self.ensure_db() {
                        if let Ok(conn) = db.read_conn() {
                            let child_sql = "SELECT name, kind, signature FROM symbols WHERE parent_symbol_id = ?1 ORDER BY start_line";
                            if let Ok(mut stmt) = conn.prepare(child_sql) {
                                let children: Vec<(String, String, Option<String>)> = stmt
                                    .query_map(rusqlite::params![uid], |row| {
                                        Ok((
                                            row.get::<_, String>(0)?,
                                            row.get::<_, String>(1)?,
                                            row.get::<_, Option<String>>(2)?,
                                        ))
                                    })
                                    .ok()
                                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                                    .unwrap_or_default();
                                for (child_name, child_kind, child_sig) in &children {
                                    if let Some(sig) = child_sig {
                                        outline_parts.push(format!(
                                            "  {} {}: {}",
                                            child_kind, child_name, sig
                                        ));
                                    } else {
                                        outline_parts
                                            .push(format!("  {} {}", child_kind, child_name));
                                    }
                                }
                            }
                        }
                    }
                    if !outline_parts.is_empty() {
                        entry["outline"] = serde_json::json!(outline_parts.join("\n"));
                    }
                } else {
                    // Full source mode
                    if let (Some(project), Some(db)) =
                        (self.project_path.as_ref(), self.index_db.as_ref())
                    {
                        if let Ok(full_path) = crate::path_guard::resolve_indexed_path_strict(
                            project,
                            &sym.file_path,
                            db,
                        ) {
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

            // Metrics
            if include_metrics {
                if let Ok(info) = self.ensure_db()?.symbol_degree_details(uid) {
                    let hint = centrality_hint(&info);
                    entry["metrics"] = serde_json::json!({
                        "in_degree": info.in_degree,
                        "out_degree": info.out_degree,
                        "caller_count": info.caller_count,
                        "callee_count": info.callee_count,
                        "ref_count": info.ref_count,
                        "centrality_hint": hint,
                    });
                }
            }

            results.push(entry);
        }

        // Group results by file for per-file clustering
        let explore_budget = tier.explore_budget();
        let mut by_file: std::collections::BTreeMap<String, Vec<&serde_json::Value>> =
            std::collections::BTreeMap::new();
        for r in &results {
            if let Some(sym) = r.get("symbol") {
                if let Some(fp) = sym.get("file_path").and_then(|v| v.as_str()) {
                    by_file.entry(fp.to_string()).or_default().push(r);
                }
            }
        }

        let mut grouped = serde_json::json!({
            "symbols": results,
        });

        if by_file.len() > 1 {
            let file_summary: Vec<serde_json::Value> = by_file
                .iter()
                .take(explore_budget.default_max_files)
                .map(|(file, syms)| {
                    let names: Vec<&str> = syms
                        .iter()
                        .filter_map(|s| {
                            s.get("symbol")
                                .and_then(|sym| sym.get("name"))
                                .and_then(|n| n.as_str())
                        })
                        .collect();
                    serde_json::json!({
                        "file": file,
                        "symbols": names,
                        "count": syms.len(),
                    })
                })
                .collect();
            grouped["by_file"] = serde_json::json!(file_summary);
        }

        Ok(grouped)
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

    /// Return a schema overview of the index: node kinds with counts,
    /// edge table counts, relationship patterns, example queries,
    /// edge provenance summary, and runtime evidence coverage.
    pub fn graph_schema(&self) -> CcResult<serde_json::Value> {
        let db = self.ensure_db()?;

        // Symbol kind counts
        let kind_rows = db.query_json(
            "SELECT kind, COUNT(*) AS cnt FROM symbols GROUP BY kind ORDER BY cnt DESC",
            &[],
        )?;
        let node_kinds: Vec<serde_json::Value> = kind_rows
            .into_iter()
            .map(|row| {
                serde_json::json!({
                    "kind": row.get("kind").cloned().unwrap_or(serde_json::Value::Null),
                    "count": row.get("cnt").cloned().unwrap_or(serde_json::json!(0)),
                })
            })
            .collect();

        // Edge table counts — query each table; tolerate missing tables
        let edge_tables = [
            "call_edges",
            "import_edges",
            "semantic_edges",
            "test_edges",
            "route_edges",
            "http_call_edges",
            "data_flow_edges",
            "co_change_edges",
        ];
        let mut edge_counts = serde_json::Map::new();
        for table in &edge_tables {
            let sql = format!("SELECT COUNT(*) AS cnt FROM {}", table);
            let count = db
                .query_json(&sql, &[])
                .ok()
                .and_then(|rows| rows.into_iter().next())
                .and_then(|row| row.get("cnt").cloned())
                .unwrap_or(serde_json::json!(0));
            edge_counts.insert(table.to_string(), count);
        }

        // Total files and chunks
        let file_count = db
            .query_json("SELECT COUNT(*) AS cnt FROM files", &[])
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| row.get("cnt").cloned())
            .unwrap_or(serde_json::json!(0));

        let chunk_count = db
            .query_json("SELECT COUNT(*) AS cnt FROM chunks", &[])
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| row.get("cnt").cloned())
            .unwrap_or(serde_json::json!(0));

        // --- Relationship patterns (static, describes the graph schema) ---
        let relationship_patterns = serde_json::json!([
            {"from": "Function", "edge": "CALLS",             "to": "Function",  "table": "call_edges",      "description": "Direct or dynamic function call"},
            {"from": "Function", "edge": "IMPORTS",           "to": "Module",    "table": "import_edges",    "description": "Import dependency between files/modules"},
            {"from": "Class",    "edge": "INHERITS",          "to": "Class",     "table": "semantic_edges",  "description": "Class inheritance (extends)"},
            {"from": "Class",    "edge": "IMPLEMENTS",        "to": "Interface", "table": "semantic_edges",  "description": "Interface implementation"},
            {"from": "Function", "edge": "DECORATES",         "to": "Function",  "table": "semantic_edges",  "description": "Decorator / annotation application"},
            {"from": "Function", "edge": "THROWS",            "to": "Class",     "table": "semantic_edges",  "description": "Exception / error throw relation"},
            {"from": "Function", "edge": "USES_TYPE",         "to": "Class",     "table": "semantic_edges",  "description": "Type usage in parameters or return types"},
            {"from": "File",     "edge": "DEFINES",           "to": "Function",  "table": "semantic_edges",  "description": "File defines a top-level symbol"},
            {"from": "Class",    "edge": "DEFINES_METHOD",    "to": "Function",  "table": "semantic_edges",  "description": "Class/struct defines a method"},
            {"from": "Module",   "edge": "CONTAINS_FILE",     "to": "File",      "table": "semantic_edges",  "description": "Folder/module contains a file"},
            {"from": "Module",   "edge": "CONTAINS_MODULE",   "to": "Module",    "table": "semantic_edges",  "description": "Module contains a submodule"},
            {"from": "Function", "edge": "RENDERS_COMPONENT", "to": "Function",  "table": "semantic_edges",  "description": "React/Vue component renders another component"},
            {"from": "Route",    "edge": "HANDLES",           "to": "Function",  "table": "route_edges",     "description": "HTTP route mapped to handler function"},
            {"from": "Function", "edge": "HTTP_CALL",         "to": "Route",     "table": "http_call_edges", "description": "Code makes an outbound HTTP request"},
            {"from": "Function", "edge": "DATA_FLOW",         "to": "Function",  "table": "data_flow_edges", "description": "Data flows between functions"},
            {"from": "File",     "edge": "CO_CHANGE",         "to": "File",      "table": "co_change_edges", "description": "Files frequently changed together in commits"},
            {"from": "Function", "edge": "TESTS",             "to": "Function",  "table": "test_edges",      "description": "Test function covers a code function"},
        ]);

        // --- Example Cypher queries (static, the 5 most useful for agents) ---
        let example_queries = serde_json::json!([
            {
                "description": "Find all functions in the index",
                "cypher": "MATCH (f:Function) RETURN f.name, f.file_path LIMIT 20"
            },
            {
                "description": "Find callers of a specific function",
                "cypher": "MATCH (caller:Function)-[:CALLS]->(f:Function {name: 'TARGET_NAME'}) RETURN caller.name, caller.file_path"
            },
            {
                "description": "Find HTTP routes and their handlers",
                "cypher": "MATCH (r:Route)-[:HANDLES]->(f:Function) RETURN r.path, r.method, f.name, f.file_path"
            },
            {
                "description": "Find potentially dead code (functions with zero in-degree)",
                "cypher": "MATCH (f:Function) WHERE in_degree(f) = 0 RETURN f.name, f.file_path LIMIT 20"
            },
            {
                "description": "Find type hierarchy (inheritance)",
                "cypher": "MATCH (c:Class)-[:INHERITS]->(parent:Class) RETURN c.name, parent.name, c.file_path"
            }
        ]);

        // --- Edge provenance summary (from call_edges dispatch_kind / synthesized_by) ---
        let edge_provenance = self.compute_edge_provenance(db);

        // --- Runtime evidence coverage (from runtime_evidence table) ---
        let runtime_evidence = self.compute_runtime_evidence(db, &edge_counts);

        // --- Edge properties: tell agents what they can filter on in queries ---
        let edge_properties = serde_json::json!({
            "CALLS": {
                "filterable": ["dispatch_kind", "call_kind", "resolution_kind", "parser_tier", "synthesized_by"],
                "informational": ["confidence", "parser_confidence", "synthesis_key", "registered_file"]
            },
            "HTTP_CALL": {
                "filterable": ["method", "call_kind", "broker_type"],
                "informational": ["confidence", "url_or_path", "normalized_path"]
            },
            "ROUTE": {
                "filterable": ["method", "framework", "route_kind"],
                "informational": ["confidence", "route_path", "handler_name"]
            },
            "DATA_FLOW": {
                "filterable": ["flow_kind"],
                "informational": ["confidence", "env_key"]
            },
            "SEMANTIC": {
                "filterable": ["relation_kind"],
                "informational": ["confidence"]
            }
        });

        // --- Next-tool hints: recommend tools for exploring each edge/node type ---
        let next_tool_hints = serde_json::json!({
            "description": "Recommended tools for exploring specific graph relationships",
            "hints": {
                "CALLS": "trace(from, to, source_mode='body') for call paths; relations(symbol, kind='callers'|'callees') for direct edges",
                "HTTP_CALL": "architecture(aspect='services') for service map; ingest_traces to validate with runtime data",
                "ROUTE": "architecture(aspect='routes') for all routes; explore(symbols, mode='flow') for request flow",
                "DATA_FLOW": "explore(symbols, mode='flow') for data dependencies; relations(symbol, kind='refs') for references",
                "SEMANTIC": "relations(symbol, kind='hierarchy') for type hierarchy; node(symbol, include='trail') for overview",
                "runtime_evidence": "ingest_traces(traces) to add observations; status(aspect='schema') to check current evidence counts"
            }
        });

        Ok(serde_json::json!({
            "node_kinds": node_kinds,
            "edge_counts": edge_counts,
            "total_files": file_count,
            "total_chunks": chunk_count,
            "relationship_patterns": relationship_patterns,
            "example_queries": example_queries,
            "edge_provenance": edge_provenance,
            "runtime_evidence": runtime_evidence,
            "edge_properties": edge_properties,
            "runtime_evidence_edges": ["HTTP_CALL"],
            "next_tool_hints": next_tool_hints,
        }))
    }

    /// Compute edge provenance breakdown from call_edges table.
    /// Groups edges by their origin: tree-sitter parsed, heuristic inferred,
    /// runtime-verified, or synthesized by post-processing.
    fn compute_edge_provenance(&self, db: &Arc<IndexDb>) -> serde_json::Value {
        // Count by dispatch_kind to distinguish tree-sitter vs heuristic
        let dispatch_rows = db
            .query_json(
                "SELECT dispatch_kind, COUNT(*) AS cnt FROM call_edges GROUP BY dispatch_kind",
                &[],
            )
            .unwrap_or_default();

        // Count synthesized edges (synthesized_by IS NOT NULL)
        let synthesized_count: i64 = db
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by IS NOT NULL",
                &[],
            )
            .ok()
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| row.get("cnt").and_then(|v| v.as_i64()))
            .unwrap_or(0);

        // Count synthesized by source
        let synth_by_rows = db
            .query_json(
                "SELECT synthesized_by, COUNT(*) AS cnt FROM call_edges WHERE synthesized_by IS NOT NULL GROUP BY synthesized_by ORDER BY cnt DESC",
                &[],
            )
            .unwrap_or_default();

        // Count by resolution_kind to separate exact/heuristic
        let resolution_rows = db
            .query_json(
                "SELECT resolution_kind, COUNT(*) AS cnt FROM call_edges GROUP BY resolution_kind",
                &[],
            )
            .unwrap_or_default();

        let total_call_edges: i64 = dispatch_rows
            .iter()
            .filter_map(|r| r.get("cnt").and_then(|v| v.as_i64()))
            .sum();

        // Classify: tree_sitter = exact + qualified + scope_resolved, heuristic = heuristic, unresolved = unresolved
        let mut tree_sitter: i64 = 0;
        let mut heuristic: i64 = 0;
        let mut unresolved: i64 = 0;
        for row in &resolution_rows {
            let kind = row
                .get("resolution_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let cnt = row.get("cnt").and_then(|v| v.as_i64()).unwrap_or(0);
            match kind {
                "exact" | "qualified" | "scope_resolved" => tree_sitter += cnt,
                "heuristic" => heuristic += cnt,
                "unresolved" | "" => unresolved += cnt,
                _ => heuristic += cnt,
            }
        }

        // Dispatch kind breakdown
        let mut dispatch_breakdown = serde_json::Map::new();
        for row in &dispatch_rows {
            let kind = row
                .get("dispatch_kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let cnt = row.get("cnt").cloned().unwrap_or(serde_json::json!(0));
            dispatch_breakdown.insert(kind, cnt);
        }

        // Synthesized-by breakdown
        let mut synth_breakdown = serde_json::Map::new();
        for row in &synth_by_rows {
            let by = row
                .get("synthesized_by")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let cnt = row.get("cnt").cloned().unwrap_or(serde_json::json!(0));
            synth_breakdown.insert(by, cnt);
        }

        serde_json::json!({
            "total_call_edges": total_call_edges,
            "by_resolution": {
                "tree_sitter": tree_sitter,
                "heuristic": heuristic,
                "unresolved": unresolved,
            },
            "synthesized": synthesized_count,
            "by_dispatch_kind": dispatch_breakdown,
            "by_synthesized_by": synth_breakdown,
        })
    }

    /// Compute runtime evidence coverage from the runtime_evidence table.
    fn compute_runtime_evidence(
        &self,
        db: &Arc<IndexDb>,
        edge_counts: &serde_json::Map<String, serde_json::Value>,
    ) -> serde_json::Value {
        match db.runtime_evidence_stats() {
            Ok(stats) => {
                let total = stats
                    .get("total_observations")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let matched = stats
                    .get("linked_to_edges")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let unmatched = total.saturating_sub(matched);

                // Coverage percentage: matched evidence / total http_call_edges
                let http_edge_total = edge_counts
                    .get("http_call_edges")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let coverage_pct = if http_edge_total > 0 {
                    (matched as f64 / http_edge_total as f64) * 100.0
                } else {
                    0.0
                };

                serde_json::json!({
                    "total_evidence": total,
                    "matched_to_edges": matched,
                    "unmatched": unmatched,
                    "http_edge_coverage_pct": (coverage_pct * 10.0).round() / 10.0,
                })
            }
            Err(_) => {
                serde_json::json!({
                    "total_evidence": 0,
                    "matched_to_edges": 0,
                    "unmatched": 0,
                    "http_edge_coverage_pct": 0.0,
                    "note": "runtime_evidence table not available or empty"
                })
            }
        }
    }
}

// ── Free functions ─────────────────────────────────────────────────────

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
