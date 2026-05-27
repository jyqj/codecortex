//! Circular dependency detection via Tarjan's SCC algorithm.

use std::collections::HashMap;
use std::sync::Arc;

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};

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
                        let v_state = state.get_mut(v_str).unwrap();
                        if w_idx < v_state.lowlink {
                            v_state.lowlink = w_idx;
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
                    loop {
                        let w = scc_stack.pop().unwrap();
                        state.get_mut(w).unwrap().on_stack = false;
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
                    let child_lowlink = state[v_str_owned].lowlink;
                    let parent_state = state.get_mut(parent_str).unwrap();
                    if child_lowlink < parent_state.lowlink {
                        parent_state.lowlink = child_lowlink;
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
        other => Err(CcError::Other(format!(
            "unknown granularity: {other}; expected file|package|community"
        ))),
    }
}

// ── File-level circular deps ───────────────────────────────────────

fn find_circular_deps_file(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    let rows = db.query_json(
        "SELECT DISTINCT file_path, resolved_path FROM imports WHERE resolved_path IS NOT NULL",
        &[],
    )?;

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for row in &rows {
        let fp = row.get("file_path").and_then(|v| v.as_str());
        let rp = row.get("resolved_path").and_then(|v| v.as_str());
        if let (Some(fp), Some(rp)) = (fp, rp) {
            if fp != rp {
                adj.entry(fp.to_string()).or_default().push(rp.to_string());
            }
        }
    }

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in sccs.iter().take(limit) {
        let severity = classify_severity("file", scc.len());
        let internal_edges = collect_file_witness_edges(db, scc)?;
        components.push(CycleComponent {
            size: scc.len(),
            nodes: scc.clone(),
            severity,
            internal_edges,
        });
    }

    // Sort by size descending for relevance
    components.sort_by(|a, b| b.size.cmp(&a.size));

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "file".to_string(),
        components,
        total_components,
        shown,
    })
}

/// Collect witness edges for a file-level SCC by querying the imports table.
fn collect_file_witness_edges(db: &Arc<IndexDb>, scc: &[String]) -> CcResult<Vec<InternalEdge>> {
    let scc_set: std::collections::HashSet<&str> = scc.iter().map(|s| s.as_str()).collect();
    let mut edges = Vec::new();

    for node in scc {
        // Find all imports from this node that land in another node in the same SCC
        let rows = db.query_json(
            "SELECT resolved_path, import_string FROM imports WHERE file_path = ?1 AND resolved_path IS NOT NULL",
            std::slice::from_ref(node),
        )?;
        for row in &rows {
            let rp = row.get("resolved_path").and_then(|v| v.as_str());
            let imp = row.get("import_string").and_then(|v| v.as_str());
            if let Some(rp) = rp {
                if scc_set.contains(rp) {
                    edges.push(InternalEdge {
                        from: node.clone(),
                        to: rp.to_string(),
                        import: imp.map(|s| s.to_string()),
                        line: None,
                    });
                }
            }
        }
    }

    Ok(edges)
}

// ── Package-level circular deps ────────────────────────────────────

fn find_circular_deps_package(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    let rows = db.query_json(
        "SELECT DISTINCT file_path, resolved_path FROM imports WHERE resolved_path IS NOT NULL",
        &[],
    )?;

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for row in &rows {
        let fp = row.get("file_path").and_then(|v| v.as_str());
        let rp = row.get("resolved_path").and_then(|v| v.as_str());
        if let (Some(fp), Some(rp)) = (fp, rp) {
            let pkg_from = extract_package(fp);
            let pkg_to = extract_package(rp);
            if pkg_from != pkg_to {
                adj.entry(pkg_from).or_default().push(pkg_to);
            }
        }
    }

    // Deduplicate adjacency lists
    for targets in adj.values_mut() {
        targets.sort();
        targets.dedup();
    }

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in sccs.iter().take(limit) {
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

    components.sort_by(|a, b| b.size.cmp(&a.size));

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "package".to_string(),
        components,
        total_components,
        shown,
    })
}

// ── Community-level circular deps ──────────────────────────────────

fn find_circular_deps_community(db: &Arc<IndexDb>, limit: usize) -> CcResult<CircularDepsResult> {
    // Build uid → community map
    let sym_rows = db.query_json(
        "SELECT DISTINCT symbol_uid, community_id FROM symbols WHERE community_id IS NOT NULL AND symbol_uid IS NOT NULL",
        &[],
    )?;

    let mut uid_to_community: HashMap<String, String> = HashMap::new();
    for row in &sym_rows {
        let uid = row.get("symbol_uid").and_then(|v| v.as_str());
        let cid = row.get("community_id");
        if let (Some(uid), Some(cid)) = (uid, cid) {
            // community_id can be integer or string in JSON
            let cid_str = match cid {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.clone(),
                _ => continue,
            };
            uid_to_community.insert(uid.to_string(), cid_str);
        }
    }

    // Load call_uid_edges and aggregate to community level
    let call_edges = db.call_uid_edges()?;

    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for (caller, callee) in &call_edges {
        let c_from = uid_to_community.get(caller);
        let c_to = uid_to_community.get(callee);
        if let (Some(from), Some(to)) = (c_from, c_to) {
            if from != to {
                adj.entry(from.clone()).or_default().push(to.clone());
            }
        }
    }

    // Deduplicate adjacency lists
    for targets in adj.values_mut() {
        targets.sort();
        targets.dedup();
    }

    let sccs = tarjan_scc(&adj);
    let total_components = sccs.len();

    let mut components: Vec<CycleComponent> = Vec::new();
    for scc in sccs.iter().take(limit) {
        let severity = classify_severity("community", scc.len());
        let internal_edges = collect_adj_edges(&adj, scc);
        components.push(CycleComponent {
            size: scc.len(),
            nodes: scc.clone(),
            severity,
            internal_edges,
        });
    }

    components.sort_by(|a, b| b.size.cmp(&a.size));

    let shown = components.len();
    Ok(CircularDepsResult {
        granularity: "community".to_string(),
        components,
        total_components,
        shown,
    })
}

// ── Utility: collect edges from adjacency map within an SCC ────────

fn collect_adj_edges(adj: &HashMap<String, Vec<String>>, scc: &[String]) -> Vec<InternalEdge> {
    let scc_set: std::collections::HashSet<&str> = scc.iter().map(|s| s.as_str()).collect();
    let mut edges = Vec::new();

    for node in scc {
        if let Some(targets) = adj.get(node) {
            for target in targets {
                if scc_set.contains(target.as_str()) {
                    edges.push(InternalEdge {
                        from: node.clone(),
                        to: target.clone(),
                        import: None,
                        line: None,
                    });
                }
            }
        }
    }

    edges
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
        let db = Arc::new(IndexDb::open(&db_path).unwrap());

        // Insert test data: files and imports forming a cycle
        {
            let conn = db.read_conn().unwrap();
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
        let db = Arc::new(IndexDb::open(&db_path).unwrap());

        {
            let conn = db.read_conn().unwrap();
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
        let db = Arc::new(IndexDb::open(&db_path).unwrap());

        let result = find_circular_deps(&db, "invalid", 10);
        assert!(result.is_err());
    }
}
