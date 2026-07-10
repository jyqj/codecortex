use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use std::path::{Path, PathBuf};

/// Resolve a file path relative to project root, with safety checks.
///
/// Rejections are client-input problems ([`CcError::InvalidParams`]), so the
/// MCP exit maps them to JSON-RPC `-32602`.
pub fn resolve_indexed_path(project_root: &Path, file_path: &str) -> CcResult<PathBuf> {
    if file_path.starts_with('/') || file_path.starts_with('\\') {
        return Err(CcError::InvalidParams(format!(
            "absolute path rejected: {}",
            file_path
        )));
    }

    if file_path.split(['/', '\\']).any(|c| c == "..") {
        return Err(CcError::InvalidParams(format!(
            "path traversal rejected: {}",
            file_path
        )));
    }

    let canon_root = project_root
        .canonicalize()
        .map_err(|e| CcError::Other(format!("cannot canonicalize project root: {}", e)))?;

    let joined = project_root.join(file_path);
    let resolved = joined.canonicalize().map_err(|e| {
        CcError::InvalidParams(format!("path does not exist: {} ({})", file_path, e))
    })?;

    if !resolved.starts_with(&canon_root) {
        return Err(CcError::InvalidParams(format!(
            "path escapes project root: {}",
            file_path
        )));
    }

    Ok(resolved)
}

/// Like [`resolve_indexed_path`] but also verifies the file is present in the index.
pub fn resolve_indexed_path_strict(
    project_root: &Path,
    file_path: &str,
    db: &IndexDb,
) -> CcResult<PathBuf> {
    let resolved = resolve_indexed_path(project_root, file_path)?;
    if !db
        .reads()
        .file_is_indexed(file_path)
        .map_err(|e| CcError::Database(format!("index check failed: {}", e)))?
    {
        return Err(CcError::InvalidParams(format!(
            "file not indexed: {}",
            file_path
        )));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_tmp() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        dir
    }

    #[test]
    fn valid_relative_path() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "src/main.rs");
        assert!(result.is_ok());
        assert!(result.unwrap().ends_with("src/main.rs"));
    }

    #[test]
    fn reject_traversal() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "../../etc/passwd");
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("traversal"));
    }

    #[test]
    fn reject_absolute_path() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "/etc/passwd");
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("absolute"));
    }

    #[test]
    fn reject_nonexistent() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "nonexistent.rs");
        let err = result.unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)));
        assert!(err.to_string().contains("does not exist"));
    }
}
