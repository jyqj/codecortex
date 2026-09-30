//! Durable resolution maintenance, not a claim of compiler or disk freshness.
use crate::{repo_path, resolution::ResolutionDependency, CcError, CcResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RECONCILE_VERSION: u32 = 1;
pub const MAX_FRONTIER_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRONTIER_ENTRIES: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Body,
    PublicSurface,
    BoundAddress,
    Configuration,
    Inventory,
    CandidateSet,
    Resumed,
    Rebased,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileStop {
    BudgetExceeded,
    PartialClosure,
    Disabled,
}
impl ReconcileStop {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BudgetExceeded => "budget_exceeded",
            Self::PartialClosure => "partial_closure",
            Self::Disabled => "disabled",
        }
    }
}

/// A replayable invalidation basis. Roots/events are retained until every
/// reachable consumer has been processed; a limited lookup is never the entire
/// frontier. `completed` only belongs to this basis. A new input event rebases
/// it, clearing completion proofs instead of reusing old bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconcileState {
    pub version: u32,
    pub basis_epoch: u64,
    pub roots: BTreeSet<String>,
    pub propagated: BTreeSet<String>,
    pub completed: BTreeSet<String>,
    pub events: BTreeSet<ResolutionDependency>,
    pub reasons: BTreeSet<ChangeKind>,
    pub stop: ReconcileStop,
}
impl ReconcileState {
    pub fn new(basis_epoch: u64) -> Self {
        Self {
            version: RECONCILE_VERSION,
            basis_epoch,
            roots: BTreeSet::new(),
            propagated: BTreeSet::new(),
            completed: BTreeSet::new(),
            events: BTreeSet::new(),
            reasons: BTreeSet::new(),
            stop: ReconcileStop::PartialClosure,
        }
    }
    pub fn validate(&self) -> CcResult<()> {
        let paths = self
            .roots
            .iter()
            .chain(&self.propagated)
            .chain(&self.completed);
        if self.version != RECONCILE_VERSION
            || self.roots.is_empty()
            || self.roots.len() + self.propagated.len() + self.completed.len() + self.events.len()
                > MAX_FRONTIER_ENTRIES
            || paths
                .clone()
                .any(|p| p.len() > 4096 || !repo_path::is_canonical_file(p))
            || self
                .events
                .iter()
                .any(|e| e.key.is_empty() || e.key.len() > 16384 || e.key.contains('\0'))
        {
            return Err(CcError::InvalidParams(
                "invalid resolution frontier; explicit full rebuild required".into(),
            ));
        }
        Ok(())
    }
    pub fn payload(&self) -> CcResult<String> {
        self.validate()?;
        let text = serde_json::to_string(self)?;
        if text.len() > MAX_FRONTIER_BYTES {
            return Err(CcError::InvalidParams("resolution frontier exceeds bounded storage; no partial commit, explicit full rebuild required".into()));
        }
        Ok(text)
    }
}

/// Written only together with the corresponding indexed file/edge batch.
#[derive(Debug, Clone)]
pub struct ReconcileUpdate {
    pub expected_index_epoch: u64,
    pub next: Option<ReconcileState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyPlanExplanation {
    /// Only bounded reverse-dependency statements, not total indexing work.
    pub dependency_sql: crate::retrieval_cost::SqlWork,
    pub parsed_files: usize,
    pub selected_dependents: usize,
    pub file_budget: usize,
    pub reasons: BTreeSet<ChangeKind>,
    pub resumed: bool,
    pub rebased: bool,
}

/// Freshness is scoped to *observed resolution invalidations*. It does not
/// certify parser completeness, unobserved filesystem changes or postprocess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionFreshness {
    pub scope: String,
    pub status: String,
    pub complete: bool,
    pub index_epoch: u64,
    pub basis_epoch: Option<u64>,
    pub root_count: usize,
    pub completed_files: usize,
    pub reason: Option<String>,
    pub retry: Option<String>,
}
impl ResolutionFreshness {
    pub fn ready(index_epoch: u64) -> Self {
        Self {
            scope: "observed_resolution_invalidations".into(),
            status: "ready".into(),
            complete: true,
            index_epoch,
            basis_epoch: None,
            root_count: 0,
            completed_files: 0,
            reason: None,
            retry: None,
        }
    }
}
