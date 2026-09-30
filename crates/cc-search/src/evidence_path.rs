//! Exact spelling and in-root source resolution, shared by every disk reader.
use cc_model::{CcError, CcResult};
use std::path::{Path, PathBuf};
pub fn resolve_source_path(project_root: &Path, file_path: &str) -> CcResult<PathBuf> {
    let normalized = cc_model::repo_path::normalize_relative(file_path)?;
    if normalized.is_empty() {
        return Err(CcError::InvalidParams("file path must not be empty".into()));
    }
    let root = project_root
        .canonicalize()
        .map_err(|e| CcError::Other(format!("cannot canonicalize project root: {e}")))?;
    let mut joined = root.clone();
    for part in normalized.split('/') {
        let exact = std::fs::read_dir(&joined)
            .map_err(|_| CcError::InvalidParams("path does not exist".into()))?
            .filter_map(Result::ok)
            .any(|e| e.file_name() == std::ffi::OsStr::new(part));
        if !exact {
            return Err(CcError::InvalidParams(
                "path does not exist with exact indexed spelling".into(),
            ));
        }
        joined.push(part);
        let target = joined
            .canonicalize()
            .map_err(|_| CcError::InvalidParams("path does not exist".into()))?;
        if !target.starts_with(&root) {
            return Err(CcError::InvalidParams("path escapes project root".into()));
        }
    }
    let resolved = joined
        .canonicalize()
        .map_err(|_| CcError::InvalidParams("path does not exist".into()))?;
    if !resolved.starts_with(&root)
        || !resolved
            .metadata()
            .map_err(|_| CcError::InvalidParams("file metadata unavailable".into()))?
            .is_file()
    {
        return Err(CcError::InvalidParams(
            "indexed path is not a regular in-root file".into(),
        ));
    }
    Ok(resolved)
}
