//! Claude Code installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct ClaudeCodeTarget;

const HOOK_TYPE: &str = "PreToolUse";
const HOOK_MATCHER: &str = "Grep|Glob|Search";

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
        let settings_path = claude_dir.join("settings.json");
        helpers::validate_claude_hook_configuration(&settings_path, HOOK_TYPE)?;

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
        helpers::upsert_claude_hook(
            &settings_path,
            HOOK_TYPE,
            HOOK_MATCHER,
            &gate_script_path.to_string_lossy(),
        )?;

        Ok(vec!["PreToolUse (Grep|Glob|Search)".into()])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        let claude_dir = home.join(".claude");
        let settings_path = claude_dir.join("settings.json");
        helpers::validate_claude_hook_configuration(&settings_path, HOOK_TYPE)?;

        // 1. Remove MCP server entry
        let mcp_config_path = claude_dir.join(".mcp.json");
        helpers::remove_json_key(&mcp_config_path, "mcpServers", "codecortex")?;

        // 2. Remove only the exact installed command, preserving sibling hooks.
        let gate_script_path = claude_dir.join("hooks/codecortex-discovery-gate");
        helpers::remove_claude_hook(
            &settings_path,
            HOOK_TYPE,
            HOOK_MATCHER,
            &gate_script_path.to_string_lossy(),
        )?;

        // 3. Remove gate script
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

    #[test]
    fn install_does_not_confuse_user_commands_with_the_owned_hook() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let settings_path = home.path().join(".claude/settings.json");
        let gate = home.path().join(".claude/hooks/codecortex-discovery-gate");
        let user_entries = serde_json::json!([
            {
                "matcher": "Grep|Glob|Search",
                "hooks": [{ "type": "command", "command": "/usr/local/bin/codecortex-audit" }]
            },
            {
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": gate.to_str().unwrap() }]
            }
        ]);
        ts::write_json(
            &settings_path,
            &serde_json::json!({ "hooks": { "PreToolUse": user_entries } }),
        );

        for _ in 0..2 {
            ClaudeCodeTarget
                .install(home.path(), &binary, false)
                .unwrap();
        }

        let settings = ts::read_json(&settings_path);
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 3, "two user entries and one installed hook");
        assert_eq!(&entries[..2], user_entries.as_array().unwrap());
        assert_eq!(entries[2]["matcher"], "Grep|Glob|Search");
        assert_eq!(entries[2]["hooks"][0]["command"], gate.to_str().unwrap());
    }

    #[test]
    fn uninstall_keeps_other_commands_in_the_owned_matcher_group() {
        let home = ts::fake_home();
        let settings_path = home.path().join(".claude/settings.json");
        let gate = home.path().join(".claude/hooks/codecortex-discovery-gate");
        let user_command = serde_json::json!({
            "type": "command", "command": "/usr/local/bin/codecortex-audit", "timeout": 9
        });
        let different_hook_type = serde_json::json!({
            "type": "prompt", "command": gate.to_str().unwrap(), "prompt": "user-owned"
        });
        ts::write_json(
            &settings_path,
            &serde_json::json!({ "hooks": { "PreToolUse": [{
                "matcher": "Grep|Glob|Search",
                "user_annotation": "preserve this group",
                "hooks": [
                    { "type": "command", "command": gate.to_str().unwrap() },
                    user_command,
                    different_hook_type
                ]
            }] } }),
        );

        ClaudeCodeTarget.uninstall(home.path()).unwrap();

        let settings = ts::read_json(&settings_path);
        let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["matcher"], "Grep|Glob|Search");
        assert_eq!(entries[0]["user_annotation"], "preserve this group");
        assert_eq!(
            entries[0]["hooks"],
            serde_json::json!([user_command, different_hook_type])
        );
    }

    #[test]
    fn uninstall_only_matches_the_installed_event_matcher_and_command() {
        let home = ts::fake_home();
        let settings_path = home.path().join(".claude/settings.json");
        let gate = home.path().join(".claude/hooks/codecortex-discovery-gate");
        let original = serde_json::json!({ "hooks": {
            "PreToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": gate.to_str().unwrap() }] },
                { "matcher": "Grep|Glob|Search", "hooks": [{ "type": "command", "command": format!("{} --user-mode", gate.display()) }] }
            ],
            "PostToolUse": [{ "matcher": "Grep|Glob|Search", "hooks": [{ "type": "command", "command": gate.to_str().unwrap() }] }]
        }});
        ts::write_json(&settings_path, &original);
        let bytes_before = std::fs::read(&settings_path).unwrap();

        ClaudeCodeTarget.uninstall(home.path()).unwrap();

        assert_eq!(ts::read_json(&settings_path), original);
        assert_eq!(std::fs::read(&settings_path).unwrap(), bytes_before);
    }

    #[test]
    fn invalid_hook_configuration_is_rejected_before_other_installation_files_change() {
        for original in [
            r#"[]"#,
            r#"{"hooks": []}"#,
            r#"{"hooks": {"PreToolUse": {"user": "keep"}}}"#,
        ] {
            for install in [true, false] {
                let home = ts::fake_home();
                let binary = ts::temp_binary(home.path());
                let settings = home.path().join(".claude/settings.json");
                let mcp = home.path().join(".claude/.mcp.json");
                let gate = home.path().join(".claude/hooks/codecortex-discovery-gate");
                std::fs::create_dir_all(gate.parent().unwrap()).unwrap();
                std::fs::write(&settings, original).unwrap();
                let mcp_before = br#"{"mcpServers":{"codecortex":{"command":"old"},"other":{"command":"keep"}}}"#;
                std::fs::write(&mcp, mcp_before).unwrap();
                std::fs::write(&gate, "existing gate bytes\n").unwrap();

                let result = if install {
                    ClaudeCodeTarget
                        .install(home.path(), &binary, false)
                        .map(|_| ())
                } else {
                    ClaudeCodeTarget.uninstall(home.path())
                };

                assert!(result.is_err(), "invalid settings accepted: {original}");
                assert_eq!(std::fs::read(&settings).unwrap(), original.as_bytes());
                assert_eq!(std::fs::read(&mcp).unwrap(), mcp_before);
                assert_eq!(
                    std::fs::read_to_string(&gate).unwrap(),
                    "existing gate bytes\n"
                );
            }
        }
    }

    #[test]
    fn uninstall_without_an_owned_hook_preserves_settings_bytes() {
        let home = ts::fake_home();
        let settings_path = home.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let original = b"{  \"user_setting\": true, \"hooks\": {\"PreToolUse\": []} }\n";
        std::fs::write(&settings_path, original).unwrap();

        ClaudeCodeTarget.uninstall(home.path()).unwrap();

        assert_eq!(std::fs::read(&settings_path).unwrap(), original);
    }
}
