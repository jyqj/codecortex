//! Independent end-to-end chunk policy contracts; never uses a daily index.
use cc_db::index_db::read_chunk_text_with_encoding;
use cc_server::engine::CodeIndex;
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn rows(root: &Path, path: &str) -> Vec<(String, u32, u32)> {
    let db = rusqlite::Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut q=db.prepare("SELECT text,text_encoding,start_line,end_line FROM chunks WHERE file_path=? ORDER BY chunk_index").unwrap();
    q.query_map([path], |r| {
        Ok((
            read_chunk_text_with_encoding(r, 0, 1)?,
            r.get(2)?,
            r.get(3)?,
        ))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}
fn policies(root: &Path) -> std::collections::BTreeMap<String, Option<String>> {
    let db = rusqlite::Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut q = db
        .prepare("SELECT file_path,chunk_policy FROM files")
        .unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
fn policy_config(root: &Path, lines: u32) {
    put(
        root,
        ".codecortex.json",
        &format!(
            r#"{{"auto_index":{{"enabled":false}},"indexing":{{"chunk_line_budget":{lines}}}}}"#
        ),
    );
}
#[test]
fn policy_change_live_session_and_failed_file_do_not_acknowledge_one_another() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    policy_config(root, 80);
    let source = format!(
        "export function policyMarker() {{\n{}return 1;\n}}\n",
        "  consume();\n".repeat(12)
    );
    for path in ["a.ts", "b.ts"] {
        put(root, path, &source);
    }
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let old = policies(root);
    policy_config(root, 3);
    std::fs::write(root.join("b.ts"), [0xff, 0xfe]).unwrap();
    let result = index.build_index(false).unwrap();
    assert!(!result.parse_errors.is_empty());
    assert_eq!(result.chunk_policy.lines, 3);
    let mixed = policies(root);
    assert_ne!(mixed["a.ts"], old["a.ts"]);
    assert_eq!(mixed["b.ts"], old["b.ts"]);
    assert!(rows(root, "a.ts").iter().all(|(_, a, z)| z - a < 3));
    put(root, "b.ts", &source);
    let fixed = index.build_index(false).unwrap();
    assert!(fixed.parse_errors.is_empty());
    assert_eq!(policies(root)["a.ts"], policies(root)["b.ts"]);
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
    index.close();
    let mut reopened = CodeIndex::new(Some(root)).unwrap();
    assert_eq!(reopened.build_index(false).unwrap().files_parsed, 0);
    let before = cc_eval::benchmark::oracle::canonical(root).unwrap();
    reopened.build_index(true).unwrap();
    assert_eq!(before, cc_eval::benchmark::oracle::canonical(root).unwrap());
}
#[test]
fn prepared_chunks_cannot_commit_under_a_different_policy() {
    use cc_index::Indexer;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    put(root, "a.ts", "export function original() {}\n");
    let db = Arc::new(
        cc_db::index_db::IndexDb::open(&root.join("index.db"))
            .unwrap()
            .0,
    );
    let original = Indexer::new(db.clone(), root, &IndexingConfig::default());
    original.build_index(root, true).unwrap();
    put(root, "a.ts", "export function changed() {}\n");
    let prepared = original.prepare_build(root, false, None).unwrap();
    let before = db.reads().get_file_state().unwrap();
    let changed = Indexer::new(
        db.clone(),
        root,
        &IndexingConfig {
            chunk_line_budget: 3,
            ..Default::default()
        },
    );
    assert!(
        changed.commit_build(root, false, None, prepared).is_err(),
        "a transport must not relabel chunks prepared with another policy"
    );
    assert_eq!(
        db.reads().get_file_state().unwrap()["a.ts"].content_hash,
        before["a.ts"].content_hash
    );
}

#[test]
fn invalid_budget_is_a_failed_build_not_empty_success_or_default_policy() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    policy_config(root, 80);
    put(root, "a.ts", "export function marker() {}\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let before = cc_eval::benchmark::oracle::canonical(root).unwrap();
    for n in [0, u32::MAX] {
        policy_config(root, n);
        assert!(index.build_index(false).is_err());
        assert_eq!(before, cc_eval::benchmark::oracle::canonical(root).unwrap());
    }
}
#[test]
fn changed_policy_expands_a_non_config_event_to_all_admitted_sources() {
    use cc_index::{BuildScope, Indexer};
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let source = format!(
        "export function marker() {{\n{}}}\n",
        "  consume();\n".repeat(12)
    );
    for p in ["a.ts", "b.ts"] {
        put(root, p, &source);
    }
    let db = Arc::new(
        cc_db::index_db::IndexDb::open(&root.join("index.db"))
            .unwrap()
            .0,
    );
    let first = Indexer::new(db.clone(), root, &IndexingConfig::default());
    first.build_index(root, true).unwrap();
    // Warm file-state cache before changing only the policy.
    db.reads().get_file_state().unwrap();
    let cfg = IndexingConfig {
        chunk_line_budget: 3,
        ..Default::default()
    };
    let next = Indexer::new(db.clone(), root, &cfg);
    let prepared = next
        .prepare_build_scoped(
            root,
            false,
            None,
            Some(&BuildScope {
                changed: vec!["a.ts".into()],
                removed: vec![],
            }),
        )
        .unwrap();
    let report = next.commit_build(root, false, None, prepared).unwrap();
    assert_eq!(report.files_parsed, 2);
    let state = db.reads().get_file_state().unwrap();
    let fp = cfg.chunk_policy().fingerprint();
    assert!(state
        .values()
        .all(|f| f.chunk_policy.as_deref() == Some(fp.as_str())));
    assert_eq!(next.build_index(root, false).unwrap().files_parsed, 0);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit product binary; actual MCP configuration update and restart"]
async fn real_mcp_chunk_budget_changes_without_restarting_then_survives_restart() {
    use cc_eval::benchmark::{
        adapters::{mcp_stdio::McpStdio, Backend},
        normalizer,
    };
    use serde_json::json;
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    policy_config(root, 80);
    put(
        root,
        "a.ts",
        &format!(
            "export function budgetMarker() {{\n{}return 1;\n}}\n",
            "  consume();\n".repeat(12)
        ),
    );
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let first = m
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    policy_config(root, 3);
    let changed = m
        .call("index", json!({"path":root,"full":false}))
        .await
        .unwrap();
    assert!(rows(root, "a.ts").iter().all(|(_, a, z)| z - a < 3));
    let response = m
        .call(
            "search",
            json!({"query":"budgetMarker","mode":"hybrid","top_k":10}),
        )
        .await
        .unwrap();
    let (mut hits, _) = normalizer::mcp(&response).unwrap();
    assert!(!hits.is_empty());
    for h in &mut hits {
        normalizer::verify_source(h, root).unwrap();
        assert_eq!(h.evidence_valid, Some(true));
    }
    let snapshot = policies(root);
    m.close().await.unwrap();
    let mut m = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(30))
        .await
        .unwrap();
    let reopened = m
        .call("index", json!({"path":root,"full":false}))
        .await
        .unwrap();
    assert_eq!(snapshot, policies(root));
    m.close().await.unwrap();
    if let Ok(path) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(Path::new(&path).join("p4b-mcp.json"),serde_json::to_vec_pretty(&json!({"first":first,"changed":changed,"reopened":reopened,"search":response,"verified_hits":hits.len()})).unwrap()).unwrap();
    }
}

#[test]
fn configured_line_budget_reaches_the_production_parser() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"chunk_line_budget":3}}"#,
    );
    let source = format!(
        "export function budgetMarker() {{\n{}return 1;\n}}\n",
        "  consume();\n".repeat(20)
    );
    put(root, "a.ts", &source);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let chunks = rows(root, "a.ts");
    assert_eq!(
        chunks.iter().map(|x| x.0.as_str()).collect::<String>(),
        source
    );
    assert!(
        chunks.iter().all(|(_, a, z)| z - a < 3),
        "configured line budget was ignored: {:?}",
        chunks.iter().map(|(_, a, z)| (a, z)).collect::<Vec<_>>()
    );
}
#[test]
fn configured_byte_budget_also_bounds_a_single_unicode_line() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"chunk_byte_budget":64}}"#,
    );
    let source = format!("value: {}\r\n", "甲😀".repeat(100));
    put(root, "a.yaml", &source);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let chunks = rows(root, "a.yaml");
    assert_eq!(
        chunks.iter().map(|x| x.0.as_str()).collect::<String>(),
        source
    );
    assert!(
        chunks.iter().all(|x| x.0.len() <= 64),
        "byte budget was ignored"
    );
}
