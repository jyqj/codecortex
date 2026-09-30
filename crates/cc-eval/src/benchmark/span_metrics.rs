//! Byte coverage is evaluated against source gold, never implementation chunk IDs.
use super::schema::{Hit, Query};
use std::collections::BTreeSet;
fn union_len(mut spans: Vec<(u64, u64)>) -> u64 {
    spans.sort_unstable();
    let mut total = 0;
    let mut end = 0;
    for (s, e) in spans {
        if e > s {
            total += e.saturating_sub(s.max(end));
            end = end.max(e);
        }
    }
    total
}
pub fn coverage(hits: &[Hit], q: &Query) -> (Option<f64>, Option<f64>) {
    let paths: BTreeSet<&str> = q
        .answers
        .iter()
        .flat_map(|g| &g.alternatives)
        .filter(|a| a.span.is_some())
        .map(|a| a.path.as_str())
        .chain(hits.iter().map(|h| h.path.as_str()))
        .collect();
    let mut relevant = 0;
    let mut returned = 0;
    let mut overlap = 0;
    for p in paths {
        let gold: Vec<(u64, u64)> = q
            .answers
            .iter()
            .flat_map(|g| &g.alternatives)
            .filter(|a| a.path == p)
            .filter_map(|a| a.span.as_ref().map(|s| (s.start, s.end)))
            .collect();
        let got: Vec<(u64, u64)> = hits
            .iter()
            .filter(|h| h.path == p && h.evidence_valid != Some(false))
            .filter_map(|h| h.span.as_ref().map(|s| (s.start, s.end)))
            .collect();
        relevant += union_len(gold.clone());
        returned += union_len(got.clone());
        overlap += union_len(
            gold.iter()
                .flat_map(|a| got.iter().map(move |b| (a.0.max(b.0), a.1.min(b.1))))
                .collect(),
        );
    }
    if relevant == 0 {
        (None, None)
    } else {
        (
            Some(if returned == 0 {
                0.0
            } else {
                overlap as f64 / returned as f64
            }),
            Some(overlap as f64 / relevant as f64),
        )
    }
}
