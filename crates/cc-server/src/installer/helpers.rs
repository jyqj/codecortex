//! Shared JSON helpers for installer targets.

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
    let obj = root
        .as_object_mut()
        .ok_or_else(|| CcError::Config(format!("{}: root is not an object", path.display())))?;
    let Some(section_value) = obj.get_mut(section) else {
        return Ok(());
    };
    let section_obj = section_value.as_object_mut().ok_or_else(|| {
        CcError::Config(format!(
            "{}: section {:?} is not an object",
            path.display(),
            section
        ))
    })?;
    if section_obj.remove(key).is_none() {
        return Ok(());
    }
    write_json_root(path, &root)
}

fn validate_claude_hook_root(
    root: &serde_json::Value,
    settings_path: &Path,
    hook_type: &str,
) -> CcResult<()> {
    let invalid = |field: &str, shape: &str| {
        CcError::Config(format!(
            "{}: {field} is not {shape}",
            settings_path.display()
        ))
    };
    let object = root
        .as_object()
        .ok_or_else(|| invalid("root", "an object"))?;
    let Some(hooks) = object.get("hooks") else {
        return Ok(());
    };
    let hooks = hooks
        .as_object()
        .ok_or_else(|| invalid("hooks", "an object"))?;
    let Some(entries) = hooks.get(hook_type) else {
        return Ok(());
    };
    let field = format!("hooks.{hook_type}");
    let entries = entries
        .as_array()
        .ok_or_else(|| invalid(&field, "an array"))?;
    for (index, entry) in entries.iter().enumerate() {
        let field = format!("hooks.{hook_type}[{index}]");
        let entry = entry
            .as_object()
            .ok_or_else(|| invalid(&field, "an object"))?;
        if entry
            .get("matcher")
            .is_some_and(|matcher| !matcher.is_string())
        {
            return Err(invalid(&format!("{field}.matcher"), "a string"));
        }
        let commands = entry
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .ok_or_else(|| invalid(&format!("{field}.hooks"), "an array"))?;
        if commands.iter().any(|hook| !hook.is_object()) {
            return Err(invalid(&format!("{field}.hooks entry"), "an object"));
        }
    }
    Ok(())
}

/// Check settings before the target changes its MCP entry or gate script.
pub(crate) fn validate_claude_hook_configuration(
    settings_path: &Path,
    hook_type: &str,
) -> CcResult<()> {
    validate_claude_hook_root(&read_json_root(settings_path)?, settings_path, hook_type)
}

fn is_owned_claude_hook(hook: &serde_json::Value, command: &str) -> bool {
    hook.get("type").and_then(|value| value.as_str()) == Some("command")
        && hook.get("command").and_then(|value| value.as_str()) == Some(command)
}

/// Upsert the exact event/matcher/command emitted by our Claude Code target.
pub(crate) fn upsert_claude_hook(
    settings_path: &Path,
    hook_type: &str,
    matcher: &str,
    command: &str,
) -> CcResult<()> {
    let mut root = read_json_root(settings_path)?;
    validate_claude_hook_root(&root, settings_path, hook_type)?;

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
            // A user's command merely mentioning codecortex is not our hook.
            let already = arr.iter().any(|h| {
                h.get("matcher").and_then(|value| value.as_str()) == Some(matcher)
                    && h.get("hooks")
                        .and_then(|hs| hs.as_array())
                        .is_some_and(|hs| hs.iter().any(|hook| is_owned_claude_hook(hook, command)))
            });
            if already {
                return Ok(()); // Already installed
            }
            arr.push(new_entry);
        } else {
            return Err(CcError::Config(format!(
                "{}: hooks.{hook_type} is not an array",
                settings_path.display()
            )));
        }
    } else {
        hooks.insert(hook_type.into(), serde_json::json!([new_entry]));
    }

    write_json_root(settings_path, &root)
}

/// Remove only our command, retaining user commands that share its matcher group.
pub(crate) fn remove_claude_hook(
    settings_path: &Path,
    hook_type: &str,
    matcher: &str,
    command: &str,
) -> CcResult<()> {
    let mut root = read_json_root(settings_path)?;
    validate_claude_hook_root(&root, settings_path, hook_type)?;
    let Some(entries) = root
        .get_mut("hooks")
        .and_then(|hooks| hooks.get_mut(hook_type))
        .and_then(|entries| entries.as_array_mut())
    else {
        return Ok(());
    };
    let mut changed = false;
    entries.retain_mut(|entry| {
        if entry.get("matcher").and_then(|value| value.as_str()) != Some(matcher) {
            return true;
        }
        let Some(hooks) = entry
            .get_mut("hooks")
            .and_then(|hooks| hooks.as_array_mut())
        else {
            return true;
        };
        let before = hooks.len();
        hooks.retain(|hook| !is_owned_claude_hook(hook, command));
        let removed = hooks.len() != before;
        changed |= removed;
        !removed || !hooks.is_empty()
    });
    if changed {
        write_json_root(settings_path, &root)?;
    }
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
    fn remove_rejects_non_object_root_and_section_without_rewriting() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        for original in [
            " [ ]\n",
            " null\n",
            r#"{"mcpServers" : [ ]}"#,
            r#"{"mcpServers":null}"#,
        ] {
            std::fs::write(&path, original).unwrap();
            let error = remove_json_key(&path, "mcpServers", "codecortex").unwrap_err();
            assert!(matches!(error, CcError::Config(_)));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }

    #[test]
    fn remove_absent_entry_preserves_exact_config_bytes() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        for original in [
            "  \n",
            " { }\n",
            r#"{"mcpServers" : {"other":{"command":"keep"}}}"#,
        ] {
            std::fs::write(&path, original).unwrap();
            remove_json_key(&path, "mcpServers", "codecortex").unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        }
    }
}
