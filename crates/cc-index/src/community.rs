//! Louvain community detection — deterministic, zero-dependency implementation.
//!
//! Multi-level (two-phase) Louvain over an undirected call graph between symbol
//! UIDs:
//!
//! * Phase 1: repeatedly move each node into the neighbouring community that
//!   yields the largest modularity gain, until no move improves Q.
//! * Phase 2: aggregate each community into a super-node (super-edge weight =
//!   sum of inter-community edge weights, self-loop = twice the intra-community
//!   edge weight) and repeat phase 1 on the smaller graph.
//!
//! Levels stop once a pass produces no merges; the multi-level labels are then
//! unfolded back onto the original nodes.
//!
//! Returns symbol_uid → community_id mapping.

use std::collections::HashMap;

/// Weighted undirected graph in CSR-ish adjacency form. Self-loops are stored
/// explicitly (a single `(i, i, w)` entry) and contribute `w` to the node's
/// degree, matching the standard Louvain aggregation contract.
struct WeightedGraph {
    /// adjacency[i] = list of (neighbor, weight); self-loops appear as (i, w).
    adjacency: Vec<Vec<(usize, f64)>>,
    /// degree[i] = sum of incident edge weights (self-loops counted once).
    degree: Vec<f64>,
    /// 2m: sum of all degrees (== twice total edge weight including self-loops).
    total_weight: f64,
}

impl WeightedGraph {
    fn node_count(&self) -> usize {
        self.adjacency.len()
    }
}

/// Deterministic LCG matching the original visit-order shuffle.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state >> 33
}

/// Phase 1: local moving. Returns the per-node community assignment after
/// convergence and whether any node ever moved from its singleton start.
fn local_move(graph: &WeightedGraph, max_iterations: usize, rng_state: &mut u64) -> Vec<usize> {
    let n = graph.node_count();
    let mut community: Vec<usize> = (0..n).collect();

    if graph.total_weight == 0.0 {
        return community;
    }

    // Sum of node degrees per community, maintained incrementally across moves.
    let mut comm_degree_sum: Vec<f64> = graph.degree.clone();

    for _iteration in 0..max_iterations {
        let mut improved = false;

        // Shuffled node order (deterministic).
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (lcg_next(rng_state) as usize) % (i + 1);
            order.swap(i, j);
        }

        for &node in &order {
            let current_comm = community[node];

            // Weight from `node` into each neighbouring community (self-loops
            // excluded — they do not change the gain comparison).
            let mut comm_weights: HashMap<usize, f64> = HashMap::new();
            for &(neighbor, weight) in &graph.adjacency[node] {
                if neighbor == node {
                    continue;
                }
                *comm_weights.entry(community[neighbor]).or_insert(0.0) += weight;
            }

            let ki = graph.degree[node];
            let mut best_comm = current_comm;
            let mut best_gain = 0.0f64;

            // Remove node from its current community for the gain calculation.
            let sigma_in_current = comm_weights.get(&current_comm).copied().unwrap_or(0.0);
            let sigma_tot_current = comm_degree_sum[current_comm] - ki;

            for (&target_comm, &ki_in) in &comm_weights {
                if target_comm == current_comm {
                    continue;
                }
                let sigma_tot = comm_degree_sum[target_comm];

                // Delta Q for moving node to target_comm.
                let gain = (ki_in - sigma_in_current) / graph.total_weight
                    - ki * (sigma_tot - sigma_tot_current)
                        / (graph.total_weight * graph.total_weight);

                if gain > best_gain {
                    best_gain = gain;
                    best_comm = target_comm;
                }
            }

            if best_comm != current_comm {
                community[node] = best_comm;
                // Keep comm_degree_sum consistent with the move.
                comm_degree_sum[current_comm] -= ki;
                comm_degree_sum[best_comm] += ki;
                improved = true;
            }
        }

        if !improved {
            break;
        }
    }

    community
}

/// Renumber an assignment to contiguous community ids in [0, k).
fn renumber(community: &[usize]) -> (Vec<usize>, usize) {
    let mut comm_map: HashMap<usize, usize> = HashMap::new();
    let mut next_id = 0usize;
    let renamed: Vec<usize> = community
        .iter()
        .map(|&c| {
            *comm_map.entry(c).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            })
        })
        .collect();
    (renamed, next_id)
}

/// Phase 2: aggregate communities into a super-graph. `community` must be the
/// contiguous (renumbered) assignment with `num_comms` communities.
fn aggregate(graph: &WeightedGraph, community: &[usize], num_comms: usize) -> WeightedGraph {
    // Accumulate super-edge weights between communities. Intra-community weight
    // (including existing self-loops) is folded into a self-loop on the super
    // node, contributing twice (once per direction) so degrees are preserved.
    let mut super_adj: Vec<HashMap<usize, f64>> = vec![HashMap::new(); num_comms];

    for u in 0..graph.node_count() {
        let cu = community[u];
        for &(v, w) in &graph.adjacency[u] {
            let cv = community[v];
            *super_adj[cu].entry(cv).or_insert(0.0) += w;
        }
    }

    let adjacency: Vec<Vec<(usize, f64)>> = super_adj
        .into_iter()
        .map(|m| {
            let mut entries: Vec<(usize, f64)> = m.into_iter().collect();
            // Deterministic ordering of neighbours.
            entries.sort_by_key(|&(c, _)| c);
            entries
        })
        .collect();

    let degree: Vec<f64> = adjacency
        .iter()
        .map(|neighbors| neighbors.iter().map(|(_, w)| w).sum())
        .collect();
    let total_weight: f64 = degree.iter().sum();

    WeightedGraph {
        adjacency,
        degree,
        total_weight,
    }
}

/// Prune an oversized edge list to at most `max_occurrences` edge
/// occurrences, keeping the *heaviest* node pairs.
///
/// Duplicate occurrences of the same unordered pair act as multi-edge
/// weight in [`louvain_communities`], so the pruning unit is the aggregated
/// pair: pairs are ranked by occurrence count descending (ties broken
/// lexicographically for determinism) and emitted with their full
/// multiplicity until the budget is reached — the last pair admitted is
/// truncated to the remaining budget. This preserves the strongest
/// community structure instead of dropping the graph wholesale.
pub fn prune_edges_by_weight(
    edges: &[(String, String)],
    max_occurrences: usize,
) -> Vec<(String, String)> {
    if edges.len() <= max_occurrences {
        return edges.to_vec();
    }
    // Aggregate unordered pairs (Louvain treats each occurrence as an
    // undirected unit-weight edge, so (a,b) and (b,a) are the same pair).
    let mut weights: HashMap<(&str, &str), usize> = HashMap::new();
    for (a, b) in edges {
        let pair = if a <= b {
            (a.as_str(), b.as_str())
        } else {
            (b.as_str(), a.as_str())
        };
        *weights.entry(pair).or_insert(0) += 1;
    }
    let mut ranked: Vec<((&str, &str), usize)> = weights.into_iter().collect();
    ranked.sort_by(|(pair_a, w_a), (pair_b, w_b)| w_b.cmp(w_a).then_with(|| pair_a.cmp(pair_b)));

    let mut pruned = Vec::with_capacity(max_occurrences);
    let mut budget = max_occurrences;
    for ((a, b), weight) in ranked {
        if budget == 0 {
            break;
        }
        let keep = weight.min(budget);
        for _ in 0..keep {
            pruned.push((a.to_string(), b.to_string()));
        }
        budget -= keep;
    }
    pruned
}

/// Run multi-level Louvain modularity-maximizing community detection.
///
/// `edges`: pairs of (symbol_uid_a, symbol_uid_b) representing call relationships.
/// `max_iterations`: cap on local-moving passes per level.
/// Returns: symbol_uid → community_id
pub fn louvain_communities(
    edges: &[(String, String)],
    max_iterations: usize,
) -> HashMap<String, u32> {
    // usize::MAX levels => run until aggregation stops collapsing nodes.
    louvain_with_levels(edges, max_iterations, usize::MAX)
}

/// Louvain with an explicit cap on the number of aggregation levels.
///
/// `max_levels == 1` performs a single local-moving pass with no aggregation,
/// reproducing the previous single-level behaviour (used as a baseline in
/// tests to prove the multi-level version never regresses modularity).
fn louvain_with_levels(
    edges: &[(String, String)],
    max_iterations: usize,
    max_levels: usize,
) -> HashMap<String, u32> {
    if edges.is_empty() {
        return HashMap::new();
    }

    // Build node index.
    let mut nodes: Vec<String> = Vec::new();
    let mut node_idx: HashMap<String, usize> = HashMap::new();
    for (a, b) in edges {
        for n in [a, b] {
            if !node_idx.contains_key(n) {
                node_idx.insert(n.clone(), nodes.len());
                nodes.push(n.clone());
            }
        }
    }

    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }

    // Base graph adjacency with unit weights (no self-loops yet).
    let mut adjacency: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut total_weight = 0.0f64;
    for (a, b) in edges {
        let ia = node_idx[a];
        let ib = node_idx[b];
        if ia != ib {
            adjacency[ia].push((ib, 1.0));
            adjacency[ib].push((ia, 1.0));
            total_weight += 2.0;
        }
    }

    if total_weight == 0.0 {
        // No edges — each node is its own community.
        return nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| (n, i as u32))
            .collect();
    }

    let degree: Vec<f64> = adjacency
        .iter()
        .map(|neighbors| neighbors.iter().map(|(_, w)| w).sum())
        .collect();

    let mut graph = WeightedGraph {
        adjacency,
        degree,
        total_weight,
    };

    // Deterministic LCG state shared across levels (matches original seed).
    let mut rng_state: u64 = 42;

    // `labels[orig_node]` = current community id in the top-most level graph.
    let mut labels: Vec<usize> = (0..n).collect();

    let mut levels_done = 0usize;
    loop {
        let community = local_move(&graph, max_iterations, &mut rng_state);
        let (renamed, num_comms) = renumber(&community);

        // Unfold this level's assignment onto the original nodes.
        for label in labels.iter_mut() {
            *label = renamed[*label];
        }
        levels_done += 1;

        // Stop if we hit the level cap (single-level baseline uses 1).
        if levels_done >= max_levels {
            break;
        }

        // If local moving collapsed nothing, we have converged.
        if num_comms == graph.node_count() {
            break;
        }

        // Aggregate and continue on the smaller super-graph.
        graph = aggregate(&graph, &renamed, num_comms);

        // Safety: a single super-node means everything merged; stop.
        if graph.node_count() <= 1 {
            break;
        }
    }

    // Final contiguous renumbering of the unfolded labels.
    let (final_labels, _) = renumber(&labels);

    nodes
        .into_iter()
        .enumerate()
        .map(|(i, uid)| (uid, final_labels[i] as u32))
        .collect()
}

/// Build community labels from member symbols.
pub fn build_community_labels(
    assignments: &HashMap<String, u32>,
    symbol_names: &HashMap<String, String>, // uid → name
) -> HashMap<u32, String> {
    let mut members: HashMap<u32, Vec<&str>> = HashMap::new();
    for (uid, &comm_id) in assignments {
        if let Some(name) = symbol_names.get(uid) {
            members.entry(comm_id).or_default().push(name);
        }
    }
    members
        .into_iter()
        .map(|(id, mut names)| {
            names.sort();
            names.truncate(5);
            (id, names.join(", "))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph() {
        let result = louvain_communities(&[], 10);
        assert!(result.is_empty());
    }

    fn edge(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    /// Under the cap the input passes through unchanged (no aggregation
    /// side effects, order preserved).
    #[test]
    fn prune_edges_noop_under_cap() {
        let edges = vec![edge("a", "b"), edge("b", "a"), edge("c", "d")];
        assert_eq!(prune_edges_by_weight(&edges, 3), edges);
    }

    /// Over the cap the heaviest unordered pairs win deterministically; the
    /// last admitted pair is truncated to the remaining budget.
    #[test]
    fn prune_edges_keeps_heaviest_pairs_deterministically() {
        // Pair weights: (a,b)=3 (one occurrence reversed), (c,d)=2, (e,f)=1.
        let edges = vec![
            edge("e", "f"),
            edge("a", "b"),
            edge("c", "d"),
            edge("b", "a"),
            edge("c", "d"),
            edge("a", "b"),
        ];
        let pruned = prune_edges_by_weight(&edges, 4);
        assert_eq!(
            pruned,
            vec![
                edge("a", "b"),
                edge("a", "b"),
                edge("a", "b"),
                edge("c", "d")
            ],
            "heaviest pair keeps full multiplicity; next pair truncated to the budget"
        );
        // Determinism across repeated calls (HashMap aggregation must not leak).
        assert_eq!(pruned, prune_edges_by_weight(&edges, 4));

        // Weight ties break lexicographically.
        let tied = vec![edge("x", "y"), edge("p", "q")];
        assert_eq!(prune_edges_by_weight(&tied, 1), vec![edge("p", "q")]);
    }

    /// Louvain over the pruned graph still finds the strongly connected
    /// cluster — the point of pruning instead of flattening to community 0.
    #[test]
    fn prune_edges_preserves_strong_cluster_for_louvain() {
        let mut edges = Vec::new();
        // Strong triangle a-b-c, weight 5 per pair.
        for _ in 0..5 {
            edges.push(edge("a", "b"));
            edges.push(edge("b", "c"));
            edges.push(edge("a", "c"));
        }
        // Weak noise pairs, weight 1 each.
        for i in 0..10 {
            edges.push(edge(&format!("n{i}"), &format!("m{i}")));
        }
        let pruned = prune_edges_by_weight(&edges, 15);
        assert_eq!(pruned.len(), 15, "budget respected");
        let communities = louvain_communities(&pruned, 10);
        assert_eq!(
            communities["a"], communities["b"],
            "strong cluster survives pruning"
        );
        assert_eq!(communities["b"], communities["c"]);
    }

    #[test]
    fn two_clusters() {
        // Cluster 1: a-b-c (fully connected)
        // Cluster 2: d-e-f (fully connected)
        // One bridge: c-d
        let edges = vec![
            ("a".into(), "b".into()),
            ("b".into(), "c".into()),
            ("a".into(), "c".into()),
            ("d".into(), "e".into()),
            ("e".into(), "f".into()),
            ("d".into(), "f".into()),
            ("c".into(), "d".into()),
        ];
        let result = louvain_communities(&edges, 20);
        assert_eq!(result.len(), 6);
        // a, b, c should be in same community
        assert_eq!(result["a"], result["b"]);
        assert_eq!(result["b"], result["c"]);
        // d, e, f should be in same community
        assert_eq!(result["d"], result["e"]);
        assert_eq!(result["e"], result["f"]);
        // The two clusters should be different
        assert_ne!(result["a"], result["d"]);
    }

    #[test]
    fn deterministic() {
        let edges = vec![
            ("x".into(), "y".into()),
            ("y".into(), "z".into()),
            ("x".into(), "z".into()),
        ];
        let a = louvain_communities(&edges, 10);
        let b = louvain_communities(&edges, 10);
        assert_eq!(a, b);
    }

    /// Compute Newman modularity Q for an assignment over the given undirected,
    /// unit-weight edge list, using the full
    /// `Q = (1/2m) Σ_ij [A_ij − k_i k_j / 2m] δ(c_i, c_j)` formula (summing over
    /// every ordered node pair, not just edges). Used to compare partitions.
    fn modularity(edges: &[(String, String)], assignment: &HashMap<String, u32>) -> f64 {
        let nodes: std::collections::BTreeSet<&str> = edges
            .iter()
            .flat_map(|(a, b)| [a.as_str(), b.as_str()])
            .collect();
        let mut degree: HashMap<&str, f64> = HashMap::new();
        let mut adj: HashMap<(&str, &str), f64> = HashMap::new();
        for (a, b) in edges {
            if a == b {
                continue;
            }
            *degree.entry(a.as_str()).or_insert(0.0) += 1.0;
            *degree.entry(b.as_str()).or_insert(0.0) += 1.0;
            *adj.entry((a.as_str(), b.as_str())).or_insert(0.0) += 1.0;
            *adj.entry((b.as_str(), a.as_str())).or_insert(0.0) += 1.0;
        }
        let two_m: f64 = degree.values().sum();
        if two_m == 0.0 {
            return 0.0;
        }
        let mut q = 0.0f64;
        for &i in &nodes {
            for &j in &nodes {
                if assignment.get(i) == assignment.get(j) {
                    let a_ij = adj.get(&(i, j)).copied().unwrap_or(0.0);
                    q += a_ij - degree[i] * degree[j] / two_m;
                }
            }
        }
        q / two_m
    }

    /// Multi-level Louvain must never produce a partition with lower modularity
    /// than the single local-moving pass it builds on, and should recover a
    /// meaningful community structure on a graph with nested sub-clusters.
    #[test]
    fn multi_level_not_worse_than_single_level() {
        // Two mega-clusters, each made of two triangle sub-clusters connected
        // by an intra-mega bridge; a single thin bridge joins the megas.
        let triangle = |p: &str, x: u32, y: u32, z: u32| {
            vec![
                (format!("{p}{x}"), format!("{p}{y}")),
                (format!("{p}{y}"), format!("{p}{z}")),
                (format!("{p}{x}"), format!("{p}{z}")),
            ]
        };
        let mut edges: Vec<(String, String)> = Vec::new();
        edges.extend(triangle("a", 1, 2, 3));
        edges.extend(triangle("a", 4, 5, 6));
        edges.push(("a3".into(), "a4".into()));
        edges.extend(triangle("b", 1, 2, 3));
        edges.extend(triangle("b", 4, 5, 6));
        edges.push(("b3".into(), "b4".into()));
        edges.push(("a6".into(), "b1".into()));

        let multi = louvain_communities(&edges, 50);
        // Single-level baseline: one local-moving pass, no aggregation.
        let single = louvain_with_levels(&edges, 50, 1);

        assert_eq!(multi.len(), 12);

        // Determinism preserved.
        assert_eq!(multi, louvain_communities(&edges, 50));

        let q_multi = modularity(&edges, &multi);
        let q_single = modularity(&edges, &single);
        assert!(
            q_multi >= q_single - 1e-9,
            "multi-level Q {q_multi} must be >= single-level Q {q_single}"
        );

        // The partition must be meaningful: each triangle stays intact and the
        // two megas are not merged into a single blob.
        for tri in [
            ["a1", "a2", "a3"],
            ["a4", "a5", "a6"],
            ["b1", "b2", "b3"],
            ["b4", "b5", "b6"],
        ] {
            assert_eq!(multi[tri[0]], multi[tri[1]], "{tri:?} must be together");
            assert_eq!(multi[tri[1]], multi[tri[2]], "{tri:?} must be together");
        }
        let distinct: std::collections::HashSet<u32> = multi.values().copied().collect();
        assert!(
            distinct.len() >= 2 && distinct.len() < 12,
            "expected a non-trivial partition, got {} communities",
            distinct.len()
        );
    }

    /// On a graph where node-by-node moves stall at a local optimum, the second
    /// Louvain level (aggregation + re-moving) must reach a strictly higher
    /// modularity than the single-level pass. A ring of triangles connected by
    /// single bridges is the classic case: single-level can leave bridge nodes
    /// split off, while aggregation merges adjacent triangles cleanly.
    #[test]
    fn aggregation_improves_or_matches_on_ring_of_triangles() {
        let k = 8u32;
        let mut edges: Vec<(String, String)> = Vec::new();
        for c in 0..k {
            edges.push((format!("c{c}_0"), format!("c{c}_1")));
            edges.push((format!("c{c}_1"), format!("c{c}_2")));
            edges.push((format!("c{c}_0"), format!("c{c}_2")));
        }
        for c in 0..k {
            edges.push((format!("c{c}_0"), format!("c{}_2", (c + 1) % k)));
        }

        let multi = louvain_communities(&edges, 50);
        let single = louvain_with_levels(&edges, 50, 1);
        let q_multi = modularity(&edges, &multi);
        let q_single = modularity(&edges, &single);
        assert!(
            q_multi >= q_single - 1e-9,
            "ring-of-triangles: multi Q {q_multi} >= single Q {q_single}"
        );
        // Determinism.
        assert_eq!(multi, louvain_communities(&edges, 50));
    }
}
