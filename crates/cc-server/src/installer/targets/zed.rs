//! Zed installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct ZedTarget;

impl ZedTarget {
    fn settings_path(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Zed/settings.json")
        } else {
            home.join(".config/zed/settings.json")
        }
    }
}

impl InstallerTarget for ZedTarget {
    fn name(&self) -> &str {
        "Zed"
    }

    fn id(&self) -> &str {
        "zed"
    }

    fn detect(&self, home: &Path) -> bool {
        Self::settings_path(home).exists()
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let settings_path = Self::settings_path(home);
        helpers::upsert_json_key(
            &settings_path,
            "context_servers",
            "codecortex",
            &serde_json::json!({
                "command": binary_path.to_string_lossy(),
                "args": ["mcp"]
            }),
        )?;
        Ok(vec![])
    }

    fn uninstall(&self, _home: &Path) -> Result<(), String> {
        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        Self::settings_path(home)
    }
}
