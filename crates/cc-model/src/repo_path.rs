//! Case-sensitive repository-relative paths shared by retrieval and source guards.
//! This is lexical normalization, not filesystem canonicalization or authorization.
use crate::{CcError, CcResult};

pub fn normalize_relative(path: &str) -> CcResult<String> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return Err(CcError::InvalidParams(
            "absolute or drive-qualified path rejected".into(),
        ));
    }
    if path.chars().any(char::is_control) {
        return Err(CcError::InvalidParams(
            "control character in path rejected".into(),
        ));
    }
    let mut parts = Vec::new();
    for part in path.split(['/', '\\']) {
        match part {
            ".." => return Err(CcError::InvalidParams("path traversal rejected".into())),
            "" | "." => {}
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// A path prefix denotes an exact path or a descendant, never a similarly named sibling.
/// Inputs are repository spelling, compared case-sensitively even on insensitive hosts.
pub fn is_within(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('/'))
}

/// Index keys use an unambiguous portable spelling. Native Unix filenames containing
/// backslashes/drive separators or non-UTF8 cannot be represented by this protocol.
pub fn from_native_relative(path: &std::path::Path) -> CcResult<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => {
                let text = name.to_str().ok_or_else(|| {
                    CcError::InvalidParams("non-UTF8 path is not representable".into())
                })?;
                if text.contains(['\\', ':']) || text.chars().any(char::is_control) {
                    return Err(CcError::InvalidParams("ambiguous path spelling".into()));
                }
                parts.push(text);
            }
            std::path::Component::CurDir => {}
            _ => return Err(CcError::InvalidParams("non-relative path rejected".into())),
        }
    }
    Ok(parts.join("/"))
}

pub fn is_canonical_file(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_normalization_rejects_traversal_and_roots() {
        assert_eq!(
            normalize_relative("./src\\api//file.rs").unwrap(),
            "src/api/file.rs"
        );
        for path in [
            "../x",
            "a/../b",
            "a\\..\\b",
            "/x",
            "C:x",
            "C:\\x",
            "\\\\host\\x",
            "a\0b",
        ] {
            assert!(normalize_relative(path).is_err(), "{path:?}");
        }
        assert_eq!(normalize_relative(".").unwrap(), "");
    }
    #[test]
    #[cfg(unix)]
    fn native_names_do_not_alias_portable_paths() {
        use std::os::unix::ffi::OsStrExt;
        for bytes in [b"a\\b.rs".as_slice(), b"C:foo.rs", b"bad\xff.rs"] {
            let native = std::path::Path::new(std::ffi::OsStr::from_bytes(bytes));
            assert!(from_native_relative(native).is_err());
        }
        assert_eq!(
            from_native_relative(std::path::Path::new("src/测%_试.rs")).unwrap(),
            "src/测%_试.rs"
        );
        assert!(!is_canonical_file("src/../secret.rs"));
        assert!(!is_canonical_file("/secret.rs"));
    }

    #[test]
    fn component_and_case_boundaries() {
        assert!(is_within("src/api/a.rs", "src/api"));
        assert!(is_within("src/api", "src/api/"));
        assert!(!is_within("src/apix/a.rs", "src/api"));
        assert!(!is_within("SRC/api/a.rs", "src/api"));
        assert!(is_within("src/测%_试/a.rs", "src/测%_试"));
    }
}
