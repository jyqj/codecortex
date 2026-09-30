//! Optional asynchronous recall port. No provider, transport or vector store dependency.
use crate::{
    query::QueryControl,
    retrieval::{HardScope, LaneOutcome},
    CcResult,
};
use std::{future::Future, pin::Pin};

pub type RecallGeneration = crate::generation::ReadGeneration;
#[derive(Debug, Clone)]
pub struct SemanticRequest {
    pub query: String,
    pub scope: HardScope,
    pub limit: usize,
    pub policy_fingerprint: String,
    pub generation: RecallGeneration,
}
#[derive(Debug, Clone)]
pub struct SemanticResponse {
    pub generation: RecallGeneration,
    pub outcome: LaneOutcome,
}
/// Implementations honour hard scope before top-k and return versioned source
/// references. The caller independently verifies all references against its DB.
pub trait SemanticRecall: Send + Sync {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>>;
}
