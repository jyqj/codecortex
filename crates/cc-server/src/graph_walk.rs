//! Shared BFS traversal kernel for graph read paths.
//!
//! Two traversal flavors share the queue/budget plumbing that used to be
//! reimplemented per call site:
//! - `bfs_simple_paths`: simple-path enumeration (per-path visited sets) used
//!   by trace/flow path discovery.
//! - `bfs_visit`: layered node visiting (global visited set) used by the type
//!   hierarchy ancestor/descendant walks.
//!
//! Direction and edge filtering are both expressed through the `next_of`
//! closure: it maps an edge to the UID the walk should continue from (callee
//! for forward call walks, source for reverse semantic walks) or `None` to
//! skip the edge entirely.

use cc_model::CcResult;
use std::collections::{HashSet, VecDeque};

/// Default expansion allowance per requested path, preserved from the
/// historical `max_paths * 500` BFS safety valve.
pub(crate) const PATH_EXPANSIONS_PER_RESULT: usize = 500;

/// Explicit traversal budget for path enumeration: depth cap, result cap, and
/// a hard cap on queue expansions so runaway graphs cannot explode the walk.
pub(crate) struct WalkBudget {
    pub max_depth: usize,
    pub max_results: usize,
    pub max_expansions: usize,
}

impl WalkBudget {
    /// Budget for simple-path enumeration between two nodes.
    pub(crate) fn for_path_enumeration(max_depth: usize, max_paths: usize) -> Self {
        Self {
            max_depth,
            max_results: max_paths,
            max_expansions: max_paths.saturating_mul(PATH_EXPANSIONS_PER_RESULT).max(1),
        }
    }
}

/// Why a path enumeration stopped before exhausting the search space.
/// All flags false means the walk completed within its budget.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct WalkTruncation {
    /// A queued partial path exceeded `max_depth` and was dropped.
    pub depth_clipped: bool,
    /// `max_results` was reached while unexplored entries remained queued.
    pub results_clipped: bool,
    /// The expansion safety valve (`max_expansions`) was exhausted.
    pub expansions_clipped: bool,
}

/// Enumerate simple paths (no node repeated within a path) from `from` to
/// `to` in BFS order. Returns up to `budget.max_results` paths as
/// (node UIDs, edges) pairs.
#[cfg(test)]
pub(crate) fn bfs_simple_paths<E, F, G>(
    from: &str,
    to: &str,
    budget: &WalkBudget,
    neighbors: F,
    next_of: G,
) -> Vec<(Vec<String>, Vec<E>)>
where
    E: Clone,
    F: FnMut(&str) -> Vec<E>,
    G: FnMut(&E) -> Option<&str>,
{
    bfs_simple_paths_explained(from, to, budget, neighbors, next_of).0
}

/// Like `bfs_simple_paths` but also reports which budget (if any) clipped the
/// walk, so callers can surface a stable truncation reason.
pub(crate) fn bfs_simple_paths_explained<E, F, G>(
    from: &str,
    to: &str,
    budget: &WalkBudget,
    mut neighbors: F,
    mut next_of: G,
) -> (Vec<(Vec<String>, Vec<E>)>, WalkTruncation)
where
    E: Clone,
    F: FnMut(&str) -> Vec<E>,
    G: FnMut(&E) -> Option<&str>,
{
    let mut truncation = WalkTruncation::default();
    let mut results: Vec<(Vec<String>, Vec<E>)> = Vec::new();
    // Each queue entry carries its own visited set so distinct paths through
    // shared intermediate nodes are all discovered (simple-path constraint:
    // no node appears twice within the *same* path).
    let mut queue: VecDeque<(Vec<String>, Vec<E>, HashSet<String>)> = VecDeque::new();
    let mut initial_visited = HashSet::new();
    initial_visited.insert(from.to_string());
    queue.push_back((vec![from.to_string()], Vec::new(), initial_visited));

    // Safety valve: cap total queue pushes to prevent runaway exploration.
    let mut expansions: usize = 0;

    while let Some((nodes, edges, visited)) = queue.pop_front() {
        if results.len() >= budget.max_results {
            // We popped an unexplored entry just to discover the result cap:
            // there was more search space than the caller asked for.
            truncation.results_clipped = true;
            break;
        }
        if nodes.len() > budget.max_depth + 1 {
            truncation.depth_clipped = true;
            continue;
        }
        let current = nodes.last().expect("path has at least one uid").clone();
        if current == to {
            results.push((nodes, edges));
            continue;
        }

        for edge in neighbors(&current) {
            // Borrowed key: visited/budget checks run without allocating; the
            // UID is cloned only when the edge is actually enqueued.
            let Some(next_ref) = next_of(&edge) else {
                continue;
            };
            if visited.contains(next_ref) {
                continue;
            }
            if expansions >= budget.max_expansions {
                truncation.expansions_clipped = true;
                tracing::debug!(
                    max_expansions = budget.max_expansions,
                    results = results.len(),
                    max_results = budget.max_results,
                    "BFS expansion budget exhausted, truncating path enumeration"
                );
                break;
            }
            expansions += 1;
            let next_uid = next_ref.to_string();
            let mut new_visited = visited.clone();
            new_visited.insert(next_uid.clone());
            let mut new_nodes = nodes.clone();
            new_nodes.push(next_uid);
            let mut new_edges = edges.clone();
            new_edges.push(edge);
            queue.push_back((new_nodes, new_edges, new_visited));
        }
    }

    (results, truncation)
}

/// Layered BFS over nodes with a global visited set. `on_visit` is invoked
/// exactly once per newly discovered node with (uid, depth, discovering edge);
/// nodes at `max_depth` are reported but not expanded further. Hierarchy walks
/// historically had no expansion cap, so depth is the only bound.
pub(crate) fn bfs_visit<E, F, G, V>(
    seeds: &[String],
    max_depth: usize,
    mut neighbors: F,
    mut next_of: G,
    mut on_visit: V,
) -> CcResult<()>
where
    F: FnMut(&str) -> Vec<E>,
    G: FnMut(&E) -> Option<&str>,
    V: FnMut(&str, usize, &E) -> CcResult<()>,
{
    let mut visited: HashSet<String> = seeds.iter().cloned().collect();
    let mut queue: VecDeque<(String, usize)> = seeds.iter().map(|seed| (seed.clone(), 0)).collect();

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        for edge in neighbors(&current) {
            let Some(next_ref) = next_of(&edge) else {
                continue;
            };
            if visited.contains(next_ref) {
                continue;
            }
            let next_uid = next_ref.to_string();
            visited.insert(next_uid.clone());
            on_visit(&next_uid, depth + 1, &edge)?;
            queue.push_back((next_uid, depth + 1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    type Edge = (String, String);

    fn graph(edges: &[(&str, &str)]) -> HashMap<String, Vec<Edge>> {
        let mut adj: HashMap<String, Vec<Edge>> = HashMap::new();
        for (src, dst) in edges {
            adj.entry(src.to_string())
                .or_default()
                .push((src.to_string(), dst.to_string()));
        }
        adj
    }

    fn edge_target(edge: &Edge) -> Option<&str> {
        Some(edge.1.as_str())
    }

    fn paths_in(
        adj: &HashMap<String, Vec<Edge>>,
        from: &str,
        to: &str,
        budget: &WalkBudget,
    ) -> Vec<Vec<String>> {
        bfs_simple_paths(
            from,
            to,
            budget,
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
        )
        .into_iter()
        .map(|(nodes, _edges)| nodes)
        .collect()
    }

    #[test]
    fn paths_enumerates_multiple_simple_paths_through_shared_node() {
        let adj = graph(&[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")]);
        let mut paths = paths_in(&adj, "A", "D", &WalkBudget::for_path_enumeration(5, 10));
        paths.sort();
        assert_eq!(
            paths,
            vec![
                vec!["A".to_string(), "B".to_string(), "D".to_string()],
                vec!["A".to_string(), "C".to_string(), "D".to_string()],
            ]
        );
    }

    #[test]
    fn paths_respects_depth_cap() {
        // A→B→C→D needs depth 3; with max_depth=2 it must not be found.
        let adj = graph(&[("A", "B"), ("B", "C"), ("C", "D")]);
        let paths = paths_in(&adj, "A", "D", &WalkBudget::for_path_enumeration(2, 10));
        assert!(paths.is_empty());
        let paths = paths_in(&adj, "A", "D", &WalkBudget::for_path_enumeration(3, 10));
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn paths_stops_at_max_results() {
        // Three parallel 2-hop paths; max_paths=2 keeps only two.
        let adj = graph(&[
            ("A", "B1"),
            ("A", "B2"),
            ("A", "B3"),
            ("B1", "D"),
            ("B2", "D"),
            ("B3", "D"),
        ]);
        let paths = paths_in(&adj, "A", "D", &WalkBudget::for_path_enumeration(5, 2));
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn paths_budget_truncates_expansion() {
        // Wide fan-out with no route to the target: a tiny expansion budget
        // must terminate the walk without finding anything (and quickly).
        let mut edges: Vec<(String, String)> = Vec::new();
        for layer in 0..4 {
            for src_idx in 0..10 {
                for dst_idx in 0..10 {
                    edges.push((
                        format!("n{layer}_{src_idx}"),
                        format!("n{}_{dst_idx}", layer + 1),
                    ));
                }
            }
        }
        let mut adj: HashMap<String, Vec<Edge>> = HashMap::new();
        for (src, dst) in &edges {
            adj.entry(src.clone())
                .or_default()
                .push((src.clone(), dst.clone()));
        }
        let mut expansions_seen = 0usize;
        let budget = WalkBudget {
            max_depth: 10,
            max_results: 1,
            max_expansions: 25,
        };
        let paths = bfs_simple_paths(
            "n0_0",
            "missing",
            &budget,
            |uid| {
                let edges = adj.get(uid).cloned().unwrap_or_default();
                // Each returned edge is inspected at most once by the kernel.
                expansions_seen += edges.len();
                edges
            },
            edge_target,
        );
        assert!(paths.is_empty());
        // The walk must not expand far past the budget (each expansion looks
        // at each neighbor edge at most once).
        assert!(
            expansions_seen <= 25 * 11,
            "expansion not bounded: {expansions_seen}"
        );
    }

    #[test]
    fn explained_reports_depth_clip() {
        // A→B→C→D needs depth 3; with max_depth=1 the queued deeper partial
        // paths are dropped, which must be reported as a depth clip.
        let adj = graph(&[("A", "B"), ("B", "C"), ("C", "D")]);
        let (paths, truncation) = bfs_simple_paths_explained(
            "A",
            "D",
            &WalkBudget::for_path_enumeration(1, 10),
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
        );
        assert!(paths.is_empty());
        assert!(truncation.depth_clipped);
        assert!(!truncation.results_clipped);
        assert!(!truncation.expansions_clipped);
    }

    #[test]
    fn explained_reports_results_clip() {
        let adj = graph(&[
            ("A", "B1"),
            ("A", "B2"),
            ("A", "B3"),
            ("B1", "D"),
            ("B2", "D"),
            ("B3", "D"),
        ]);
        let (paths, truncation) = bfs_simple_paths_explained(
            "A",
            "D",
            &WalkBudget::for_path_enumeration(5, 2),
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
        );
        assert_eq!(paths.len(), 2);
        assert!(truncation.results_clipped);
    }

    #[test]
    fn explained_complete_walk_reports_no_truncation() {
        let adj = graph(&[("A", "B"), ("B", "C")]);
        let (paths, truncation) = bfs_simple_paths_explained(
            "A",
            "C",
            &WalkBudget::for_path_enumeration(5, 10),
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
        );
        assert_eq!(paths.len(), 1);
        assert!(!truncation.depth_clipped);
        assert!(!truncation.results_clipped);
        assert!(!truncation.expansions_clipped);
    }

    #[test]
    fn paths_terminates_on_cycles() {
        let adj = graph(&[("A", "B"), ("B", "A"), ("B", "C")]);
        let paths = paths_in(&adj, "A", "C", &WalkBudget::for_path_enumeration(5, 10));
        assert_eq!(
            paths,
            vec![vec!["A".to_string(), "B".to_string(), "C".to_string()]]
        );
    }

    #[test]
    fn visit_dedups_diamond_and_reports_depth() {
        let adj = graph(&[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")]);
        let mut visits: Vec<(String, usize)> = Vec::new();
        bfs_visit(
            &["A".to_string()],
            5,
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
            |uid, depth, _edge| {
                visits.push((uid.to_string(), depth));
                Ok(())
            },
        )
        .unwrap();
        visits.sort();
        // D discovered exactly once (global visited), at depth 2.
        assert_eq!(
            visits,
            vec![
                ("B".to_string(), 1),
                ("C".to_string(), 1),
                ("D".to_string(), 2),
            ]
        );
    }

    #[test]
    fn visit_depth_cap_stops_expansion() {
        let adj = graph(&[("A", "B"), ("B", "C")]);
        let mut visited: Vec<String> = Vec::new();
        bfs_visit(
            &["A".to_string()],
            1,
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
            |uid, _depth, _edge| {
                visited.push(uid.to_string());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(visited, vec!["B".to_string()]);
    }

    #[test]
    fn visit_edge_filter_skips_edges() {
        let adj = graph(&[("A", "B"), ("A", "skipme"), ("B", "C")]);
        let mut visited: Vec<String> = Vec::new();
        bfs_visit(
            &["A".to_string()],
            5,
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            |edge: &Edge| (edge.1 != "skipme").then_some(edge.1.as_str()),
            |uid, _depth, _edge| {
                visited.push(uid.to_string());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(visited, vec!["B".to_string(), "C".to_string()]);
    }

    #[test]
    fn visit_terminates_on_cycles() {
        let adj = graph(&[("A", "B"), ("B", "A")]);
        let mut visited: Vec<String> = Vec::new();
        bfs_visit(
            &["A".to_string()],
            10,
            |uid| adj.get(uid).cloned().unwrap_or_default(),
            edge_target,
            |uid, _depth, _edge| {
                visited.push(uid.to_string());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(visited, vec!["B".to_string()]);
    }
}
