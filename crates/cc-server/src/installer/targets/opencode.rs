//! opencode installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct OpenCodeTarget;

impl OpenCodeTarget {
    fn config_path(home: &Path) -> PathBuf {
        home.join(".config/opencode/opencode.json")
    }
}

impl InstallerTarget for OpenCodeTarget {
    fn name(&self) -> &str {
        "opencode"
    }
    fn id(&self) -> &str {
        "opencode"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".config/opencode").exists() || home.join(".opencode").exists()
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let path = Self::config_path(home);
        helpers::upsert_json_key(
            &path,
            "mcp",
            "codecortex",
            &serde_json::json!({
                "type": "local",
                "command": [binary_path.to_string_lossy(), "mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> Result<(), String> {
        helpers::remove_json_key(&Self::config_path(home), "mcp", "codecortex")
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::config_path(home)
    }
}
