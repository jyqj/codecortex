//! Versioned public-protocol benchmarks; adapters never receive gold answers.
pub mod ablation;
pub mod adapters;
pub mod comparison;
pub mod fnmatch;
pub mod gate;
pub mod importer_oce;
pub mod manifest;
pub mod metrics;
pub mod mutation_case;
pub mod mutations;
pub mod normalizer;
pub mod oracle;
pub mod p8_load;
pub mod p8_scale;
pub mod readiness;
pub mod report;
pub mod runner;
pub mod sampler;
pub mod schema;
pub mod span_metrics;
pub mod statistics;
pub mod validation;

use thiserror::Error;
#[derive(Debug, Error)]
pub enum BenchError {
    #[error("invalid benchmark input: {0}")]
    Invalid(String),
    #[error("protocol/infrastructure error: {0}")]
    Protocol(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("deadline exceeded: {0}")]
    Timeout(String),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, BenchError>;
pub fn invalid(message: impl Into<String>) -> BenchError {
    BenchError::Invalid(message.into())
}
