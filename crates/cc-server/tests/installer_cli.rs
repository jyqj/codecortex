//! Real CLI contract using an isolated home; never touches user agent settings.
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, action: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_codecortex"))
        .arg(action)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .unwrap()
}

#[test]
fn malformed_codex_config_makes_install_and_uninstall_fail_without_rewriting() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".codex/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let invalid = b"model = 'unterminated";
    std::fs::write(&config, invalid).unwrap();
    for action in ["install", "uninstall"] {
        let result = run(home.path(), action);
        assert!(
            !result.status.success(),
            "{action} unexpectedly succeeded: {result:?}"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("Codex CLI"));
        assert_eq!(std::fs::read(&config).unwrap(), invalid);
    }
}

#[test]
fn install_and_uninstall_round_trip_the_exact_server_entry() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".codex/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let existing = "# codecortex installer test\nmodel = 'test-model'\n\n[mcp_servers.other]\ncommand = 'other-bin'\n";
    std::fs::write(&config, existing).unwrap();
    let installed = run(home.path(), "install");
    assert!(installed.status.success(), "{installed:?}");
    let document: toml_edit::DocumentMut =
        std::fs::read_to_string(&config).unwrap().parse().unwrap();
    assert_eq!(
        document["mcp_servers"]["codecortex"]["command"].as_str(),
        Some(env!("CARGO_BIN_EXE_codecortex"))
    );
    assert_eq!(
        document["mcp_servers"]["codecortex"]["args"][0].as_str(),
        Some("mcp")
    );
    assert_eq!(
        document["mcp_servers"]["other"]["command"].as_str(),
        Some("other-bin")
    );
    let removed = run(home.path(), "uninstall");
    assert!(removed.status.success(), "{removed:?}");
    let document: toml_edit::DocumentMut =
        std::fs::read_to_string(&config).unwrap().parse().unwrap();
    assert!(document["mcp_servers"].get("codecortex").is_none());
    assert_eq!(
        document["mcp_servers"]["other"]["command"].as_str(),
        Some("other-bin")
    );
    assert_eq!(document["model"].as_str(), Some("test-model"));
}
