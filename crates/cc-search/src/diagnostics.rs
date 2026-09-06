//! Per-request retrieval work/coverage diagnostics. No production score is a gold.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LaneDiagnostic {
    pub enabled: bool,
    pub returned: usize,
    pub scanned: usize,
    pub work_limited: bool,
    pub candidate_limit_reached: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateLocation {
    pub chunk_id: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrievalDiagnostics {
    pub lanes: BTreeMap<String, LaneDiagnostic>,
    pub candidate_union: usize,
    pub rerank_candidates: usize,
    pub returned: usize,
    /// Optional instrumented runs only; capture stops at 512 unique candidates.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stages: BTreeMap<String, Vec<CandidateLocation>>,
    pub trace_truncated: bool,
}
impl RetrievalDiagnostics {
    /// Work-limit results are valid partial observations, but should be retried
    /// rather than pinned to a cache generation. Ordinary top-k is not an error.
    pub fn cacheable(&self) -> bool {
        self.lanes
            .values()
            .all(|l| !l.work_limited && l.errors.is_empty())
    }
}
