use serde::{Deserialize, Serialize};

/// A single observed HTTP call from runtime trace data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceObservation {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub source_service: Option<String>,
    #[serde(default)]
    pub target_service: Option<String>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// Result of trace ingestion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceIngestResult {
    pub traces_received: usize,
    pub edges_confirmed: usize,
    pub edges_discovered: usize,
    pub edges_unmatched: usize,
}
