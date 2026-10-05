//! Real file builds, public hydration and controlled corruption in owned databases.
use cc_model::{query::RetrievalStrategy, search::SearchRequest};
use cc_server::engine::CodeIndex;
use serde_json::Value;

const SOURCE: &str = "class Alpha:\n    def needle(self):\n        return '中文'\nclass Beta:\n    @staticmethod\n    def needle():\n        return 'β'\n";
fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.py"), SOURCE).unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    (dir, index)
}
fn public(index: &CodeIndex) -> Value {
    serde_json::to_value(
        index
            .search()
            .search_in_context("needle", 10, None)
            .unwrap(),
    )
    .unwrap()
}
fn names(value: &Value) -> Vec<String> {
    let mut result: Vec<_> = value["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["symbol_name"] == "needle")
        .map(|h| h["metadata"]["qname"].as_str().unwrap().to_owned())
        .collect();
    result.sort();
    result
}

#[test]
fn original_snapshot_disk_rejection_incremental_rename_delete_and_reopen() {
    let (root, mut index) = fixture();
    assert_eq!(names(&public(&index)), ["Alpha.needle", "Beta.needle"]);
    let before = index.index_db().unwrap().reads().read_generation().unwrap();
    let report = index.build_index(false).unwrap();
    assert_eq!(report.files_parsed, 0);
    assert_eq!(names(&public(&index)), ["Alpha.needle", "Beta.needle"]);
    index.close();
    index.reopen().unwrap();
    assert_eq!(names(&public(&index)), ["Alpha.needle", "Beta.needle"]);
    let old = public(&index);
    std::fs::write(
        root.path().join("a.py"),
        SOURCE.replace("Alpha", "RenamedAlpha"),
    )
    .unwrap();
    // Indexed qnames still describe the old snapshot; public disk validation rejects it.
    let low =
        cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let indexed = low
        .search(&SearchRequest {
            query: "needle".into(),
            top_k: 10,
            ..Default::default()
        })
        .unwrap();
    assert!(indexed
        .iter()
        .any(|h| h.metadata["qname"] == "Alpha.needle"));
    let rejected = public(&index);
    assert!(rejected["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        rejected["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    index.build_index(false).unwrap();
    assert_eq!(
        names(&public(&index)),
        ["Beta.needle", "RenamedAlpha.needle"]
    );
    assert!(cc_search::evidence_hydrator::validate_envelope_generation(
        index.index_db().unwrap(),
        &old
    )
    .is_err());
    assert!(
        index
            .index_db()
            .unwrap()
            .reads()
            .read_generation()
            .unwrap()
            .index_epoch
            > before.index_epoch
    );
    std::fs::rename(root.path().join("a.py"), root.path().join("renamed.py")).unwrap();
    index.build_index(false).unwrap();
    let renamed = public(&index);
    assert_eq!(names(&renamed), ["Beta.needle", "RenamedAlpha.needle"]);
    assert!(renamed["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"] == "renamed.py"));
    std::fs::remove_file(root.path().join("renamed.py")).unwrap();
    index.build_index(false).unwrap();
    assert!(public(&index)["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM chunk_symbol_identity", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn persisted_identity_corruption_fails_cold_and_warm_final_hydration() {
    for sql in [
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.qname','Beta.needle') WHERE json_extract(record_json,'$.qname')='Alpha.needle'",
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.format_version',99)",
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.owner.start',1)",
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.source.snapshot_id','wrong')",
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.symbol_uid','wrong')",
        "UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.document.doc_version','wrong')",
        "UPDATE chunk_symbol_identity SET doc_version='wrong'",
        "UPDATE chunk_symbol_identity SET file_path='other.py'",
        "UPDATE symbols SET qname='wrong' WHERE name='needle'",
        "UPDATE chunks SET symbol_name='Beta' WHERE symbol_name='needle'",
        "UPDATE chunks SET source_json=json_set(source_json,'$.owner.start',1) WHERE symbol_name='needle'",
    ] {
        let (_root,index) = fixture();
        assert_eq!(names(&public(&index)), ["Alpha.needle","Beta.needle"]);
        // Warm the final public window before mutation, without bumping its epoch.
        assert_eq!(names(&public(&index)), ["Alpha.needle","Beta.needle"]);
        let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
        conn.execute_batch(sql).unwrap();
        assert!(index.search().search_in_context("needle",10,None).is_err(),"accepted warm corrupt identity: {sql}");
        let cold = cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(),None);
        assert!(cold.search(&SearchRequest {query:"needle".into(),top_k:10,..Default::default()}).is_err(),"accepted cold corrupt identity: {sql}");
    }
}

#[test]
fn missing_association_omits_identity_and_hard_scope_never_crosses_file() {
    let (root, mut index) = fixture();
    std::fs::write(root.path().join("b.py"), SOURCE).unwrap();
    index.build_index(false).unwrap();
    let value = index
        .search()
        .search_in_context_with(
            "needle",
            10,
            None,
            SearchRequest {
                file_paths: Some(vec!["a.py".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(value.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"] == "a.py"));
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    conn.execute(
        "DELETE FROM chunk_symbol_identity WHERE file_path='a.py'",
        [],
    )
    .unwrap();
    // A fresh reader does not substitute a symbol-name lookup for missing authority.
    let cold =
        cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let hits = cold
        .search(&SearchRequest {
            query: "needle".into(),
            top_k: 10,
            file_paths: Some(vec!["a.py".into()]),
            ..Default::default()
        })
        .unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.metadata.get("qname").is_none()));
}

#[tokio::test]
async fn pinned_handle_retries_current_generation_after_real_incremental_publication() {
    let (root, mut index) = fixture();
    let handle = index.query_handle().unwrap();
    std::fs::write(
        root.path().join("a.py"),
        SOURCE.replace("Alpha", "NewAlpha"),
    )
    .unwrap();
    index.build_index(false).unwrap();
    let value = handle
        .search_async(
            "needle".into(),
            10,
            None,
            SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Local),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        names(&serde_json::to_value(value).unwrap()),
        ["Beta.needle", "NewAlpha.needle"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_incremental_reads_never_attach_a_new_qname_to_an_old_source_snapshot() {
    let (root, mut index) = fixture();
    let handle = index.query_handle().unwrap();
    let samples: Vec<_> = (0..5)
        .map(|i| {
            let name = if i == 0 {
                "Alpha".to_owned()
            } else {
                format!("Generation{i}Alpha")
            };
            let text = SOURCE.replace("Alpha", &name);
            let identity = cc_model::source::SourceSnapshot::new(text.as_bytes())
                .identity()
                .clone();
            (name, text, identity)
        })
        .collect();
    let expected = samples.clone();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let reader_barrier = barrier.clone();
    let reader = tokio::spawn(async move {
        reader_barrier.wait().await;
        let mut accepted = 0;
        for _ in 0..16 {
            match handle
                .search_async(
                    "needle".into(),
                    10,
                    None,
                    SearchRequest {
                        retrieval_strategy: Some(RetrievalStrategy::Local),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(envelope) => {
                    for hit in envelope.machine_pack["hits"].as_array().unwrap() {
                        if hit["symbol_name"] != "needle" {
                            continue;
                        }
                        let proof: cc_model::source::ChunkSource =
                            serde_json::from_value(hit["metadata"]["source_evidence"].clone())
                                .unwrap();
                        let sample = expected
                            .iter()
                            .find(|(_, _, s)| *s == proof.source)
                            .expect("unknown/mixed source identity");
                        let qname = hit["metadata"]["qname"].as_str().unwrap();
                        assert!(
                            qname == "Beta.needle" || qname == format!("{}.needle", sample.0),
                            "qname from another source generation"
                        );
                        accepted += 1;
                    }
                }
                Err(cc_model::CcError::RetrievalChanged { .. }) => {}
                Err(error) => panic!("unexpected concurrent read error: {error}"),
            }
        }
        accepted
    });
    barrier.wait().await;
    for (_, text, _) in samples.iter().skip(1) {
        std::fs::write(root.path().join("a.py"), text).unwrap();
        index.build_index(false).unwrap();
    }
    let accepted = reader.await.unwrap();
    assert!(accepted > 0);
    assert_eq!(
        names(&public(&index)),
        ["Beta.needle", "Generation4Alpha.needle"]
    );
}

#[test]
fn owned_v24_cache_rebuilds_and_unchanged_source_is_reparsed_at_v25() {
    let root = tempfile::tempdir().unwrap();
    let source = "class Alpha:\n    def needle(self):\n        return '中文'\nclass Beta:\n    def needle(self):\n        return 'β'\n";
    std::fs::write(root.path().join("a.py"), source).unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    let path = cc_model::config::IndexPaths::new(root.path()).index_db;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Actual persisted v24 real-parser cache from the captured baseline run,
    // copied only into this test's uniquely owned project/cache namespace.
    std::fs::write(&path,include_bytes!("../../../artifacts/checkpoints/qname-source-identity-implementation-20261003/v24-original-fixture.sqlite3")).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        24
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM symbols", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='chunk_symbol_identity'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(conn);
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    assert!(index.needs_initial_index());
    let report = index.build_index(false).unwrap();
    assert_eq!(report.files_parsed, 1);
    assert_eq!(names(&public(&index)), ["Alpha.needle", "Beta.needle"]);
    index.close();
    index.reopen().unwrap();
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
    assert_eq!(names(&public(&index)), ["Alpha.needle", "Beta.needle"]);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        cc_db::index_migrate::CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn public_python_nested_decorated_split_methods_and_go_receiver_methods_have_exact_owners() {
    let (root, mut index) = fixture();
    let mut python = String::from("class Alpha:\n    @decorate\n    async def needle(self):\n        def inner():\n            return '中文'\n        value = 0\n");
    for i in 0..220 {
        python.push_str(&format!("        value += {i}\n"));
    }
    python
        .push_str("        return value\nclass Beta:\n    def needle(self):\n        return 'β'\n");
    let mut go = String::from("package sample\ntype Alpha struct {}\ntype Beta struct {}\nfunc (a Alpha) needle() int {\n    value := 0\n");
    for i in 0..220 {
        go.push_str(&format!("    value += {i}\n"));
    }
    go.push_str("    return value\n}\nfunc (b Beta) needle() int { return 2 }\n");
    std::fs::write(root.path().join("a.py"), &python).unwrap();
    std::fs::write(root.path().join("a.go"), &go).unwrap();
    index.build_index(false).unwrap();
    for (file, text) in [("a.py", &python), ("a.go", &go)] {
        let snapshot = cc_model::source::SourceSnapshot::new(text.as_bytes());
        let envelope = index
            .search()
            .search_in_context_with(
                "needle",
                20,
                None,
                SearchRequest {
                    file_paths: Some(vec![file.into()]),
                    ..Default::default()
                },
            )
            .unwrap();
        let hits = envelope.machine_pack["hits"].as_array().unwrap();
        let mut alpha = 0;
        let mut beta = 0;
        for hit in hits.iter().filter(|h| h["symbol_name"] == "needle") {
            let proof: cc_model::source::ChunkSource =
                serde_json::from_value(hit["metadata"]["source_evidence"].clone()).unwrap();
            assert_eq!(proof.source, *snapshot.identity());
            assert_eq!(
                snapshot.slice(proof.span).unwrap(),
                hit["text"].as_str().unwrap()
            );
            let declaration = snapshot.slice(proof.owner.unwrap()).unwrap();
            match hit["metadata"]["qname"].as_str().unwrap() {
                "Alpha.needle" => {
                    alpha += 1;
                    assert!(declaration.contains("value += 219"));
                }
                "Beta.needle" => {
                    beta += 1;
                    assert!(!declaration.contains("value += 219"));
                }
                other => panic!("neighbor/class/local identity on method body: {other}"),
            }
        }
        assert!(alpha > 1, "real split method not represented in {file}");
        assert_eq!(beta, 1);
    }
    let nested = index
        .search()
        .search_in_context_with(
            "inner",
            10,
            None,
            SearchRequest {
                file_paths: Some(vec!["a.py".into()]),
                ..Default::default()
            },
        )
        .unwrap();
    let locals: Vec<_> = nested.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["symbol_name"] == "inner")
        .collect();
    assert_eq!(locals.len(), 1);
    assert_eq!(locals[0]["metadata"]["qname"], "Alpha.needle.inner");
    assert_eq!(locals[0]["symbol_kind"], "function");
}
