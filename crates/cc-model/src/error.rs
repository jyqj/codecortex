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

    #[error("{0}")]
    Other(String),
}

pub type CcResult<T> = Result<T, CcError>;
