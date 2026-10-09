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

#[test]
fn claude_install_and_uninstall_preserve_user_hooks_in_shared_groups() {
    let home = tempfile::tempdir().unwrap();
    let settings_path = home.path().join(".claude/settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    let user_hook = serde_json::json!({
        "matcher": "Grep|Glob|Search",
        "hooks": [{ "type": "command", "command": "/usr/local/bin/codecortex-audit" }]
    });
    std::fs::write(
        &settings_path,
        serde_json::to_vec(&serde_json::json!({ "hooks": { "PreToolUse": [user_hook] } })).unwrap(),
    )
    .unwrap();

    let installed = run(home.path(), "install");
    assert!(installed.status.success(), "{installed:?}");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    let entries = settings["hooks"]["PreToolUse"].as_array_mut().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "the user command must not suppress installation"
    );
    assert_eq!(entries[0], user_hook);
    let sibling = serde_json::json!({ "type": "command", "command": "/usr/local/bin/user-linter" });
    entries[1]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(sibling.clone());
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();

    let removed = run(home.path(), "uninstall");
    assert!(removed.status.success(), "{removed:?}");
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
    let entries = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], user_hook);
    assert_eq!(entries[1]["hooks"], serde_json::json!([sibling]));
}

#[test]
fn malformed_claude_hook_configuration_fails_without_modifying_other_installation_files() {
    let home = tempfile::tempdir().unwrap();
    let settings_path = home.path().join(".claude/settings.json");
    let mcp_path = home.path().join(".claude/.mcp.json");
    let gate_path = home.path().join(".claude/hooks/codecortex-discovery-gate");
    std::fs::create_dir_all(gate_path.parent().unwrap()).unwrap();
    let settings = br#"{"hooks":{"PreToolUse":{"user":"keep"}}}"#;
    let mcp = br#"{"mcpServers":{"codecortex":{"command":"old"}}}"#;
    let gate = b"existing gate bytes\n";
    std::fs::write(&settings_path, settings).unwrap();
    std::fs::write(&mcp_path, mcp).unwrap();
    std::fs::write(&gate_path, gate).unwrap();

    for action in ["install", "uninstall"] {
        let result = run(home.path(), action);
        assert!(
            !result.status.success(),
            "{action} unexpectedly succeeded: {result:?}"
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("Claude Code"));
        assert_eq!(std::fs::read(&settings_path).unwrap(), settings);
        assert_eq!(std::fs::read(&mcp_path).unwrap(), mcp);
        assert_eq!(std::fs::read(&gate_path).unwrap(), gate);
    }
}

#[test]
fn json_uninstall_rejects_wrong_types_and_preserves_configs_without_our_entry() {
    for (original, succeeds) in [
        (" [ ]\n", false),
        ("{\"mcpServers\" : [ ]}\n", false),
        (
            "{\"mcpServers\" : {\"other\": {\"command\": \"keep\"}}}\n",
            true,
        ),
    ] {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, original).unwrap();
        let result = run(home.path(), "uninstall");
        assert_eq!(result.status.success(), succeeds, "{result:?}");
        if !succeeds {
            assert!(String::from_utf8_lossy(&result.stderr).contains("Cursor"));
        }
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    }
}
