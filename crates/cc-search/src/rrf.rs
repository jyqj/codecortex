//! Reciprocal Rank Fusion (RRF) — merges ranked lists from multiple retrieval channels.

use std::collections::HashMap;

/// Accumulate RRF scores from a ranked list.
/// score(d) += weight / (k + rank)
pub fn rrf_accumulate(
    scores: &mut HashMap<String, f64>,
    ranked_ids: &[String],
    weight: f64,
    k: usize,
) {
    for (rank, id) in ranked_ids.iter().enumerate() {
        *scores.entry(id.clone()).or_insert(0.0) += weight / (k + rank + 1) as f64;
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
        rrf_accumulate(&mut scores, &["a".into(), "b".into(), "c".into()], 1.0, 50);
        assert!(scores["a"] > scores["b"]);
        assert!(scores["b"] > scores["c"]);
    }

    #[test]
    fn rrf_merge_two_lists() {
        let mut scores = HashMap::new();
        rrf_accumulate(&mut scores, &["a".into(), "b".into()], 1.0, 50);
        rrf_accumulate(&mut scores, &["b".into(), "a".into()], 1.0, 50);
        // b appears at rank 1 in list 2, a at rank 1 in list 1 => equal
        assert!((scores["a"] - scores["b"]).abs() < 1e-10);
    }
}
