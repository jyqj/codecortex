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

    // The "index" aspect attaches diagnostics, which now surface the search
    // result-cache hit/miss snapshot (the counters exist on the engine, but
    // were previously unread by any diagnostic path).
    let index_aspect = call_result(
        &client,
        "status",
        json!({"aspect": "index", "project_path": pp}),
    )
    .await?;
    assert!(
        index_aspect["diagnostics"]["search_cache"].is_object(),
        "status (index) diagnostics should surface search cache hit/miss: {index_aspect}"
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
// Envelope tools: search (hybrid) / context / impact (changes blast radius)
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_hybrid_search_context_envelope_and_impact_blast_radius(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    // search (mode="hybrid", the default mode): serialized ContextEnvelope
    // with ranked nodes, a human-readable summary, and a token estimate.
    let hybrid = call_result(
        &client,
        "search",
        json!({"query": "validate payload", "mode": "hybrid", "top_k": 5, "project_path": pp}),
    )
    .await?;
    let nodes = hybrid["nodes"]
        .as_array()
        .expect("hybrid search should return a nodes array");
    assert!(
        !nodes.is_empty(),
        "hybrid search should rank at least one node: {hybrid}"
    );
    assert!(
        nodes.iter().any(|n| n["file_path"] == json!("src/lib.rs")),
        "hybrid hits should point at the fixture source file: {hybrid}"
    );
    assert!(
        hybrid["summary"].as_str().is_some_and(|s| !s.is_empty()),
        "hybrid envelope should carry a non-empty summary: {hybrid}"
    );
    assert!(
        hybrid["token_estimate"].as_u64().unwrap_or(0) > 0,
        "hybrid envelope should estimate its token cost: {hybrid}"
    );
    assert!(
        hybrid["evidence_summary"]["search_hits"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "hybrid envelope should report search hits in evidence_summary: {hybrid}"
    );

    // context: a task naming three fixture symbols takes the symbol-matched
    // path and returns matched symbols + expanded call edges + source details
    // in one envelope.
    let context = call_result(
        &client,
        "context",
        json!({
            "task": "trace how handle_request calls validate_payload and parse_field",
            "max_symbols": 5,
            "project_path": pp
        }),
    )
    .await?;
    let matched = context["matched_symbols"]
        .as_array()
        .expect("context should return matched_symbols for a symbol-rich task");
    assert!(
        matched.len() >= 3,
        "context should match the three symbols named in the task: {context}"
    );
    assert!(
        matched
            .iter()
            .any(|s| s["name"] == json!("validate_payload")),
        "context matched_symbols should include validate_payload: {context}"
    );
    assert!(
        context["expanded_callers"]
            .as_array()
            .is_some_and(|edges| !edges.is_empty()),
        "context should expand caller edges for matched symbols: {context}"
    );
    assert!(
        context["relevant_files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|f| f == "src/lib.rs")),
        "context should list the fixture file as relevant: {context}"
    );
    assert!(
        context.get("symbol_details").is_some(),
        "context (include_source default) should attach symbol_details: {context}"
    );

    // impact (scope="changes"): blast radius for an explicit changed-file set.
    // All four functions in src/lib.rs sit on one call chain, so the report
    // must cover the whole chain.
    let impact = call_result(
        &client,
        "impact",
        json!({"scope": "changes", "files": ["src/lib.rs"], "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        impact["changed_files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|f| f == "src/lib.rs")),
        "impact(changes) should echo the changed file: {impact}"
    );
    let impacted = impact["impacted_symbols"]
        .as_array()
        .expect("impact(changes) should return impacted_symbols");
    for name in ["validate_payload", "entrypoint"] {
        assert!(
            impacted.iter().any(|s| s["name"] == json!(name)),
            "impact(changes) blast radius should include {name}: {impact}"
        );
    }
    assert!(
        impact["risk_summary"]["total_impacted"]
            .as_u64()
            .unwrap_or(0)
            >= 4,
        "impact(changes) should count the full fixture call chain: {impact}"
    );
    assert_eq!(
        impact["truncated"],
        json!(false),
        "impact(changes) on the tiny fixture must not truncate: {impact}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// Structured views: architecture aspects / files region+expand /
// node outline+summary / relations refs+both
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_architecture_files_node_and_relations_views(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    // architecture overview: languages + packages for the mixed-language fixture.
    let overview = call_result(
        &client,
        "architecture",
        json!({"aspect": "overview", "limit": 20, "project_path": pp}),
    )
    .await?;
    let languages = overview["languages"]
        .as_array()
        .expect("architecture(overview) should return a languages array");
    for lang in ["rust", "python"] {
        assert!(
            languages.iter().any(|l| l["language"] == json!(lang)),
            "architecture(overview) languages should include {lang}: {overview}"
        );
    }
    assert!(
        overview["packages"]
            .as_array()
            .is_some_and(|pkgs| !pkgs.is_empty()),
        "architecture(overview) should list at least one package: {overview}"
    );

    // architecture communities: detected module communities with member counts.
    let communities = call_result(
        &client,
        "architecture",
        json!({"aspect": "communities", "project_path": pp}),
    )
    .await?;
    let communities_arr = communities
        .as_array()
        .expect("architecture(communities) should return an array");
    assert!(
        !communities_arr.is_empty(),
        "fixture call chain should form at least one community: {communities}"
    );
    assert!(
        communities_arr
            .iter()
            .all(|c| c["member_count"].as_u64().unwrap_or(0) >= 1),
        "every community should have members: {communities}"
    );

    // architecture routes: the Flask handler with framework + handler fields.
    let routes = call_result(
        &client,
        "architecture",
        json!({"aspect": "routes", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        routes["count"].as_u64().unwrap_or(0) >= 1,
        "architecture(routes) should find the Flask route: {routes}"
    );
    let handlers = routes["route_handlers"]
        .as_array()
        .expect("architecture(routes) should return route_handlers");
    assert!(
        handlers.iter().any(|h| h["handler"] == json!("get_user")
            && h["framework"] == json!("flask")
            && h["route_path"] == json!("/users/<uid>")),
        "route_handlers should contain the fixture Flask handler: {routes}"
    );

    // files region: exact line range plus the symbols overlapping it.
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
        region["content"]
            .as_str()
            .is_some_and(|c| c.contains("parse_field")),
        "files(region) content should contain the parse_field source: {region}"
    );
    assert!(
        region["symbols"]
            .as_array()
            .is_some_and(|syms| syms.iter().any(|s| s["name"] == json!("parse_field"))),
        "files(region) should list parse_field among region symbols: {region}"
    );

    // files expand: the range widened by context_lines on both sides.
    let expand = call_result(
        &client,
        "files",
        json!({
            "action": "expand",
            "path": "src/lib.rs",
            "start_line": 5,
            "end_line": 7,
            "context_lines": 2,
            "project_path": pp
        }),
    )
    .await?;
    assert_eq!(
        expand["start_line"],
        json!(3),
        "files(expand) should widen start_line by context_lines: {expand}"
    );
    assert_eq!(
        expand["end_line"],
        json!(9),
        "files(expand) should widen end_line by context_lines: {expand}"
    );
    assert!(
        expand["content"]
            .as_str()
            .is_some_and(|c| c.contains("validate_payload")),
        "files(expand) content should cover the requested symbol: {expand}"
    );

    // node include="outline": signature-only view with relations attached.
    let outline = call_result(
        &client,
        "node",
        json!({"symbol": "handle_request", "include": "outline", "project_path": pp}),
    )
    .await?;
    let outline_syms = outline["symbols"]
        .as_array()
        .expect("node(outline) should return a symbols array");
    assert!(
        outline_syms.first().is_some_and(|s| s["outline"]
            .as_str()
            .is_some_and(|o| o.contains("handle_request"))),
        "node(outline) should return the handle_request signature: {outline}"
    );
    assert!(
        outline_syms
            .first()
            .is_some_and(|s| s["callers"].as_array().is_some_and(|c| !c.is_empty())),
        "node(outline) should attach caller edges: {outline}"
    );

    // node include="summary": heuristic per-file summary keyed by file path.
    let summary = call_result(
        &client,
        "node",
        json!({"symbol": "src/lib.rs", "include": "summary", "project_path": pp}),
    )
    .await?;
    assert_eq!(
        summary["language"],
        json!("rust"),
        "node(summary) should report the file language: {summary}"
    );
    assert!(
        summary["symbols_count"].as_u64().unwrap_or(0) >= 4,
        "node(summary) should count the fixture functions: {summary}"
    );

    // relations kind="refs": reference sites for a symbol.
    let refs = call_result(
        &client,
        "relations",
        json!({"symbol": "validate_payload", "kind": "refs", "limit": 20, "project_path": pp}),
    )
    .await?;
    let refs_arr = refs
        .as_array()
        .expect("relations(refs) should return an array of reference sites");
    assert!(
        !refs_arr.is_empty(),
        "validate_payload is called by handle_request, so refs must not be empty: {refs}"
    );
    assert!(
        refs_arr
            .iter()
            .all(|r| r["file_path"].is_string() && r["line"].is_u64()),
        "every reference site should carry file_path and line: {refs}"
    );

    // relations kind="both" (the default): callers and callees in one call.
    let both = call_result(
        &client,
        "relations",
        json!({"symbol": "validate_payload", "kind": "both", "limit": 20, "project_path": pp}),
    )
    .await?;
    assert!(
        both["callers"]
            .as_array()
            .is_some_and(|edges| !edges.is_empty()),
        "relations(both) should list callers: {both}"
    );
    assert!(
        both["callees"]
            .as_array()
            .is_some_and(|edges| !edges.is_empty()),
        "relations(both) should list callees: {both}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// ingest_traces: runtime observations land as evidence and match the
// fixture route; adr delete completes the ADR lifecycle.
// ───────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_ingest_traces_and_adr_delete() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    let client = spawn_client(project).await?;
    index_fixture(&client, project).await?;
    let pp = project.to_string_lossy();

    // Two observations for the Flask route: one flat OTLP-ish record and one
    // wrapped in a spans array. Both normalize to /users/* and must match the
    // indexed route.
    let ingest = call_result(
        &client,
        "ingest_traces",
        json!({
            "traces": [
                {
                    "service_name": "api",
                    "method": "GET",
                    "path": "/users/{uid}",
                    "status_code": "200"
                },
                {
                    "service_name": "api",
                    "spans": [{
                        "method": "GET",
                        "path": "/users/<uid>",
                        "status_code": "200",
                        "kind": "server"
                    }]
                }
            ],
            "project_path": pp
        }),
    )
    .await?;
    assert_eq!(
        ingest["accepted"],
        json!(2),
        "both observations should be accepted as evidence: {ingest}"
    );
    assert_eq!(
        ingest["spans_processed"],
        json!(2),
        "both observations should be processed: {ingest}"
    );
    assert_eq!(
        ingest["total_submitted"],
        json!(2),
        "total_submitted should echo the trace count: {ingest}"
    );
    assert_eq!(
        ingest["routes_matched"],
        json!(2),
        "both observations should match the indexed Flask route: {ingest}"
    );
    assert_eq!(
        ingest["write_errors"],
        json!(0),
        "evidence writes should succeed on a healthy index: {ingest}"
    );

    // The ingested evidence must be visible through status(aspect="index").
    let status = call_result(
        &client,
        "status",
        json!({"aspect": "index", "project_path": pp}),
    )
    .await?;
    assert!(
        status["runtime_evidence"]["evidence_rows"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "status(index) should surface the stored runtime evidence: {status}"
    );
    assert!(
        status["runtime_evidence"]["total_observations"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "status(index) should count both ingested observations: {status}"
    );

    // adr delete: store a record, delete it, and verify get/list no longer
    // return it (get reports not-found in-band, not as a JSON-RPC error).
    let stored = call_result(
        &client,
        "adr",
        json!({
            "action": "store",
            "adr_id": "ADR-100",
            "title": "Temporary decision",
            "status": "proposed",
            "decision": "To be deleted by this test.",
            "project_path": pp
        }),
    )
    .await?;
    assert_eq!(
        stored["stored"],
        json!("ADR-100"),
        "adr(store) should confirm the stored id: {stored}"
    );

    let deleted = call_result(
        &client,
        "adr",
        json!({"action": "delete", "adr_id": "ADR-100", "project_path": pp}),
    )
    .await?;
    assert_eq!(
        deleted["deleted"],
        json!(true),
        "adr(delete) should report the record as deleted: {deleted}"
    );
    assert_eq!(
        deleted["adr_id"],
        json!("ADR-100"),
        "adr(delete) should echo the deleted id: {deleted}"
    );

    let got = call_result(
        &client,
        "adr",
        json!({"action": "get", "adr_id": "ADR-100", "project_path": pp}),
    )
    .await?;
    assert!(
        got["error"].as_str().is_some_and(|e| e.contains("ADR-100")),
        "adr(get) after delete should report not-found in-band: {got}"
    );

    let listed = call_result(
        &client,
        "adr",
        json!({"action": "list", "project_path": pp}),
    )
    .await?;
    assert!(
        !listed.to_string().contains("ADR-100"),
        "adr(list) must not return the deleted record: {listed}"
    );

    client.cancel().await?;
    Ok(())
}

// ───────────────────────────────────────────────────────────────────────
// Error contract: invalid enum values must surface as JSON-RPC -32602
// (invalid params) listing the legal values, per docs/MCP_TOOLS.md.
// ───────────────────────────────────────────────────────────────────────

/// Call `tool` with `args` and return the JSON-RPC error it produces,
/// panicking if the call unexpectedly succeeds or fails at transport level.
async fn expect_rpc_error(
    client: &RunningService<RoleClient, ()>,
    tool: &str,
    args: Value,
) -> rmcp::model::ErrorData {
    let result = client
        .call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(json_args(args)))
        .await;
    match result {
        Err(rmcp::ServiceError::McpError(err)) => err,
        other => panic!("expected a JSON-RPC error from {tool}, got: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stdio_invalid_enum_value_maps_to_invalid_params() -> Result<(), Box<dyn std::error::Error>>
{
    let tmp = TempDir::new()?;
    let project = tmp.path();
    write_fixture(project)?;

    // Sanitization runs before any index access, so no indexing is needed.
    let client = spawn_client(project).await?;
    let pp = project.to_string_lossy();

    let relations_err = expect_rpc_error(
        &client,
        "relations",
        json!({"symbol": "validate_payload", "kind": "sideways", "project_path": pp}),
    )
    .await;
    assert_eq!(
        relations_err.code.0, -32602,
        "invalid relations kind must map to JSON-RPC invalid params: {relations_err:?}"
    );
    assert!(
        relations_err.message.contains("invalid kind") && relations_err.message.contains("callers"),
        "the error should name the parameter and list legal values: {relations_err:?}"
    );

    let architecture_err = expect_rpc_error(
        &client,
        "architecture",
        json!({"aspect": "bogus", "project_path": pp}),
    )
    .await;
    assert_eq!(
        architecture_err.code.0, -32602,
        "invalid architecture aspect must map to JSON-RPC invalid params: {architecture_err:?}"
    );
    assert!(
        architecture_err.message.contains("invalid aspect")
            && architecture_err.message.contains("overview"),
        "the error should name the parameter and list legal values: {architecture_err:?}"
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
