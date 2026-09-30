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
