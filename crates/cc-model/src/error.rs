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

    #[error("{0}")]
    Other(String),
}

pub type CcResult<T> = Result<T, CcError>;
