/// Unified error type for the CodeCortex system.
#[derive(Debug, thiserror::Error)]
pub enum CcError {
    #[error("project not set; call set_project first")]
    ProjectNotSet,

    #[error("database error: {0}")]
    Database(String),

    #[error("parse error in {file}: {message}")]
    Parse { file: String, message: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("search error: {0}")]
    Search(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// A split-build commit observed a newer index write than the one its
    /// prepare phase snapshotted: committing would overwrite fresher index
    /// content with stale data (and bump the epoch so caches trust it).
    #[error("stale prepared build: prepared at index_epoch {prepared_epoch}, current index_epoch {current_epoch}")]
    StalePreparedBuild {
        prepared_epoch: u64,
        current_epoch: u64,
    },

    /// A structural index build is already running for this project. The
    /// request is safe to retry after the in-flight build completes.
    #[error("index build already in progress")]
    BuildBusy,

    /// Client-supplied arguments failed handler-level validation (missing
    /// required parameter, unknown action, malformed path, …). Mapped to
    /// JSON-RPC `-32602` invalid-params at the MCP exit; the payload is the
    /// full human-readable message.
    #[error("{0}")]
    InvalidParams(String),

    /// An explicit lookup (symbol / type / uid) matched nothing. Only used
    /// where the tool contract treats a miss as an error — most tools return
    /// an Ok envelope with an `error` field instead (see MCP_TOOLS.md). The
    /// payload is the full human-readable message.
    #[error("{0}")]
    NotFound(String),

    /// The runtime has no open index database (project not set, or the
    /// handle was closed by idle eviction and not yet reopened).
    #[error("no index database")]
    IndexUnavailable,

    #[error("{0}")]
    Other(String),
}

impl CcError {
    /// Whether the operation is safe to retry as-is once the transient
    /// condition (concurrent build / stale prepare) clears. Surfaced to MCP
    /// clients as `data.retryable` on the JSON-RPC error.
    pub fn is_retryable(&self) -> bool {
        matches!(self, CcError::BuildBusy | CcError::StalePreparedBuild { .. })
    }
}

pub type CcResult<T> = Result<T, CcError>;
