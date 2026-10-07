//! Real binary/stdio smoke checks complement isolated shared-gate API fixtures.
use rmcp::{transport::TokioChildProcess, ServiceExt};
use std::{process::Stdio, time::Duration};

#[tokio::test]
async fn none_session_completes_mcp_initialization() {
    // The CLI discovers ancestor markers; explicit None is the library API.
    // Use real MCP framing to avoid accidentally discovering sandbox .git mounts.
    let server = cc_server::mcp::CodeCortexMcpServer::new(None).unwrap();
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    let task = tokio::spawn(async move {
        let service = server.serve(server_io).await.unwrap();
        service.waiting().await.unwrap();
    });
    let client = tokio::time::timeout(Duration::from_secs(5), ().serve(client_io))
        .await
        .expect("MCP initialize deadline")
        .unwrap();
    assert!(!client.list_all_tools().await.unwrap().is_empty());
    client.cancel().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn normal_project_completes_stdio_initialization() {
    let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex"));
    command
        .args(["mcp", "--project-path"])
        .arg(root.path())
        .current_dir(root.path())
        .env("CODECORTEX_PPID_POLL_MS", "0")
        .env("CODECORTEX_SEMANTIC_CACHE_ROOT", root.path().join("cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let transport = TokioChildProcess::new(command).unwrap();
    let client = tokio::time::timeout(Duration::from_secs(5), ().serve(transport))
        .await
        .expect("stdio initialize deadline")
        .unwrap();
    assert!(!client.list_all_tools().await.unwrap().is_empty());
    client.cancel().await.unwrap();
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
