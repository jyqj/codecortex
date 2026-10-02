//! Validate optional semantic receipts without trusting provider locators as source facts.
use crate::execution::ExecutionPool;
use cc_model::{
    query::QueryControl,
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus, LANE_OUTCOME_SCHEMA_VERSION},
    semantic::{SemanticRecall, SemanticRequest, SemanticResponse},
    CcError, CcResult,
};

pub async fn recall(
    pool: &ExecutionPool,
    port: &dyn SemanticRecall,
    request: SemanticRequest,
    control: QueryControl,
) -> CcResult<SemanticResponse> {
    let generation = request.generation;
    let limit = request.limit;
    let started = std::time::Instant::now();
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        port.recall(request, control.clone())
    })) {
        Ok(future) => pool.run_async(&control, future).await,
        Err(_) => Err(CcError::Search(
            "semantic port construction panicked".into(),
        )),
    };
    let mut outcome = match result {
        Ok(value) => value,
        Err(CcError::QueryCancelled) => return Err(CcError::QueryCancelled),
        Err(CcError::QueryTimedOut) => empty(LaneStatus::Timeout, "semantic_deadline"),
        Err(CcError::QueryBusy) => empty(LaneStatus::Unavailable, "semantic_capacity"),
        Err(_) => empty(LaneStatus::Error, "semantic_read_error"),
    };
    outcome.elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    outcome.validate()?;
    if outcome.lane_id != "semantic"
        || outcome.candidate_count > limit
        || outcome.candidates.iter().any(|c| c.exact_identity)
    {
        return Err(CcError::InvalidParams(
            "invalid semantic candidate receipt".into(),
        ));
    }
    // The caller's policy fixes the weight; providers cannot buy ranking influence.
    outcome.weight = 1.0;
    Ok(SemanticResponse {
        generation,
        outcome,
    })
}
fn empty(status: LaneStatus, reason: &str) -> LaneOutcome {
    LaneOutcome {
        schema_version: LANE_OUTCOME_SCHEMA_VERSION,
        lane_id: "semantic".into(),
        weight: 1.0,
        status,
        elapsed_us: 0,
        candidate_count: 0,
        coverage: LaneCoverage::not_run(),
        truncation_reason: Some(reason.into()),
        candidates: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::search::SearchRequest;
    use std::future::Future;
    use std::pin::Pin;
    use std::time::Duration;

    fn generation() -> cc_model::semantic::RecallGeneration {
        cc_model::semantic::RecallGeneration {
            incarnation: [7; 16],
            index_epoch: 0,
            evidence_epoch: 0,
            semantic_epoch: None,
        }
    }

    /// A port whose recall never finishes (the "slow HTTP" shape of the
    /// P7-013 cancellation matrix; the transport itself lives behind the
    /// port, the adapter only sees the future).
    struct PendingPort;
    impl SemanticRecall for PendingPort {
        fn recall(
            &self,
            _request: SemanticRequest,
            _control: QueryControl,
        ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
            Box::pin(async { std::future::pending::<CcResult<LaneOutcome>>().await })
        }
    }

    /// A port that would succeed instantly — used to prove the adapter's own
    /// entry checkpoint fires before the port runs on an expired budget.
    struct InstantPort;
    impl SemanticRecall for InstantPort {
        fn recall(
            &self,
            _request: SemanticRequest,
            _control: QueryControl,
        ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
            Box::pin(async { Ok(LaneOutcome::disabled("semantic", 1.0)) })
        }
    }

    fn request() -> SemanticRequest {
        SemanticRequest {
            query: "deadline matrix".into(),
            scope: cc_model::retrieval::HardScope::default(),
            limit: 4,
            policy_fingerprint: "test".into(),
            generation: generation(),
        }
    }

    fn default_policy() -> crate::query_policy::QueryPolicy {
        crate::query_policy::QueryPolicy::resolve(
            &cc_model::query::QueryConfig::default(),
            &SearchRequest::default(),
            false,
        )
        .expect("policy")
    }

    /// A small lane share (20ms) under a comfortable total deadline — the
    /// "share fits under the parent" allocation shape.
    fn small_share_policy() -> crate::query_policy::QueryPolicy {
        crate::query_policy::QueryPolicy::resolve(
            &cc_model::query::QueryConfig {
                semantic_timeout_ms: 20,
                ..Default::default()
            },
            &SearchRequest::default(),
            false,
        )
        .expect("policy")
    }

    // ── P7-013 deadline trigger matrix ───────────────────────────────────

    #[tokio::test]
    async fn budget_exhausted_before_the_lane_runs_is_a_timeout_receipt_not_a_query_error() {
        // "编码超时" face: the lane checkpoint (total deadline) is already
        // expired when the lane is admitted — the adapter degrades to a
        // Timeout receipt instead of failing the query or running the port.
        let pool = ExecutionPool::new(1, 1, 1).unwrap();
        let control = QueryControl::new(Duration::ZERO).unwrap();
        let response = recall(&pool, &InstantPort, request(), control)
            .await
            .expect("degraded receipt, not an error");
        assert_eq!(response.outcome.status, LaneStatus::Timeout);
        assert_eq!(
            response.outcome.truncation_reason.as_deref(),
            Some("semantic_deadline")
        );
        assert_eq!(response.outcome.candidate_count, 0);
    }

    #[tokio::test]
    async fn recall_timeout_degrades_the_lane_and_leaves_the_parent_budget_usable() {
        // 召回超时: the lane's own share expires mid-recall; the PARENT
        // total deadline is untouched, so the rest of the query (local
        // lanes) still runs — the isolation contract.
        let pool = ExecutionPool::new(1, 1, 1).unwrap();
        let parent = QueryControl::new(Duration::from_secs(5)).unwrap();
        let child = crate::execution::semantic_child_budget(&parent, &small_share_policy());
        let response = recall(&pool, &PendingPort, request(), child)
            .await
            .expect("degraded receipt");
        assert_eq!(response.outcome.status, LaneStatus::Timeout);
        parent
            .check()
            .expect("parent budget unaffected by the lane timeout");
        let local = pool.run_cpu(parent, || Ok("local-lanes".to_string())).await;
        assert_eq!(local.expect("local work runs"), "local-lanes");
    }

    #[tokio::test]
    async fn total_budget_exhaustion_governs_after_the_lane_degrades() {
        // 预算耗尽: the share is clamped to the parent remainder, the lane
        // degrades to Timeout — and the NEXT total-deadline checkpoint (the
        // local search admission) fails the whole query with QueryTimedOut.
        let pool = ExecutionPool::new(1, 1, 1).unwrap();
        let parent = QueryControl::new(Duration::from_millis(40)).unwrap();
        let child = crate::execution::semantic_child_budget(&parent, &default_policy());
        assert_eq!(child.deadline(), parent.deadline(), "share clamps to total");
        let response = recall(&pool, &PendingPort, request(), child)
            .await
            .expect("degraded receipt");
        assert_eq!(response.outcome.status, LaneStatus::Timeout);
        let result: CcResult<()> = pool.run_cpu(parent, || Ok(())).await;
        assert!(
            matches!(result, Err(CcError::QueryTimedOut)),
            "the exhausted total deadline must fail the whole query: {result:?}"
        );
    }
}
