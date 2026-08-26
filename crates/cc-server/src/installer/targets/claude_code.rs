//! Claude Code installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct ClaudeCodeTarget;

impl InstallerTarget for ClaudeCodeTarget {
    fn name(&self) -> &str {
        "Claude Code"
    }

    fn id(&self) -> &str {
        "claude_code"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".claude").exists()
    }

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let claude_dir = home.join(".claude");

        // 1. MCP configuration
        let mcp_config_path = claude_dir.join(".mcp.json");
        helpers::upsert_json_key(
            &mcp_config_path,
            "mcpServers",
            "codecortex",
            &serde_json::json!({
                "command": binary_path.to_string_lossy(),
                "args": ["mcp"]
            }),
        )?;

        // 2. Install PreToolUse hook gate script
        let hooks_dir = claude_dir.join("hooks");
        std::fs::create_dir_all(&hooks_dir)?;
        let gate_script_path = hooks_dir.join("codecortex-discovery-gate");
        let gate_script = r#"#!/bin/bash
# Gate hook: nudges Claude toward CodeCortex MCP for code discovery.
# First Grep/Glob/Read/Search per session -> block. Subsequent -> allow.
GATE=/tmp/codecortex-gate-$PPID
find /tmp -name 'codecortex-gate-*' -mtime +1 -delete 2>/dev/null
if [ -f "$GATE" ]; then
    exit 0
fi
touch "$GATE"
echo 'BLOCKED: For code discovery, prefer CodeCortex MCP tools first: search(query) to locate code, context(task) to build full task context, relations(symbol) for callers/callees, trace(from,to) for call paths, explore(symbols) for batch inspection. If the project is not indexed yet, call index(path) first. Fall back to Grep/Glob/Read only for non-structural searches. If you need Grep, retry.' >&2
exit 2
"#;
        std::fs::write(&gate_script_path, gate_script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gate_script_path, std::fs::Permissions::from_mode(0o755))?;
        }

        // 3. Upsert PreToolUse hook in settings.json
        let settings_path = claude_dir.join("settings.json");
        helpers::upsert_claude_hook(
            &settings_path,
            "PreToolUse",
            "Grep|Glob|Search",
            &gate_script_path.to_string_lossy(),
        )?;

        Ok(vec!["PreToolUse (Grep|Glob|Search)".into()])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        let claude_dir = home.join(".claude");

        // 1. Remove MCP server entry
        let mcp_config_path = claude_dir.join(".mcp.json");
        helpers::remove_json_key(&mcp_config_path, "mcpServers", "codecortex")?;

        // 2. Remove PreToolUse hook entries containing codecortex from settings.json
        let settings_path = claude_dir.join("settings.json");
        if settings_path.exists() {
            let mut root = helpers::read_json_root(&settings_path)?;
            if let Some(hooks) = root
                .as_object_mut()
                .and_then(|o| o.get_mut("hooks"))
                .and_then(|h| h.as_object_mut())
            {
                for (_hook_type, entries) in hooks.iter_mut() {
                    if let Some(arr) = entries.as_array_mut() {
                        arr.retain(|entry| {
                            let is_ours = entry
                                .get("hooks")
                                .and_then(|hs| hs.as_array())
                                .map(|hs| {
                                    hs.iter().any(|hh| {
                                        hh.get("command")
                                            .and_then(|c| c.as_str())
                                            .map(|c| c.contains("codecortex"))
                                            .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false);
                            !is_ours
                        });
                    }
                }
            }
            std::fs::write(&settings_path, serde_json::to_string_pretty(&root)?)?;
        }

        // 3. Remove gate script
        let gate_script_path = claude_dir.join("hooks/codecortex-discovery-gate");
        if gate_script_path.exists() {
            std::fs::remove_file(&gate_script_path)?;
        }

        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".claude/.mcp.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_requires_claude_dir() {
        let home = ts::fake_home();
        assert!(!ClaudeCodeTarget.detect(home.path()));
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        assert!(ClaudeCodeTarget.detect(home.path()));
    }

    #[test]
    fn mcp_json_lifecycle() {
        ts::assert_json_config_lifecycle(&ClaudeCodeTarget, "mcpServers");
    }

    #[test]
    fn install_writes_mcp_entry_gate_script_and_hook() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let hooks = ClaudeCodeTarget
            .install(home.path(), &binary, false)
            .unwrap();
        assert_eq!(hooks, vec!["PreToolUse (Grep|Glob|Search)".to_string()]);

        let mcp = ts::read_json(&home.path().join(".claude/.mcp.json"));
        assert_eq!(
            mcp["mcpServers"]["codecortex"]["command"],
            binary.to_string_lossy().as_ref()
        );
        assert_eq!(mcp["mcpServers"]["codecortex"]["args"][0], "mcp");

        let gate = home.path().join(".claude/hooks/codecortex-discovery-gate");
        assert!(gate.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&gate).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "gate script must be executable");
        }

        let settings = ts::read_json(&home.path().join(".claude/settings.json"));
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["matcher"], "Grep|Glob|Search");
        let command = entries[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(command.contains("codecortex-discovery-gate"));
    }

    #[test]
    fn install_preserves_user_hooks_and_is_idempotent() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let settings_path = home.path().join(".claude/settings.json");
        let user_hook = serde_json::json!({
            "matcher": "Bash",
            "hooks": [{ "type": "command", "command": "/usr/local/bin/my-linter" }]
        });
        ts::write_json(
            &settings_path,
            &serde_json::json!({ "hooks": { "PreToolUse": [user_hook] } }),
        );

        ClaudeCodeTarget
            .install(home.path(), &binary, false)
            .unwrap();
        ClaudeCodeTarget
            .install(home.path(), &binary, false)
            .unwrap();

        let settings = ts::read_json(&settings_path);
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "user hook + exactly one codecortex hook");
        assert_eq!(
            entries[0]["hooks"][0]["command"],
            "/usr/local/bin/my-linter"
        );
    }

    #[test]
    fn uninstall_removes_hook_and_gate_but_keeps_user_hooks() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let settings_path = home.path().join(".claude/settings.json");
        ts::write_json(
            &settings_path,
            &serde_json::json!({ "hooks": { "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": "/usr/local/bin/my-linter" }]
            }] } }),
        );
        ClaudeCodeTarget
            .install(home.path(), &binary, false)
            .unwrap();

        ClaudeCodeTarget.uninstall(home.path()).unwrap();

        let mcp = ts::read_json(&home.path().join(".claude/.mcp.json"));
        assert!(mcp["mcpServers"].get("codecortex").is_none());
        assert!(!home
            .path()
            .join(".claude/hooks/codecortex-discovery-gate")
            .exists());
        let settings = ts::read_json(&settings_path);
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["hooks"][0]["command"],
            "/usr/local/bin/my-linter"
        );
    }

    #[test]
    fn uninstall_without_any_config_is_noop() {
        let home = ts::fake_home();
        ClaudeCodeTarget.uninstall(home.path()).unwrap();
    }
}
