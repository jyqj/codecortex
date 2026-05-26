//! Codex CLI installer target.

use std::path::{Path, PathBuf};

use crate::installer::helpers;
use crate::installer::InstallerTarget;

pub struct CodexCliTarget;

impl InstallerTarget for CodexCliTarget {
    fn name(&self) -> &str {
        "Codex CLI"
    }

    fn id(&self) -> &str {
        "codex_cli"
    }

    fn detect(&self, home: &Path) -> bool {
        home.join(".codex").exists()
    }

    fn install(
        &self,
        home: &Path,
        binary_path: &Path,
        _force: bool,
    ) -> Result<Vec<String>, String> {
        let config_path = home.join(".codex/config.toml");
        let toml_entry = format!(
            "\n[mcp_servers.codecortex]\ncommand = \"{}\"\nargs = [\"mcp\"]\n",
            binary_path.to_string_lossy()
        );
        helpers::append_if_missing(&config_path, "codecortex", &toml_entry)?;
        Ok(vec![])
    }

    fn uninstall(&self, _home: &Path) -> Result<(), String> {
        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".codex/config.toml")
    }
}
