//! Deterministic rank-only fusion over versioned document candidates.
use std::cmp::Ordering;
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
///
/// Dense votes ride the same arithmetic: a fusable semantic receipt
/// (`Complete` | `Partial`) votes `weight / (rrf_k + lane_rank)` per
/// candidate exactly like a local lane, its cosine raw scores stay
/// diagnostic, and a receipt in any other state (`Timeout`, `Unavailable`,
/// `Error`, `Cancelled`, `Disabled`, `NotConfigured`) casts no votes at all —
/// the fused ordering then rests on the local lanes alone while the receipt
/// stays visible on the lane surface for explain.
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

/// Final fused-candidate ordering: the exact-identity tier first, then the
/// fused RRF total descending, then `chunk_id` ascending. The last key makes
/// total ties independent of HashMap iteration order and of lane completion
/// order — the fused order is a pure function of the lane receipts.
pub(crate) fn fused_candidate_ordering(
    a: &(String, FusedScore),
    b: &(String, FusedScore),
) -> Ordering {
    b.1.exact_identity
        .cmp(&a.1.exact_identity)
        .then_with(|| b.1.total.total_cmp(&a.1.total))
        .then_with(|| a.0.cmp(&b.0))
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
    use crate::lanes::{materialize_test_outcomes, test_lane_outcome, LaneOutcome};
    use cc_model::context::LaneCoverageExplain;
    use cc_model::retrieval::LaneCoverage;

    fn one(id: &'static str) -> LaneOutcome {
        test_lane_outcome(id, 1.0, vec![("source".into(), 1.0)])
    }

    /// Author a semantic receipt the way `append_semantic_outcome`
    /// materializes one: status/coverage/reason ride on the lane outcome and
    /// only fusable statuses carry candidates (cc-model `validate`).
    fn semantic_receipt(
        status: LaneStatus,
        truncation_reason: Option<&str>,
        hits: Vec<(&'static str, f64)>,
    ) -> LaneOutcome {
        let count = hits.len();
        let mut outcome = LaneOutcome {
            lane_id: "semantic",
            weight: 1.0,
            annotates_hits: true,
            score_slot: None,
            exact_ids: HashSet::new(),
            status,
            coverage: match status {
                LaneStatus::Complete => LaneCoverage::complete(None, count),
                LaneStatus::Partial => LaneCoverage::partial(None, count),
                _ => LaneCoverage::not_run(),
            },
            truncation_reason: truncation_reason.map(str::to_string),
            elapsed_us: 0,
            public: None,
            hits: hits
                .into_iter()
                .map(|(id, score)| (id.to_string(), score))
                .collect(),
            grep: None,
            lexical_work: Default::default(),
        };
        materialize_test_outcomes(std::slice::from_mut(&mut outcome));
        outcome
    }

    /// A fixed local-lane bill standing in for the registry lanes.
    fn local_outcomes() -> Vec<LaneOutcome> {
        vec![
            test_lane_outcome("exact_symbol", 1.1, vec![("src/a.rs".into(), 1.0)]),
            test_lane_outcome(
                "lexical",
                1.1,
                vec![("src/a.rs".into(), 9.0), ("src/b.rs".into(), 4.0)],
            ),
            test_lane_outcome("graph", 0.6, vec![("src/b.rs".into(), 0.5)]),
        ]
    }

    fn fused_with_semantic(
        status: LaneStatus,
        reason: Option<&str>,
        hits: Vec<(&'static str, f64)>,
    ) -> HashMap<String, FusedScore> {
        let mut lanes = local_outcomes();
        lanes.push(semantic_receipt(status, reason, hits));
        fuse_outcomes(&lanes, 50).unwrap()
    }

    fn explain(outcome: &LaneOutcome) -> LaneCoverageExplain {
        LaneCoverageExplain::from_lane_outcome(outcome.public.as_ref().unwrap())
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

    // ── P7-012: dense vote fusion, tie-break determinism, partial coverage ──

    #[test]
    fn dense_votes_fuse_by_rank_only_and_never_mix_raw_scores() {
        // A cosine-scale perturbation of the semantic raw scores cannot move
        // the fused bill: only lane rank and configured weight enter RRF.
        let fused_with_raw = |raw: f64| {
            fused_with_semantic(
                LaneStatus::Complete,
                None,
                vec![("src/c.rs", raw), ("src/a.rs", raw / 2.0)],
            )
        };
        let baseline = fused_with_raw(0.97);
        let disturbed = fused_with_raw(-1e200);
        for key in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            assert_eq!(baseline[key].total.to_bits(), disturbed[key].total.to_bits());
            assert_eq!(baseline[key].by_lane, disturbed[key].by_lane);
        }
        // The semantic vote is the plain weighted rank term, billed per lane.
        assert_eq!(
            baseline["src/c.rs"].by_lane,
            vec![("semantic".to_string(), 1.0 / 51.0)]
        );
        assert_eq!(
            baseline["src/a.rs"].by_lane,
            vec![
                ("exact_symbol".to_string(), 1.1 / 51.0),
                ("lexical".to_string(), 1.1 / 51.0),
                ("semantic".to_string(), 1.0 / 52.0),
            ]
        );
        // Final order: fused total descending, ties by chunk_id ascending.
        let mut ranked: Vec<_> = baseline.into_iter().collect();
        ranked.sort_by(fused_candidate_ordering);
        let ids: Vec<_> = ranked.into_iter().map(|(id, _)| id).collect();
        assert_eq!(
            ids,
            vec![
                "src/a.rs".to_string(),
                "src/b.rs".to_string(),
                "src/c.rs".to_string()
            ]
        );
    }

    #[test]
    fn partial_semantic_receipts_vote_but_never_masquerade_as_complete() {
        let complete = fused_with_semantic(
            LaneStatus::Complete,
            None,
            vec![("src/d.rs", 0.9), ("src/a.rs", 0.4)],
        );
        let partial = fused_with_semantic(
            LaneStatus::Partial,
            Some("semantic_coverage_uncovered"),
            vec![("src/d.rs", 0.9), ("src/a.rs", 0.4)],
        );
        // Same ranks, same votes: coverage honesty lives on the receipt, not
        // in the fusion arithmetic (Partial is fusable by contract).
        for key in ["src/a.rs", "src/b.rs", "src/d.rs"] {
            assert_eq!(complete[key].total.to_bits(), partial[key].total.to_bits());
            assert_eq!(complete[key].by_lane, partial[key].by_lane);
        }
        // But the fused result's explain face keeps the receipt's honesty.
        let receipt = semantic_receipt(
            LaneStatus::Partial,
            Some("semantic_coverage_uncovered"),
            vec![("src/d.rs", 0.9)],
        );
        let projected = explain(&receipt);
        assert_eq!(projected.lane_id, "semantic");
        assert_eq!(projected.status, LaneStatus::Partial);
        assert_eq!(
            projected.truncation_reason.as_deref(),
            Some("semantic_coverage_uncovered")
        );
        assert!(!projected.coverage.complete);
    }

    #[test]
    fn degraded_semantic_lanes_cast_no_votes_and_leave_local_fusion_untouched() {
        let locals = fuse_outcomes(&local_outcomes(), 50).unwrap();
        for (status, reason) in [
            (LaneStatus::Timeout, Some("semantic_deadline")),
            (LaneStatus::Unavailable, Some("semantic_capacity")),
            (LaneStatus::Error, Some("semantic_read_error")),
            (LaneStatus::Disabled, None),
            (LaneStatus::NotConfigured, None),
        ] {
            let fused = fused_with_semantic(status, reason, vec![]);
            assert_eq!(fused.len(), locals.len(), "{status:?} must add no candidate");
            for (id, score) in &locals {
                assert_eq!(score.total.to_bits(), fused[id].total.to_bits());
                assert_eq!(score.by_lane, fused[id].by_lane);
            }
        }
    }

    #[test]
    fn semantic_timeout_and_complete_zero_hits_stay_distinguishable() {
        // Acceptance pairing: "timeout 与无命中可区分". Both scenarios fuse to
        // zero semantic votes; the difference lives on the explain face and
        // is carried by status + truncation_reason, never collapsed.
        let timed_out = semantic_receipt(LaneStatus::Timeout, Some("semantic_deadline"), vec![]);
        let ran_empty = semantic_receipt(LaneStatus::Complete, None, vec![]);
        let timed_out = explain(&timed_out);
        let ran_empty = explain(&ran_empty);
        assert_eq!(timed_out.status, LaneStatus::Timeout);
        assert_eq!(timed_out.truncation_reason.as_deref(), Some("semantic_deadline"));
        assert_eq!(timed_out.candidate_count, 0);
        assert!(!timed_out.coverage.complete);
        assert_eq!(ran_empty.status, LaneStatus::Complete);
        assert_eq!(ran_empty.truncation_reason, None);
        assert_eq!(ran_empty.candidate_count, 0);
        assert!(ran_empty.coverage.complete);
    }

    #[test]
    fn declared_full_semantic_coverage_points_at_its_evidence() {
        // "declared full coverage 有证据": a Complete receipt's claim is
        // backed by its versioned candidate bill — count == lower bound ==
        // bill, and every candidate carries an identity that final hydration
        // verifies.
        let receipt = semantic_receipt(
            LaneStatus::Complete,
            None,
            vec![("src/d.rs", 0.9), ("src/a.rs", 0.4)],
        );
        let public = receipt.public.as_ref().unwrap();
        let projected = explain(&receipt);
        assert_eq!(projected.status, LaneStatus::Complete);
        assert!(projected.coverage.complete);
        assert_eq!(projected.candidate_count, public.candidates.len());
        assert_eq!(projected.coverage.total_lower_bound, projected.candidate_count);
        for candidate in &public.candidates {
            candidate.validate().unwrap();
            validate_hydrated_candidate(
                candidate,
                Some(&candidate.document),
                Some(candidate.source_span),
            )
            .unwrap();
        }
    }

    #[test]
    fn fused_order_ties_break_deterministically() {
        // Two documents with identical fused totals from symmetric votes: the
        // final key (chunk_id ascending) decides, independent of HashMap
        // iteration order.
        let lanes = vec![
            test_lane_outcome("lane-a", 1.0, vec![("src/z.rs".into(), 1.0)]),
            test_lane_outcome("lane-b", 1.0, vec![("src/a.rs".into(), 1.0)]),
        ];
        let fused = fuse_outcomes(&lanes, 50).unwrap();
        let ids = |mut ranked: Vec<(String, FusedScore)>| {
            ranked.sort_by(fused_candidate_ordering);
            ranked.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
        };
        let forward = ids(fused.clone().into_iter().collect());
        let mut entries: Vec<_> = fused.into_iter().collect();
        entries.reverse();
        let reversed = ids(entries);
        assert_eq!(forward, reversed);
        assert_eq!(forward, vec!["src/a.rs".to_string(), "src/z.rs".to_string()]);

        // The exact-identity tier outranks any fused total.
        let mut plain = test_lane_outcome("lane-a", 1.0, vec![("src/z.rs".into(), 1.0)]);
        let mut exact = test_lane_outcome("lane-b", 1.0, vec![("src/a.rs".into(), 1.0)]);
        exact.public.as_mut().unwrap().candidates[0].exact_identity = true;
        plain.public.as_mut().unwrap().candidates[0].raw_score = 1e200;
        let fused = fuse_outcomes(&[plain, exact], 50).unwrap();
        let mut ranked: Vec<_> = fused.into_iter().collect();
        ranked.sort_by(fused_candidate_ordering);
        let ids: Vec<_> = ranked.into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["src/a.rs".to_string(), "src/z.rs".to_string()]);
    }
}
