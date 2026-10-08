use cc_eval::benchmark::{
    adapters::{mcp_stdio::McpStdio, Backend},
    manifest,
    readiness::State,
};
use std::{path::PathBuf, time::Duration};

fn binary() -> PathBuf {
    PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary required"),
    )
}
fn project() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    d
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY pointing at an explicitly built product"]
async fn real_mcp_inventory_crosses_pages_and_preserves_exact_paths() {
    let d = project();
    let mut paths = Vec::new();
    for i in 0..137 {
        let path = if i == 63 {
            "f063'quoted.py".to_owned()
        } else {
            format!("f{i:03}.py")
        };
        std::fs::write(
            d.path().join(&path),
            format!("def function_{i}():\n    return {i}\n"),
        )
        .unwrap();
        paths.push(path);
    }
    let files = manifest::inventory(d.path(), &paths).unwrap();
    let mut client = McpStdio::spawn(&binary(), d.path(), Duration::from_secs(30))
        .await
        .unwrap();
    client.prepare(&files).await.unwrap();
    let state = client.readiness(&files).await.unwrap();
    assert_eq!(state.state, State::Ready);
    assert_eq!(state.ready, 137);
    let observation = client.readiness_observation().unwrap();
    assert!(observation["pages"].as_array().unwrap().len() >= 3);
    for file in &files {
        assert_eq!(
            observation["indexed_paths_and_bytes"][file.path.as_str()].as_u64(),
            Some(file.bytes)
        );
    }
    client.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY pointing at an explicitly built product"]
async fn real_mcp_wrong_input_with_same_count_is_rejected() {
    let d = project();
    std::fs::write(d.path().join("wrong.py"), "def wrong():\n    return 0\n").unwrap();
    std::fs::write(d.path().join(".wanted.py"), "def wanted():\n    return 1\n").unwrap();
    let files = manifest::inventory(d.path(), &[".wanted.py".into()]).unwrap();
    let mut client = McpStdio::spawn(&binary(), d.path(), Duration::from_secs(30))
        .await
        .unwrap();
    client.prepare(&files).await.unwrap();
    let error = client.readiness(&files).await.unwrap_err();
    assert!(error.to_string().contains("outside the source manifest"));
    assert!(client.readiness_observation().is_some());
    client.close().await.unwrap();
}
