//! Shared JSON/TOML helpers for installer targets.

use std::path::Path;

use cc_model::{CcError, CcResult};

/// Read and parse a JSON config file. Missing or empty files yield an empty
/// object so installs can bootstrap fresh configs; malformed content is an
/// error (never silently overwrite a user's config).
pub(crate) fn read_json_root(path: &Path) -> CcResult<serde_json::Value> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = std::fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content)
        .map_err(|e| CcError::Config(format!("failed to parse {}: {}", path.display(), e)))
}

/// Pretty-print `root` to `path`, creating parent directories as needed.
fn write_json_root(path: &Path, root: &serde_json::Value) -> CcResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(root)?)?;
    Ok(())
}

/// Upsert a key under a top-level object in a JSON file.
pub(crate) fn upsert_json_key(
    path: &Path,
    section: &str,
    key: &str,
    value: &serde_json::Value,
) -> CcResult<()> {
    let mut root = read_json_root(path)?;

    let obj = root
        .as_object_mut()
        .ok_or_else(|| CcError::Config(format!("{}: root is not an object", path.display())))?;
    let section_obj = obj
        .entry(section)
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            CcError::Config(format!(
                "{}: section {:?} is not an object",
                path.display(),
                section
            ))
        })?;
    section_obj.insert(key.into(), value.clone());

    write_json_root(path, &root)
}

/// Remove a key under a top-level object in a JSON file. Missing files/keys are OK.
pub(crate) fn remove_json_key(path: &Path, section: &str, key: &str) -> CcResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = read_json_root(path)?;
    if let Some(section_obj) = root
        .as_object_mut()
        .and_then(|o| o.get_mut(section))
        .and_then(|v| v.as_object_mut())
    {
        section_obj.remove(key);
    }
    write_json_root(path, &root)
}

/// Upsert a Claude Code PreToolUse/PostToolUse hook in settings.json.
pub(crate) fn upsert_claude_hook(
    settings_path: &Path,
    hook_type: &str,
    matcher: &str,
    command: &str,
) -> CcResult<()> {
    let mut root = read_json_root(settings_path)?;

    let obj = root.as_object_mut().ok_or_else(|| {
        CcError::Config(format!(
            "{}: root is not an object",
            settings_path.display()
        ))
    })?;
    let hooks = obj
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            CcError::Config(format!(
                "{}: hooks is not an object",
                settings_path.display()
            ))
        })?;

    let new_entry = serde_json::json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": command
        }]
    });

    // Append to existing array or create new one, preserving user hooks
    if let Some(existing) = hooks.get_mut(hook_type) {
        if let Some(arr) = existing.as_array_mut() {
            // Check if our hook already exists
            let already = arr.iter().any(|h| {
                h.get("hooks")
                    .and_then(|hs| hs.as_array())
                    .map(|hs| {
                        hs.iter().any(|hh| {
                            hh.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains("codecortex"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            });
            if already {
                return Ok(()); // Already installed
            }
            arr.push(new_entry);
        } else {
            // Existing value is not an array — replace (shouldn't happen normally)
            hooks.insert(hook_type.into(), serde_json::json!([new_entry]));
        }
    } else {
        hooks.insert(hook_type.into(), serde_json::json!([new_entry]));
    }

    write_json_root(settings_path, &root)
}

/// Append content to a file if a marker string is not already present.
pub(crate) fn append_if_missing(path: &Path, marker: &str, content: &str) -> CcResult<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.contains(marker) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{}{}", existing, content))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_bootstraps_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("nested/config.json");
        upsert_json_key(
            &path,
            "mcpServers",
            "codecortex",
            &serde_json::json!({"a": 1}),
        )
        .unwrap();
        let root = read_json_root(&path).unwrap();
        assert_eq!(root["mcpServers"]["codecortex"]["a"], 1);
    }

    #[test]
    fn upsert_treats_empty_file_as_fresh_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "  \n").unwrap();
        upsert_json_key(&path, "mcpServers", "codecortex", &serde_json::json!({})).unwrap();
        assert!(read_json_root(&path).unwrap()["mcpServers"]
            .get("codecortex")
            .is_some());
    }

    #[test]
    fn upsert_rejects_malformed_json_without_clobbering() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err =
            upsert_json_key(&path, "mcpServers", "codecortex", &serde_json::json!({})).unwrap_err();
        assert!(matches!(err, CcError::Config(_)));
        assert!(err.to_string().contains("config.json"));
        // Original content must survive the failed upsert.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn upsert_rejects_non_object_root_and_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        std::fs::write(&path, "[]").unwrap();
        let err =
            upsert_json_key(&path, "mcpServers", "codecortex", &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("root is not an object"));

        std::fs::write(&path, r#"{"mcpServers": []}"#).unwrap();
        let err =
            upsert_json_key(&path, "mcpServers", "codecortex", &serde_json::json!({})).unwrap_err();
        assert!(err.to_string().contains("is not an object"));
        assert!(matches!(err, CcError::Config(_)));
    }

    #[test]
    fn upsert_maps_io_failure_to_io_error() {
        let dir = tempfile::TempDir::new().unwrap();
        // The config path itself is a directory: read_to_string fails with an IO error.
        let err = upsert_json_key(
            dir.path(),
            "mcpServers",
            "codecortex",
            &serde_json::json!({}),
        )
        .unwrap_err();
        assert!(matches!(err, CcError::Io(_)));
    }

    #[test]
    fn remove_missing_file_is_noop() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("absent.json");
        remove_json_key(&path, "mcpServers", "codecortex").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn remove_rejects_malformed_json_without_clobbering() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = remove_json_key(&path, "mcpServers", "codecortex").unwrap_err();
        assert!(matches!(err, CcError::Config(_)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn append_if_missing_is_idempotent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        append_if_missing(&path, "codecortex", "[codecortex]\n").unwrap();
        append_if_missing(&path, "codecortex", "[codecortex]\n").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("codecortex").count(), 1);
    }
}
