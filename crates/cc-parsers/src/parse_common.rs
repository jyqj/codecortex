//! Shared tree-sitter parsing helpers used across language parsers.

use cc_model::{CcError, CcResult};

/// Build a tree-sitter parser for `language`, apply an optional timeout, and
/// parse `content` into a syntax tree.
pub(crate) fn parse_tree(
    language: &tree_sitter::Language,
    content: &str,
    file_path: &str,
    timeout_micros: Option<u64>,
) -> CcResult<tree_sitter::Tree> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).map_err(|e| CcError::Parse {
        file: file_path.to_string(),
        message: e.to_string(),
    })?;
    if let Some(timeout) = timeout_micros {
        parser.set_timeout_micros(timeout);
    }

    parser.parse(content, None).ok_or_else(|| CcError::Parse {
        file: file_path.to_string(),
        message: if timeout_micros.is_some() {
            "tree-sitter parse failed or timed out".to_string()
        } else {
            "tree-sitter parse failed".to_string()
        },
    })
}
