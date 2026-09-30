use cc_model::{CcError, CcResult};
use std::path::{Path, PathBuf};

/// Resolve a file path relative to project root, with safety checks.
///
/// Rejections are client-input problems ([`CcError::InvalidParams`]), so the
/// MCP exit maps them to JSON-RPC `-32602`.
#[cfg(test)]
pub fn resolve_indexed_path(project_root: &Path, file_path: &str) -> CcResult<PathBuf> {
    cc_search::evidence::resolve_source_path(project_root, file_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_directory_as_indexed_file() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        assert!(resolve_indexed_path(root.path(), "folder").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_unix_socket_before_any_reader_opens_it() {
        let root = tempfile::tempdir().unwrap();
        let _socket =
            std::os::unix::net::UnixListener::bind(root.path().join("socket.rs")).unwrap();
        assert!(resolve_indexed_path(root.path(), "socket.rs").is_err());
    }

    fn setup_tmp() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.rs"), "fn main() {}").unwrap();
        dir
    }

    #[test]
    fn separator_normalization_and_exact_spelling() {
        let tmp = setup_tmp();
        assert!(resolve_indexed_path(tmp.path(), "src\\main.rs").is_ok());
        assert!(resolve_indexed_path(tmp.path(), "./src//main.rs").is_ok());
        assert!(resolve_indexed_path(tmp.path(), "SRC/main.rs").is_err());
        assert!(resolve_indexed_path(tmp.path(), "C:main.rs").is_err());
        assert!(resolve_indexed_path(tmp.path(), "").is_err());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_targets_are_checked_before_following_descendants() {
        let tmp = setup_tmp();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.rs"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), tmp.path().join("external")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("src/main.rs"), tmp.path().join("inside.rs"))
            .unwrap();
        assert!(resolve_indexed_path(tmp.path(), "external/secret.rs").is_err());
        assert!(resolve_indexed_path(tmp.path(), "inside.rs").is_ok());
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
