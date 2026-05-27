//! Claude Code installer target.

use std::path::{Path, PathBuf};

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

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
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
        std::fs::create_dir_all(&hooks_dir).map_err(|e| e.to_string())?;
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
        std::fs::write(&gate_script_path, gate_script).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&gate_script_path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
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

    fn uninstall(&self, home: &Path) -> Result<(), String> {
        let claude_dir = home.join(".claude");

        // 1. Remove MCP server entry
        let mcp_config_path = claude_dir.join(".mcp.json");
        helpers::remove_json_key(&mcp_config_path, "mcpServers", "codecortex")?;

        // 2. Remove PreToolUse hook entries containing codecortex from settings.json
        let settings_path = claude_dir.join("settings.json");
        if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
            let mut root: serde_json::Value =
                serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
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
            std::fs::write(
                &settings_path,
                serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }

        // 3. Remove gate script
        let gate_script_path = claude_dir.join("hooks/codecortex-discovery-gate");
        if gate_script_path.exists() {
            std::fs::remove_file(&gate_script_path).map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".claude/.mcp.json")
    }
}
