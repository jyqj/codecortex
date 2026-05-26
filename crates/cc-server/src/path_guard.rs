use cc_db::index_db::IndexDb;
use std::path::{Path, PathBuf};

/// Resolve a file path relative to project root, with safety checks.
pub fn resolve_indexed_path(project_root: &Path, file_path: &str) -> Result<PathBuf, String> {
    if file_path.starts_with('/') || file_path.starts_with('\\') {
        return Err(format!("absolute path rejected: {}", file_path));
    }

    if file_path.split(['/', '\\']).any(|c| c == "..") {
        return Err(format!("path traversal rejected: {}", file_path));
    }

    let canon_root = project_root
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize project root: {}", e))?;

    let joined = project_root.join(file_path);
    let resolved = joined
        .canonicalize()
        .map_err(|e| format!("path does not exist: {} ({})", file_path, e))?;

    if !resolved.starts_with(&canon_root) {
        return Err(format!("path escapes project root: {}", file_path));
    }

    Ok(resolved)
}

/// Like [`resolve_indexed_path`] but also verifies the file is present in the index.
pub fn resolve_indexed_path_strict(
    project_root: &Path,
    file_path: &str,
    db: &IndexDb,
) -> Result<PathBuf, String> {
    let resolved = resolve_indexed_path(project_root, file_path)?;
    if !db
        .file_is_indexed(file_path)
        .map_err(|e| format!("index check failed: {}", e))?
    {
        return Err(format!("file not indexed: {}", file_path));
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
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("traversal"));
    }

    #[test]
    fn reject_absolute_path() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "/etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("absolute"));
    }

    #[test]
    fn reject_nonexistent() {
        let tmp = setup_tmp();
        let result = resolve_indexed_path(tmp.path(), "nonexistent.rs");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }
}
