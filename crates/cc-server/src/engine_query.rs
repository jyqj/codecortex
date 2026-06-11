//! Advanced query and analysis methods for the `CodeIndex` domain views.
//!
//! Split from `engine.rs` for maintainability. Contains the `ImpactOps` view
//! (impact analysis), the heavier `GraphOps` methods (explore/graph_schema,
//! symbol source retrieval), the tier/budget infrastructure that stays on
//! `CodeIndex` itself, and package boundary analysis.

use cc_db::index_db::IndexDb;
use cc_model::config::{OutputBudget, RepoSizeTier};
use cc_model::graph_catalog::{graph_relationship_table_names, graph_relationships};
use cc_model::impact::ImpactReport;
use cc_model::{CcError, CcResult};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::impact::{ImpactAnalyzer, ImpactOptions};

use super::engine::{centrality_hint, CodeIndex, GraphOps, ImpactOps};

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

impl ImpactOps<'_> {
    pub fn detect_impact(
        &self,
        changed_files: &[String],
        confidence_threshold: Option<f32>,
    ) -> CcResult<ImpactReport> {
        self.detect_impact_capped(changed_files, confidence_threshold, None, None, None)
    }

    /// Like `detect_impact` but with explicit BFS safety caps. `result_limit`
    /// clips the returned `impacted_symbols`; `max_nodes`/`max_per_layer` bound
    /// the BFS expansion. `None` for all caps reproduces the legacy behaviour.
    pub fn detect_impact_capped(
        &self,
        changed_files: &[String],
        confidence_threshold: Option<f32>,
        result_limit: Option<usize>,
        max_nodes: Option<usize>,
        max_per_layer: Option<usize>,
    ) -> CcResult<ImpactReport> {
        let opts = ImpactOptions {
            max_depth: ImpactOptions::DEFAULT_MAX_DEPTH,
            confidence_threshold: confidence_threshold.map(|v| v as f64),
            max_nodes,
            max_per_layer,
            result_limit,
        };
        ImpactAnalyzer::new(self.0.ensure_db()?.clone()).analyze_with(changed_files, &opts)
    }

    /// Like `detect_impact_capped` but with the MCP `impact` handler's
    /// adaptive default budget filled in for any cap left `None`: node cap =
    /// `result_limit × 10` capped at 5000, layer cap = 500, depth 3 (see
    /// [`ImpactOptions::default_for`]). Direct engine callers get exactly the
    /// same bounds as the MCP tool.
    pub fn detect_impact_with_default_budget(
        &self,
        changed_files: &[String],
        confidence_threshold: Option<f32>,
        result_limit: usize,
        max_nodes: Option<usize>,
        max_per_layer: Option<usize>,
    ) -> CcResult<ImpactReport> {
        let opts = ImpactOptions::default_for(
            result_limit,
            confidence_threshold.map(|v| v as f64),
            max_nodes,
            max_per_layer,
        );
        ImpactAnalyzer::new(self.0.ensure_db()?.clone()).analyze_with(changed_files, &opts)
    }

    /// Git-diff-based counterpart to `detect_impact_with_default_budget`.
    pub fn analyze_impact_with_default_budget(
        &self,
        base_ref: Option<&str>,
        confidence_threshold: Option<f32>,
        result_limit: usize,
        max_nodes: Option<usize>,
        max_per_layer: Option<usize>,
    ) -> CcResult<ImpactReport> {
        let changed = self.git_changed_files(base_ref)?;
        self.detect_impact_with_default_budget(
            &changed,
            confidence_threshold,
            result_limit,
            max_nodes,
            max_per_layer,
        )
    }

    pub fn analyze_impact(
        &self,
        base_ref: Option<&str>,
        confidence_threshold: Option<f32>,
    ) -> CcResult<ImpactReport> {
        self.analyze_impact_capped(base_ref, confidence_threshold, None, None, None)
    }

    /// Git-diff-based counterpart to `detect_impact_capped`.
    pub fn analyze_impact_capped(
        &self,
        base_ref: Option<&str>,
        confidence_threshold: Option<f32>,
        result_limit: Option<usize>,
        max_nodes: Option<usize>,
        max_per_layer: Option<usize>,
    ) -> CcResult<ImpactReport> {
        let changed = self.git_changed_files(base_ref)?;
        let opts = ImpactOptions {
            max_depth: ImpactOptions::DEFAULT_MAX_DEPTH,
            confidence_threshold: confidence_threshold.map(|v| v as f64),
            max_nodes,
            max_per_layer,
            result_limit,
        };
        ImpactAnalyzer::new(self.0.ensure_db()?.clone()).analyze_with(&changed, &opts)
    }

    pub fn git_changed_files(&self, base_ref: Option<&str>) -> CcResult<Vec<String>> {
        let project = self.0.ensure_project()?;
        let analyzer = ImpactAnalyzer::new(self.0.ensure_db()?.clone())
            .with_project_root(project.display().to_string());
        Ok(analyzer.git_changed_files(base_ref))
    }

    pub fn find_impacted_tests(&self, files: &[String]) -> CcResult<Vec<String>> {
        self.0.ensure_db()?.reads().find_impacted_tests(files)
    }
}

impl CodeIndex {
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
}

impl GraphOps<'_> {
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
        let tier = self.0.repo_size_tier();
        let caller_limit = max_callers.unwrap_or(tier.explore_max_symbols());
        let callee_limit = max_callees.unwrap_or(tier.explore_max_symbols());
        let max_src_chars = max_source_per_file.unwrap_or(tier.max_source_chars_per_symbol());
        let db = self.0.ensure_db()?.clone();

        let mut results = Vec::with_capacity(capped.len());

        for name in capped {
            // Exact match first, fuzzy fallback
            let mut syms = db.reads().find_symbol(name, true, 3)?;
            if syms.is_empty() {
                syms = db.reads().find_symbol(name, false, 3)?;
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

            // GraphReadModel bypass (declared, deliberate): the callers /
            // callees / semantic-relations / metrics reads below go straight
            // to cc-db typed queries (`caller_rows_by_uid`,
            // `callee_rows_by_uid`, `query_semantic_edges`,
            // `symbol_degree_details`) instead of GraphReadModel — relocating
            // them buys nothing (no shared adjacency reuse, point lookups
            // only) and is too invasive. The graph subset this block consults
            // is declared in `tool_graph_subsets::RELATIONS` and surfaced via
            // the `graph_explain` envelope attached to the response below.

            // Callers
            let callers = db.reads().caller_rows_by_uid(uid, caller_limit)?;
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
            let callees = db.reads().callee_rows_by_uid(uid, callee_limit)?;
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
                    // Query child symbols via parent_symbol_id (best-effort:
                    // a query failure just omits the child outline).
                    let children = db
                        .reads()
                        .child_symbol_outline_rows(uid)
                        .unwrap_or_default();
                    for (child_name, child_kind, child_sig) in &children {
                        if let Some(sig) = child_sig {
                            outline_parts.push(format!("  {} {}: {}", child_kind, child_name, sig));
                        } else {
                            outline_parts.push(format!("  {} {}", child_kind, child_name));
                        }
                    }
                    if !outline_parts.is_empty() {
                        entry["outline"] = serde_json::json!(outline_parts.join("\n"));
                    }
                } else {
                    // Full source mode
                    if let (Some(project), Some(db)) =
                        (self.0.project_path.as_ref(), self.0.index_db.as_ref())
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
                if let Ok(edges) = db.reads().query_semantic_edges(Some(uid), None, None) {
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
                if let Ok(edges) = db.reads().query_semantic_edges(None, Some(uid), None) {
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
                if let Ok(info) = db.reads().symbol_degree_details(uid) {
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

        // Additive contract visibility for the GraphReadModel bypass above:
        // which catalog edge kinds this surface consults (CALLS callers/
        // callees + degree, SEMANTIC relations, REFERENCES ref counts).
        // Declaration only — traversal is unchanged.
        grouped["graph_explain"] = serde_json::to_value(cc_model::GraphExplain::declared_only(
            cc_model::graph_catalog::tool_graph_subsets::RELATIONS,
        ))
        .map_err(|e| CcError::Other(e.to_string()))?;

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
        let project = self.0.ensure_project()?.to_path_buf();
        let db = self.0.ensure_db()?.clone();
        let tier = self.0.repo_size_tier();
        let max_src_chars = max_chars.unwrap_or_else(|| tier.max_source_chars_per_symbol());

        let rows = db.reads().symbol_source_candidates(symbol, exact)?;

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
        let db = self.0.ensure_db()?;

        // Symbol kind counts
        let node_kinds: Vec<NodeKindCount> = db
            .reads()
            .symbol_kind_counts()?
            .into_iter()
            .map(|(kind, count)| NodeKindCount {
                kind: serde_json::json!(kind),
                count: serde_json::json!(count),
            })
            .collect();

        // Edge table counts — derive tables from the shared graph catalog and
        // tolerate missing tables for older/stale indexes.
        let edge_tables = graph_relationship_table_names();
        let mut edge_counts = serde_json::Map::new();
        for table in &edge_tables {
            let count = db
                .reads()
                .count_table_rows(table)
                .map(|n| serde_json::json!(n))
                .unwrap_or(serde_json::json!(0));
            edge_counts.insert(table.to_string(), count);
        }

        // Total files and chunks
        let file_count = db
            .reads()
            .count_table_rows("files")
            .map(|n| serde_json::json!(n))
            .unwrap_or(serde_json::json!(0));

        let chunk_count = db
            .reads()
            .count_table_rows("chunks")
            .map(|n| serde_json::json!(n))
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
        // Grouped dispatch/synthesis/resolution counters (each sub-query is
        // best-effort and degrades to an empty breakdown inside cc-db).
        let provenance = db.reads().call_edge_provenance().unwrap_or_default();

        let total_call_edges: i64 = provenance
            .by_dispatch_kind
            .iter()
            .map(|(_, cnt)| *cnt)
            .sum();

        // Classify: tree_sitter = exact + qualified + scope_resolved, heuristic = heuristic, unresolved = unresolved
        let mut tree_sitter: i64 = 0;
        let mut heuristic: i64 = 0;
        let mut unresolved: i64 = 0;
        for (kind, cnt) in &provenance.by_resolution_kind {
            match kind.as_deref().unwrap_or("") {
                "exact" | "qualified" | "scope_resolved" => tree_sitter += cnt,
                "heuristic" => heuristic += cnt,
                "unresolved" | "" => unresolved += cnt,
                _ => heuristic += cnt,
            }
        }

        // Dispatch kind breakdown
        let mut dispatch_breakdown = serde_json::Map::new();
        for (kind, cnt) in &provenance.by_dispatch_kind {
            dispatch_breakdown.insert(
                kind.clone().unwrap_or_else(|| "unknown".to_string()),
                serde_json::json!(cnt),
            );
        }

        // Synthesized-by breakdown
        let mut synth_breakdown = serde_json::Map::new();
        for (by, cnt) in &provenance.by_synthesized_by {
            synth_breakdown.insert(
                by.clone().unwrap_or_else(|| "unknown".to_string()),
                serde_json::json!(cnt),
            );
        }

        EdgeProvenanceSummary {
            total_call_edges,
            by_resolution: ResolutionBreakdown {
                tree_sitter,
                heuristic,
                unresolved,
            },
            synthesized: provenance.synthesized_total,
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
        match db.reads().runtime_evidence_stats() {
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
    let cross_file_rows = db.reads().cross_file_call_file_pairs()?;

    let mut pkg_counts: HashMap<(String, String), u32> = HashMap::new();
    for (from_fp, to_fp) in &cross_file_rows {
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
        let conn = db.reads().read_conn().unwrap();
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
        let schema = idx.graph().graph_schema().unwrap();

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
    fn explore_symbols_surfaces_declared_relations_subset() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let result = idx
            .graph()
            .explore_symbols(
                &["A".to_string()],
                None,
                None,
                false,
                true,
                true,
                false,
                None,
            )
            .unwrap();

        // Additive envelope on the bypass surface: declared kinds only.
        assert_eq!(
            result["graph_explain"]["declared_edge_kinds"],
            serde_json::json!(cc_model::graph_catalog::tool_graph_subsets::RELATIONS.kinds())
        );
        // Existing fields stay untouched.
        assert!(result["symbols"].is_array());
    }

    #[test]
    fn detect_impact_high_confidence_threshold_filters_callers() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let report = idx
            .impact()
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
    fn detect_impact_with_default_budget_matches_handler_caps() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let limit: usize = 20;

        // Engine path with defaults filled in.
        let with_defaults = idx
            .impact()
            .detect_impact_with_default_budget(&["src/a.rs".to_string()], None, limit, None, None)
            .unwrap();

        // The exact caps the MCP handler used to compute inline:
        // node cap = limit×10 (≤5000), layer cap = 500, result cap = limit.
        let manual = idx
            .impact()
            .detect_impact_capped(
                &["src/a.rs".to_string()],
                None,
                Some(limit),
                Some(limit.saturating_mul(10).min(5000)),
                Some(500),
            )
            .unwrap();

        assert_eq!(
            serde_json::to_value(&with_defaults).unwrap(),
            serde_json::to_value(&manual).unwrap()
        );
    }

    #[test]
    fn detect_impact_without_threshold_keeps_callers() {
        let (_dir, idx) = index_with_low_confidence_edge();
        let report = idx
            .impact()
            .detect_impact(&["src/a.rs".to_string()], None)
            .unwrap();
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
