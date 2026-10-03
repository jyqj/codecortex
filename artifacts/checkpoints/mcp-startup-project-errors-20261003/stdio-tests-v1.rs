//! Superseded fixture: sandbox ancestor .git marker invalidated its None interpretation.
use rmcp::{transport::TokioChildProcess, ServiceExt};
use std::{process::Stdio, time::Duration};

#[tokio::test]
async fn none_and_normal_project_complete_stdio_initialization() {
    let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join(".codecortex.json"), r#"{"auto_index":{"enabled":false}}"#).unwrap();
    for explicit in [false, true] {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex"));
        command.arg("mcp").current_dir(root.path())
            .env("CODECORTEX_PPID_POLL_MS", "0")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        if explicit { command.arg("--project-path").arg(&project); }
        let transport = TokioChildProcess::new(command).unwrap();
        let client = tokio::time::timeout(Duration::from_secs(5), ().serve(transport))
            .await.expect("stdio initialize deadline").unwrap();
        assert!(!client.list_all_tools().await.unwrap().is_empty());
        client.cancel().await.unwrap();
    }
}
#[cfg(feature = "semantic")]
#[tokio::test]
async fn rejected_project_exits_before_stdio_handshake() {
    let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"semantic":{"enabled":true,"worker_lease_secs":0}}"#,
    )
    .unwrap();
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex"))
        .args(["mcp", "--project-path"])
        .arg(root.path())
        .env("CODECORTEX_SEMANTIC_CACHE_ROOT", root.path().join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .expect("rejected configuration must exit")
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty(), "no MCP response may be published");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("semantic.worker_lease_secs must be at least 1"),
        "{stderr}"
    );
    assert!(!stderr.contains("MCP server started"));
    assert!(!root.path().join("cache").exists());
}
