//! Circular dependency detection via Tarjan's SCC algorithm.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};

use crate::graph_read_model::GraphReadModel;
use crate::graph_types::{CircularDepsResult, CycleComponent, InternalEdge};

// ── Tarjan's SCC (iterative) ───────────────────────────────────────

/// Iterative Tarjan's SCC. Returns only components with size > 1.
pub fn tarjan_scc(adj: &HashMap<String, Vec<String>>) -> Vec<Vec<String>> {
    // Collect all nodes (both keys and targets) so isolated targets are included.
    let mut all_nodes: Vec<&String> = adj.keys().collect();
    for targets in adj.values() {
        for t in targets {
            all_nodes.push(t);
        }
    }
    all_nodes.sort();
    all_nodes.dedup();

    let mut index_counter: u32 = 0;
    let mut result: Vec<Vec<String>> = Vec::new();
    let empty_vec: Vec<String> = Vec::new();

    // Iterative Tarjan using a work-stack with neighbor iterator index.
    struct TarjanState {
        index: u32,
        lowlink: u32,
        on_stack: bool,
    }

    let mut state: HashMap<&str, TarjanState> = HashMap::new();
    let mut scc_stack: Vec<&str> = Vec::new();

    // Work frame: (node, neighbor_index)
    let mut work: Vec<(&str, usize)> = Vec::new();

    for start in &all_nodes {
        let start_str: &str = start.as_str();
        if state.contains_key(start_str) {
            continue;
        }

        work.push((start_str, 0));
        state.insert(
            start_str,
            TarjanState {
                index: index_counter,
                lowlink: index_counter,
                on_stack: true,
            },
        );
        index_counter += 1;
        scc_stack.push(start_str);

        while let Some((v, ni)) = work.last_mut() {
            let v_str: &str = v;
            let neighbors = adj.get(v_str).map(|ns| ns.as_slice()).unwrap_or(&empty_vec);

            if *ni < neighbors.len() {
                let w: &str = neighbors[*ni].as_str();
                *ni += 1;

                if let Some(w_state) = state.get(w) {
                    if w_state.on_stack {
                        let w_idx = w_state.index;
                        if let Some(v_state) = state.get_mut(v_str) {
                            if w_idx < v_state.lowlink {
                                v_state.lowlink = w_idx;
                            }
                        }
                    }
                } else {
                    // Not visited — push onto work stack
                    state.insert(
                        w,
                        TarjanState {
                            index: index_counter,
                            lowlink: index_counter,
                            on_stack: true,
                        },
                    );
                    index_counter += 1;
                    scc_stack.push(w);
                    work.push((w, 0));
                }
            } else {
                // Done with all neighbors of v — pop and propagate lowlink.
                let v_str_owned = v_str;
                let v_index = state[v_str_owned].index;
                let v_lowlink = state[v_str_owned].lowlink;

                if v_lowlink == v_index {
                    // v is an SCC root — pop the SCC.
                    let mut component: Vec<String> = Vec::new();
                    while let Some(w) = scc_stack.pop() {
                        if let Some(ws) = state.get_mut(w) {
                            ws.on_stack = false;
                        }
                        component.push(w.to_string());
                        if w == v_str_owned {
                            break;
                        }
                    }
                    if component.len() > 1 {
                        result.push(component);
                    }
                }

                // Pop this frame off work stack.
                work.pop();

                // Propagate lowlink to parent.
                if let Some((parent, _)) = work.last() {
                    let parent_str: &str = parent;
                    let child_lowlink = state.get(v_str_owned).map(|s| s.lowlink).unwrap_or(0);
                    if let Some(parent_state) = state.get_mut(parent_str) {
                        if child_lowlink < parent_state.lowlink {
                            parent_state.lowlink = child_lowlink;
                        }
                    }
                }
            }
        }
    }

    result
}

// ── Package extraction helper ──────────────────────────────────────

/// Extract package name from a file path.
///
/// Takes directory path up to the first `/src/`, `/lib/`, `/app/` boundary,
/// or just the first 2 directory components.
fn extract_package(file_path: &str) -> String {
    let path = file_path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();

    // Look for src/lib/app boundary
    for (i, part) in parts.iter().enumerate() {
        if (*part == "src" || *part == "lib" || *part == "app") && i > 0 {
            return parts[..i].join("/");
        }
    }

    // Fallback: first 2 directory components (excluding the file name itself)
    if parts.len() >= 3 {
        // e.g. "packages/core/utils/foo.ts" → "packages/core"
        return parts[..2].join("/");
    }
    if parts.len() == 2 {
        // e.g. "core/foo.ts" → "core"
        return parts[0].to_string();
    }

    // Single component — use itself
    path
}

// ── Severity classification ────────────────────────────────────────

fn classify_severity(granularity: &str, size: usize) -> String {
    match granularity {
        "community" => "critical".to_string(),
        "package" => {
            if size >= 4 {
                "high".to_string()
            } else {
                "medium".to_string()
            }
        }
        _ => {
            // file level
            if size >= 5 {
                "high".to_string()
            } else if size >= 3 {
                "medium".to_string()
            } else {
                "low".to_string()
            }
        }
    }
}

// ── Main entry point ───────────────────────────────────────────────

/// Detect circular dependencies at the given granularity level.
pub fn find_circular_deps(
    db: &Arc<IndexDb>,
    granularity: &str,
    limit: usize,
) -> CcResult<CircularDepsResult> {
    match granularity {
        "file" => find_circular_deps_file(db, limit),
        "package" => find_circular_deps_package(db, limit),
        "community" => find_circular_deps_community(db, limit),
        other => Err(CcError::InvalidParams(format!(
            "unknown granularity: {other}; expected file|package|community"
        ))),
    }
}

/// Declared graph subset for this tool (`tool_graph_subsets::CYCLES`),
/// surfaced additively on every result: file/package granularity runs Tarjan
/// over IMPORTS adjacency, community granularity over the community-projected
/// CALLS adjacency. Visibility only — traversal is unchanged.
fn cycles_graph_explain() -> Option<cc_model::GraphExplain> {
    Some(cc_model::GraphExplain::declared_only(
        cc_model::graph_catalog::tool_graph_subsets::CYCLES,
    ))
}

// ── File-level circular deps ───────────────────────────────────────

fn find_circular_deps_file(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    let read_model = GraphReadModel::without_http_bridges(Arc::clone(db));
    let adj = read_model.file_import_adjacency()?;

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    // Sort SCCs by size descending BEFORE taking limit,
    // so we always keep the largest cycles even when limit truncates.
    let mut ordered: Vec<&Vec<String>> = sccs.iter().collect();
    ordered.sort_by_key(|scc| Reverse(scc.len()));

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in ordered.into_iter().take(limit) {
        let severity = classify_severity("file", scc.len());
        let internal_edges = collect_file_witness_edges(&read_model, scc)?;
        components.push(CycleComponent {
            size: scc.len(),
            nodes: scc.clone(),
            severity,
            internal_edges,
        });
    }

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "file".to_string(),
        components,
        total_components,
        shown,
        graph_explain: cycles_graph_explain(),
    })
}

/// Collect witness edges for a file-level SCC by querying the imports table.
fn collect_file_witness_edges(
    read_model: &GraphReadModel,
    scc: &[String],
) -> CcResult<Vec<InternalEdge>> {
    read_model.file_import_witness_edges(scc)
}

// ── Package-level circular deps ────────────────────────────────────

fn find_circular_deps_package(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    let read_model = GraphReadModel::without_http_bridges(Arc::clone(db));
    let adj = read_model.projected_import_adjacency(extract_package)?;

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    // Sort SCCs by size descending BEFORE taking limit,
    // so we always keep the largest cycles even when limit truncates.
    let mut ordered: Vec<&Vec<String>> = sccs.iter().collect();
    ordered.sort_by_key(|scc| Reverse(scc.len()));

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in ordered.into_iter().take(limit) {
        let severity = classify_severity("package", scc.len());
        // For package level, report edges between component nodes without detailed witness data.
        let internal_edges = collect_adj_edges(&adj, scc);
        components.push(CycleComponent {
            size: scc.len(),
            nodes: scc.clone(),
            severity,
            internal_edges,
        });
    }

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "package".to_string(),
        components,
        total_components,
        shown,
        graph_explain: cycles_graph_explain(),
    })
}

// ── Community-level circular deps ──────────────────────────────────

fn find_circular_deps_community(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    let read_model = GraphReadModel::without_http_bridges(Arc::clone(db));
    let adj = read_model.community_call_adjacency()?;

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    // Sort SCCs by size descending BEFORE taking limit,
    // so we always keep the largest cycles even when limit truncates.
    let mut ordered: Vec<&Vec<String>> = sccs.iter().collect();
    ordered.sort_by_key(|scc| Reverse(scc.len()));

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in ordered.into_iter().take(limit) {
        let severity = classify_severity("community", scc.len());
        let internal_edges = collect_adj_edges(&adj, scc);
        components.push(CycleComponent {
            size: scc.len(),
            nodes: scc.clone(),
            severity,
            internal_edges,
        });
    }

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "community".to_string(),
        components,
        total_components,
        shown,
        graph_explain: cycles_graph_explain(),
    })
}

// ── Utility: collect edges from adjacency map within an SCC ────────

fn collect_adj_edges(adj: &HashMap<String, Vec<String>>, scc: &[String]) -> Vec<InternalEdge> {
    GraphReadModel::internal_edges_from_adjacency(adj, scc)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tarjan_scc_simple_3_cycle() {
        // A → B → C → A
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string()]);
        adj.insert("B".to_string(), vec!["C".to_string()]);
        adj.insert("C".to_string(), vec!["A".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 1);
        let mut scc = sccs[0].clone();
        scc.sort();
        assert_eq!(scc, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_tarjan_scc_no_cycles() {
        // A → B → C (DAG)
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string()]);
        adj.insert("B".to_string(), vec!["C".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert!(sccs.is_empty(), "DAG should produce no SCCs with size > 1");
    }

    #[test]
    fn test_tarjan_scc_two_separate_cycles() {
        // Cycle 1: A → B → A
        // Cycle 2: C → D → E → C
        // No connection between them
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["B".to_string()]);
        adj.insert("B".to_string(), vec!["A".to_string()]);
        adj.insert("C".to_string(), vec!["D".to_string()]);
        adj.insert("D".to_string(), vec!["E".to_string()]);
        adj.insert("E".to_string(), vec!["C".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 2, "Should find 2 separate SCCs");

        let mut sizes: Vec<usize> = sccs.iter().map(|s| s.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![2, 3]);
    }

    #[test]
    fn test_tarjan_scc_self_loop_excluded() {
        // A → A (self-loop) should NOT be returned since we only look at size > 1
        // But Tarjan considers self-loops as SCCs of size 1 with a self-edge.
        // Our filter for size > 1 handles this.
        let mut adj = HashMap::new();
        adj.insert("A".to_string(), vec!["A".to_string()]);

        let sccs = tarjan_scc(&adj);
        // Self-loops produce an SCC of size 1, which is filtered out.
        assert!(sccs.is_empty());
    }

    #[test]
    fn test_extract_package_with_src_boundary() {
        assert_eq!(
            extract_package("packages/core/src/utils/foo.ts"),
            "packages/core"
        );
        assert_eq!(extract_package("my-lib/src/index.ts"), "my-lib");
    }

    #[test]
    fn test_extract_package_with_lib_boundary() {
        assert_eq!(
            extract_package("packages/utils/lib/helpers.ts"),
            "packages/utils"
        );
    }

    #[test]
    fn test_extract_package_with_app_boundary() {
        assert_eq!(extract_package("frontend/app/page.tsx"), "frontend");
    }

    #[test]
    fn test_extract_package_fallback_two_components() {
        // No src/lib/app boundary — take first 2 components
        assert_eq!(
            extract_package("packages/core/utils/foo.ts"),
            "packages/core"
        );
    }

    #[test]
    fn test_extract_package_single_dir() {
        assert_eq!(extract_package("core/foo.ts"), "core");
    }

    #[test]
    fn test_extract_package_bare_file() {
        assert_eq!(extract_package("index.ts"), "index.ts");
    }

    #[test]
    fn test_severity_classification() {
        assert_eq!(classify_severity("community", 2), "critical");
        assert_eq!(classify_severity("community", 10), "critical");

        assert_eq!(classify_severity("package", 2), "medium");
        assert_eq!(classify_severity("package", 4), "high");

        assert_eq!(classify_severity("file", 2), "low");
        assert_eq!(classify_severity("file", 3), "medium");
        assert_eq!(classify_severity("file", 5), "high");
    }

    #[test]
    fn test_tarjan_scc_complex_graph() {
        // Larger graph:
        //   1 → 2 → 3 → 1  (cycle)
        //   3 → 4 → 5 → 4  (separate cycle via 4-5)
        //   5 → 6           (no cycle for 6)
        let mut adj = HashMap::new();
        adj.insert("1".to_string(), vec!["2".to_string()]);
        adj.insert("2".to_string(), vec!["3".to_string()]);
        adj.insert("3".to_string(), vec!["1".to_string(), "4".to_string()]);
        adj.insert("4".to_string(), vec!["5".to_string()]);
        adj.insert("5".to_string(), vec!["4".to_string(), "6".to_string()]);

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 2);

        let mut sorted_sccs: Vec<Vec<String>> = sccs
            .into_iter()
            .map(|mut s| {
                s.sort();
                s
            })
            .collect();
        sorted_sccs.sort_by_key(|s| s.len());

        assert_eq!(sorted_sccs[0], vec!["4", "5"]);
        assert_eq!(sorted_sccs[1], vec!["1", "2", "3"]);
    }

    #[test]
    fn test_find_circular_deps_file_with_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite3");
        let db = Arc::new(IndexDb::open(&db_path).unwrap().0);

        // Insert test data: files and imports forming a cycle
        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute_batch(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES ('a.ts', 'typescript', 'h1', 0.0, 100, '2024-01-01');
                 INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES ('b.ts', 'typescript', 'h2', 0.0, 100, '2024-01-01');
                 INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES ('c.ts', 'typescript', 'h3', 0.0, 100, '2024-01-01');

                 INSERT INTO imports(file_path, import_string, resolved_path) VALUES ('a.ts', './b', 'b.ts');
                 INSERT INTO imports(file_path, import_string, resolved_path) VALUES ('b.ts', './c', 'c.ts');
                 INSERT INTO imports(file_path, import_string, resolved_path) VALUES ('c.ts', './a', 'a.ts');",
            )
            .unwrap();
        }

        let result = find_circular_deps(&db, "file", 10).unwrap();
        assert_eq!(result.granularity, "file");
        assert_eq!(result.total_components, 1);
        assert_eq!(result.shown, 1);
        assert_eq!(result.components[0].size, 3);

        // The declared graph subset is surfaced additively on every result.
        let explain = result.graph_explain.as_ref().expect("declared envelope");
        assert_eq!(
            explain.declared_edge_kinds,
            cc_model::graph_catalog::tool_graph_subsets::CYCLES
                .kinds()
                .to_vec()
        );

        let mut nodes = result.components[0].nodes.clone();
        nodes.sort();
        assert_eq!(nodes, vec!["a.ts", "b.ts", "c.ts"]);

        // Verify witness edges exist
        assert!(!result.components[0].internal_edges.is_empty());
    }

    #[test]
    fn test_find_circular_deps_file_no_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite3");
        let db = Arc::new(IndexDb::open(&db_path).unwrap().0);

        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute_batch(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES ('a.ts', 'typescript', 'h1', 0.0, 100, '2024-01-01');
                 INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES ('b.ts', 'typescript', 'h2', 0.0, 100, '2024-01-01');

                 INSERT INTO imports(file_path, import_string, resolved_path) VALUES ('a.ts', './b', 'b.ts');",
            )
            .unwrap();
        }

        let result = find_circular_deps(&db, "file", 10).unwrap();
        assert_eq!(result.total_components, 0);
        assert!(result.components.is_empty());
    }

    #[test]
    fn test_find_circular_deps_invalid_granularity() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite3");
        let db = Arc::new(IndexDb::open(&db_path).unwrap().0);

        let result = find_circular_deps(&db, "invalid", 10);
        assert!(result.is_err());
    }

    #[test]
    fn cycle_sort_returns_largest_first() {
        // Construct a graph with 3 SCCs: size 2, 5, 3.
        // Tarjan discovery order depends on graph structure;
        // we arrange edges so the largest SCC is NOT discovered first.
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        // SCC 1: a <-> b (size 2)
        adj.entry("a".into()).or_default().push("b".into());
        adj.entry("b".into()).or_default().push("a".into());

        // SCC 2: c -> d -> e -> f -> g -> c (size 5)
        adj.entry("c".into()).or_default().push("d".into());
        adj.entry("d".into()).or_default().push("e".into());
        adj.entry("e".into()).or_default().push("f".into());
        adj.entry("f".into()).or_default().push("g".into());
        adj.entry("g".into()).or_default().push("c".into());

        // SCC 3: h -> i -> j -> h (size 3)
        adj.entry("h".into()).or_default().push("i".into());
        adj.entry("i".into()).or_default().push("j".into());
        adj.entry("j".into()).or_default().push("h".into());

        // Link SCCs in order that forces discovery to find size-2 first
        adj.entry("a".into()).or_default().push("c".into());
        adj.entry("c".into()).or_default().push("h".into());

        let sccs = tarjan_scc(&adj);
        assert_eq!(sccs.len(), 3);

        // Verify our sort-before-take fix:
        // After sorting by size descending, order should be [5, 3, 2]
        let mut ordered: Vec<&Vec<String>> = sccs.iter().collect();
        ordered.sort_by_key(|scc| Reverse(scc.len()));

        assert_eq!(ordered[0].len(), 5);
        assert_eq!(ordered[1].len(), 3);
        assert_eq!(ordered[2].len(), 2);

        // With limit=2, we should get size 5 and 3, NOT the first two by discovery order
        let limited: Vec<&Vec<String>> = ordered.into_iter().take(2).collect();
        assert_eq!(limited[0].len(), 5);
        assert_eq!(limited[1].len(), 3);
    }
}
