//! Multi-symbol flow discovery: find call paths connecting multiple symbols.

use cc_db::index_db::IndexDb;
use cc_model::CcResult;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::graph_trace::{bfs_paths_labeled, build_bfs_adj_full, read_symbol_snippet};
use crate::graph_types::{
    AmbiguousSymbol, DisconnectedSymbol, FlowPath, FlowResult, SymbolBrief, TraceEdge, TraceNode,
    UnresolvedSymbol,
};

/// Discover call flow paths connecting multiple symbols.
///
/// `max_output_chars` – if `Some(n)`, truncate `flow_paths` so the serialized
/// JSON result stays within `n` characters.
#[allow(clippy::too_many_arguments)]
pub fn explore_flow(
    db: &Arc<IndexDb>,
    project_root: Option<&Path>,
    symbols: &[String],
    max_depth: usize,
    include_source: bool,
    max_paths_per_pair: usize,
    exact: bool,
    file_path_filter: Option<&str>,
    max_candidates: usize,
    max_output_chars: Option<usize>,
) -> CcResult<serde_json::Value> {
    // 1. Cap symbols to 10.
    let symbols = &symbols[..symbols.len().min(10)];

    // 2. Resolve each symbol name → UID with three-state handling.
    let mut unresolved: Vec<UnresolvedSymbol> = Vec::new();
    let mut ambiguous: Vec<AmbiguousSymbol> = Vec::new();
    let mut resolved: Vec<(String, String)> = Vec::new(); // (name, uid)

    for name in symbols {
        let mut rows = db.find_symbol(name, exact, max_candidates)?;

        if rows.is_empty() {
            unresolved.push(UnresolvedSymbol {
                query: name.clone(),
            });
            continue;
        }

        // Apply file_path_filter if provided
        if let Some(filter) = file_path_filter {
            rows.retain(|r| r.file_path.contains(filter));
        }

        if rows.is_empty() {
            unresolved.push(UnresolvedSymbol {
                query: name.clone(),
            });
            continue;
        }

        if rows.len() > 1 {
            // Co-location heuristic: if one candidate shares a directory with
            // an already-resolved symbol, prefer it over the others.
            let colocated = rows.iter().find(|r| {
                let candidate_dir = dir_of(&r.file_path);
                resolved.iter().any(|(_, resolved_uid)| {
                    // Look up the resolved symbol's file_path from the DB
                    if let Ok(map) = db.symbol_rows_by_uids(std::slice::from_ref(resolved_uid)) {
                        if let Some(resolved_row) = map.get(resolved_uid) {
                            return dir_of(&resolved_row.file_path) == candidate_dir;
                        }
                    }
                    false
                })
            });

            if let Some(best) = colocated {
                if let Some(uid) = &best.symbol_uid {
                    resolved.push((name.clone(), uid.clone()));
                    continue;
                }
            }

            let candidates: Vec<SymbolBrief> = rows
                .iter()
                .filter_map(|r| {
                    Some(SymbolBrief {
                        name: r.name.clone(),
                        kind: r.kind.clone(),
                        file_path: r.file_path.clone(),
                        uid: r.symbol_uid.clone()?,
                    })
                })
                .collect();
            ambiguous.push(AmbiguousSymbol {
                query: name.clone(),
                candidates,
            });
            continue;
        }

        // Exactly 1 result
        let row = &rows[0];
        if let Some(uid) = &row.symbol_uid {
            resolved.push((name.clone(), uid.clone()));
        } else {
            unresolved.push(UnresolvedSymbol {
                query: name.clone(),
            });
        }
    }

    // 3. Build BFS adjacency once.
    let adj = build_bfs_adj_full(db)?;

    // 4. Pairwise path finding.
    let uid_names = db.symbol_names_by_uid()?;

    let mut all_labeled_paths = Vec::new();
    let mut path_endpoints: Vec<(usize, usize)> = Vec::new(); // indices into resolved

    for i in 0..resolved.len() {
        for j in (i + 1)..resolved.len() {
            let uid_i = &resolved[i].1;
            let uid_j = &resolved[j].1;

            // Forward: i → j
            let fwd = bfs_paths_labeled(&adj, uid_i, uid_j, max_depth, max_paths_per_pair);
            for lp in fwd {
                path_endpoints.push((i, j));
                all_labeled_paths.push(lp);
            }

            // Reverse: j → i
            let rev = bfs_paths_labeled(&adj, uid_j, uid_i, max_depth, max_paths_per_pair);
            for lp in rev {
                path_endpoints.push((j, i));
                all_labeled_paths.push(lp);
            }
        }
    }

    // 5. Collect unique nodes and edges from all paths.
    let mut all_uids = HashSet::new();
    let mut edge_seen: HashSet<(String, String, u32)> = HashSet::new();
    let mut flow_edges: Vec<TraceEdge> = Vec::new();

    for lp in &all_labeled_paths {
        for uid in &lp.node_uids {
            all_uids.insert(uid.clone());
        }
        for el in &lp.edge_lites {
            let key = (el.caller_uid.clone(), el.callee_uid.clone(), el.line);
            if edge_seen.insert(key) {
                flow_edges.push(TraceEdge {
                    from_uid: el.caller_uid.clone(),
                    to_uid: el.callee_uid.clone(),
                    dispatch_kind: el.dispatch_kind.clone(),
                    synthesized_by: el.synthesized_by.clone(),
                    synthesis_key: el.synthesis_key.clone(),
                    confidence: el.confidence,
                    file_path: el.file_path.clone(),
                    line: el.line,
                    registered_file: el.registered_file.clone(),
                    registered_line: el.registered_line,
                    resolution_kind: el.resolution_kind.clone(),
                    parser_tier: el.parser_tier.clone(),
                    resolution_strategy: el.resolution_strategy.clone(),
                    parser_confidence: el.parser_confidence,
                    evidence: None,
                });
            }
        }
    }

    let uid_vec: Vec<String> = all_uids.iter().cloned().collect();
    let sym_map = db.symbol_rows_by_uids(&uid_vec)?;

    // Per-node snippet budget: 32 KiB total, shared across all nodes.
    let mut snippet_budget: usize = 32 * 1024;

    let flow_nodes: Vec<TraceNode> = uid_vec
        .iter()
        .filter_map(|uid| {
            let sym = sym_map.get(uid)?;
            let snippet = if include_source && snippet_budget > 0 {
                let max_chars = snippet_budget.min(4096); // cap per-symbol
                let s = read_symbol_snippet(
                    db,
                    project_root,
                    &sym.file_path,
                    sym.start_line,
                    sym.end_line,
                    max_chars,
                );
                if let Some(ref text) = s {
                    snippet_budget = snippet_budget.saturating_sub(text.len());
                }
                s
            } else {
                None
            };
            Some(TraceNode {
                uid: sym.symbol_uid.clone().unwrap_or_default(),
                name: sym.name.clone(),
                kind: sym.kind.clone(),
                file_path: sym.file_path.clone(),
                start_line: sym.start_line,
                end_line: sym.end_line,
                signature: sym.signature.clone(),
                snippet,
                outgoing_calls: None,
            })
        })
        .collect();

    // 6. Disconnected symbols: resolved symbols whose UID appears in no discovered path.
    let mut connected_uids: HashSet<String> = HashSet::new();
    for lp in &all_labeled_paths {
        for uid in &lp.node_uids {
            connected_uids.insert(uid.clone());
        }
    }

    let disconnected: Vec<DisconnectedSymbol> = resolved
        .iter()
        .filter(|(_, uid)| !connected_uids.contains(uid))
        .filter_map(|(name, uid)| {
            let sym = sym_map.get(uid).or(
                // Symbol may not be in sym_map if it wasn't in any path;
                // we need to look it up individually.
                None,
            );
            if let Some(sym) = sym {
                Some(DisconnectedSymbol {
                    name: name.clone(),
                    kind: sym.kind.clone(),
                    file_path: sym.file_path.clone(),
                    uid: uid.clone(),
                })
            } else {
                // Fetch from db for disconnected symbols not in any path
                let rows = db.symbol_rows_by_uids(std::slice::from_ref(uid)).ok()?;
                let sym = rows.get(uid)?;
                Some(DisconnectedSymbol {
                    name: name.clone(),
                    kind: sym.kind.clone(),
                    file_path: sym.file_path.clone(),
                    uid: uid.clone(),
                })
            }
        })
        .collect();

    // 7. Build FlowPath entries.
    let flow_paths: Vec<FlowPath> = all_labeled_paths
        .iter()
        .zip(path_endpoints.iter())
        .map(|(lp, &(from_idx, to_idx))| {
            let path_names: Vec<String> = lp
                .node_uids
                .iter()
                .map(|uid| uid_names.get(uid).cloned().unwrap_or_else(|| uid.clone()))
                .collect();
            let length = lp.node_uids.len();
            FlowPath {
                from_symbol: resolved[from_idx].0.clone(),
                to_symbol: resolved[to_idx].0.clone(),
                path_names,
                path_uids: lp.node_uids.clone(),
                length,
            }
        })
        .collect();

    // 8. Summary string.
    let connected_count = resolved.len() - disconnected.len();
    let total_symbols = symbols.len();
    let summary = format!(
        "Found {} paths connecting {} of {} symbols",
        flow_paths.len(),
        connected_count,
        total_symbols
    );

    // 9. Optionally truncate flow_paths to stay within the output budget.
    let flow_paths = if let Some(budget) = max_output_chars {
        let mut trimmed = flow_paths;
        loop {
            let candidate = FlowResult {
                flow_paths: trimmed.clone(),
                flow_nodes: flow_nodes.clone(),
                flow_edges: flow_edges.clone(),
                unresolved: unresolved.clone(),
                ambiguous: ambiguous.clone(),
                disconnected: disconnected.clone(),
                summary: summary.clone(),
            };
            let serialized = serde_json::to_string(&candidate).unwrap_or_default();
            if serialized.len() <= budget || trimmed.is_empty() {
                break trimmed;
            }
            trimmed.pop();
        }
    } else {
        flow_paths
    };

    // 10. Build FlowResult and serialize.
    let result = FlowResult {
        flow_paths,
        flow_nodes,
        flow_edges,
        unresolved,
        ambiguous,
        disconnected,
        summary,
    };

    Ok(serde_json::to_value(result).unwrap_or_default())
}

/// Return the directory portion of a file_path (everything before the last `/`).
fn dir_of(file_path: &str) -> &str {
    match file_path.rfind('/') {
        Some(pos) => &file_path[..pos],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create an IndexDb and insert symbols + call_edges for A→B→C, with D isolated.
    fn setup_abcd_graph() -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap());

        let conn = db.read_conn().unwrap();

        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/lib.rs','Rust','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();

        // Insert four symbols: A, B, C (connected), D (isolated)
        for (sid, uid, name, start, end) in [
            ("s1", "uid_a", "fn_a", 1, 5),
            ("s2", "uid_b", "fn_b", 10, 15),
            ("s3", "uid_c", "fn_c", 20, 25),
            ("s4", "uid_d", "fn_d", 30, 35),
        ] {
            conn.execute(
                "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
                  start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
                  parent_symbol_id, scope_id, export_name, is_default_export,
                  framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
                 VALUES(?1,?2,?3,'function','src/lib.rs',NULL,?4,?5,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params![sid, uid, name, start, end],
            ).unwrap();
        }

        // Insert call edges: A→B, B→C (D has no edges)
        for (eid, caller_uid, callee_uid, callee_name, line) in [
            ("e1", "uid_a", "uid_b", "fn_b", 3),
            ("e2", "uid_b", "uid_c", "fn_c", 12),
        ] {
            conn.execute(
                "INSERT INTO call_edges(edge_id, file_path, caller_symbol_uid, callee_symbol_uid, callee_symbol, line,
                  dispatch_kind, resolution_kind, resolution_confidence, synthesized_by, synthesis_key, registered_file, registered_line,
                  parser_tier, parser_confidence)
                 VALUES(?1,'src/lib.rs',?2,?3,?4,?5,'static','exact',1.0,NULL,NULL,NULL,NULL,'tree_sitter',1.0)",
                rusqlite::params![eid, caller_uid, callee_uid, callee_name, line],
            ).unwrap();
        }

        (tmp, db)
    }

    #[test]
    fn explore_flow_finds_paths_and_disconnected() {
        let (_tmp, db) = setup_abcd_graph();
        let result = explore_flow(
            &db,
            None,
            &["fn_a".into(), "fn_c".into(), "fn_d".into()],
            5,
            false,
            10,
            true,
            None,
            10,
            None,
        )
        .unwrap();

        let flow: FlowResult = serde_json::from_value(result).unwrap();

        // Should find at least one path connecting A and C (A→B→C)
        assert!(
            !flow.flow_paths.is_empty(),
            "expected at least one flow path"
        );
        let ac_path = flow
            .flow_paths
            .iter()
            .find(|p| p.from_symbol == "fn_a" && p.to_symbol == "fn_c")
            .expect("expected A→C path");
        assert_eq!(ac_path.path_names, vec!["fn_a", "fn_b", "fn_c"]);

        // D should be disconnected
        assert_eq!(flow.disconnected.len(), 1);
        assert_eq!(flow.disconnected[0].name, "fn_d");
        assert_eq!(flow.disconnected[0].uid, "uid_d");
    }

    #[test]
    fn explore_flow_unresolved_symbol() {
        let (_tmp, db) = setup_abcd_graph();
        let result = explore_flow(
            &db,
            None,
            &["NonExistent".into()],
            5,
            false,
            10,
            true,
            None,
            10,
            None,
        )
        .unwrap();

        let flow: FlowResult = serde_json::from_value(result).unwrap();
        assert_eq!(flow.unresolved.len(), 1);
        assert_eq!(flow.unresolved[0].query, "NonExistent");
    }

    #[test]
    fn explore_flow_ambiguous_symbol() {
        let (_tmp, db) = setup_abcd_graph();

        // Insert a second symbol with the same name but different file
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/other.rs','Rust','h2',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
              start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
              parent_symbol_id, scope_id, export_name, is_default_export,
              framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
             VALUES('s5','uid_a2','fn_a','function','src/other.rs',NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
            rusqlite::params![],
        ).unwrap();

        let result = explore_flow(
            &db,
            None,
            &["fn_a".into()],
            5,
            false,
            10,
            true,
            None,
            10,
            None,
        )
        .unwrap();

        let flow: FlowResult = serde_json::from_value(result).unwrap();
        assert_eq!(flow.ambiguous.len(), 1);
        assert_eq!(flow.ambiguous[0].query, "fn_a");
        assert_eq!(flow.ambiguous[0].candidates.len(), 2);
    }

    #[test]
    fn explore_flow_two_unrelated_symbols_both_disconnected() {
        let (_tmp, db) = setup_abcd_graph();

        // Insert another isolated symbol E
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
              start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
              parent_symbol_id, scope_id, export_name, is_default_export,
              framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
             VALUES('s5','uid_e','fn_e','function','src/lib.rs',NULL,40,45,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
            rusqlite::params![],
        ).unwrap();

        let result = explore_flow(
            &db,
            None,
            &["fn_d".into(), "fn_e".into()],
            5,
            false,
            10,
            true,
            None,
            10,
            None,
        )
        .unwrap();

        let flow: FlowResult = serde_json::from_value(result).unwrap();
        assert!(flow.flow_paths.is_empty(), "expected no flow paths");
        assert_eq!(flow.disconnected.len(), 2);

        let disc_names: HashSet<String> =
            flow.disconnected.iter().map(|d| d.name.clone()).collect();
        assert!(disc_names.contains("fn_d"));
        assert!(disc_names.contains("fn_e"));
    }
}
