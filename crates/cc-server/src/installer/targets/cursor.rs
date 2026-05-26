//! Cursor installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct CursorTarget;

impl CursorTarget {
    fn mcp_path(home: &Path) -> PathBuf {
        home.join(".cursor/mcp.json")
    }
}

impl InstallerTarget for CursorTarget {
    fn name(&self) -> &str {
        "Cursor"
    }
    fn id(&self) -> &str {
        "cursor"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".cursor").exists()
            || home.join("Library/Application Support/Cursor").exists()
            || home.join(".config/Cursor").exists()
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let path = Self::mcp_path(home);
        helpers::upsert_json_key(
            &path,
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
