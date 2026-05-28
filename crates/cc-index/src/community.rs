//! Louvain community detection — deterministic, zero-dependency implementation.
//!
//! Operates on undirected call graph edges between symbol UIDs.
//! Returns symbol_uid → community_id mapping.

use std::collections::HashMap;

/// Run Louvain modularity-maximizing community detection.
///
/// `edges`: pairs of (symbol_uid_a, symbol_uid_b) representing call relationships.
/// Returns: symbol_uid → community_id
pub fn louvain_communities(
    edges: &[(String, String)],
    max_iterations: usize,
) -> HashMap<String, u32> {
    if edges.is_empty() {
        return HashMap::new();
    }

    // Build adjacency with weights
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

    // Adjacency list with weights
    let mut adj: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut total_weight = 0.0f64;

    for (a, b) in edges {
        let ia = node_idx[a];
        let ib = node_idx[b];
        if ia != ib {
            adj[ia].push((ib, 1.0));
            adj[ib].push((ia, 1.0));
            total_weight += 2.0;
        }
    }

    if total_weight == 0.0 {
        // No edges — each node is its own community
        return nodes
            .into_iter()
            .enumerate()
            .map(|(i, n)| (n, i as u32))
            .collect();
    }

    // Degree of each node (sum of edge weights)
    let degree: Vec<f64> = adj
        .iter()
        .map(|neighbors| neighbors.iter().map(|(_, w)| w).sum())
        .collect();

    // Initial: each node in its own community
    let mut community: Vec<u32> = (0..n as u32).collect();

    // Deterministic LCG for node visit order (matches Python version)
    let mut rng_state: u64 = 42;
    let lcg_next = |state: &mut u64| -> u64 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        *state >> 33
    };

    // Sum of node degrees per community, maintained incrementally across moves
    // (rebuilding this O(n) map inside the per-node loop made the algorithm O(n²)).
    let mut comm_degree_sum: HashMap<u32, f64> = HashMap::new();
    for i in 0..n {
        *comm_degree_sum.entry(community[i]).or_insert(0.0) += degree[i];
    }

    for _iteration in 0..max_iterations {
        let mut improved = false;

        // Shuffled node order (deterministic)
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (lcg_next(&mut rng_state) as usize) % (i + 1);
            order.swap(i, j);
        }

        for &node in &order {
            let current_comm = community[node];

            // Compute neighboring community weights
            let mut comm_weights: HashMap<u32, f64> = HashMap::new();
            for &(neighbor, weight) in &adj[node] {
                *comm_weights.entry(community[neighbor]).or_insert(0.0) += weight;
            }

            // Find best community (modularity gain)
            let ki = degree[node];
            let mut best_comm = current_comm;
            let mut best_gain = 0.0f64;

            // Remove node from current community for calculation
            let sigma_in_current = comm_weights.get(&current_comm).copied().unwrap_or(0.0);
            let sigma_tot_current = comm_degree_sum.get(&current_comm).copied().unwrap_or(0.0) - ki;

            for (&target_comm, &ki_in) in &comm_weights {
                if target_comm == current_comm {
                    continue;
                }
                let sigma_tot = comm_degree_sum.get(&target_comm).copied().unwrap_or(0.0);

                // Delta Q for moving node to target_comm
                let gain = (ki_in - sigma_in_current) / total_weight
                    - ki * (sigma_tot - sigma_tot_current) / (total_weight * total_weight);

                if gain > best_gain {
                    best_gain = gain;
                    best_comm = target_comm;
                }
            }

            if best_comm != current_comm {
                community[node] = best_comm;
                // Keep comm_degree_sum consistent with the move.
                *comm_degree_sum.entry(current_comm).or_insert(0.0) -= ki;
                *comm_degree_sum.entry(best_comm).or_insert(0.0) += ki;
                improved = true;
            }
        }

        if !improved {
            break;
        }
    }

    // Renumber communities contiguously
    let mut comm_map: HashMap<u32, u32> = HashMap::new();
    let mut next_id = 0u32;
    for c in &community {
        comm_map.entry(*c).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
    }

    nodes
        .into_iter()
        .enumerate()
        .map(|(i, uid)| (uid, comm_map[&community[i]]))
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
}
