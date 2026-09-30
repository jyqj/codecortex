//! Independent authored document/storage/source consistency regressions.
use cc_server::engine::CodeIndex;
use rusqlite::Connection;
use std::path::Path;
fn put(root: &Path, path: &str, text: &str) {
    std::fs::write(root.join(path), text.as_bytes()).unwrap();
}
fn setup() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    put(
        t.path(),
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false}}"#,
    );
    t
}
fn records(root: &Path) -> Vec<cc_model::identity::DocumentRecord> {
    let c = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut s = c
        .prepare("SELECT record_json FROM document_manifest ORDER BY doc_key")
        .unwrap();
    s.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| serde_json::from_str(&r.unwrap()).unwrap())
        .collect()
}
#[test]
fn manifest_reopen_delete_rename_policy_and_full_incremental_parity() {
    let t = setup();
    let root = t.path();
    let source = "def sample():\r\n    return '甲'\r\n";
    put(root, "a.py", source);
    put(root, "b.py", source);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let old = records(root);
    assert!(old.len() >= 2);
    let keys: std::collections::BTreeSet<_> = old.iter().map(|r| &r.reference.doc_key).collect();
    assert_eq!(keys.len(), old.len());
    let first = cc_eval::benchmark::oracle::canonical(root).unwrap();
    index.close();
    let mut index = CodeIndex::new(Some(root)).unwrap();
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
    assert_eq!(cc_eval::benchmark::oracle::canonical(root).unwrap(), first);
    std::fs::rename(root.join("a.py"), root.join("renamed.py")).unwrap();
    std::fs::remove_file(root.join("b.py")).unwrap();
    index.build_index(false).unwrap();
    let after = records(root);
    assert!(!after.is_empty());
    assert!(after.iter().all(|r| r.file_path == "renamed.py"));
    for r in &old {
        assert!(
            !cc_db::document_store::is_current(index.index_db().unwrap(), &r.reference).unwrap()
        );
    }
    let incremental = cc_eval::benchmark::oracle::canonical(root).unwrap();
    index.build_index(true).unwrap();
    assert_eq!(
        cc_eval::benchmark::oracle::canonical(root).unwrap(),
        incremental
    );
    let db = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let orphans:i64=db.query_row("SELECT COUNT(*) FROM document_manifest d LEFT JOIN chunks c USING(chunk_id) WHERE c.chunk_id IS NULL",[],|r|r.get(0)).unwrap();
    assert_eq!(orphans, 0);
    let fts:i64=db.query_row("SELECT COUNT(*) FROM chunks_fts f JOIN chunks c ON c.rowid=f.rowid WHERE f.chunk_id<>c.chunk_id",[],|r|r.get(0)).unwrap();
    assert_eq!(fts, 0);
}
#[test]
fn changed_renderer_spec_reprocesses_unchanged_source() {
    let t = setup();
    put(t.path(), "a.py", "def marker():\n    return 1\n");
    let mut index = CodeIndex::new(Some(t.path())).unwrap();
    index.build_index(true).unwrap();
    index.close();
    {
        let c = Connection::open(t.path().join(".codecortex/index.sqlite3")).unwrap();
        c.execute("UPDATE files SET document_spec='old-spec'", [])
            .unwrap();
        c.execute("DELETE FROM metadata WHERE key LIKE '%aggregate%'", [])
            .unwrap();
    }
    let mut index = CodeIndex::new(Some(t.path())).unwrap();
    assert_eq!(index.build_index(false).unwrap().files_parsed, 1);
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
}
#[test]
fn failed_document_insert_rolls_back_files_chunks_and_fts() {
    let t = setup();
    let root = t.path();
    put(root, "a.py", "def sample():\n    return 'old'\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let before = cc_eval::benchmark::oracle::canonical(root).unwrap();
    let source = "def sample():\n    return 'changed'\n";
    let mut outcome = cc_parsers::ParserRegistry::new()
        .parse("a.py", source, cc_model::Language::Python)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    let snapshot = cc_model::source::SourceSnapshot::new(source.as_bytes());
    let mut batch = cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap();
    batch.records[0].reference.doc_version = "invalid".into();
    outcome.documents = Some(batch);
    let unit = cc_db::index_db::FileWriteUnit {
        rel_path: "a.py".into(),
        language: cc_model::Language::Python,
        content_hash: snapshot.identity().content_digest.clone(),
        mtime: 2.0,
        size: source.len() as u64,
        outcome,
    };
    assert!(index
        .index_db()
        .unwrap()
        .writes()
        .replace_files_batch(&[unit])
        .is_err());
    assert_eq!(cc_eval::benchmark::oracle::canonical(root).unwrap(), before);
}
#[test]
fn valid_bytes_reappear_after_stale_or_deleted_reads_without_poisoning_cache() {
    let t = setup();
    let root = t.path();
    let source = "def marker_unique():\n    return 'old'\n";
    put(root, "a.py", source);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    for changed in [
        "def marker_unique():\n    return 'new'\n",
        "# shift\nother=1\n",
    ] {
        put(root, "a.py", changed);
        let v = index
            .graph()
            .get_symbol_source("marker_unique", true, false, None)
            .unwrap();
        assert!(v["source"].is_null());
        let hits = index
            .search()
            .search_in_context("marker_unique", 5, None)
            .unwrap();
        assert!(hits.machine_pack["hits"].as_array().unwrap().is_empty());
    }
    put(root, "a.py", source);
    let result = index
        .search()
        .search_in_context("marker_unique", 5, None)
        .unwrap();
    assert!(!result.machine_pack["hits"].as_array().unwrap().is_empty());
    let current = index
        .graph()
        .get_symbol_source("marker_unique", true, false, None)
        .unwrap();
    assert_eq!(current["source_freshness"]["status"], "current_verified");
}
#[test]
fn empty_files_and_render_failures_have_explicit_manifest_states() {
    let t = setup();
    let root = t.path();
    put(root, "empty.py", "");
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"chunk_line_budget":1000,"chunk_byte_budget":65536,"chunk_char_budget":65536,"chunk_token_budget":16384}}"#,
    );
    let long = format!("value = '{}'\n", "x".repeat(40000));
    put(root, "large.py", &long);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    let report = index.build_index(true).unwrap();
    assert!(report.parse_errors.is_empty());
    assert!(report.document_changes.render_failed > 0);
    let docs = records(root);
    assert!(docs.iter().all(|d| d.file_path != "empty.py"));
    assert!(docs
        .iter()
        .any(|d| d.input.is_none() && d.render_error.is_some()));
    for doc in docs {
        doc.validate(&long[doc.source.span.start..doc.source.span.end])
            .unwrap();
    }
}
#[test]
fn direct_rebuild_and_standard_rebuild_produce_the_same_documents() {
    let t = setup();
    let root = t.path();
    put(root, "a.py", "def sample():\n    return 1\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let original = cc_eval::benchmark::oracle::canonical(root).unwrap();
    put(
        root,
        ".codecortex.json",
        r#"{"auto_index":{"enabled":false},"indexing":{"use_direct_writer":true}}"#,
    );
    index.build_index(true).unwrap();
    assert_eq!(
        cc_eval::benchmark::oracle::canonical(root).unwrap(),
        original
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit product binary; real MCP source/document lifecycle"]
async fn real_mcp_document_versions_and_disk_freshness() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    use serde_json::json;
    let t = setup();
    let root = t.path();
    let original = "def manifest_marker():\r\n    return 'old'\r\n";
    put(root, "a.py", original);
    let binary = std::path::PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").unwrap());
    let mut mcp = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(40))
        .await
        .unwrap();
    let indexed = mcp
        .call("index", json!({"path":root,"full":true}))
        .await
        .unwrap();
    let query = json!({"query":"manifest_marker","top_k":5,"project_path":root});
    let first = mcp.call("search", query.clone()).await.unwrap();
    let hits = first["machine_pack"]["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    let old = hits[0]["metadata"]["document"].clone();
    assert!(old["doc_version"].is_string());
    put(root, "a.py", "# shifted new code\nx=1\n");
    let stale = mcp.call("search", query.clone()).await.unwrap();
    assert!(stale["machine_pack"]["hits"].as_array().unwrap().is_empty());
    assert_eq!(
        stale["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    put(
        root,
        "a.py",
        "def manifest_marker():\r\n    return 'new'\r\n",
    );
    mcp.call("index", json!({"path":root,"full":false}))
        .await
        .unwrap();
    let newer = mcp.call("search", query.clone()).await.unwrap();
    let new = &newer["machine_pack"]["hits"][0]["metadata"]["document"];
    assert_ne!(new["doc_version"], old["doc_version"]);
    mcp.close().await.unwrap();
    let mut mcp = McpStdio::spawn(&binary, root, std::time::Duration::from_secs(40))
        .await
        .unwrap();
    let reopened = mcp.call("search", query.clone()).await.unwrap();
    assert_eq!(
        reopened["machine_pack"]["hits"][0]["metadata"]["document"],
        *new
    );
    std::fs::remove_file(root.join("a.py")).unwrap();
    let deleted = mcp.call("search", query).await.unwrap();
    assert!(deleted["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    mcp.call("index", json!({"path":root,"full":false}))
        .await
        .unwrap();
    assert!(records(root).is_empty());
    mcp.close().await.unwrap();
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join("p4c-mcp.json"),serde_json::to_vec_pretty(&json!({"index":indexed,"first":first,"stale":stale,"new":newer,"reopened":reopened,"deleted":deleted,"manifest_empty_after_index":true})).unwrap()).unwrap();
    }
}
#[test]
fn hydration_rejects_deleted_or_malformed_required_manifest() {
    let t = setup();
    let root = t.path();
    put(root, "a.py", "def marker():\n    return 42\n");
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let record = records(root).remove(0);
    let db = index.index_db().unwrap();
    let ids = [record.chunk_id.as_str()];
    assert_eq!(
        db.retrieval()
            .chunk_rows_by_ids(&ids, &Default::default())
            .unwrap()[0]
            .document
            .as_ref(),
        Some(&record.reference)
    );
    {
        let c = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
        c.execute("UPDATE document_manifest SET record_json='{}'", [])
            .unwrap();
    }
    assert!(db
        .retrieval()
        .chunk_rows_by_ids(&ids, &Default::default())
        .is_err());
    {
        let c = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
        c.execute("DELETE FROM document_manifest", []).unwrap();
    }
    assert!(db
        .retrieval()
        .chunk_rows_by_ids(&ids, &Default::default())
        .is_err());
}
#[test]
fn source_read_limits_and_path_guards_are_fail_closed() {
    let t = setup();
    let root = t.path();
    let content = "def marker():\n    return 42\n";
    put(root, "a.py", content);
    let mut index = CodeIndex::new(Some(root)).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap();
    let small = cc_search::evidence::read_verified(db, root, "a.py", 2).unwrap();
    assert!(!small.is_current());
    assert!(small.text.is_none());
    assert!(cc_search::evidence::read_verified(db, root, "../a.py", 100).is_err());
    #[cfg(unix)]
    {
        std::fs::remove_file(root.join("a.py")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        put(outside.path(), "secret.py", content);
        std::os::unix::fs::symlink(outside.path().join("secret.py"), root.join("a.py")).unwrap();
        let result = cc_search::evidence::read_verified(db, root, "a.py", 100).unwrap();
        assert!(!result.is_current());
        assert!(result.text.is_none());
    }
}
#[test]
fn context_keeps_flask_handler_after_freshness_metadata() {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_name() == ".codecortex" {
                continue;
            }
            let dst = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &dst)
            } else if e.file_type().unwrap().is_file() {
                std::fs::copy(e.path(), dst).unwrap();
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sample-project");
    copy(&fixture, temp.path());
    let backend = cc_eval::runner::CodeIndexBackend::new(temp.path()).unwrap();
    let value=backend.call_tool("context",&serde_json::json!({"task":"understand the Flask API routes and their handlers","max_symbols":10,"include_source":true})).unwrap();
    if let Ok(dir) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Path::new(&dir).join("flask-context.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    assert!(
        value.to_string().contains("api_get_user"),
        "original handler contract: {value}"
    );
}
#[test]
fn build_has_current_document_manifest_for_each_verified_chunk() {
    let t = setup();
    put(t.path(), "a.py", "def sample():\r\n    return '甲'\r\n");
    let mut index = CodeIndex::new(Some(t.path())).unwrap();
    index.build_index(true).unwrap();
    let db = Connection::open(t.path().join(".codecortex/index.sqlite3")).unwrap();
    let n: i64 = db
        .query_row("SELECT COUNT(*) FROM document_manifest", [], |r| r.get(0))
        .unwrap();
    let chunks: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE source_json IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n > 0);
    assert_eq!(n, chunks);
}
#[test]
fn old_symbol_lines_never_slice_new_disk_text() {
    let t = setup();
    put(t.path(), "a.py", "def sample():\n    return 'old'\n");
    let mut index = CodeIndex::new(Some(t.path())).unwrap();
    index.build_index(true).unwrap();
    put(t.path(), "a.py", "# unrelated new bytes\nsecret = 'new'\n");
    let value = index
        .graph()
        .get_symbol_source("sample", true, false, Some(400))
        .unwrap();
    assert!(
        value.get("source").is_none_or(|s| s.is_null()),
        "must not attribute new text to old symbol: {value}"
    );
    assert_eq!(
        value["source_freshness"]["status"],
        "stale_with_disk_change"
    );
}
#[test]
fn deleted_source_is_not_resurrected_by_warm_search_cache() {
    let t = setup();
    put(
        t.path(),
        "a.py",
        "def sample_unique_marker():\n    return 42\n",
    );
    let mut index = CodeIndex::new(Some(t.path())).unwrap();
    index.build_index(true).unwrap();
    let first = index
        .search()
        .search_in_context("sample_unique_marker", 5, None)
        .unwrap();
    assert!(!first.machine_pack["hits"].as_array().unwrap().is_empty());
    std::fs::remove_file(t.path().join("a.py")).unwrap();
    let next = index
        .search()
        .search_in_context("sample_unique_marker", 5, None)
        .unwrap();
    assert!(
        next.machine_pack["hits"].as_array().unwrap().is_empty(),
        "deleted source cannot be current evidence"
    );
    assert_eq!(next.evidence_summary["source_freshness"]["partial"], true);
}
