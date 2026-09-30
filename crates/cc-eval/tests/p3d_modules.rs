//! Cross-language G3 truth assertions, independent of full/incremental parity.
#[path = "common/p3d_fixture.rs"]
mod fixture;
use cc_eval::benchmark::oracle;
use cc_server::engine::CodeIndex;
fn assert_calls(root: &std::path::Path, lookups: &[fixture::Lookup]) {
    let facts = oracle::canonical(root).unwrap();
    for lookup in lookups {
        assert!(
            facts["call_edges"].iter().any(
                |r| r["file_path"] == lookup.importer && r["target_file_path"] == lookup.target
            ),
            "{}: {} -> {}",
            lookup.language,
            lookup.importer,
            lookup.target
        );
    }
}
#[test]
fn all_four_languages_keep_independent_targets_on_full_noop_and_reopen() {
    let d = tempfile::tempdir().unwrap();
    let lookups = fixture::fixture(d.path(), 3);
    let mut index = CodeIndex::new(Some(d.path())).unwrap();
    index.build_index(true).unwrap();
    assert_calls(d.path(), &lookups);
    let before = oracle::canonical(d.path()).unwrap();
    index.build_index(false).unwrap();
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
    index.close();
    let mut reopened = CodeIndex::new(Some(d.path())).unwrap();
    reopened.build_index(false).unwrap();
    assert_calls(d.path(), &lookups);
    assert_eq!(before, oracle::canonical(d.path()).unwrap());
    // Syntax references and native package resolution share the same declared targets.
    for l in &lookups {
        assert!(!l.spec.is_empty());
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit product binary; real MCP stdio"]
async fn real_mcp_multilanguage_graph_keeps_concrete_target_files() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    let lookups = fixture::fixture(d.path(), 2);
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, d.path(), std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let report = m
        .call("index", json!({"path":d.path(),"full":true}))
        .await
        .unwrap();
    let mut evidence = vec![json!({"index":report})];
    for l in &lookups {
        let query=format!("MATCH (a:Symbol)-[:CALLS]->(b:Symbol) WHERE a.file_path = '{}' RETURN b.file_path AS target LIMIT 20",l.importer);
        let r = m.call("graph_query", json!({"query":query})).await.unwrap();
        assert!(
            r["results"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["target"] == l.target),
            "{}: {r}",
            l.importer
        );
        evidence.push(
            json!({"language":l.language,"importer":l.importer,"expected":l.target,"response":r}),
        );
    }
    m.close().await.unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("p3d-mcp.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
}
