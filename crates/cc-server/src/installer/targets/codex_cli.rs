//! Codex CLI installer target.

use std::path::{Path, PathBuf};

use cc_model::CcResult;

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

    fn install(&self, home: &Path, binary_path: &Path, _force: bool) -> CcResult<Vec<String>> {
        let config_path = home.join(".codex/config.toml");
        let toml_entry = format!(
            "\n[mcp_servers.codecortex]\ncommand = \"{}\"\nargs = [\"mcp\"]\n",
            binary_path.to_string_lossy()
        );
        helpers::append_if_missing(&config_path, "codecortex", &toml_entry)?;
        Ok(vec![])
    }

    fn uninstall(&self, home: &Path) -> CcResult<()> {
        let config_path = home.join(".codex/config.toml");
        if !config_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&config_path)?;

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
        std::fs::write(&config_path, output)?;
        Ok(())
    }

    fn config_location(&self, home: &Path) -> PathBuf {
        home.join(".codex/config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::test_support as ts;

    #[test]
    fn detect_requires_codex_dir() {
        let home = ts::fake_home();
        assert!(!CodexCliTarget.detect(home.path()));
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        assert!(CodexCliTarget.detect(home.path()));
    }

    #[test]
    fn install_appends_toml_section() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        CodexCliTarget.install(home.path(), &binary, false).unwrap();

        let config = home.path().join(".codex/config.toml");
        let content = std::fs::read_to_string(&config).unwrap();
        assert!(content.contains("[mcp_servers.codecortex]"));
        assert!(content.contains(&format!("command = \"{}\"", binary.to_string_lossy())));
        assert!(content.contains("args = [\"mcp\"]"));

        // Idempotent: a second install must not duplicate the section.
        CodexCliTarget.install(home.path(), &binary, false).unwrap();
        let content = std::fs::read_to_string(&config).unwrap();
        assert_eq!(content.matches("[mcp_servers.codecortex]").count(), 1);
    }

    #[test]
    fn install_preserves_existing_toml_content() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let config = home.path().join(".codex/config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let existing = "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"other-bin\"\n";
        std::fs::write(&config, existing).unwrap();

        CodexCliTarget.install(home.path(), &binary, false).unwrap();

        let content = std::fs::read_to_string(&config).unwrap();
        assert!(content.starts_with("model = \"gpt-5\""));
        assert!(content.contains("[mcp_servers.other]"));
        assert!(content.contains("command = \"other-bin\""));
        assert!(content.contains("[mcp_servers.codecortex]"));
    }

    #[test]
    fn uninstall_removes_only_codecortex_section() {
        let home = ts::fake_home();
        let binary = ts::temp_binary(home.path());
        let config = home.path().join(".codex/config.toml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"other-bin\"\n",
        )
        .unwrap();
        CodexCliTarget.install(home.path(), &binary, false).unwrap();

        CodexCliTarget.uninstall(home.path()).unwrap();

        let content = std::fs::read_to_string(&config).unwrap();
        assert!(!content.contains("codecortex"));
        assert!(content.contains("model = \"gpt-5\""));
        assert!(content.contains("[mcp_servers.other]"));
        assert!(content.contains("command = \"other-bin\""));
    }

    #[test]
    fn uninstall_without_config_is_noop() {
        let home = ts::fake_home();
        CodexCliTarget.uninstall(home.path()).unwrap();
        assert!(!home.path().join(".codex/config.toml").exists());
    }
}
