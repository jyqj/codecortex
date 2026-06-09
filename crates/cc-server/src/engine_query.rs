//! Advanced query and analysis methods for `CodeIndex`.
//!
//! Split from `engine.rs` for maintainability. Contains impact analysis,
//! explore/graph_schema, symbol source retrieval, and package boundary analysis.

use cc_db::index_db::IndexDb;
use cc_model::config::{OutputBudget, RepoSizeTier};
use cc_model::graph_catalog::{graph_relationship_table_names, graph_relationships};
use cc_model::impact::ImpactReport;
use cc_model::{CcError, CcResult};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::impact::ImpactAnalyzer;

use super::engine::{centrality_hint, CodeIndex};

#[derive(Debug, Serialize)]
struct GraphSchemaResponse {
    node_kinds: Vec<NodeKindCount>,
    edge_counts: serde_json::Map<String, serde_json::Value>,
    total_files: serde_json::Value,
    total_chunks: serde_json::Value,
    relationship_patterns: Vec<RelationshipPattern>,
    example_queries: Vec<ExampleQuery>,
    edge_provenance: EdgeProvenanceSummary,
    runtime_evidence: RuntimeEvidenceSummary,
    edge_properties: BTreeMap<&'static str, EdgePropertyInfo>,
    runtime_evidence_edges: Vec<&'static str>,
    next_tool_hints: NextToolHints,
}

#[derive(Debug, Serialize)]
struct NodeKindCount {
    kind: serde_json::Value,
    count: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct RelationshipPattern {
    from: &'static str,
    edge: &'static str,
    to: &'static str,
    table: &'static str,
    description: &'static str,
}

#[derive(Debug, Serialize)]
struct ExampleQuery {
    description: &'static str,
    cypher: &'static str,
}

#[derive(Debug, Serialize)]
struct EdgePropertyInfo {
    filterable: Vec<&'static str>,
    informational: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct NextToolHints {
    description: &'static str,
    hints: BTreeMap<&'static str, &'static str>,
}

#[derive(Debug, Serialize)]
struct EdgeProvenanceSummary {
    total_call_edges: i64,
    by_resolution: ResolutionBreakdown,
    synthesized: i64,
    by_dispatch_kind: serde_json::Map<String, serde_json::Value>,
    by_synthesized_by: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ResolutionBreakdown {
    tree_sitter: i64,
    heuristic: i64,
    unresolved: i64,
}

#[derive(Debug, Serialize)]
struct RuntimeEvidenceSummary {
    total_evidence: u64,
    matched_to_edges: u64,
    unmatched: u64,
    http_edge_coverage_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'static str>,
}

fn edge_property_info(
    filterable: Vec<&'static str>,
    informational: Vec<&'static str>,
) -> EdgePropertyInfo {
    EdgePropertyInfo {
        filterable,
        informational,
    }
}

impl CodeIndex {
    pub fn detect_impact(
        &self,
        changed_files: &[String],
        confidence_threshold: Option<f32>,
    ) -> CcResult<ImpactReport> {
        ImpactAnalyzer::new(self.ensure_db()?.clone()).analyze_with_options(
            changed_files,
            3,
            confidence_threshold.map(|v| v as f64),
        )
    }

    pub fn analyze_impact(
        &self,
        base_ref: Option<&str>,
        confidence_threshold: Option<f32>,
    ) -> CcResult<ImpactReport> {
        let changed = self.git_changed_files(base_ref)?;
        ImpactAnalyzer::new(self.ensure_db()?.clone()).analyze_with_options(
            &changed,
            3,
            confidence_threshold.map(|v| v as f64),
        )
    }

    pub fn git_changed_files(&self, base_ref: Option<&str>) -> CcResult<Vec<String>> {
        let project = self.ensure_project()?;
        let analyzer = ImpactAnalyzer::new(self.ensure_db()?.clone())
            .with_project_root(project.display().to_string());
        Ok(analyzer.git_changed_files(base_ref))
    }

    pub fn repo_size_tier(&self) -> RepoSizeTier {
        if let Some(tier) = self.repo_tier {
            return tier;
        }
        self.compute_repo_tier()
    }

    pub(crate) fn compute_repo_tier(&self) -> RepoSizeTier {
        let count = self.index_status().map(|s| s.indexed_files).unwrap_or(0);
        RepoSizeTier::from_file_count(count)
    }

    /// Return an adaptive output budget for the given handler name.
    pub fn output_budget(&self, handler: &str) -> OutputBudget {
        self.repo_size_tier().output_budget(handler)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn explore_symbols(
        &self,
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
        let db = self.ensure_db()?.clone();

        let mut results = Vec::with_capacity(capped.len());

        // Outline mode queries child symbols per symbol; reuse one read
        // connection + a cached statement across the loop instead of acquiring a
        // connection and recompiling the SQL for every symbol.
        let outline_conn = if include_source && outline {
            Some(db.read_conn()?)
        } else {
            None
        };

        for name in capped {
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
            let callers = db.caller_rows_by_uid(uid, caller_limit)?;
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
            let callees = db.callee_rows_by_uid(uid, callee_limit)?;
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
                    // Query child symbols via parent_symbol_id, reusing the
                    // hoisted connection + cached statement.
                    if let Some(conn) = &outline_conn {
                        let child_sql = "SELECT name, kind, signature FROM symbols WHERE parent_symbol_id = ?1 ORDER BY start_line";
                        if let Ok(mut stmt) = conn.prepare_cached(child_sql) {
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
                                    outline_parts
                                        .push(format!("  {} {}: {}", child_kind, child_name, sig));
                                } else {
                                    outline_parts.push(format!("  {} {}", child_kind, child_name));
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
                if let Ok(info) = db.symbol_degree_details(uid) {
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
        &self,
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
        let node_kinds: Vec<NodeKindCount> = kind_rows
            .into_iter()
            .map(|row| NodeKindCount {
                kind: row.get("kind").cloned().unwrap_or(serde_json::Value::Null),
                count: row.get("cnt").cloned().unwrap_or(serde_json::json!(0)),
            })
            .collect();

        // Edge table counts — derive tables from the shared graph catalog and
        // tolerate missing tables for older/stale indexes.
        let edge_tables = graph_relationship_table_names();
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

        // --- Relationship patterns (shared catalog, describes the graph schema) ---
        let relationship_patterns: Vec<RelationshipPattern> = graph_relationships()
            .iter()
            .filter(|rel| rel.visible_in_schema)
            .map(|rel| RelationshipPattern {
                from: rel.schema.from,
                edge: rel.edge,
                to: rel.schema.to,
                table: rel.table,
                description: rel.schema.description,
            })
            .collect();

        // --- Example Cypher queries (static, the 5 most useful for agents) ---
        let example_queries = vec![
            ExampleQuery {
                description: "Find all functions in the index",
                cypher: "MATCH (f:Function) RETURN f.name, f.file_path LIMIT 20",
            },
            ExampleQuery {
                description: "Find callers of a specific function",
                cypher: "MATCH (caller:Function)-[:CALLS]->(f:Function {name: 'TARGET_NAME'}) RETURN caller.name, caller.file_path",
            },
            ExampleQuery {
                description: "Find HTTP routes and their handlers",
                cypher: "MATCH (r:Route)-[:HANDLES]->(f:Function) RETURN r.route_path, r.method, f.name, f.file_path",
            },
            ExampleQuery {
                description: "Find potentially dead code (functions with zero in-degree)",
                cypher: "MATCH (f:Function) WHERE in_degree(f) = 0 RETURN f.name, f.file_path LIMIT 20",
            },
            ExampleQuery {
                description: "Find type hierarchy (inheritance)",
                cypher: "MATCH (c:Class)-[:INHERITS]->(parent:Class) RETURN c.name, parent.name, c.file_path",
            },
        ];

        // --- Edge provenance summary (from call_edges dispatch_kind / synthesized_by) ---
        let edge_provenance = self.compute_edge_provenance(db);

        // --- Runtime evidence coverage (from runtime_evidence table) ---
        let runtime_evidence = self.compute_runtime_evidence(db, &edge_counts);

        // --- Edge properties: tell agents what they can filter on in queries ---
        let edge_properties = graph_relationships()
            .iter()
            .filter(|rel| !rel.properties.is_empty())
            .map(|rel| {
                (
                    rel.edge,
                    edge_property_info(
                        rel.properties.filterable.to_vec(),
                        rel.properties.informational.to_vec(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();

        // --- Next-tool hints: recommend tools for exploring each edge/node type ---
        let mut next_tool_hint_map = graph_relationships()
            .iter()
            .filter_map(|rel| rel.next_tool_hint.map(|hint| (rel.edge, hint)))
            .collect::<BTreeMap<_, _>>();
        next_tool_hint_map.insert(
            "runtime_evidence",
            "ingest_traces(traces) to add observations; status(aspect='schema') to check current evidence counts",
        );
        let next_tool_hints = NextToolHints {
            description: "Recommended tools for exploring specific graph relationships",
            hints: next_tool_hint_map,
        };

        let runtime_evidence_edges = graph_relationships()
            .iter()
            .filter(|rel| rel.runtime_evidence)
            .map(|rel| rel.edge)
            .collect::<Vec<_>>();

        serde_json::to_value(GraphSchemaResponse {
            node_kinds,
            edge_counts,
            total_files: file_count,
            total_chunks: chunk_count,
            relationship_patterns,
            example_queries,
            edge_provenance,
            runtime_evidence,
            edge_properties,
            runtime_evidence_edges,
            next_tool_hints,
        })
        .map_err(|e| CcError::Other(e.to_string()))
    }

    /// Compute edge provenance breakdown from call_edges table.
    /// Groups edges by their origin: tree-sitter parsed, heuristic inferred,
    /// runtime-verified, or synthesized by post-processing.
    fn compute_edge_provenance(&self, db: &Arc<IndexDb>) -> EdgeProvenanceSummary {
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

        EdgeProvenanceSummary {
            total_call_edges,
            by_resolution: ResolutionBreakdown {
                tree_sitter,
                heuristic,
                unresolved,
            },
            synthesized: synthesized_count,
            by_dispatch_kind: dispatch_breakdown,
            by_synthesized_by: synth_breakdown,
        }
    }

    /// Compute runtime evidence coverage from the runtime_evidence table.
    fn compute_runtime_evidence(
        &self,
        db: &Arc<IndexDb>,
        edge_counts: &serde_json::Map<String, serde_json::Value>,
    ) -> RuntimeEvidenceSummary {
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

                RuntimeEvidenceSummary {
                    total_evidence: total,
                    matched_to_edges: matched,
                    unmatched,
                    http_edge_coverage_pct: (coverage_pct * 10.0).round() / 10.0,
                    note: None,
                }
            }
            Err(_) => RuntimeEvidenceSummary {
                total_evidence: 0,
                matched_to_edges: 0,
                unmatched: 0,
                http_edge_coverage_pct: 0.0,
                note: Some("runtime_evidence table not available or empty"),
            },
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

pub fn compute_package_boundaries(db: &IndexDb) -> CcResult<Vec<PackageBoundary>> {
    // SQL JOIN: fetch only cross-file caller/callee file paths (no full edge materialization)
    let cross_file_rows = db.query_json(
        "SELECT s1.file_path AS caller_file, s2.file_path AS callee_file \
         FROM call_edges ce \
         JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
         JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
         WHERE ce.caller_symbol_uid IS NOT NULL \
           AND ce.callee_symbol_uid IS NOT NULL \
           AND s1.file_path != s2.file_path",
        &[],
    )?;

    let mut pkg_counts: HashMap<(String, String), u32> = HashMap::new();
    for row in &cross_file_rows {
        let from_fp = row.get("caller_file").and_then(|v| v.as_str()).unwrap_or("");
        let to_fp = row.get("callee_file").and_then(|v| v.as_str()).unwrap_or("");
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a `CodeIndex` over a temp project and seed a single low-confidence
    /// call edge: `B` (src/b.rs) calls `A` (src/a.rs). Returns the index.
    fn index_with_low_confidence_edge() -> (TempDir, CodeIndex) {
        let dir = TempDir::new().unwrap();
        let mut idx = CodeIndex::empty();
        idx.set_project(dir.path(), false).unwrap();
        let db = idx.index_db().expect("db initialized");
        let conn = db.read_conn().unwrap();
        for fp in ["src/a.rs", "src/b.rs"] {
            conn.execute(
                "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES(?1, 'rust', 'hash', 0.0, 100, '2025-01-01')",
                rusqlite::params![fp],
            )
            .unwrap();
        }
        for (uid, name, fp) in [("uid_a", "A", "src/a.rs"), ("uid_b", "B", "src/b.rs")] {
            conn.execute(
                "INSERT OR REPLACE INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, symbol_uid) \
                 VALUES(?1, ?2, ?3, 'function', 1, 10, ?4)",
                rusqlite::params![format!("sid_{}", uid), fp, name, uid],
            )
            .unwrap();
        }
        // Edge with default parser_confidence (0.5) from B -> A.
        conn.execute(
            "INSERT OR REPLACE INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid) \
             VALUES('edge_ba', 'src/b.rs', 'A', 1, 'uid_b', 'uid_a')",
            [],
        )
        .unwrap();
        drop(conn);
        (dir, idx)
    }

    #[test]
    fn graph_schema_uses_relationship_catalog() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let schema = idx.graph_schema().unwrap();

        let patterns = schema["relationship_patterns"].as_array().unwrap();
        assert!(
            patterns
                .iter()
                .any(|p| p["edge"] == "CALLS" && p["table"] == "call_edges"),
            "CALLS pattern should come from graph catalog: {patterns:?}"
        );
        assert!(
            patterns
                .iter()
                .any(|p| p["edge"] == "HTTP_CALLS" && p["table"] == "http_call_edges"),
            "schema should advertise queryable HTTP_CALLS edge: {patterns:?}"
        );
        assert!(
            schema["edge_counts"].get("symbol_refs").is_some(),
            "edge_counts should include catalog table symbol_refs"
        );
        assert!(
            schema["edge_properties"].get("HTTP_CALLS").is_some(),
            "edge_properties should use catalog edge names"
        );
        let runtime_edges = schema["runtime_evidence_edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>();
        assert!(
            runtime_edges.contains(&"HTTP_CALLS"),
            "runtime evidence edges should include canonical HTTP_CALLS: {runtime_edges:?}"
        );
        assert!(
            schema["edge_properties"].get("HTTP_CALL").is_some(),
            "compatibility alias HTTP_CALL should remain catalog-backed"
        );
        assert!(
            schema["next_tool_hints"]["hints"].get("CALLS").is_some(),
            "next-tool hints should be derived from catalog"
        );
    }

    #[test]
    fn detect_impact_high_confidence_threshold_filters_callers() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let report = idx
            .detect_impact(&["src/a.rs".to_string()], Some(0.9))
            .unwrap();
        let hop1: Vec<_> = report
            .impacted_symbols
            .iter()
            .filter(|s| s.hop_depth > 0)
            .collect();
        assert!(
            hop1.is_empty(),
            "confidence_threshold=0.9 should filter the 0.5-confidence caller"
        );
    }

    #[test]
    fn detect_impact_without_threshold_keeps_callers() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let report = idx.detect_impact(&["src/a.rs".to_string()], None).unwrap();
        let hop1: Vec<_> = report
            .impacted_symbols
            .iter()
            .filter(|s| s.hop_depth > 0)
            .collect();
        assert_eq!(
            hop1.len(),
            1,
            "without a threshold the low-confidence caller must remain"
        );
        assert_eq!(hop1[0].name, "B");
    }
}
