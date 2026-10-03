//! Independent public lifecycle and single-field contradiction review.
use cc_model::search::SearchRequest;
use cc_server::engine::CodeIndex;
use serde_json::Value;
const TEXT: &str = "# fresh independent fixture 雪\nclass Harbor:\n    def beacon(self):\n        return '灯'\n\nclass Ridge:\n    def beacon(self):\n        return '山'\n";
fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":2}}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("review.py"), TEXT).unwrap();
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    (dir, index)
}
fn public(index: &CodeIndex) -> Value {
    serde_json::to_value(
        index
            .search()
            .search_in_context("beacon", 10, None)
            .unwrap(),
    )
    .unwrap()
}
fn names(v: &Value) -> Vec<String> {
    let mut names = v["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["symbol_name"] == "beacon")
        .map(|h| h["metadata"]["qname"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}
fn cold(index: &CodeIndex) -> cc_model::CcResult<std::sync::Arc<[cc_model::SearchHit]>> {
    cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None)
        .search(&SearchRequest {
            query: "beacon".into(),
            top_k: 10,
            ..Default::default()
        })
}
#[test]
fn independent_cold_and_warm_single_field_contradictions() {
    for (label,sql) in [
        ("row-version","UPDATE chunk_symbol_identity SET doc_version='broken'"),
        ("source","UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.source.content_digest','broken')"),
        ("document","UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.document.doc_version','broken')"),
        ("tag","UPDATE chunk_symbol_identity SET record_json=json_set(record_json,'$.format_version',77)"),
        ("symbol","UPDATE symbols SET start_col=start_col+1 WHERE name='beacon'"),
        ("current-file","UPDATE files SET content_hash='broken'"),
        ("source-row","UPDATE chunks SET source_json=json_set(source_json,'$.owner.end',1) WHERE symbol_name='beacon'"),
    ] {
        let (_dir,index)=fixture();
        assert_eq!(names(&public(&index)),["Harbor.beacon","Ridge.beacon"]);
        assert_eq!(names(&public(&index)),["Harbor.beacon","Ridge.beacon"]);
        let db=index.index_db().unwrap();
        let generation=db.reads().read_generation().unwrap();
        let conn=rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.execute_batch(sql).unwrap();
        assert_eq!(db.reads().read_generation().unwrap(),generation,"test must hold cache key constant");
        assert!(cold(&index).is_err(),"cold accepted {label}");
        assert!(index.search().search_in_context("beacon",10,None).is_err(),"warm accepted {label}");
        println!("rejected cold+warm: {label}");
    }
}
#[test]
fn independent_warm_document_reference_mirror_must_fail_closed() {
    let (_dir, index) = fixture();
    let before = public(&index);
    assert_eq!(names(&before), ["Harbor.beacon", "Ridge.beacon"]);
    let db = index.index_db().unwrap();
    let generation = db.reads().read_generation().unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute("UPDATE document_manifest SET reference_json=json_set(reference_json,'$.doc_version','single-field-corruption')",[]).unwrap();
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    let rejected = cold(&index);
    assert!(
        rejected.is_err(),
        "cold unexpectedly accepted corrupt mirror"
    );
    println!("cold rejected corrupt mirror: {:?}", rejected.err());
    let warm = index.search().search_in_context("beacon", 10, None);
    if let Ok(ref result) = warm {
        println!(
            "COUNTEREXAMPLE warm accepted names={:?}",
            names(&serde_json::to_value(result).unwrap())
        );
    }
    assert!(
        warm.is_err(),
        "warm final hydration accepted contradictory document reference_json"
    );
}
#[test]
fn independent_missing_association_cold_omits_and_warm_rejects_cached_identity() {
    let (_dir, index) = fixture();
    assert_eq!(names(&public(&index)), ["Harbor.beacon", "Ridge.beacon"]);
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    conn.execute("DELETE FROM chunk_symbol_identity", [])
        .unwrap();
    let hits = cold(&index).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.metadata.get("qname").is_none()));
    assert!(index
        .search()
        .search_in_context("beacon", 10, None)
        .is_err());
}
#[test]
fn independent_reopen_rename_removal_and_cached_generation_rejection() {
    let (root, mut index) = fixture();
    let old = public(&index);
    assert_eq!(names(&old), ["Harbor.beacon", "Ridge.beacon"]);
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
    index.close();
    index.reopen().unwrap();
    assert_eq!(names(&public(&index)), ["Harbor.beacon", "Ridge.beacon"]);
    std::fs::write(
        root.path().join("review.py"),
        TEXT.replace("Harbor", "Port"),
    )
    .unwrap();
    index.build_index(false).unwrap();
    assert!(cc_search::evidence_hydrator::validate_envelope_generation(
        index.index_db().unwrap(),
        &old
    )
    .is_err());
    assert_eq!(names(&public(&index)), ["Port.beacon", "Ridge.beacon"]);
    std::fs::rename(root.path().join("review.py"), root.path().join("moved.py")).unwrap();
    index.build_index(false).unwrap();
    let renamed = public(&index);
    assert_eq!(names(&renamed), ["Port.beacon", "Ridge.beacon"]);
    assert!(renamed["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"] == "moved.py"));
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM chunk_symbol_identity WHERE file_path='review.py'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    std::fs::remove_file(root.path().join("moved.py")).unwrap();
    index.build_index(false).unwrap();
    assert!(public(&index)["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM chunk_symbol_identity", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn independent_generation_fence_discards_controlled_same_database_write() {
    let (_dir, index) = fixture();
    let db = index.index_db().unwrap();
    let engine = cc_search::SearchEngine::new(db.clone(), &Default::default(), None);
    let mut attempts = 0;
    let (accepted, names) = engine
        .with_stable_generation(|g| {
            attempts += 1;
            let hits = cold(&index)?;
            if attempts == 1 {
                let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
                conn.execute(
                    "UPDATE metadata SET value=CAST(value AS INTEGER)+1 WHERE key='index_epoch'",
                    [],
                )
                .unwrap();
            }
            Ok((
                g,
                hits.iter()
                    .filter_map(|h| h.metadata.get("qname").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ))
        })
        .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(accepted, names.0);
    assert_eq!(accepted, db.reads().read_generation().unwrap());
    assert!(names.1.iter().any(|n| n == "Harbor.beacon"));
}

#[test]
fn independent_base_v24_cache_rebuilds_25_reparses_and_reopen_is_unchanged() {
    assert_eq!(cc_model::project_model::PROJECT_MODEL_VERSION, 3);
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("review.py"), TEXT).unwrap();
    let path = cc_model::config::IndexPaths::new(root.path()).index_db;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        include_bytes!("../../../docs/reviews/qname-db-20261003/independent-v24.sqlite3"),
    )
    .unwrap();
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
    assert_eq!(index.build_index(false).unwrap().files_parsed, 1);
    assert_eq!(names(&public(&index)), ["Harbor.beacon", "Ridge.beacon"]);
    let generation = index.index_db().unwrap().reads().read_generation().unwrap();
    index.close();
    index.reopen().unwrap();
    assert_eq!(index.build_index(false).unwrap().files_parsed, 0);
    assert_eq!(
        generation,
        index.index_db().unwrap().reads().read_generation().unwrap()
    );
    assert_eq!(names(&public(&index)), ["Harbor.beacon", "Ridge.beacon"]);
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        25
    );
    println!("independently generated actual base v24 -> schema25; module model stays 3; unchanged reopen generation preserved");
}
