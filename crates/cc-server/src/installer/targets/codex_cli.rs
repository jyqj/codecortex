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

    fn uninstall(&self, home: &Path) -> Result<(), String> {
        let config_path = home.join(".codex/config.toml");
        if !config_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;

        // Remove the [mcp_servers.codecortex] section and its key-value pairs.
        // The section was appended by install() as a block like:
        //   \n[mcp_servers.codecortex]\ncommand = "..."\nargs = ["mcp"]\n
        let mut result = String::new();
        let mut skip = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[mcp_servers.codecortex]" {
                skip = true;
                continue;
            }
            // Stop skipping when we hit the next section header or end of content
            if skip {
                if trimmed.starts_with('[') {
                    skip = false;
                } else {
                    continue;
                }
            }
            result.push_str(line);
            result.push('\n');
        }

        // Remove trailing blank lines that may have been left
        let trimmed = result.trim_end_matches('\n');
        let output = if trimmed.is_empty() {
            String::new()
        } else {
            format!("{}\n", trimmed)
        };
        std::fs::write(&config_path, output).map_err(|e| e.to_string())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".codex/config.toml")
    }
}
