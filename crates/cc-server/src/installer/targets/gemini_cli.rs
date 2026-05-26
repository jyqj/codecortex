//! Gemini CLI installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct GeminiCliTarget;

impl InstallerTarget for GeminiCliTarget {
    fn name(&self) -> &str {
        "Gemini CLI"
    }

    fn id(&self) -> &str {
        "gemini_cli"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".gemini").exists()
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let settings_path = home.join(".gemini/settings.json");
        helpers::upsert_json_key(
            &settings_path,
            "mcpServers",
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
        home.join(".gemini/settings.json")
    }
}
