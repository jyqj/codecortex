//! VS Code installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct VsCodeTarget;

impl VsCodeTarget {
    fn mcp_path(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Code/User/mcp.json")
        } else {
            home.join(".config/Code/User/mcp.json")
        }
    }
}

impl InstallerTarget for VsCodeTarget {
    fn name(&self) -> &str {
        "VS Code"
    }

    fn id(&self) -> &str {
        "vscode"
    }

    fn detect(&self, home: &Path) -> bool {
        let mcp_path = Self::mcp_path(home);
        mcp_path.parent().map(|p| p.exists()).unwrap_or(false)
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let mcp_path = Self::mcp_path(home);
        helpers::upsert_json_key(
            &mcp_path,
            "mcpServers",
            "codecortex",
            &serde_json::json!({
                "command": binary_path.to_string_lossy(),
                "args": ["mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> Result<(), String> {
        helpers::remove_json_key(&Self::mcp_path(home), "mcpServers", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::mcp_path(home)
    }
}
