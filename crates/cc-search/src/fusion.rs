//! Deterministic rank-only fusion over versioned document candidates.
use std::collections::{HashMap, HashSet};

use cc_model::retrieval::LaneStatus;
use cc_model::{CcError, CcResult};

use crate::lanes::LaneOutcome;

#[derive(Debug, Clone, Default)]
pub(crate) struct FusedScore {
    pub(crate) total: f64,
    pub(crate) by_lane: Vec<(String, f64)>,
    pub(crate) exact_identity: bool,
}

struct Working {
    chunk_id: String,
    score: FusedScore,
}

/// RRF only consumes lane rank and configured lane weight. Raw BM25/grep/path
/// scores are diagnostics and never enter this arithmetic.
pub(crate) fn fuse_outcomes(
    outcomes: &[LaneOutcome],
    rrf_k: usize,
) -> CcResult<HashMap<String, FusedScore>> {
    let mut by_document: HashMap<(String, String), Working> = HashMap::new();
    let mut lane_ids = HashSet::new();
    let mut versions = HashMap::new();
    let mut references: HashMap<(&str, &str), &cc_model::retrieval::CandidateRef> = HashMap::new();
    for outcome in outcomes {
        let public = outcome.public.as_ref().ok_or_else(|| {
            CcError::InvalidParams("lane outcome was fused before identity materialization".into())
        })?;
        public.validate()?;
        if !lane_ids.insert(public.lane_id.as_str()) {
            return Err(CcError::InvalidParams(
                "duplicate retrieval lane vote".into(),
            ));
        }
        if !public.status.is_fusable() {
            continue;
        }
        debug_assert!(matches!(
            public.status,
            LaneStatus::Complete | LaneStatus::Partial
        ));
        for candidate in &public.candidates {
            let doc_key = candidate.document.doc_key.as_str();
            let version = candidate.document.doc_version.as_str();
            if versions
                .insert(doc_key, version)
                .is_some_and(|old| old != version)
            {
                return Err(CcError::Search(
                    "mixed document versions during retrieval; retry".into(),
                ));
            }
            if let Some(prior) = references.insert((doc_key, version), candidate) {
                if prior.document != candidate.document
                    || prior.source_span != candidate.source_span
                {
                    return Err(CcError::InvalidParams(
                        "inconsistent source evidence for one document version".into(),
                    ));
                }
            }

            let denominator = rrf_k
                .checked_add(candidate.lane_rank)
                .ok_or_else(|| CcError::InvalidParams("RRF denominator overflow".into()))?;
            let contribution = public.weight / denominator as f64;
            if !contribution.is_finite() || contribution < 0.0 {
                return Err(CcError::InvalidParams(
                    "non-finite or negative RRF contribution".into(),
                ));
            }
            let key = (
                candidate.document.doc_key.clone(),
                candidate.document.doc_version.clone(),
            );
            let entry = by_document.entry(key).or_insert_with(|| Working {
                chunk_id: candidate.legacy_chunk_id.clone(),
                score: FusedScore::default(),
            });
            if entry.chunk_id != candidate.legacy_chunk_id {
                return Err(CcError::InvalidParams(
                    "one document version mapped to multiple chunk locators".into(),
                ));
            }
            entry.score.total += contribution;
            entry
                .score
                .by_lane
                .push((public.lane_id.clone(), contribution));
            entry.score.exact_identity |= candidate.exact_identity;
        }
    }
    let mut by_chunk = HashMap::with_capacity(by_document.len());
    for working in by_document.into_values() {
        if !working.score.total.is_finite()
            || by_chunk.insert(working.chunk_id, working.score).is_some()
        {
            return Err(CcError::InvalidParams(
                "invalid or aliased fused document candidate".into(),
            ));
        }
    }
    Ok(by_chunk)
}

/// Final hydration must still describe the document selected by the lanes.
/// This fence does not claim a whole-query/whole-filesystem atomic snapshot.
pub(crate) fn validate_hydrated_candidate(
    expected: &cc_model::retrieval::CandidateRef,
    actual: Option<&cc_model::identity::DocumentRef>,
    span: Option<cc_model::source::ByteSpan>,
) -> CcResult<()> {
    if actual != Some(&expected.document) || span != Some(expected.source_span) {
        return Err(CcError::Search(
            "document changed between retrieval and hydration; retry".into(),
        ));
    }
    Ok(())
}

/// Recompute the fused score from the public bill in accumulation order.
pub(crate) fn replay_fused(score: &FusedScore) -> CcResult<f64> {
    let mut total = 0.0;
    for (_, value) in &score.by_lane {
        if !value.is_finite() || *value < 0.0 {
            return Err(CcError::InvalidParams("invalid RRF trace component".into()));
        }
        total += value;
    }
    if total.to_bits() != score.total.to_bits() {
        return Err(CcError::InvalidParams("RRF trace does not replay".into()));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::test_lane_outcome;
    fn one(id: &'static str) -> LaneOutcome {
        test_lane_outcome(id, 1.0, vec![("source".into(), 1.0)])
    }
    #[test]
    fn duplicate_lanes_cannot_cast_two_votes() {
        assert!(fuse_outcomes(&[one("same"), one("same")], 50).is_err());
    }
    #[test]
    fn raw_score_scale_does_not_change_rrf_and_trace_replays() {
        let first = one("first");
        let mut second = one("second");
        second.public.as_mut().unwrap().candidates[0].raw_score = -1e200;
        let result = fuse_outcomes(&[first, second], 50).unwrap();
        assert_eq!(result["source"].total, 2.0 / 51.0);
        replay_fused(&result["source"]).unwrap();
    }
    #[test]
    fn conflicting_document_spans_and_versions_are_rejected() {
        let mut span = one("second");
        span.public.as_mut().unwrap().candidates[0].source_span.end += 1;
        assert!(fuse_outcomes(&[one("first"), span], 50).is_err());
        let mut version = one("second");
        let c = &mut version.public.as_mut().unwrap().candidates[0];
        c.document.doc_version = "a".repeat(64);
        c.legacy_chunk_id = "different-locator".into();
        assert!(fuse_outcomes(&[one("first"), version], 50).is_err());
    }
    #[test]
    fn final_hydration_cannot_substitute_new_or_missing_evidence() {
        let lane = one("first");
        let c = &lane.public.as_ref().unwrap().candidates[0];
        validate_hydrated_candidate(c, Some(&c.document), Some(c.source_span)).unwrap();
        assert!(validate_hydrated_candidate(c, None, Some(c.source_span)).is_err());
        let mut different = c.document.clone();
        different.doc_version = "b".repeat(64);
        assert!(validate_hydrated_candidate(c, Some(&different), Some(c.source_span)).is_err());
    }
}
