//! P5-A public retrieval-lane contract over real parser/SQLite/search context.
use cc_model::search::SearchRequest;
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::Path;

fn put(root: &Path, path: &str, content: &str) {
    let destination = root.join(path);
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(destination, content).unwrap();
}

fn setup() -> (tempfile::TempDir, CodeIndex) {
    let temp = tempfile::tempdir().unwrap();
    put(
        temp.path(),
        ".codecortex.json",
        &json!({
            "auto_index": {"enabled": false},
            "search": {
                "lexical_weight": 0.0,
                "exact_symbol_weight": 1.0,
                "path_weight": 1.0,
                "grep_weight": 0.0,
                "graph_weight": 0.0,
                "exact_symbol_top_k": 16,
                "path_top_k": 16
            }
        })
        .to_string(),
    );
    put(
        temp.path(),
        "src/target.rs",
        "pub fn sought_symbol() -> i32 { 42 }\n",
    );
    put(
        temp.path(),
        "src/noise.rs",
        "pub fn unrelated() -> i32 { 0 }\n",
    );
    put(
        temp.path(),
        "tests/noise.rs",
        "pub fn test_noise() -> i32 { 0 }\n",
    );
    let mut index = CodeIndex::new(Some(temp.path())).unwrap();
    let build = index.build_index(true).unwrap();
    assert!(build.parse_errors.is_empty());
    assert!(build.resolution_freshness.complete);
    (temp, index)
}

fn lanes(envelope: &cc_model::context::ContextEnvelope) -> &[Value] {
    envelope.evidence_summary["retrieval"]["lanes"]
        .as_array()
        .expect("public lane outcomes")
}

fn lane<'a>(envelope: &'a cc_model::context::ContextEnvelope, id: &str) -> &'a Value {
    lanes(envelope)
        .iter()
        .find(|lane| lane["lane_id"] == id)
        .unwrap_or_else(|| panic!("missing lane {id}"))
}

#[test]
fn public_exact_and_path_lanes_are_versioned_scoped_and_replayable() {
    let (_temp, index) = setup();
    let exact = index
        .search()
        .search_in_context_with(
            "sought_symbol",
            1,
            None,
            SearchRequest {
                pinned_file_paths: Some(vec!["src/noise.rs".into()]),
                file_preselect_limit: Some(1),
                include_grep: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(exact.machine_pack["hits"][0]["file_path"], "src/target.rs");
    let exact_lane = lane(&exact, "exact_symbol");
    assert_eq!(exact_lane["status"], "complete");
    assert_eq!(exact_lane["candidate_count"], 1);
    assert_eq!(
        exact_lane["candidates"][0]["document"]["doc_key"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        exact_lane["candidates"][0]["document"]["doc_version"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(exact_lane["candidates"][0]["exact_identity"], true);
    assert_eq!(lane(&exact, "path")["status"], "complete");
    // The public context helper intentionally enables bounded grep for source
    // recall even when its fusion weight is zero; graph is genuinely disabled.
    assert_eq!(lane(&exact, "grep")["status"], "complete");
    assert_eq!(lane(&exact, "graph")["status"], "disabled");
    for hit in exact.machine_pack["hits"].as_array().unwrap() {
        let replay: f64 = hit["score_trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry[1].as_f64().unwrap())
            .sum();
        assert!((replay - hit["rerank_score"].as_f64().unwrap()).abs() < 1e-9);
    }

    let path = index
        .search()
        .search_in_context_with(
            "src/target.rs",
            1,
            None,
            SearchRequest {
                pinned_file_paths: Some(vec!["src/noise.rs".into()]),
                file_preselect_limit: Some(1),
                include_grep: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(path.machine_pack["hits"][0]["file_path"], "src/target.rs");
    assert_eq!(lane(&path, "path")["candidates"][0]["exact_identity"], true);

    let scoped = index
        .search()
        .search_in_context_with(
            "src/target.rs",
            10,
            None,
            SearchRequest {
                path_prefix: Some("tests/".into()),
                include_grep: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(scoped.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["file_path"].as_str().unwrap().starts_with("tests/")));
    assert!(lanes(&scoped)
        .iter()
        .flat_map(|lane| lane["candidates"].as_array().unwrap())
        .all(|candidate| candidate["legacy_chunk_id"] != "chunk:src/target.rs:0"));

    let empty = index
        .search()
        .search_in_context_with(
            "name:definitely_absent definitely_absent",
            5,
            None,
            SearchRequest {
                include_grep: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(lane(&empty, "exact_symbol")["status"], "complete");
    assert_eq!(lane(&empty, "exact_symbol")["candidate_count"], 0);
    assert_eq!(lane(&empty, "graph")["status"], "disabled");

    if let Ok(output) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            Path::new(&output).join("p5a-retrieval.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "status": "passed",
                "scope": "real parser + SQLite + public context envelope; exact/path/local lane contract only, not holdout or tail-latency certification",
                "lane_ids": lanes(&exact).iter().map(|lane| lane["lane_id"].clone()).collect::<Vec<_>>(),
                "exact_top1": exact.machine_pack["hits"][0]["file_path"],
                "path_top1": path.machine_pack["hits"][0]["file_path"],
                "hard_scope_outside_hits": scoped.machine_pack["hits"].as_array().unwrap().iter().filter(|hit| !hit["file_path"].as_str().unwrap().starts_with("tests/")).count(),
                "versioned_candidate": exact_lane["candidates"][0],
                "disabled_status_distinct_from_complete_empty": true,
                "score_trace_replayed": true
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; real MCP P5-A lane contract"]
async fn real_mcp_exposes_versioned_exact_and_path_lanes() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let (temp, _) = setup();
    let root = temp.path();
    let binary = std::path::PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product binary"),
    );
    let mut client = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(40))
        .await
        .unwrap();
    client
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    let exact = client
        .call(
            "search",
            json!({
                "query":"sought_symbol",
                "mode":"hybrid",
                "top_k":1,
                "pinned_files":["src/noise.rs"],
                "file_preselect_limit":1,
                "project_path":root
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        exact["machine_pack"]["hits"][0]["file_path"],
        "src/target.rs"
    );
    let lanes = exact["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .expect("public lane receipts");
    assert_eq!(lanes.len(), 5);
    let exact_lane = lanes
        .iter()
        .find(|lane| lane["lane_id"] == "exact_symbol")
        .unwrap();
    assert_eq!(exact_lane["status"], "complete");
    assert_eq!(exact_lane["candidates"][0]["exact_identity"], true);
    assert!(exact_lane["candidates"][0]["document"]["doc_version"].is_string());

    let path = client
        .call(
            "search",
            json!({
                "query":"src/target.rs",
                "mode":"hybrid",
                "top_k":1,
                "pinned_files":["src/noise.rs"],
                "file_preselect_limit":1,
                "project_path":root
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        path["machine_pack"]["hits"][0]["file_path"],
        "src/target.rs"
    );
    let path_lane = path["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["lane_id"] == "path")
        .unwrap();
    assert_eq!(path_lane["candidates"][0]["exact_identity"], true);
    client.close().await.unwrap();

    if let Ok(output) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            Path::new(&output).join("p5a-real-mcp.json"),
            serde_json::to_vec_pretty(&json!({"exact":exact,"path":path})).unwrap(),
        )
        .unwrap();
    }
}
