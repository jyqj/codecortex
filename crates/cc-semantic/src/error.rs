//! Skeleton error surface for the semantic seam (P6-002).
//!
//! Workspace convention: errors surface as `cc_model::CcError` (see
//! `crates/cc-db/src/unit_of_work.rs`). This module only declares the
//! semantic-local causes and their mapping into that unified type; cache
//! corruption handling (quarantine, P6-018) adds its own variants later and
//! must not be invented here.

use cc_model::CcError;

/// Semantic-seam-local error causes (skeleton: one variant, mapping only).
#[derive(Debug, thiserror::Error)]
pub enum SemanticError {
    #[error("invalid semantic input: {0}")]
    InvalidInput(String),
}

impl From<SemanticError> for CcError {
    fn from(err: SemanticError) -> Self {
        match err {
            SemanticError::InvalidInput(message) => CcError::InvalidParams(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_input_maps_to_invalid_params() {
        let err: CcError = SemanticError::InvalidInput("NaN vector".into()).into();
        match err {
            CcError::InvalidParams(message) => assert_eq!(message, "NaN vector"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
