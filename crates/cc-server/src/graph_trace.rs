//! Trace path algorithms: BFS-based call path discovery.

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::graph_read_model::GraphReadModel;
use crate::graph_types::{
    BfsAdj, DisambiguationCandidate, DisambiguationInfo, LabeledPath, TraceNode, TracePathResult,
};
use crate::symbol_resolution::{resolve, Resolution, ResolutionOpts};

/// Build edge-labeled adjacency from call_uid_edges_lite.
#[cfg(test)]
pub fn build_bfs_adj(db: &IndexDb) -> CcResult<BfsAdj> {
    GraphReadModel::call_adjacency(db)
}

/// Build edge-labeled adjacency augmented with cross-service HTTP/async bridge edges.
///
/// Starts from the base call_edges adjacency, then loads http_call_edges and route_nodes
/// to synthesize edges that connect HTTP callers to route handler symbols.
#[allow(dead_code)]
pub fn build_bfs_adj_full(db: &IndexDb) -> CcResult<BfsAdj> {
    GraphReadModel::call_adjacency_with_bridges(db)
}

/// BFS returning labeled paths (node UIDs + edge data).
#[cfg(test)]
pub fn bfs_paths_labeled(
    adj: &BfsAdj,
    from_uid: &str,
    to_uid: &str,
    max_depth: usize,
    max_paths: usize,
) -> Vec<LabeledPath> {
    GraphReadModel::paths_between_adj(adj, from_uid, to_uid, max_depth, max_paths)
}

/// Legacy trace_path returning just symbol name arrays.
#[cfg(test)]
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
    let adj = GraphReadModel::call_adjacency(db)?;
    let paths = GraphReadModel::paths_between_adj(&adj, &from_uid, &to_uid, max_depth, usize::MAX)
        .into_iter()
        .map(|path| {
            path.node_uids
                .iter()
                .map(|uid| uid_names.get(uid).cloned().unwrap_or_else(|| uid.clone()))
                .collect()
        })
        .collect();
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
    let from_uid = resolve_symbol_uid(db, from, from_uid_override, "from", &mut disambiguation)?;
    let to_uid = resolve_symbol_uid(db, to, to_uid_override, "to", &mut disambiguation)?;

    // 2. Lazy BFS: load edges on demand instead of pre-loading the full graph.
    let read_model = GraphReadModel::new(Arc::clone(db))?;
    let labeled_paths = read_model.paths_between(&from_uid, &to_uid, max_depth, 20);

    // 3. Collect all unique UIDs across all paths.
    let uid_vec = collect_unique_uids(&labeled_paths);

    // 4. Bulk lookup symbol metadata.
    let sym_map = db.symbol_rows_by_uids(&uid_vec)?;

    // Preload the uid→name map once (only needed for outgoing-call labels); doing
    // this per node re-scanned the entire symbols table on every BFS node.
    let uid_names = if include_outgoing {
        db.symbol_names_by_uid()?
    } else {
        HashMap::new()
    };

    // 5. Build TraceNode for each unique symbol, with optional snippet.
    let nodes = build_trace_nodes(
        &uid_vec,
        &sym_map,
        db,
        &read_model,
        &uid_names,
        project_root,
        include_snippets,
        max_snippet_lines,
        snippet_budget_override,
        include_outgoing,
    );

    // 6. Build TraceEdge list and backward-compat name paths.
    let (paths, edges) = read_model.named_paths_and_trace_edges(&labeled_paths, &sym_map, true);

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

/// Resolve a symbol name (or UID override) to a UID, collecting disambiguation info.
fn resolve_symbol_uid(
    db: &Arc<IndexDb>,
    name: &str,
    uid_override: Option<&str>,
    role: &str,
    disambiguation: &mut Vec<DisambiguationInfo>,
) -> CcResult<String> {
    if let Some(uid) = uid_override.filter(|u| u.contains(':')) {
        return Ok(uid.to_string());
    }
    let not_found = || CcError::Search(format!("symbol not found: {}", name));
    match resolve(db, name, &ResolutionOpts::for_trace())? {
        Resolution::Unresolved(_) => Err(not_found()),
        Resolution::Unique(row) => row.symbol_uid.ok_or_else(not_found),
        Resolution::Ambiguous(candidates) => {
            let chosen = candidates
                .first()
                .and_then(|s| s.symbol_uid.clone())
                .ok_or_else(not_found)?;
            disambiguation.push(DisambiguationInfo {
                role: role.to_string(),
                query: name.to_string(),
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
            Ok(chosen)
        }
    }
}

/// Collect all unique UIDs across labeled BFS paths.
fn collect_unique_uids(labeled_paths: &[LabeledPath]) -> Vec<String> {
    let mut all_uids = HashSet::new();
    for lp in labeled_paths {
        for uid in &lp.node_uids {
            all_uids.insert(uid.clone());
        }
    }
    all_uids.into_iter().collect()
}

/// Build TraceNode list for each unique symbol, with optional snippets and outgoing calls.
#[allow(clippy::too_many_arguments)]
fn build_trace_nodes(
    uid_vec: &[String],
    sym_map: &HashMap<String, cc_db::index_db::SymbolRow>,
    db: &Arc<IndexDb>,
    read_model: &GraphReadModel,
    uid_names: &HashMap<String, String>,
    project_root: Option<&Path>,
    include_snippets: bool,
    max_snippet_lines: usize,
    snippet_budget_override: Option<usize>,
    include_outgoing: bool,
) -> Vec<TraceNode> {
    let mut snippet_budget: usize = snippet_budget_override.unwrap_or(32 * 1024);
    let mut nodes: Vec<TraceNode> = Vec::new();
    for uid in uid_vec {
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
                outgoing_call_names_lazy(uid_names, read_model, uid)
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
    nodes
}

/// Collect outgoing call names from lazy adjacency (used by trace_path_rich).
///
/// `uid_names` is the project-wide uid→name map, loaded once by the caller; doing
/// the lookup per node here previously re-scanned the whole symbols table each time.
fn outgoing_call_names_lazy(
    uid_names: &HashMap<String, String>,
    read_model: &GraphReadModel,
    uid: &str,
) -> Option<Vec<String>> {
    let neighbors = read_model.neighbors(uid);
    if neighbors.is_empty() {
        return None;
    }
    let mut names: Vec<String> = neighbors
        .iter()
        .filter_map(|e| {
            uid_names
                .get(&e.callee_uid)
                .cloned()
                .or_else(|| Some(e.callee_uid.clone()))
        })
        .collect();
    names.sort();
    names.dedup();
    Some(names)
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
        Some(crate::tools::utf8_prefix(&snippet, max_chars).to_string())
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
        let db = Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap().0);

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
                  parent_symbol_id, export_name, is_default_export,
                  framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
                 VALUES(?1,?2,?3,'function','src/lib.rs',NULL,?4,?5,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
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
