//! Shared bounded source read for compact language facts, separate from pure resolution.
use cc_model::{CcError, CcResult};
use std::path::Path;
pub(super) fn content(
    root: &Path,
    file: &str,
    pending: Option<&str>,
    expected: Option<&str>,
) -> CcResult<(String, bool)> {
    let read = pending.is_none();
    let content = if let Some(c) = pending {
        if c.len() > 4 * 1024 * 1024 {
            return Err(CcError::Config("module source byte budget".into()));
        }
        c.to_owned()
    } else {
        let bytes = cc_model::input_file::read(root, file, 4 * 1024 * 1024)
            .map_err(|e| CcError::Config(format!("module source read rejected: {e:?}")))?
            .ok_or_else(|| CcError::Config("module source disappeared".into()))?;
        String::from_utf8(bytes).map_err(|_| CcError::Config("module source not UTF-8".into()))?
    };
    if content.len() > 4 * 1024 * 1024 {
        return Err(CcError::Config("module source byte budget".into()));
    }
    let digest = blake3::hash(content.as_bytes()).to_hex().to_string();
    if expected.is_some_and(|e| e != digest) {
        return Err(CcError::Config(format!(
            "source changed during model capture: {file}"
        )));
    }
    Ok((content, read))
}
