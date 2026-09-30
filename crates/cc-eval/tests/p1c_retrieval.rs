//! P1-C end-to-end cache and explanation contracts; no model calls.
use cc_model::{search::SearchRequest, Language};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"search":{"grep_scan_cap":1}}"#,
    );
    put(d.path(), "src/a.py", "def needle():\n    return 1\n");
    put(d.path(), "src/b.rs", "pub fn needle() -> i32 { 2 }\n");
    put(
        d.path(),
        "excluded/private-name.py",
        "def needle():\n    return 3\n",
    );
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    (d, i)
}
fn query(i: &CodeIndex, req: SearchRequest) -> Value {
    serde_json::to_value(
        i.search()
            .search_in_context_with("needle", 10, None, req)
            .unwrap(),
    )
    .unwrap()
}
fn request() -> SearchRequest {
    SearchRequest {
        path_prefix: Some("src".into()),
        languages: Some(vec![Language::Python]),
        pinned_file_paths: Some(vec!["excluded/private-name.py".into(), "src/a.py".into()]),
        ..Default::default()
    }
}
#[test]
fn p1c_scope_explanation_exists_on_cold_warm_and_empty_without_inventory_leak() {
    let (_d, i) = fixture();
    let cold = query(&i, request());
    let warm = query(&i, request());
    let explain = &cold["evidence_summary"]["retrieval"]["scope"];
    assert!(explain.is_object(), "missing scope explanation: {cold}");
    assert_eq!(explain, &warm["evidence_summary"]["retrieval"]["scope"]);
    assert_eq!(explain["hard"]["path_prefix"], "src");
    assert_eq!(explain["hard"]["languages"], json!(["python"]));
    assert_eq!(explain["soft"]["role"], "ranking_and_scan_priority_only");
    assert!(!explain.to_string().contains("private-name"));
    let empty = query(
        &i,
        SearchRequest {
            file_paths: Some(vec![]),
            ..request()
        },
    );
    assert_eq!(
        empty["evidence_summary"]["retrieval"]["scope"]["hard"]["empty"],
        true
    );
    assert_eq!(empty["machine_pack"]["hits"], json!([]));
    assert!(!empty["evidence_summary"]
        .to_string()
        .contains("private-name"));
}
#[test]
fn p1c_warm_scope_variants_and_trace_remain_correct() {
    let (_d, i) = fixture();
    for req in [
        request(),
        SearchRequest {
            file_paths: Some(vec![]),
            ..request()
        },
        SearchRequest {
            languages: Some(vec![Language::Rust]),
            ..request()
        },
        request(),
    ] {
        let v = query(&i, req.clone());
        let scope = cc_model::retrieval::HardScope::from(&req);
        for h in v["machine_pack"]["hits"].as_array().unwrap() {
            let path = h["file_path"].as_str().unwrap();
            let lang = Language::from_name(h["language"].as_str().unwrap());
            assert!(scope.passes(path, lang));
            let sum: f64 = h["score_trace"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c[1].as_f64().unwrap())
                .sum();
            assert!((sum - h["rerank_score"].as_f64().unwrap()).abs() < 1e-9);
        }
    }
}
#[test]
fn p1c_config_reload_recreates_engine_and_empty_budget_diagnostics() {
    let (d, mut i) = fixture();
    let before = query(&i, request());
    assert_eq!(
        before["evidence_summary"]["retrieval"]["scope"]["budget"]["grep_scan_cap"],
        1
    );
    put(
        d.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"search":{"grep_scan_cap":0}}"#,
    );
    i.set_project(d.path(), false).unwrap();
    let after = query(&i, request());
    assert_eq!(
        after["evidence_summary"]["retrieval"]["scope"]["budget"]["grep_scan_cap"],
        0
    );
    assert_eq!(after["evidence_summary"]["retrieval"]["grep"]["scanned"], 0);
    assert_eq!(
        after["evidence_summary"]["retrieval"]["grep"]["status"],
        "partial"
    );
    let warm = query(&i, request());
    assert_eq!(
        after["evidence_summary"]["retrieval"],
        warm["evidence_summary"]["retrieval"]
    );
}
#[test]
fn p1c_scope_explain_does_not_enumerate_explicit_file_set() {
    let (_d, i) = fixture();
    let v = query(
        &i,
        SearchRequest {
            file_paths: Some(vec!["src/a.py".into(), "excluded/private-name.py".into()]),
            ..request()
        },
    );
    let e = &v["evidence_summary"]["retrieval"]["scope"];
    assert_eq!(e["hard"]["explicit_file_count"], 1);
    assert!(!e.to_string().contains("private-name"));
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY; explicit old/new product"]
async fn p1c_stdio_scope_explain_survives_cache_and_redacts_hints() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use std::time::Duration;
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let (d, _i) = fixture();
    let mut c = McpStdio::spawn(Path::new(&bin), d.path(), Duration::from_secs(30))
        .await
        .unwrap();
    let args = json!({"query":"needle","path_prefix":"src","pinned_files":["excluded/private-name.py","src/a.py"],"top_k":10});
    let a = c.call("search", args.clone()).await.unwrap();
    let b = c.call("search", args).await.unwrap();
    c.close().await.unwrap();
    assert!(a["evidence_summary"]["retrieval"]["scope"].is_object());
    assert_eq!(
        a["evidence_summary"]["retrieval"],
        b["evidence_summary"]["retrieval"]
    );
    assert!(!a["evidence_summary"]["retrieval"]
        .to_string()
        .contains("private-name"));
}
