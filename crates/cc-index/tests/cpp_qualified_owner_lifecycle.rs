//! Owned synthetic B1 persistence; no user cache, server, or C++ execution.
use cc_db::{
    index_db::{FileWriteUnit, IndexDb},
    index_migrate::SchemaStatus,
};
use cc_index::Indexer;
use cc_model::{config::IndexingConfig, source::SourceSnapshot, Language};
use cc_parsers::ParserRegistry;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

const V26_DB: &[u8] = include_bytes!("fixtures/cpp-qualified-v26/index-v26.dbfixture");
const V26_SOURCES: &str = include_str!("fixtures/cpp-qualified-v26/source-manifest.json");
const DELTAS: &str = include_str!("fixtures/cpp-qualified-v26/expected-b1-deltas.json");
const GENERATION: &str = include_str!("fixtures/cpp-qualified-v26/generation-v26.json");

fn config() -> IndexingConfig {
    IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    }
}
fn metadata(root: &Path) -> Vec<(String, Vec<u8>, u128)> {
    let sources: Vec<Value> = serde_json::from_str(V26_SOURCES).unwrap();
    sources
        .iter()
        .map(|s| {
            let name = s["file_path"].as_str().unwrap();
            let path = root.join(name);
            (
                name.into(),
                std::fs::read(&path).unwrap(),
                std::fs::metadata(path)
                    .unwrap()
                    .modified()
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            )
        })
        .collect()
}
fn copy_v26(root: &Path) -> PathBuf {
    let sources: Vec<Value> = serde_json::from_str(V26_SOURCES).unwrap();
    for source in sources {
        let path = root.join(source["file_path"].as_str().unwrap());
        let text = source["source"].as_str().unwrap();
        std::fs::write(&path, text).unwrap();
        let time = UNIX_EPOCH
            + Duration::new(
                source["mtime_seconds"].as_u64().unwrap(),
                source["mtime_nanos"].as_u64().unwrap() as u32,
            );
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(time))
            .unwrap();
    }
    let path = root.join(".codecortex/index.sqlite3");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, V26_DB).unwrap();
    path
}
fn assert_rows(db: &IndexDb, path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let deltas: Vec<Value> = serde_json::from_str(DELTAS).unwrap();
    for delta in deltas {
        let id = delta["old_symbol_id"].as_str().unwrap();
        let expected_qname = delta["expected_qname"].as_str();
        let row: (String, Option<String>, Option<String>, String) = conn
            .query_row(
                "SELECT kind,qname,symbol_uid,cpp_qualified_owner FROM symbols WHERE symbol_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.1.as_deref(), expected_qname, "{id}");
        let associations: i64 = conn
            .query_row(
                "SELECT count(*) FROM chunk_symbol_identity WHERE symbol_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        if let Some(qname) = expected_qname {
            assert_eq!(row.0, delta["expected_kind"].as_str().unwrap());
            assert!(row.2.is_some());
            assert!(row.3 == "proven_namespace" || row.3 == "proven_type");
            assert!(
                associations > 0,
                "missing source-bound public identity: {qname}"
            );
            let mut query = conn
                .prepare("SELECT chunk_id FROM chunk_symbol_identity WHERE symbol_id=?1")
                .unwrap();
            let chunks: Vec<String> = query
                .query_map([id], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            let ids: Vec<_> = chunks.iter().map(String::as_str).collect();
            let public = db
                .retrieval()
                .chunk_rows_by_ids(&ids, &Default::default())
                .unwrap();
            assert!(!public.is_empty());
            assert!(public.iter().all(|r| r.qname.as_deref() == Some(qname)));
        } else {
            assert!(row.2.is_none());
            assert_eq!(row.3, "unproven");
            assert_eq!(associations, 0);
        }
        let old = delta["old_uid"].as_str().unwrap();
        let stale: i64 = conn
            .query_row(
                "SELECT count(*) FROM symbols WHERE symbol_uid=?1",
                [old],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0);
        let stale:i64=conn.query_row("SELECT count(*) FROM chunk_symbol_identity WHERE json_extract(record_json,'$.symbol_uid')=?1",[old],|r|r.get(0)).unwrap();
        assert_eq!(stale, 0);
    }
    for name in ["duplicate", "twin"] {
        let count: i64 = conn
            .query_row(
                "SELECT count(DISTINCT symbol_uid) FROM symbols WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
    let bad:i64=conn.query_row("SELECT count(*) FROM call_edges WHERE callee_symbol IN ('leaf','orphan') AND (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR callee_symbol_uid IS NOT NULL OR resolution_strategy!='cpp_qualified_owner_unproven')",[],|r|r.get(0)).unwrap();
    assert_eq!(bad, 0);
    let bad:i64=conn.query_row("SELECT count(*) FROM symbol_refs WHERE symbol_name IN ('leaf','orphan') AND (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR target_symbol_uid IS NOT NULL OR resolution_strategy!='cpp_qualified_owner_unproven')",[],|r|r.get(0)).unwrap();
    assert_eq!(bad, 0);
    let bad:i64=conn.query_row("SELECT count(*) FROM semantic_edges e JOIN symbols s ON s.symbol_uid=e.target_symbol_uid WHERE s.cpp_qualified_owner!='non_b1' AND e.relation_kind IN ('defines','defines_method')",[],|r|r.get(0)).unwrap();
    assert_eq!(bad, 0);
}

#[test]
fn actual_owned_v26_rebuilds_unchanged_source_then_reopen_rename_delete() {
    let root = tempfile::tempdir().unwrap();
    let path = copy_v26(root.path());
    let before = metadata(root.path());
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            26
        );
        let columns:i64=conn.query_row("SELECT count(*) FROM pragma_table_info('symbols') WHERE name='cpp_qualified_owner'",[],|r|r.get(0)).unwrap();
        assert_eq!(columns, 0, "must be real unchanged v26 schema");
        let rows: i64 = conn
            .query_row(
                "SELECT count(*) FROM symbols WHERE name IN ('duplicate','twin')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 2, "old parser UID collisions really survived SQL");
    }
    let old: cc_model::generation::ReadGeneration = serde_json::from_str(GENERATION).unwrap();
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(
        status,
        SchemaStatus::Initialized,
        "B1 must invalidate a real v26 cache, even with unchanged source"
    );
    assert_eq!(
        db.reads().schema_version().unwrap(),
        cc_db::index_migrate::CURRENT_SCHEMA_VERSION
    );
    let opened = db.reads().read_generation().unwrap();
    assert_ne!(opened.incarnation, old.incarnation);
    assert!(opened.index_epoch > old.index_epoch);
    assert!(opened.evidence_epoch > old.evidence_epoch);
    let db = Arc::new(db);
    let cfg = config();
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        4
    );
    assert_eq!(metadata(root.path()), before);
    assert_rows(&db, &path);
    let generation = db.reads().read_generation().unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    assert_rows(&db, &path);
    drop(indexer);
    drop(db);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    let db = Arc::new(db);
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(metadata(root.path()), before);
    assert_rows(&db, &path);
    std::fs::rename(
        root.path().join("namespace_owner.cpp"),
        root.path().join("renamed.cpp"),
    )
    .unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let old_rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM symbols WHERE file_path='namespace_owner.cpp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_rows, 0);
    let renamed:i64=conn.query_row("SELECT count(*) FROM symbols WHERE file_path='renamed.cpp' AND qname='grove::leaf' AND cpp_qualified_owner='proven_namespace'",[],|r|r.get(0)).unwrap();
    assert_eq!(renamed, 1);
    drop(conn);
    std::fs::remove_file(root.path().join("renamed.cpp")).unwrap();
    indexer.build_index(root.path(), false).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let left: i64 = conn
        .query_row(
            "SELECT count(*) FROM symbols WHERE file_path='renamed.cpp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0);
}

fn unit(source: &str) -> FileWriteUnit {
    let snapshot = SourceSnapshot::new(source.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse("owner.cpp", source, Language::Cpp)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents =
        Some(cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&snapshot, &outcome).unwrap();
    FileWriteUnit {
        rel_path: "owner.cpp".into(),
        language: Language::Cpp,
        content_hash: snapshot.identity().content_digest.clone(),
        mtime: 0.0,
        size: source.len() as u64,
        outcome,
    }
}
#[test]
fn b1_identity_requires_unique_current_proof_and_writer_rechecks_it() {
    let source="namespace grove { int leaf(); } int grove::leaf() { return 1; } int Unknown::member() { return 2; }";
    let data = unit(source);
    assert!(data
        .outcome
        .symbol_identities
        .iter()
        .any(|i| i.qname == "grove::leaf"));
    assert!(!data
        .outcome
        .symbol_identities
        .iter()
        .any(|i| i.name == "member"));
    let mut downgraded = unit(source);
    downgraded
        .outcome
        .symbols
        .iter_mut()
        .find(|s| s.name == "leaf")
        .unwrap()
        .cpp_qualified_owner = Default::default();
    assert!(!cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(source.as_bytes()),
        &downgraded.outcome
    )
    .unwrap()
    .iter()
    .any(|i| i.name == "leaf"));
    let mut missing = unit(source);
    missing.outcome.cpp_qualified_owner_proofs.clear();
    let ids = cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(source.as_bytes()),
        &missing.outcome,
    )
    .unwrap();
    assert!(!ids.iter().any(|i| i.name == "leaf"));
    let root = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
    db.writes().replace_files_batch(&[data]).unwrap();
    let before = db.reads().read_generation().unwrap();
    assert!(db.writes().replace_files_batch(&[missing]).is_err());
    assert_eq!(db.reads().read_generation().unwrap(), before);
    let mut duplicate = unit(source);
    duplicate
        .outcome
        .cpp_qualified_owner_proofs
        .push(duplicate.outcome.cpp_qualified_owner_proofs[0].clone());
    assert!(!cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(source.as_bytes()),
        &duplicate.outcome
    )
    .unwrap()
    .iter()
    .any(|i| i.name == "leaf"));
}
