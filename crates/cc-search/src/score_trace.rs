//! ScoreTrace — additive score accumulator that makes `rerank_score`
//! fully replayable from its trace.
//!
//! Every rerank contribution must flow through [`ScoreTrace::push`]; the
//! final score is [`ScoreTrace::total`] (the components summed in insertion
//! order, so the floating-point result is bit-identical to the historical
//! incremental `rerank += …` chain).
//!
//! Component naming mirrors the established reason-token vocabulary
//! (`preselect:<layer>:+<v>` style):
//! - `rrf:<lane>` — one per-lane RRF fusion contribution;
//! - `overlap` — the query/text token-overlap term;
//! - `boost:<name>` — every rerank boost (`boost:symbol-exact`,
//!   `boost:doc-file`, `boost:stage-a`, `boost:dsl-name`,
//!   `boost:graph-rerank`, …).

use cc_model::search::SearchHit;

/// Additive accumulator for one hit's rerank score.
#[derive(Debug, Default)]
pub(crate) struct ScoreTrace {
    components: Vec<(String, f64)>,
}

impl ScoreTrace {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record one additive component.
    ///
    /// Zero contributions are skipped: they cannot change the total
    /// (`x + 0.0 == x` for the non-negative scores used here) and would
    /// only add noise to the bill.
    pub(crate) fn push(&mut self, component: &str, amount: f64) {
        if amount == 0.0 {
            return;
        }
        self.components.push((component.to_string(), amount));
    }

    /// Sum of all components, in insertion order (left-to-right fold, so
    /// the float result matches an equivalent incremental `+=` chain).
    pub(crate) fn total(&self) -> f64 {
        self.components.iter().map(|(_, amount)| amount).sum()
    }

    /// Consume the trace, yielding the `(component, amount)` bill.
    pub(crate) fn into_components(self) -> Vec<(String, f64)> {
        self.components
    }
}

/// The single seam for post-construction score boosts.
///
/// Once a [`SearchHit`] has been built (`rerank_score == sum(score_trace)`),
/// any further additive contribution MUST go through this function: it
/// updates `rerank_score` and bills the matching `score_trace` component
/// atomically, so the two can never drift apart.
///
/// Zero contributions are skipped entirely (no score change, no trace
/// entry), matching [`ScoreTrace::push`] semantics.
pub(crate) fn apply_traced_boost(hit: &mut SearchHit, component: &str, amount: f64) {
    if amount == 0.0 {
        return;
    }
    hit.rerank_score += amount;
    hit.score_trace.push((component.to_string(), amount));
}

/// Debug-build invariant check at the search pipeline's exit: for every hit
/// with a non-empty trace, `sum(score_trace)` must replay `rerank_score`
/// (1e-9 tolerance).
///
/// Hits with an **empty** trace are skipped: manually constructed
/// `SearchHit` literals (e.g. test fixtures in cc-server) legitimately
/// carry no trace, and older serialized payloads deserialize with an
/// empty one.  Only hits produced by the cc-search pipeline are required
/// to carry a replayable bill.
///
/// Compiles to a no-op in release builds.
pub(crate) fn debug_assert_trace_consistency(hits: &[SearchHit]) {
    if !cfg!(debug_assertions) {
        return;
    }
    for hit in hits {
        if hit.score_trace.is_empty() {
            continue;
        }
        let component_sum: f64 = hit.score_trace.iter().map(|(_, amount)| amount).sum();
        debug_assert!(
            (component_sum - hit.rerank_score).abs() < 1e-9,
            "score_trace must sum to rerank_score for {}: sum={} rerank={} trace={:?}",
            hit.chunk_id,
            component_sum,
            hit.rerank_score,
            hit.score_trace,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_is_sum_of_pushed_components_in_order() {
        let mut trace = ScoreTrace::new();
        trace.push("rrf:lexical", 0.0196);
        trace.push("overlap", 0.35);
        trace.push("boost:symbol-exact", 0.5);

        let expected = 0.0196 + 0.35 + 0.5;
        assert_eq!(trace.total(), expected);
        assert_eq!(
            trace.into_components(),
            vec![
                ("rrf:lexical".to_string(), 0.0196),
                ("overlap".to_string(), 0.35),
                ("boost:symbol-exact".to_string(), 0.5),
            ]
        );
    }

    #[test]
    fn zero_contributions_are_skipped() {
        let mut trace = ScoreTrace::new();
        trace.push("overlap", 0.0);
        trace.push("boost:doc-file", 0.15);
        assert_eq!(trace.total(), 0.15);
        assert_eq!(
            trace.into_components(),
            vec![("boost:doc-file".to_string(), 0.15)]
        );
    }

    #[test]
    fn empty_trace_totals_zero() {
        assert_eq!(ScoreTrace::new().total(), 0.0);
    }

    fn hit_with(rerank_score: f64, score_trace: Vec<(String, f64)>) -> SearchHit {
        SearchHit {
            chunk_id: "chunk-1".to_string(),
            file_path: "src/lib.rs".to_string(),
            language: cc_model::Language::Rust,
            start_line: 1,
            end_line: 10,
            breadcrumb: String::new(),
            symbol_name: None,
            symbol_kind: None,
            text: String::new(),
            fused_score: 0.0,
            lexical_score: 0.0,
            grep_score: 0.0,
            graph_score: 0.0,
            rerank_score,
            reasons: Vec::new(),
            score_trace,
            source: "index".to_string(),
            lane: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn apply_traced_boost_updates_score_and_trace_atomically() {
        let mut hit = hit_with(0.5, vec![("overlap".to_string(), 0.5)]);
        apply_traced_boost(&mut hit, "boost:dsl-name", 0.3);

        assert_eq!(hit.rerank_score, 0.5 + 0.3);
        assert_eq!(
            hit.score_trace,
            vec![
                ("overlap".to_string(), 0.5),
                ("boost:dsl-name".to_string(), 0.3),
            ]
        );
        // Invariant holds after the boost.
        let component_sum: f64 = hit.score_trace.iter().map(|(_, amount)| amount).sum();
        assert!((component_sum - hit.rerank_score).abs() < 1e-9);
    }

    #[test]
    fn apply_traced_boost_skips_zero_amount() {
        let mut hit = hit_with(0.5, vec![("overlap".to_string(), 0.5)]);
        apply_traced_boost(&mut hit, "boost:graph-rerank", 0.0);

        assert_eq!(hit.rerank_score, 0.5);
        assert_eq!(hit.score_trace, vec![("overlap".to_string(), 0.5)]);
    }

    #[test]
    fn apply_traced_boost_handles_negative_amount() {
        // No current call site bills a negative boost, but the seam must not
        // silently break the invariant if one ever does (e.g. a penalty).
        let mut hit = hit_with(0.5, vec![("overlap".to_string(), 0.5)]);
        apply_traced_boost(&mut hit, "boost:penalty", -0.2);

        assert!((hit.rerank_score - 0.3).abs() < 1e-12);
        assert_eq!(
            hit.score_trace,
            vec![
                ("overlap".to_string(), 0.5),
                ("boost:penalty".to_string(), -0.2),
            ]
        );
        let component_sum: f64 = hit.score_trace.iter().map(|(_, amount)| amount).sum();
        assert!((component_sum - hit.rerank_score).abs() < 1e-9);
    }
}
