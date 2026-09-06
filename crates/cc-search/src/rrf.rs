//! Reciprocal Rank Fusion (RRF) — merges ranked lists from multiple retrieval channels.

use std::collections::HashMap;

/// Accumulate RRF scores from a ranked list.
/// score(d) += weight / (k + rank)
///
/// Accepts `&[&str]` to avoid cloning every id on each call.
/// Only allocates a new `String` when inserting a previously-unseen id.
pub fn rrf_accumulate(
    scores: &mut HashMap<String, f64>,
    ranked_ids: &[&str],
    weight: f64,
    k: usize,
) {
    let mut seen = std::collections::HashSet::new();
    for (rank, id) in ranked_ids.iter().enumerate() {
        if !seen.insert(*id) {
            continue;
        }
        let score = weight / (k + rank + 1) as f64;
        if let Some(existing) = scores.get_mut(*id) {
            *existing += score;
        } else {
            scores.insert(id.to_string(), score);
        }
    }
}

/// Compute overlap score between query tokens and text tokens.
pub fn overlap_score(query_tokens: &[String], text: &str) -> f64 {
    let hay: std::collections::HashSet<String> =
        cc_db::fts::tokenize_codeish(text).into_iter().collect();
    if hay.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }
    let overlap = query_tokens
        .iter()
        .filter(|t| hay.contains(t.as_str()))
        .count();
    overlap as f64 / query_tokens.len().max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_basic() {
        let mut scores = HashMap::new();
        rrf_accumulate(&mut scores, &["a", "b", "c"], 1.0, 50);
        assert!(scores["a"] > scores["b"]);
        assert!(scores["b"] > scores["c"]);
    }

    #[test]
    fn rrf_merge_two_lists() {
        let mut scores = HashMap::new();
        rrf_accumulate(&mut scores, &["a", "b"], 1.0, 50);
        rrf_accumulate(&mut scores, &["b", "a"], 1.0, 50);
        // b appears at rank 1 in list 2, a at rank 1 in list 1 => equal
        assert!((scores["a"] - scores["b"]).abs() < 1e-10);
    }
}
