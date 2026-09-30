//! Exact byte-union accounting, namespaced by file AND source snapshot.
use cc_model::{source::ByteSpan, CcError, CcResult};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize)]
pub struct OverlapStats {
    pub input_bytes: usize,
    pub unique_bytes: usize,
    pub redundant_bytes: usize,
    pub fully_covered_candidates: usize,
}
#[derive(Default)]
pub struct SourceCoverage {
    intervals: BTreeMap<(String, String), Vec<ByteSpan>>,
    pub stats: OverlapStats,
}
impl SourceCoverage {
    /// Query novelty against evidence actually selected, not merely seen.
    pub fn new_bytes(&self, path: &str, snapshot: &str, span: ByteSpan) -> CcResult<usize> {
        if path.is_empty() || snapshot.is_empty() || span.start >= span.end {
            return Err(CcError::InvalidParams(
                "invalid overlap provenance/span".into(),
            ));
        }
        let covered: usize = self
            .intervals
            .get(&(path.into(), snapshot.into()))
            .into_iter()
            .flatten()
            .map(|old| {
                old.end
                    .min(span.end)
                    .saturating_sub(old.start.max(span.start))
            })
            .sum();
        Ok(span.len().saturating_sub(covered))
    }
    /// Return new source bytes; merge intervals without treating same-text
    /// copies in different files (or versions) as the same provenance.
    pub fn add(&mut self, path: &str, snapshot: &str, span: ByteSpan) -> CcResult<usize> {
        if path.is_empty() || snapshot.is_empty() || span.start >= span.end {
            return Err(CcError::InvalidParams(
                "invalid overlap provenance/span".into(),
            ));
        }
        let spans = self
            .intervals
            .entry((path.into(), snapshot.into()))
            .or_default();
        let duplicate: usize = spans
            .iter()
            .map(|old| {
                old.end
                    .min(span.end)
                    .saturating_sub(old.start.max(span.start))
            })
            .sum();
        let bytes = span.end - span.start;
        let new_bytes = bytes.saturating_sub(duplicate);
        self.stats.input_bytes = self.stats.input_bytes.saturating_add(bytes);
        self.stats.unique_bytes = self.stats.unique_bytes.saturating_add(new_bytes);
        self.stats.redundant_bytes = self.stats.redundant_bytes.saturating_add(duplicate);
        self.stats.fully_covered_candidates += usize::from(new_bytes == 0);
        spans.push(span);
        spans.sort_by_key(|s| (s.start, s.end));
        let mut merged: Vec<ByteSpan> = Vec::with_capacity(spans.len());
        for range in spans.drain(..) {
            if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        *spans = merged;
        Ok(new_bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn union_is_exact_and_different_provenance_survives() {
        let mut c = SourceCoverage::default();
        for (start, end, new) in [
            (10, 20, 10),
            (15, 25, 5),
            (12, 18, 0),
            (25, 30, 5),
            (5, 12, 5),
        ] {
            assert_eq!(c.add("a.rs", "v1", ByteSpan { start, end }).unwrap(), new);
        }
        assert_eq!(c.stats.unique_bytes, 25);
        assert_eq!(
            c.stats.input_bytes - c.stats.unique_bytes,
            c.stats.redundant_bytes
        );
        assert_eq!(
            c.add("b.py", "v1", ByteSpan { start: 10, end: 20 })
                .unwrap(),
            10
        );
        assert_eq!(
            c.add("a.rs", "v2", ByteSpan { start: 10, end: 20 })
                .unwrap(),
            10
        );
        assert!(c
            .add("a.rs", "v1", ByteSpan { start: 20, end: 20 })
            .is_err());
    }
}
