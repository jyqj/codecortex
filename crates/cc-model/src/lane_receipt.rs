//! Bounded wire projection of an already validated LaneOutcome. This preserves
//! execution/coverage truth without pretending omitted candidates were transmitted.
use crate::{
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus},
    CcError, CcResult,
};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaneReceipt {
    pub schema_version: u32,
    pub lane_id: String,
    pub weight: f64,
    pub status: LaneStatus,
    pub elapsed_us: u64,
    pub candidate_count: usize,
    pub coverage: LaneCoverage,
    pub truncation_reason: Option<String>,
    pub candidate_details_omitted: bool,
}
impl From<&LaneOutcome> for LaneReceipt {
    fn from(lane: &LaneOutcome) -> Self {
        Self {
            schema_version: lane.schema_version,
            lane_id: lane.lane_id.clone(),
            weight: lane.weight,
            status: lane.status,
            elapsed_us: lane.elapsed_us,
            candidate_count: lane.candidate_count,
            coverage: lane.coverage.clone(),
            truncation_reason: lane.truncation_reason.clone(),
            candidate_details_omitted: true,
        }
    }
}
impl LaneReceipt {
    pub fn validate(&self) -> CcResult<()> {
        if !self.candidate_details_omitted
            || self.coverage.total_lower_bound < self.candidate_count
            || (!self.status.is_fusable() && self.candidate_count != 0)
        {
            return Err(CcError::InvalidParams(
                "invalid projected lane receipt".into(),
            ));
        }
        // Reuse header invariants only. Candidate count/content validation is
        // explicitly unavailable on this projection, never claimed as performed.
        LaneOutcome {
            schema_version: self.schema_version,
            lane_id: self.lane_id.clone(),
            weight: self.weight,
            status: self.status,
            elapsed_us: self.elapsed_us,
            candidate_count: 0,
            coverage: self.coverage.clone(),
            truncation_reason: self.truncation_reason.clone(),
            candidates: vec![],
        }
        .validate()
    }
}
