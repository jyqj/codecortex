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

    fn read_document(home: &Path) -> toml_edit::DocumentMut {
        std::fs::read_to_string(home.join(".codex/config.toml"))
            .unwrap()
            .parse()
            .unwrap()
    }

    fn write_config(home: &Path, content: &[u8]) -> PathBuf {
        let path = home.join(".codex/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn install_round_trips_literal_binary_paths() {
        for binary in [
            Path::new(r#"C:\Program Files\Code"Cortex\codecortex.exe"#),
            Path::new("/tmp/line\nbreak\t雪/codecortex"),
        ] {
            let home = ts::fake_home();
            CodexCliTarget.install(home.path(), binary, false).unwrap();
            let document = read_document(home.path());
            assert_eq!(
                document["mcp_servers"]["codecortex"]["command"].as_str(),
                binary.to_str()
            );
            let args = document["mcp_servers"]["codecortex"]["args"]
                .as_array()
                .unwrap();
            assert_eq!(args.len(), 1);
            assert_eq!(args.get(0).and_then(toml_edit::Value::as_str), Some("mcp"));
        }
    }

    #[test]
    fn install_matches_server_key_not_comments_or_other_values() {
        let home = ts::fake_home();
        let existing = "# codecortex is not installed yet\nmodel = 'test'\n\n[mcp_servers.other]\ncommand = '/codecortex/other'\n";
        write_config(home.path(), existing.as_bytes());
        let binary = ts::temp_binary(home.path());
        CodexCliTarget.install(home.path(), &binary, false).unwrap();
        let document = read_document(home.path());
        assert_eq!(
            document["mcp_servers"]["codecortex"]["command"].as_str(),
            binary.to_str()
        );
        assert_eq!(document["mcp_servers"]["other"]["command"].as_str(), Some("/codecortex/other"));
        assert!(document.to_string().starts_with(existing));
    }

    #[test]
    fn reinstall_updates_command_and_keeps_user_options_and_comments() {
        let home = ts::fake_home();
        let existing = "# keep model comment\nmodel = 'test'\n\n[mcp_servers.\"codecortex\"]\ncommand = '/old/bin' # keep command comment\nargs = ['old']\nenabled = false\nstartup_timeout_sec = 45\n\n[mcp_servers.codecortex.env]\nCC_LOG = 'debug'\n\n[mcp_servers.other]\ncommand = 'other-bin'\n";
        write_config(home.path(), existing.as_bytes());
        let binary = ts::temp_binary(home.path());
        CodexCliTarget.install(home.path(), &binary, false).unwrap();
        let document = read_document(home.path());
        let server = &document["mcp_servers"]["codecortex"];
        assert_eq!(server["command"].as_str(), binary.to_str());
        assert_eq!(server["args"][0].as_str(), Some("mcp"));
        assert_eq!(server["enabled"].as_bool(), Some(false));
        assert_eq!(server["startup_timeout_sec"].as_integer(), Some(45));
        assert_eq!(server["env"]["CC_LOG"].as_str(), Some("debug"));
        assert_eq!(document["mcp_servers"]["other"]["command"].as_str(), Some("other-bin"));
        let installed = document.to_string();
        assert!(installed.contains("# keep model comment"));
        assert!(installed.contains("# keep command comment"));
        CodexCliTarget.install(home.path(), &binary, false).unwrap();
        assert_eq!(read_document(home.path()).to_string(), installed);
    }

    #[test]
    fn install_and_uninstall_accept_inline_and_dotted_tables() {
        for existing in [
            "mcp_servers = { other = { command = 'other-bin' } }\n",
            "mcp_servers = { codecortex = { command = 'old', env = { CC_LOG = 'debug' } }, other = { command = 'other-bin' } }\n",
            "[mcp_servers]\ncodecortex = { command = 'old' }\nother = { command = 'other-bin' }\n",
            "mcp_servers.codecortex.command = 'old'\nmcp_servers.other.command = 'other-bin'\n",
        ] {
            let home = ts::fake_home();
            write_config(home.path(), existing.as_bytes());
            let binary = ts::temp_binary(home.path());
            CodexCliTarget.install(home.path(), &binary, false).unwrap();
            let document = read_document(home.path());
            assert_eq!(document["mcp_servers"]["codecortex"]["command"].as_str(), binary.to_str());
            assert_eq!(document["mcp_servers"]["other"]["command"].as_str(), Some("other-bin"));
            CodexCliTarget.uninstall(home.path()).unwrap();
            let document = read_document(home.path());
            assert!(document["mcp_servers"].get("codecortex").is_none());
            assert_eq!(document["mcp_servers"]["other"]["command"].as_str(), Some("other-bin"));
        }
    }

    #[test]
    fn uninstall_understands_nested_tables_and_multiline_values() {
        let home = ts::fake_home();
        let existing = "model = 'test'\n\n[mcp_servers.\"codecortex\"] # target\ncommand = 'old'\nargs = [\n  ['nested array'],\n]\nnote = '''\n[mcp_servers.apparent_header_in_string]\n'''\n[mcp_servers.codecortex.env]\nCC_LOG = 'debug'\n\n[mcp_servers.\"codecortex.other\"]\ncommand = 'keep-dotted-name'\n\n[mcp_servers.other]\ncommand = 'other-bin'\n";
        write_config(home.path(), existing.as_bytes());
        CodexCliTarget.uninstall(home.path()).unwrap();
        let document = read_document(home.path());
        assert_eq!(document["model"].as_str(), Some("test"));
        assert!(document["mcp_servers"].get("codecortex").is_none());
        assert_eq!(document["mcp_servers"]["codecortex.other"]["command"].as_str(), Some("keep-dotted-name"));
        assert_eq!(document["mcp_servers"]["other"]["command"].as_str(), Some("other-bin"));
    }

    #[test]
    fn malformed_or_wrong_typed_config_is_rejected_without_writing() {
        for existing in [
            &b"model = 'unterminated"[..],
            &b"mcp_servers = 42\n"[..],
            &b"[mcp_servers]\ncodecortex = false\n"[..],
            &b"# invalid UTF8: \xff\n"[..],
        ] {
            let home = ts::fake_home();
            let config = write_config(home.path(), existing);
            assert!(CodexCliTarget.install(home.path(), Path::new("/new/bin"), false).is_err());
            assert_eq!(std::fs::read(&config).unwrap(), existing);
            assert!(CodexCliTarget.uninstall(home.path()).is_err());
            assert_eq!(std::fs::read(&config).unwrap(), existing);
        }
    }

    #[test]
    fn uninstall_without_server_preserves_original_bytes() {
        let home = ts::fake_home();
        let original = b"# codecortex notes\r\nmodel = 'test'";
        let config = write_config(home.path(), original);
        CodexCliTarget.uninstall(home.path()).unwrap();
        assert_eq!(std::fs::read(config).unwrap(), original);
    }

    #[test]
    fn install_does_not_silently_replace_existing_url_transport() {
        let home = ts::fake_home();
        let original = b"[mcp_servers.codecortex]\nurl = 'http://localhost:8080/mcp'\n";
        let config = write_config(home.path(), original);
        assert!(CodexCliTarget.install(home.path(), Path::new("/new/bin"), false).is_err());
        assert_eq!(std::fs::read(config).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_non_utf8_binary_path_without_creating_config() {
        use std::os::unix::ffi::OsStringExt;
        let home = ts::fake_home();
        let binary = PathBuf::from(std::ffi::OsString::from_vec(b"/bin/codecortex-\xff".to_vec()));
        assert!(CodexCliTarget.install(home.path(), &binary, false).is_err());
        assert!(!home.path().join(".codex/config.toml").exists());
    }
}
