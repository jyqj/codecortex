//! Persistent read identity, distinct from a process-local DB instance id.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadGeneration {
    pub incarnation: [u8; 16],
    pub index_epoch: u64,
    pub evidence_epoch: u64,
    /// None means semantic publication has not been implemented/configured.
    /// It is not a fabricated zero-valued semantic readiness claim.
    pub semantic_epoch: Option<u64>,
}
impl ReadGeneration {
    pub fn local_key(self, query: u64) -> ([u8; 16], u64, u64) {
        (self.incarnation, self.index_epoch, query)
    }
    pub fn graph_key(self, query: u64) -> ([u8; 16], u64, u64, u64) {
        (
            self.incarnation,
            self.index_epoch,
            self.evidence_epoch,
            query,
        )
    }
}
