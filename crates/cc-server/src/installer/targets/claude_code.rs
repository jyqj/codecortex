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
echo 'BLOCKED: For code discovery, prefer CodeCortex MCP tools first: search(query) to find functions/classes, callers()/callees() for call chains, graph_query() for Cypher queries. If the project is not indexed yet, call set_project first. Fall back to Grep/Glob/Read only for non-structural searches. If you need Grep, retry.' >&2
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

    fn uninstall(&self, _home: &Path) -> Result<(), String> {
        // TODO: Remove codecortex entries from mcp.json and settings.json
        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".claude/.mcp.json")
    }
}
