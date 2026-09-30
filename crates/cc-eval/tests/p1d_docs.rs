//! Extract documentation examples, rather than maintaining a second set of JSON.
use serde_json::Value;
use std::{path::Path, time::Duration};
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn examples() -> Vec<Value> {
    let doc = std::fs::read_to_string(root().join("docs/internals/SEARCH.md")).unwrap();
    let section = doc
        .split_once("<!-- p1-search-contract:start -->")
        .unwrap()
        .1
        .split_once("<!-- p1-search-contract:end -->")
        .unwrap()
        .0;
    let json = section
        .split_once("```json\n")
        .unwrap()
        .1
        .split_once("```")
        .unwrap()
        .0;
    serde_json::from_str(json).unwrap()
}
#[test]
fn search_examples_and_configuration_match_current_parameter_types() {
    for value in examples() {
        let mut p: cc_server::tools::SearchParams = serde_json::from_value(value).unwrap();
        p.sanitize().unwrap();
    }
    let text = std::fs::read_to_string(root().join("docs/CONFIGURATION.md")).unwrap();
    let value: Value = serde_json::from_str(
        text.split_once("```json\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0,
    )
    .unwrap();
    let _: cc_model::config::ProjectConfig = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        value["search"],
        serde_json::to_value(cc_model::config::SearchConfig::default()).unwrap()
    );
    assert!(!text.contains("base + 1 / (1 + |bm25|)"));
}
#[tokio::test]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; execute documented JSON over MCP"]
async fn p1d_documented_search_requests_respect_actual_scope_and_wire_shapes() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let d = tempfile::tempdir().unwrap();
    for folder in ["src", "excluded"] {
        std::fs::create_dir(d.path().join(folder)).unwrap();
        std::fs::write(
            d.path().join(folder).join("private.py"),
            "def needle():\n    return 1\n",
        )
        .unwrap();
    }
    std::fs::write(
        d.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let binary = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let mut c = McpStdio::spawn(Path::new(&binary), d.path(), Duration::from_secs(20))
        .await
        .unwrap();
    c.prepare(&[]).await.unwrap();
    for (i, args) in examples().into_iter().enumerate() {
        let value = c.call("search", args).await.unwrap();
        let hits = if i == 1 {
            value.as_array().unwrap()
        } else {
            value["machine_pack"]["hits"].as_array().unwrap()
        };
        if i == 2 {
            assert!(hits.is_empty());
        } else {
            assert!(!hits.is_empty());
            assert!(hits.iter().all(|h| h["file_path"] == "src/private.py"));
        }
        if i == 0 {
            assert!(value["evidence_summary"]["retrieval"]["cost"].is_object());
        }
    }
    c.close().await.unwrap();
}
