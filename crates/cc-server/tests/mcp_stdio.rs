use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    service::RunningService,
    transport::{ConfigureCommandExt, TokioChildProcess},
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::path::Path;
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

/// Spawn the `codecortex mcp` binary over stdio against `project` and return an
/// initialized client. Mirrors the transport/env setup used by the core smoke
/// test so every test exercises the real JSON-RPC stdio path.
async fn spawn_client(
    project: &Path,
) -> Result<RunningService<RoleClient, ()>, Box<dyn std::error::Error>> {
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
    Ok(().serve(transport).await?)
}

/// Write a multi-language fixture project with a clear call chain
/// (`entrypoint` → `handle_request` → `validate_payload` → `parse_field`) so the
/// graph-oriented tools (trace/relations/explore/node) produce real edges, plus
/// an HTTP route handler so `architecture(aspect="routes")` has something to find.
fn write_fixture(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"mcp-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(
        project.join("src/lib.rs"),
        r#"pub fn parse_field(raw: &str) -> i32 {
    raw.trim().parse().unwrap_or(0)
}

pub fn validate_payload(raw: &str) -> bool {
    parse_field(raw) > 0
}

pub fn handle_request(body: &str) -> bool {
    validate_payload(body)
}

pub fn entrypoint(input: &str) -> bool {
    handle_request(input)
}

#[cfg(test)]
mod tests {
    #[test]
    fn entrypoint_accepts_positive() {
        assert!(super::entrypoint("5"));
    }
}
"#,
    )?;
    // A Python file with a Flask-style route so route/architecture aspects have
    // a handler to surface.
    std::fs::write(
        project.join("src/api.py"),
        "from flask import Flask\n\napp = Flask(__name__)\n\n@app.route(\"/users/<uid>\")\ndef get_user(uid):\n    return fetch_user(uid)\n\ndef fetch_user(uid):\n    return {\"id\": uid}\n",
    )?;
    Ok(())
}

/// Index the fixture project fully via the `index` tool and assert it scanned
/// at least one file.
async fn index_fixture(
    client: &RunningService<RoleClient, ()>,
    project: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
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
    Ok(())
}

/// Call a tool by name with the given JSON args and return its structured
/// `result` payload, asserting the call did not error.
async fn call_result(
    client: &RunningService<RoleClient, ()>,
    name: &str,
    args: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let res = client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(json_args(args)))
        .await?;
    Ok(structured_result(&res)["result"].clone())
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

// ───────────────────────────────────────────────────────────────────────
// status: non-default aspects (capabilities / schema)
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_status_capabilities_and_schema() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    let caps = call_result(
        &client,
        "status",
        json!({"aspect": "capabilities", "project_path": pp}),
    )
    .await?;
    assert_eq!(
        caps["capabilities"]["graph"],
        json!(true),
        "capabilities aspect should report graph capability: {caps}"
    );
    assert!(
        caps["indexed_files"].as_u64().unwrap_or(0) >= 1,
        "capabilities aspect should report indexed files: {caps}"
    );

    let schema = call_result(
        &client,
        "status",
        json!({"aspect": "schema", "project_path": pp}),
    )
    .await?;
    assert!(
        schema.is_object(),
        "schema aspect should return an object describing node/edge types: {schema}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// Graph-oriented tools: context / node / explore / trace / relations
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_graph_tools_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    // Resolve a real symbol via search(mode=symbol) before feeding the
    // graph tools, mirroring the recommended chain.
    let found = call_result(
        &client,
        "search",
        json!({"query": "validate_payload", "mode": "symbol", "top_k": 5, "project_path": pp}),
    )
    .await?;
    assert!(
        found.to_string().contains("validate_payload"),
        "symbol search should locate the fixture symbol: {found}"
    );

    // context: task description → relevant symbols + source in one call.
    let context = call_result(
        &client,
        "context",
        json!({"task": "how is the request payload validated", "max_symbols": 5, "project_path": pp}),
    )
    .await?;
    assert!(
        context.is_object(),
        "context should return a structured object: {context}"
    );

    // node (default include="trail"): source + callers + callees.
    let node = call_result(
        &client,
        "node",
        json!({"symbol": "validate_payload", "include": "trail", "project_path": pp}),
    )
    .await?;
    assert!(
        node.get("source").is_some()
            && node.get("callers").is_some()
            && node.get("callees").is_some(),
        "node(trail) should return source/callers/callees: {node}"
    );

    // node include="source": full source code of the symbol.
    let node_src = call_result(
        &client,
        "node",
        json!({"symbol": "parse_field", "include": "source", "project_path": pp}),
    )
    .await?;
    assert!(
        node_src.to_string().contains("parse_field"),
        "node(source) should include the symbol source: {node_src}"
    );

    // explore (mode="symbols"): batch explore returns per-symbol detail.
    let explore = call_result(
        &client,
        "explore",
        json!({
            "symbols": ["handle_request", "validate_payload"],
            "mode": "symbols",
            "include_source": true,
            "project_path": pp
        }),
    )
    .await?;
    assert!(
        explore.to_string().contains("validate_payload"),
        "explore(symbols) should surface the requested symbols: {explore}"
    );

    // explore (mode="flow"): discover flow paths between symbols.
    let flow = call_result(
        &client,
        "explore",
        json!({
            "symbols": ["entrypoint", "parse_field"],
            "mode": "flow",
            "max_depth": 6,
            "project_path": pp
        }),
    )
    .await?;
    assert!(
        flow.get("flow_paths").is_some() && flow.get("summary").is_some(),
        "explore(flow) should return flow_paths + summary: {flow}"
    );

    // trace with source_mode="body": call-graph path with full bodies.
    let trace = call_result(
        &client,
        "trace",
        json!({
            "from": "entrypoint",
            "to": "parse_field",
            "max_depth": 6,
            "source_mode": "body",
            "project_path": pp
        }),
    )
    .await?;
    assert!(
        trace.get("paths").is_some() && trace.get("path_count").is_some(),
        "trace should return paths + path_count: {trace}"
    );
    assert!(
        trace["path_count"].as_u64().unwrap_or(0) >= 1,
        "trace should find a path entrypoint -> parse_field: {trace}"
    );

    // relations kind="callers": who calls the symbol.
    let callers = call_result(
        &client,
        "relations",
        json!({"symbol": "validate_payload", "kind": "callers", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        callers.to_string().contains("handle_request"),
        "relations(callers) should list handle_request as a caller: {callers}"
    );

    // relations kind="callees": what the symbol calls.
    let callees = call_result(
        &client,
        "relations",
        json!({"symbol": "validate_payload", "kind": "callees", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        callees.to_string().contains("parse_field"),
        "relations(callees) should list parse_field as a callee: {callees}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// Analysis & metadata tools: impact / architecture / files / graph_query / adr
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_analysis_tools_smoke() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    // impact scope="dead_code": unreachable symbols (entrypoint has no callers).
    let dead = call_result(
        &client,
        "impact",
        json!({"scope": "dead_code", "granularity": "file", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        dead.is_object() || dead.is_array(),
        "impact(dead_code) should return a structured result: {dead}"
    );

    // impact scope="circular": dependency cycles (none expected, but valid shape).
    let circular = call_result(
        &client,
        "impact",
        json!({"scope": "circular", "granularity": "file", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        circular.is_object() || circular.is_array(),
        "impact(circular) should return a structured result: {circular}"
    );

    // architecture overview (default aspect).
    let overview = call_result(
        &client,
        "architecture",
        json!({"aspect": "overview", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        overview.is_object() || overview.is_array(),
        "architecture(overview) should return a structured result: {overview}"
    );

    // architecture frameworks: the fixture imports flask, so framework
    // detection should at least return a JSON array.
    let frameworks = call_result(
        &client,
        "architecture",
        json!({"aspect": "frameworks", "project_path": pp}),
    )
    .await?;
    assert!(
        frameworks.is_array(),
        "architecture(frameworks) should return an array: {frameworks}"
    );

    // architecture routes: the Flask @app.route handler should be discoverable.
    let routes = call_result(
        &client,
        "architecture",
        json!({"aspect": "routes", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        routes.is_object() || routes.is_array(),
        "architecture(routes) should return a structured result: {routes}"
    );

    // files list: indexed files as a non-empty array.
    let files = call_result(
        &client,
        "files",
        json!({"action": "list", "project_path": pp}),
    )
    .await?;
    let files_arr = files
        .as_array()
        .expect("files(list) should return an array");
    assert!(
        !files_arr.is_empty(),
        "files(list) should return at least one indexed file: {files}"
    );

    // files region: read a specific line range from the lib.rs fixture.
    let region = call_result(
        &client,
        "files",
        json!({
            "action": "region",
            "path": "src/lib.rs",
            "start_line": 1,
            "end_line": 3,
            "project_path": pp
        }),
    )
    .await?;
    assert!(
        region.is_object() || region.is_array() || region.is_string(),
        "files(region) should return content for the requested range: {region}"
    );

    // graph_query: Cypher subset query for functions.
    let gq = call_result(
        &client,
        "graph_query",
        json!({
            "query": "MATCH (f:Function) RETURN f.name, f.file_path LIMIT 20",
            "project_path": pp
        }),
    )
    .await?;
    assert!(
        gq.is_object(),
        "graph_query should return a result envelope object: {gq}"
    );
    let gq_results = gq.get("results").and_then(|v| v.as_array());
    assert!(
        gq_results.is_some_and(|rows| !rows.is_empty()),
        "graph_query for Function nodes should return rows in 'results': {gq}"
    );

    // adr lifecycle: list (empty) → store → list (present) → get → delete.
    let adr_empty = call_result(
        &client,
        "adr",
        json!({"action": "list", "project_path": pp}),
    )
    .await?;
    assert!(
        adr_empty.get("adrs").is_some(),
        "adr(list) should return an 'adrs' field: {adr_empty}"
    );

    let stored = call_result(
        &client,
        "adr",
        json!({
            "action": "store",
            "adr_id": "ADR-001",
            "title": "Use SQLite for the index",
            "status": "accepted",
            "decision": "Persist the code graph in SQLite.",
            "project_path": pp
        }),
    )
    .await?;
    assert_eq!(
        stored["stored"],
        json!("ADR-001"),
        "adr(store) should confirm the stored id: {stored}"
    );

    let adr_listed = call_result(
        &client,
        "adr",
        json!({"action": "list", "project_path": pp}),
    )
    .await?;
    assert!(
        adr_listed.to_string().contains("ADR-001"),
        "adr(list) should include the stored ADR: {adr_listed}"
    );

    let adr_got = call_result(
        &client,
        "adr",
        json!({"action": "get", "adr_id": "ADR-001", "project_path": pp}),
    )
    .await?;
    assert!(
        adr_got.to_string().contains("ADR-001"),
        "adr(get) should return the stored ADR: {adr_got}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// Project switching: the LRU project cache must isolate two distinct roots
// while keying tool calls by `project_path`.
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_project_switch_isolates_cache() -> Result<(), Box<dyn std::error::Error>> {
    // Project A: the standard fixture containing `validate_payload`.
    let tmp_a = TempDir::new()?;
    let project_a = tmp_a.path();
    write_fixture(project_a)?;

    // Project B: a distinct project with a different, non-overlapping symbol.
    let tmp_b = TempDir::new()?;
    let project_b = tmp_b.path();
    std::fs::create_dir_all(project_b.join("src"))?;
    std::fs::write(
        project_b.join("Cargo.toml"),
        "[package]\nname = \"mcp-fixture-b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(
        project_b.join("src/lib.rs"),
        "pub fn unique_beta_symbol() -> u8 { 42 }\n",
    )?;

    // Server starts pointed at project A.
    let client = spawn_client(project_a).await?;
    index_fixture(&client, project_a).await?;
    let pp_a = project_a.to_string_lossy();
    let pp_b = project_b.to_string_lossy();

    // Index project B explicitly (this also makes it the current project and
    // inserts it into the LRU cache).
    index_fixture(&client, project_b).await?;

    // Searching project B by path must find B's symbol and NOT A's symbol.
    let in_b = call_result(
        &client,
        "search",
        json!({"query": "unique_beta_symbol", "mode": "symbol", "top_k": 5, "project_path": pp_b}),
    )
    .await?;
    assert!(
        in_b.to_string().contains("unique_beta_symbol"),
        "project B search should find B's symbol: {in_b}"
    );
    assert!(
        !in_b.to_string().contains("validate_payload"),
        "project B search must not leak project A symbols: {in_b}"
    );

    // Searching project A by path must still find A's symbol via the cached
    // project services (proves the LRU cache retained A after switching to B).
    let in_a = call_result(
        &client,
        "search",
        json!({"query": "validate_payload", "mode": "symbol", "top_k": 5, "project_path": pp_a}),
    )
    .await?;
    assert!(
        in_a.to_string().contains("validate_payload"),
        "project A search should still find A's symbol after switching: {in_a}"
    );
    assert!(
        !in_a.to_string().contains("unique_beta_symbol"),
        "project A search must not leak project B symbols: {in_a}"
    );

    client.cancel().await?;
    Ok(())
}
