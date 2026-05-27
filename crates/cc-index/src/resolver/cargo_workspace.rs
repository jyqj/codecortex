//! Cargo workspace crate alias resolution.
//!
//! Parses `Cargo.toml` workspace members and builds a mapping from
//! crate alias (e.g. `cc_model`) to entry point path (e.g.
//! `crates/cc-model/src/lib.rs`).
//!
//! This enables cross-crate resolution: when a Rust file contains
//! `use cc_model::parse::ParseOutcome`, the resolver can map the
//! first segment `cc_model` to `crates/cc-model/src/lib.rs` and
//! resolve the import path accordingly.

use std::collections::HashMap;
use std::path::Path;

/// Build a mapping from Rust crate alias to the crate's entry-point file path
/// (relative to the project root).
///
/// For each workspace member, reads its `Cargo.toml` to get the package name,
/// converts `-` to `_` (the Rust crate naming convention), and maps to
/// `{member}/src/lib.rs` (or `src/main.rs` if `lib.rs` doesn't exist).
///
/// Uses simple line-by-line parsing to avoid adding a `toml` dependency.
///
/// # Example
///
/// ```text
/// resolve_cargo_workspace("my-project/") =>
///   { "cc_model" => "crates/cc-model/src/lib.rs",
///     "cc_db"    => "crates/cc-db/src/lib.rs", ... }
/// ```
pub fn resolve_cargo_workspace(project_path: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();

    let root_toml_path = project_path.join("Cargo.toml");
    let root_toml = match std::fs::read_to_string(&root_toml_path) {
        Ok(content) => content,
        Err(_) => return result,
    };

    let members = parse_workspace_members(&root_toml);
    if members.is_empty() {
        return result;
    }

    // Expand glob patterns and resolve each member
    for member_pattern in &members {
        let expanded = expand_member_pattern(project_path, member_pattern);
        for member_dir in expanded {
            let member_toml_path = project_path.join(&member_dir).join("Cargo.toml");
            let member_toml = match std::fs::read_to_string(&member_toml_path) {
                Ok(content) => content,
                Err(_) => continue,
            };

            let package_name = match parse_package_name(&member_toml) {
                Some(name) => name,
                None => continue,
            };

            // Rust crate alias: hyphens become underscores
            let crate_alias = package_name.replace('-', "_");

            // Determine entry point: prefer src/lib.rs, fall back to src/main.rs
            let lib_path = project_path.join(&member_dir).join("src/lib.rs");
            let main_path = project_path.join(&member_dir).join("src/main.rs");

            let entry_point = if lib_path.exists() {
                format!("{}/src/lib.rs", member_dir)
            } else if main_path.exists() {
                format!("{}/src/main.rs", member_dir)
            } else {
                continue;
            };

            result.insert(crate_alias, entry_point);
        }
    }

    result
}

/// Resolve a Rust `use` import string against the workspace alias map.
///
/// Given an import like `cc_model::parse::ParseOutcome`, checks if the first
/// path segment (`cc_model`) matches a workspace crate alias. If so, returns
/// the mapped entry-point file path (e.g. `crates/cc-model/src/lib.rs`).
///
/// Returns `None` if the import doesn't match any workspace crate.
pub fn resolve_rust_workspace_import(
    import_string: &str,
    workspace_map: &HashMap<String, String>,
) -> Option<String> {
    // Rust use paths use `::` as separator
    let first_segment = import_string.split("::").next()?;
    let trimmed = first_segment.trim();
    if trimmed.is_empty() {
        return None;
    }
    workspace_map.get(trimmed).cloned()
}

// ---------------------------------------------------------------------------
// Internal parsing helpers (no `toml` crate dependency)
// ---------------------------------------------------------------------------

/// Parse `[workspace] members = [...]` from a Cargo.toml string.
///
/// Handles multi-line arrays like:
/// ```toml
/// [workspace]
/// members = [
///     "crates/cc-model",
///     "crates/cc-db",
/// ]
/// ```
fn parse_workspace_members(content: &str) -> Vec<String> {
    let mut members = Vec::new();
    let mut in_workspace = false;
    let mut in_members_array = false;
    let mut collecting = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Track section headers
        if trimmed.starts_with('[') {
            if trimmed == "[workspace]" {
                in_workspace = true;
                in_members_array = false;
                collecting = false;
            } else {
                // Any other section header ends [workspace]
                if in_workspace {
                    in_workspace = false;
                    in_members_array = false;
                    collecting = false;
                }
            }
            continue;
        }

        if !in_workspace {
            continue;
        }

        // Look for `members = [...]`
        if !in_members_array {
            if let Some(rest) = trimmed.strip_prefix("members") {
                let rest = rest.trim();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim();
                    if let Some(rest) = rest.strip_prefix('[') {
                        // Inline or start of multi-line array
                        in_members_array = true;
                        collecting = true;
                        // Parse the rest of this line
                        extract_quoted_strings(rest, &mut members);
                        if rest.contains(']') {
                            in_members_array = false;
                            collecting = false;
                        }
                    }
                }
            }
            continue;
        }

        // Inside members array
        if collecting {
            if trimmed.contains(']') {
                // Extract anything before the closing bracket
                let before_bracket = trimmed.split(']').next().unwrap_or("");
                extract_quoted_strings(before_bracket, &mut members);
                in_members_array = false;
                collecting = false;
            } else {
                extract_quoted_strings(trimmed, &mut members);
            }
        }
    }

    members
}

/// Extract quoted strings from a line (handles both `"..."` values and trailing commas).
fn extract_quoted_strings(line: &str, out: &mut Vec<String>) {
    let mut chars = line.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch == '"' {
            chars.next(); // consume opening quote
            let mut value = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
            if !value.is_empty() {
                out.push(value);
            }
        } else {
            chars.next();
        }
    }
}

/// Parse `[package] name = "..."` from a member's Cargo.toml.
fn parse_package_name(content: &str) -> Option<String> {
    let mut in_package = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }

        if !in_package {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim();
            if let Some(rest) = rest.strip_prefix('=') {
                let rest = rest.trim();
                // Extract the quoted value
                if let Some(rest) = rest.strip_prefix('"') {
                    if let Some(end) = rest.find('"') {
                        return Some(rest[..end].to_string());
                    }
                }
            }
        }
    }

    None
}

/// Expand a member pattern that may contain globs (e.g. `crates/*`).
///
/// Returns a list of directory paths (relative to project root) matching
/// the pattern. Non-glob patterns are returned as-is if the directory exists.
fn expand_member_pattern(project_path: &Path, pattern: &str) -> Vec<String> {
    if !pattern.contains('*') {
        // No glob — return as-is if the directory exists
        let dir = project_path.join(pattern);
        if dir.is_dir() {
            return vec![pattern.to_string()];
        }
        return Vec::new();
    }

    // Simple glob expansion: only support trailing `/*`
    // e.g. "crates/*" → list all subdirectories of "crates/"
    if let Some(prefix) = pattern.strip_suffix("/*") {
        let base_dir = project_path.join(prefix);
        if !base_dir.is_dir() {
            return Vec::new();
        }
        let mut results = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check if this directory has a Cargo.toml
                    if path.join("Cargo.toml").exists() {
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            results.push(format!("{}/{}", prefix, name));
                        }
                    }
                }
            }
        }
        results.sort(); // deterministic order
        return results;
    }

    // Unsupported glob pattern — skip
    Vec::new()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_workspace_members_multiline() {
        let toml = r#"
[workspace]
resolver = "2"
members = [
    "crates/cc-model",
    "crates/cc-db",
    "crates/cc-parsers",
]

[workspace.package]
version = "0.1.0"
"#;
        let members = parse_workspace_members(toml);
        assert_eq!(
            members,
            vec!["crates/cc-model", "crates/cc-db", "crates/cc-parsers"]
        );
    }

    #[test]
    fn test_parse_workspace_members_inline() {
        let toml = r#"
[workspace]
members = ["foo", "bar"]
"#;
        let members = parse_workspace_members(toml);
        assert_eq!(members, vec!["foo", "bar"]);
    }

    #[test]
    fn test_parse_package_name() {
        let toml = r#"
[package]
name = "cc-model"
version.workspace = true
edition.workspace = true
"#;
        assert_eq!(parse_package_name(toml), Some("cc-model".to_string()));
    }

    #[test]
    fn test_parse_package_name_missing() {
        let toml = r#"
[dependencies]
serde = "1"
"#;
        assert_eq!(parse_package_name(toml), None);
    }

    #[test]
    fn test_resolve_rust_workspace_import_match() {
        let mut map = HashMap::new();
        map.insert(
            "cc_model".to_string(),
            "crates/cc-model/src/lib.rs".to_string(),
        );
        map.insert("cc_db".to_string(), "crates/cc-db/src/lib.rs".to_string());

        // Full path import
        assert_eq!(
            resolve_rust_workspace_import("cc_model::parse::ParseOutcome", &map),
            Some("crates/cc-model/src/lib.rs".to_string())
        );

        // Simple crate import
        assert_eq!(
            resolve_rust_workspace_import("cc_db::IndexDb", &map),
            Some("crates/cc-db/src/lib.rs".to_string())
        );
    }

    #[test]
    fn test_resolve_rust_workspace_import_no_match() {
        let mut map = HashMap::new();
        map.insert(
            "cc_model".to_string(),
            "crates/cc-model/src/lib.rs".to_string(),
        );

        // std library — not in workspace
        assert_eq!(
            resolve_rust_workspace_import("std::collections::HashMap", &map),
            None
        );

        // Unknown crate
        assert_eq!(
            resolve_rust_workspace_import("serde::Deserialize", &map),
            None
        );

        // Empty string
        assert_eq!(resolve_rust_workspace_import("", &map), None);
    }

    #[test]
    fn test_resolve_cargo_workspace_filesystem() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Create workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = [
    "crates/my-lib",
    "crates/my-app",
]
"#,
        )
        .unwrap();

        // Create member: my-lib with src/lib.rs
        fs::create_dir_all(root.join("crates/my-lib/src")).unwrap();
        fs::write(
            root.join("crates/my-lib/Cargo.toml"),
            r#"
[package]
name = "my-lib"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(root.join("crates/my-lib/src/lib.rs"), "").unwrap();

        // Create member: my-app with src/main.rs only
        fs::create_dir_all(root.join("crates/my-app/src")).unwrap();
        fs::write(
            root.join("crates/my-app/Cargo.toml"),
            r#"
[package]
name = "my-app"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(root.join("crates/my-app/src/main.rs"), "").unwrap();

        let result = resolve_cargo_workspace(root);
        assert_eq!(
            result.get("my_lib"),
            Some(&"crates/my-lib/src/lib.rs".to_string())
        );
        assert_eq!(
            result.get("my_app"),
            Some(&"crates/my-app/src/main.rs".to_string())
        );
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_resolve_cargo_workspace_glob() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Create workspace with glob pattern
        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/*"]
"#,
        )
        .unwrap();

        // Create two crates under crates/
        for name in &["alpha", "beta"] {
            let dir = root.join(format!("crates/{}", name));
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!(
                    r#"
[package]
name = "{}"
version = "0.1.0"
"#,
                    name
                ),
            )
            .unwrap();
            fs::write(dir.join("src/lib.rs"), "").unwrap();
        }

        let result = resolve_cargo_workspace(root);
        assert_eq!(
            result.get("alpha"),
            Some(&"crates/alpha/src/lib.rs".to_string())
        );
        assert_eq!(
            result.get("beta"),
            Some(&"crates/beta/src/lib.rs".to_string())
        );
    }

    #[test]
    fn test_no_workspace_returns_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // Non-workspace Cargo.toml
        fs::write(
            root.join("Cargo.toml"),
            r#"
[package]
name = "standalone"
version = "0.1.0"
"#,
        )
        .unwrap();

        let result = resolve_cargo_workspace(root);
        assert!(result.is_empty());
    }

    #[test]
    fn test_no_cargo_toml_returns_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = resolve_cargo_workspace(tmp.path());
        assert!(result.is_empty());
    }

    #[test]
    fn test_hyphen_to_underscore_alias() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        fs::write(
            root.join("Cargo.toml"),
            r#"
[workspace]
members = ["my-crate"]
"#,
        )
        .unwrap();

        fs::create_dir_all(root.join("my-crate/src")).unwrap();
        fs::write(
            root.join("my-crate/Cargo.toml"),
            r#"
[package]
name = "my-crate"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(root.join("my-crate/src/lib.rs"), "").unwrap();

        let result = resolve_cargo_workspace(root);
        assert!(result.contains_key("my_crate"));
        assert_eq!(
            result.get("my_crate"),
            Some(&"my-crate/src/lib.rs".to_string())
        );
    }
}
