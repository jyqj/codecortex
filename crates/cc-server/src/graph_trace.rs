//! Trace path algorithms: BFS-based call path discovery.

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use crate::graph_types::{
    BfsAdj, DisambiguationCandidate, DisambiguationInfo, EdgeLite, LabeledPath, TraceEdge,
    TraceEdgeEvidence, TraceNode, TracePathResult,
};

/// Build edge-labeled adjacency from call_uid_edges_lite.
pub fn build_bfs_adj(db: &IndexDb) -> CcResult<BfsAdj> {
    let edges = db.call_uid_edges_lite()?;
    let mut adj: HashMap<String, Vec<EdgeLite>> = HashMap::new();
    for e in edges {
        adj.entry(e.caller_uid.clone()).or_default().push(e);
    }
    Ok(BfsAdj { adj })
}

/// Build edge-labeled adjacency augmented with cross-service HTTP/async bridge edges.
///
/// Starts from the base call_edges adjacency, then loads http_call_edges and route_nodes
/// to synthesize edges that connect HTTP callers to route handler symbols.
pub fn build_bfs_adj_full(db: &IndexDb) -> CcResult<BfsAdj> {
    let mut bfs = build_bfs_adj(db)?;

    // Load HTTP call edges and route nodes for bridge synthesis.
    let http_edges = db.all_http_call_edges_lite(5000)?;
    let route_nodes = db.all_route_nodes_lite(5000)?;

    // Build lookup: normalized_path → Vec<(handler_symbol_uid, confidence)>
    let mut route_lookup: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for rn in &route_nodes {
        if let (Some(ref norm_path), Some(ref handler_uid)) =
            (&rn.normalized_path, &rn.handler_symbol_uid)
        {
            route_lookup
                .entry(norm_path.clone())
                .or_default()
                .push((handler_uid.clone(), rn.confidence));
        }
    }

    // For each HTTP call edge, synthesize bridge edges to matching route handlers.
    for hce in &http_edges {
        let caller_uid = match &hce.caller_symbol_uid {
            Some(uid) => uid,
            None => continue,
        };
        let norm_path = match &hce.normalized_path {
            Some(p) => p,
            None => continue,
        };
        if let Some(handlers) = route_lookup.get(norm_path) {
            for (handler_uid, handler_confidence) in handlers {
                let dispatch_kind = if hce.call_kind == "http" {
                    "http_bridge"
                } else {
                    "async_bridge"
                };
                let bridge_edge = EdgeLite {
                    caller_uid: caller_uid.clone(),
                    callee_uid: handler_uid.clone(),
                    dispatch_kind: dispatch_kind.to_string(),
                    synthesized_by: Some("http_bridge".to_string()),
                    synthesis_key: Some(norm_path.clone()),
                    confidence: f64::min(hce.confidence, *handler_confidence),
                    file_path: hce.file_path.clone(),
                    line: hce.line,
                    registered_file: None,
                    registered_line: None,
                    resolution_kind: Some("synthesized".to_string()),
                    parser_tier: None,
                    resolution_strategy: None,
                    parser_confidence: None,
                };
                bfs.adj
                    .entry(caller_uid.clone())
                    .or_default()
                    .push(bridge_edge);
            }
        }
    }

    Ok(bfs)
}

/// BFS returning labeled paths (node UIDs + edge data).
pub fn bfs_paths_labeled(
    adj: &BfsAdj,
    from_uid: &str,
    to_uid: &str,
    max_depth: usize,
    max_paths: usize,
) -> Vec<LabeledPath> {
    let mut results = Vec::new();
    let mut queue: VecDeque<(Vec<String>, Vec<EdgeLite>)> = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back((vec![from_uid.to_string()], Vec::new()));
    visited.insert(from_uid.to_string());

    while let Some((nodes, edges)) = queue.pop_front() {
        if results.len() >= max_paths {
            break;
        }
        if nodes.len() > max_depth + 1 {
            break;
        }
        let current = nodes.last().unwrap();
        if current == to_uid {
            results.push(LabeledPath {
                node_uids: nodes,
                edge_lites: edges,
            });
            continue;
        }
        if let Some(neighbors) = adj.adj.get(current.as_str()) {
            for edge in neighbors {
                if !visited.contains(&edge.callee_uid) {
                    visited.insert(edge.callee_uid.clone());
                    let mut new_nodes = nodes.clone();
                    new_nodes.push(edge.callee_uid.clone());
                    let mut new_edges = edges.clone();
                    new_edges.push(edge.clone());
                    queue.push_back((new_nodes, new_edges));
                }
            }
        }
    }
    results
}

/// Legacy trace_path returning just symbol name arrays.
pub fn trace_path_names(
    db: &Arc<IndexDb>,
    from: &str,
    to: &str,
    max_depth: usize,
) -> CcResult<Vec<Vec<String>>> {
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

/// Rich trace_path: resolves names → UIDs, runs BFS, returns full metadata.
///
/// `snippet_budget_override` – if `Some(n)`, use `n` as the total snippet byte budget
/// instead of the default 32 KiB.
/// `include_outgoing` – when true, populate `outgoing_calls` on each TraceNode.
/// `from_uid_override` / `to_uid_override` – if `Some`, skip `find_symbol` for that
/// endpoint and use the provided UID directly (must contain `":"`).
#[allow(clippy::too_many_arguments)]
pub fn trace_path_rich(
    db: &Arc<IndexDb>,
    project_root: Option<&Path>,
    from: &str,
    to: &str,
    max_depth: usize,
    include_snippets: bool,
    max_snippet_lines: usize,
    snippet_budget_override: Option<usize>,
    include_outgoing: bool,
    from_uid_override: Option<&str>,
    to_uid_override: Option<&str>,
) -> CcResult<TracePathResult> {
    // 1. Resolve from/to names to UIDs, with disambiguation.
    let mut disambiguation: Vec<DisambiguationInfo> = Vec::new();

    let from_uid = if let Some(uid) = from_uid_override.filter(|u| u.contains(':')) {
        uid.to_string()
    } else {
        let candidates = db.find_symbol(from, true, 5)?;
        let chosen = candidates
            .first()
            .and_then(|s| s.symbol_uid.clone())
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", from)))?;
        if candidates.len() > 1 {
            disambiguation.push(DisambiguationInfo {
                role: "from".to_string(),
                query: from.to_string(),
                chosen_uid: chosen.clone(),
                chosen_file: candidates[0].file_path.clone(),
                candidates: candidates
                    .iter()
                    .filter_map(|s| {
                        Some(DisambiguationCandidate {
                            uid: s.symbol_uid.clone()?,
                            name: s.name.clone(),
                            file_path: s.file_path.clone(),
                            kind: s.kind.clone(),
                            start_line: s.start_line,
                        })
                    })
                    .collect(),
            });
        }
        chosen
    };

    let to_uid = if let Some(uid) = to_uid_override.filter(|u| u.contains(':')) {
        uid.to_string()
    } else {
        let candidates = db.find_symbol(to, true, 5)?;
        let chosen = candidates
            .first()
            .and_then(|s| s.symbol_uid.clone())
            .ok_or_else(|| CcError::Search(format!("symbol not found: {}", to)))?;
        if candidates.len() > 1 {
            disambiguation.push(DisambiguationInfo {
                role: "to".to_string(),
                query: to.to_string(),
                chosen_uid: chosen.clone(),
                chosen_file: candidates[0].file_path.clone(),
                candidates: candidates
                    .iter()
                    .filter_map(|s| {
                        Some(DisambiguationCandidate {
                            uid: s.symbol_uid.clone()?,
                            name: s.name.clone(),
                            file_path: s.file_path.clone(),
                            kind: s.kind.clone(),
                            start_line: s.start_line,
                        })
                    })
                    .collect(),
            });
        }
        chosen
    };

    // 2. Build edge-labeled adjacency (with HTTP/async bridge edges) and run BFS.
    let adj = build_bfs_adj_full(db)?;
    let labeled_paths = bfs_paths_labeled(&adj, &from_uid, &to_uid, max_depth, 20);

    // 3. Collect all unique UIDs across all paths.
    let mut all_uids = HashSet::new();
    for lp in &labeled_paths {
        for uid in &lp.node_uids {
            all_uids.insert(uid.clone());
        }
    }
    let uid_vec: Vec<String> = all_uids.into_iter().collect();

    // 4. Bulk lookup symbol metadata.
    let sym_map = db.symbol_rows_by_uids(&uid_vec)?;

    // 5. Build TraceNode for each unique symbol, with optional snippet.
    let mut snippet_budget: usize = snippet_budget_override.unwrap_or(32 * 1024);
    let mut nodes: Vec<TraceNode> = Vec::new();
    for uid in &uid_vec {
        if let Some(row) = sym_map.get(uid) {
            let snippet = if include_snippets && snippet_budget > 0 {
                let effective_max_lines = if max_snippet_lines == usize::MAX {
                    (row.end_line.saturating_sub(row.start_line) + 1) as usize
                } else {
                    max_snippet_lines
                };
                read_snippet(
                    project_root,
                    db,
                    &row.file_path,
                    row.start_line,
                    effective_max_lines,
                    &mut snippet_budget,
                )
            } else {
                None
            };

            let outgoing_calls = if include_outgoing {
                outgoing_call_names(db, &adj, uid)
            } else {
                None
            };

            nodes.push(TraceNode {
                uid: uid.clone(),
                name: row.name.clone(),
                kind: row.kind.clone(),
                file_path: row.file_path.clone(),
                start_line: row.start_line,
                end_line: row.end_line,
                signature: row.signature.clone(),
                snippet,
                outgoing_calls,
            });
        }
    }

    // 6. Build TraceEdge list and backward-compat name paths.
    let uid_names: HashMap<String, String> = sym_map
        .iter()
        .map(|(uid, row)| (uid.clone(), row.name.clone()))
        .collect();

    let mut edges: Vec<TraceEdge> = Vec::new();
    let mut edge_seen: HashSet<(String, String, u32)> = HashSet::new();
    let mut paths: Vec<Vec<String>> = Vec::new();

    // Collect all synthesis_keys from http_bridge edges for evidence lookup
    let mut bridge_norm_paths: Vec<String> = Vec::new();
    for lp in &labeled_paths {
        for el in &lp.edge_lites {
            if el.synthesized_by.as_deref() == Some("http_bridge") {
                if let Some(ref sk) = el.synthesis_key {
                    bridge_norm_paths.push(sk.clone());
                }
            }
        }
    }
    bridge_norm_paths.sort();
    bridge_norm_paths.dedup();

    // Bulk-query evidence for all bridge normalized paths
    let evidence_map = if !bridge_norm_paths.is_empty() {
        db.evidence_for_normalized_paths(&bridge_norm_paths)
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    for lp in &labeled_paths {
        let named: Vec<String> = lp
            .node_uids
            .iter()
            .map(|uid| uid_names.get(uid).cloned().unwrap_or_else(|| uid.clone()))
            .collect();
        paths.push(named);

        for el in &lp.edge_lites {
            let key = (el.caller_uid.clone(), el.callee_uid.clone(), el.line);
            if edge_seen.insert(key) {
                // Attach runtime evidence for http_bridge edges
                let evidence = if el.synthesized_by.as_deref() == Some("http_bridge") {
                    el.synthesis_key
                        .as_ref()
                        .and_then(|sk| evidence_map.get(sk))
                        .map(|(count, last_seen)| TraceEdgeEvidence {
                            observed_count: *count,
                            last_seen: last_seen.clone(),
                        })
                } else {
                    None
                };

                edges.push(TraceEdge {
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
                    evidence,
                });
            }
        }
    }

    let path_count = paths.len();
    let diagnostic = if path_count == 0 {
        Some(format!(
            "No call path found from '{}' to '{}'. The gap may be due to: dynamic dispatch, \
             callback/closure, framework bridge, or async message passing. \
             Try: relations(symbol='{}', kind='callees') to see outgoing calls from the source.",
            from, to, from
        ))
    } else {
        None
    };
    Ok(TracePathResult {
        paths,
        nodes,
        edges,
        path_count,
        disambiguation,
        diagnostic,
    })
}

/// Collect outgoing call names for a symbol UID from the BFS adjacency.
fn outgoing_call_names(db: &Arc<IndexDb>, adj: &BfsAdj, uid: &str) -> Option<Vec<String>> {
    let neighbors = adj.adj.get(uid)?;
    if neighbors.is_empty() {
        return None;
    }
    let uid_names = db.symbol_names_by_uid().ok()?;
    let names: Vec<String> = neighbors
        .iter()
        .filter_map(|e| {
            uid_names
                .get(&e.callee_uid)
                .cloned()
                .or_else(|| Some(e.callee_uid.clone()))
        })
        .collect::<Vec<_>>();
    let mut deduped = names;
    deduped.sort();
    deduped.dedup();
    Some(deduped)
}

/// Read a source snippet for a symbol, identified by file_path + line range.
///
/// Returns `None` if the file cannot be read or the lines are out of range.
/// The snippet is truncated to `max_chars` characters.
pub(crate) fn read_symbol_snippet(
    db: &IndexDb,
    project_root: Option<&Path>,
    file_path: &str,
    start_line: u32,
    end_line: u32,
    max_chars: usize,
) -> Option<String> {
    let root = project_root?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(root, file_path, db).ok()?;
    let content = std::fs::read_to_string(&full_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start = start_line.saturating_sub(1) as usize;
    let end = (end_line as usize).min(lines.len());
    if start >= lines.len() || start >= end {
        return None;
    }
    let snippet: String = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}| {}", start + i + 1, line))
        .collect::<Vec<_>>()
        .join("\n");
    if snippet.len() > max_chars {
        Some(snippet[..max_chars].to_string())
    } else {
        Some(snippet)
    }
}

/// Read a source snippet for a symbol node, respecting budget and safety guards.
fn read_snippet(
    project_root: Option<&Path>,
    db: &IndexDb,
    file_path: &str,
    start_line: u32,
    max_lines: usize,
    budget: &mut usize,
) -> Option<String> {
    let root = project_root?;
    let full_path = crate::path_guard::resolve_indexed_path_strict(root, file_path, db).ok()?;
    let content = std::fs::read_to_string(&full_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start = start_line.saturating_sub(1) as usize;
    let end = (start + max_lines).min(lines.len());
    if start >= lines.len() {
        return None;
    }
    let snippet: String = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}| {}", start + i + 1, line))
        .collect::<Vec<_>>()
        .join("\n");
    let byte_len = snippet.len();
    if byte_len > *budget {
        return None;
    }
    *budget -= byte_len;
    Some(snippet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create an IndexDb and insert symbols + call_edges for A→B→C.
    fn setup_abc_graph() -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap());

        // Use the read pool connection for inserts (WAL mode allows this).
        let conn = db.read_conn().unwrap();

        // Insert a file record (needed for foreign key / indexed checks).
        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/lib.rs','Rust','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();

        // Insert three symbols: A, B, C
        for (sid, uid, name, start, end) in [
            ("s1", "uid_a", "fn_a", 1, 5),
            ("s2", "uid_b", "fn_b", 10, 15),
            ("s3", "uid_c", "fn_c", 20, 25),
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

        // Insert call edges: A→B, B→C
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
    fn trace_path_names_abc() {
        let (_tmp, db) = setup_abc_graph();
        let paths = trace_path_names(&db, "fn_a", "fn_c", 5).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], vec!["fn_a", "fn_b", "fn_c"]);
    }

    #[test]
    fn bfs_paths_labeled_finds_edges() {
        let (_tmp, db) = setup_abc_graph();
        let adj = build_bfs_adj(&db).unwrap();
        let paths = bfs_paths_labeled(&adj, "uid_a", "uid_c", 5, 10);
        assert_eq!(paths.len(), 1);

        let lp = &paths[0];
        assert_eq!(lp.node_uids, vec!["uid_a", "uid_b", "uid_c"]);
        assert_eq!(lp.edge_lites.len(), 2);
        assert_eq!(lp.edge_lites[0].caller_uid, "uid_a");
        assert_eq!(lp.edge_lites[0].callee_uid, "uid_b");
        assert_eq!(lp.edge_lites[0].dispatch_kind, "static");
        assert_eq!(lp.edge_lites[1].caller_uid, "uid_b");
        assert_eq!(lp.edge_lites[1].callee_uid, "uid_c");
    }

    #[test]
    fn trace_path_rich_returns_full_result() {
        let (_tmp, db) = setup_abc_graph();
        let result = trace_path_rich(
            &db, None, "fn_a", "fn_c", 5, false, 10, None, false, None, None,
        )
        .unwrap();
        assert_eq!(result.path_count, 1);
        assert_eq!(result.paths[0], vec!["fn_a", "fn_b", "fn_c"]);
        assert_eq!(result.nodes.len(), 3);
        assert_eq!(result.edges.len(), 2);

        // Verify edge metadata
        let edge_a_b = result
            .edges
            .iter()
            .find(|e| e.from_uid == "uid_a" && e.to_uid == "uid_b")
            .expect("edge A→B");
        assert_eq!(edge_a_b.dispatch_kind, "static");
        assert_eq!(edge_a_b.line, 3);
    }
}
