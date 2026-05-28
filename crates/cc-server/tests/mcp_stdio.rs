use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use std::process::Stdio;
use tempfile::TempDir;

fn json_args(value: Value) -> rmcp::model::JsonObject {
    value
        .as_object()
        .expect("tool args must be an object")
        .clone()
}

fn structured_result(result: &CallToolResult) -> &Value {
    assert_eq!(result.is_error, Some(false));
    result
        .structured_content
        .as_ref()
        .expect("tool should return structured content")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_mcp_lists_tools_and_calls_core_tools() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"mcp-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(
        project.join("src/lib.rs"),
        "pub fn add_one(x: i32) -> i32 { x + 1 }\n#[cfg(test)] mod tests { #[test] fn add_one_works() { assert_eq!(super::add_one(1), 2); } }\n",
    )?;

    let bin = env!("CARGO_BIN_EXE_codecortex");
    let transport = TokioChildProcess::new(tokio::process::Command::new(bin).configure(|cmd| {
        cmd.arg("mcp")
            .arg("--project-path")
            .arg(project)
            .env("CODECORTEX_PPID_POLL_MS", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
    }))?;

    let client = ().serve(transport).await?;

    let tools = client.list_all_tools().await?;
    assert_eq!(tools.len(), 14, "MCP tool surface drifted");
    assert!(tools.iter().any(|tool| tool.name == "status"));
    assert!(tools.iter().any(|tool| tool.name == "index"));
    assert!(tools.iter().any(|tool| tool.name == "search"));

    let index = client
        .call_tool(
            CallToolRequestParams::new("index").with_arguments(json_args(json!({
                "path": project.to_string_lossy(),
                "full": true
            }))),
        )
        .await?;
    let index_json = structured_result(&index);
    assert!(
        index_json["result"]["files_scanned"].as_u64().unwrap_or(0) >= 1,
        "index tool did not index the fixture project: {index_json}"
    );

    let status = client
        .call_tool(
            CallToolRequestParams::new("status").with_arguments(json_args(json!({
                "aspect": "index",
                "project_path": project.to_string_lossy()
            }))),
        )
        .await?;
    let status_json = structured_result(&status);
    assert!(
        status_json["result"]["indexed_files"].as_u64().unwrap_or(0) >= 1,
        "status tool did not report indexed files: {status_json}"
    );

    let search = client
        .call_tool(
            CallToolRequestParams::new("search").with_arguments(json_args(json!({
                "query": "add_one",
                "mode": "symbol",
                "top_k": 5,
                "project_path": project.to_string_lossy()
            }))),
        )
        .await?;
    let search_json = structured_result(&search);
    assert!(
        search_json["result"].to_string().contains("add_one"),
        "search tool did not find the fixture symbol: {search_json}"
    );

    client.cancel().await?;
    Ok(())
}
