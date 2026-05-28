//! Shared JSON/TOML helpers for installer targets.

use std::path::Path;

/// Upsert a key under a top-level object in a JSON file.
pub(crate) fn upsert_json_key(
    path: &Path,
    section: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let mut root: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let obj = root.as_object_mut().ok_or("root is not an object")?;
    let section_obj = obj
        .entry(section)
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .ok_or("section is not an object")?;
    section_obj.insert(key.into(), value.clone());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Remove a key under a top-level object in a JSON file. Missing files/keys are OK.
pub(crate) fn remove_json_key(path: &Path, section: &str, key: &str) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut root: serde_json::Value =
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
    if let Some(obj) = root.as_object_mut() {
        if let Some(section_obj) = obj.get_mut(section).and_then(|v| v.as_object_mut()) {
            section_obj.remove(key);
        }
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Upsert a Claude Code PreToolUse/PostToolUse hook in settings.json.
pub(crate) fn upsert_claude_hook(
    settings_path: &Path,
    hook_type: &str,
    matcher: &str,
    command: &str,
) -> Result<(), String> {
    let mut root: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let obj = root.as_object_mut().ok_or("root is not an object")?;
    let hooks = obj
        .entry("hooks")
        .or_insert(serde_json::json!({}))
        .as_object_mut()
        .ok_or("hooks is not an object")?;

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

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        settings_path,
        serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Append content to a file if a marker string is not already present.
pub(crate) fn append_if_missing(path: &Path, marker: &str, content: &str) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    if existing.contains(marker) {
        return Ok(());
    }
    std::fs::write(path, format!("{}{}", existing, content)).map_err(|e| e.to_string())
}
