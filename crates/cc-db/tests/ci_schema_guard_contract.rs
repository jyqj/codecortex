//! CI declaration is backed by the committed v21 schema, not a fabricated
//! current-schema database relabelled as v21.
use cc_db::{
    index_db::IndexDb,
    index_migrate::{migrate_index_db, SchemaStatus},
};
use rusqlite::Connection;

const HISTORICAL_V21: &str = include_str!("fixtures/p7-ci-schema-v21.sql");
fn legacy_objects(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    conn.prepare("SELECT name,type,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn check_semantic_objects(conn: &Connection) {
    for name in [
        "semantic_manifest",
        "semantic_outbox",
        "semantic_spaces",
        "semantic_manifest_space",
        "semantic_manifest_file",
        "semantic_manifest_artifact",
        "semantic_outbox_ready",
        "semantic_outbox_doc",
        "semantic_outbox_live_per_doc",
    ] {
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1, "missing required v22 object: {name}");
    }
}

#[test]
fn fresh_index_really_initializes_declared_v22() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fresh.sqlite3");
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    assert_eq!(db.reads().schema_version().unwrap(), 22);
    let conn = Connection::open(&path).unwrap();
    check_semantic_objects(&conn);
    assert_ne!(db.reads().read_generation().unwrap().incarnation, [0; 16]);
}

#[test]
fn committed_v21_upgrade_preserves_legacy_objects_rows_fts_and_incarnation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(HISTORICAL_V21).unwrap();
    conn.pragma_update(None, "user_version", 21).unwrap();
    conn.execute_batch("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('sentinel.rs','rust','synthetic-legacy-hash',1,17,'2026-01-01'); INSERT INTO metadata(key,value) VALUES('index_incarnation','01010101010101010101010101010101'),('index_epoch','17'),('evidence_epoch','19');").unwrap();
    let before = legacy_objects(&conn);
    assert!(before
        .iter()
        .all(|(name, _, _)| name != "semantic_manifest"));
    drop(conn);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Migrated { from: 21 });
    assert_eq!(db.reads().schema_version().unwrap(), 22);
    let generation = db.reads().read_generation().unwrap();
    assert_eq!(generation.incarnation, [1; 16]);
    assert_eq!(
        (generation.index_epoch, generation.evidence_epoch),
        (17, 19)
    );
    let conn = Connection::open(&path).unwrap();
    let legacy_after: Vec<_> = legacy_objects(&conn)
        .into_iter()
        .filter(|(name, _, _)| before.iter().any(|(old, _, _)| old == name))
        .collect();
    assert_eq!(
        legacy_after, before,
        "additive migration must not replace legacy schema objects"
    );
    let hash: String = conn
        .query_row(
            "SELECT content_hash FROM files WHERE file_path='sentinel.rs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash, "synthetic-legacy-hash");
    let fts: String = conn
        .query_row(
            "SELECT file_path FROM file_paths_fts WHERE file_paths_fts MATCH 'sentinel'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts, "sentinel.rs");
    check_semantic_objects(&conn);
    drop(conn);
    drop(db);
    let (reopened, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_eq!(reopened.reads().read_generation().unwrap(), generation);
}

#[test]
fn nonadjacent_versions_still_require_rebuild() {
    for stored in [20, 23] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(HISTORICAL_V21).unwrap();
        conn.pragma_update(None, "user_version", stored).unwrap();
        assert_eq!(
            migrate_index_db(&conn).unwrap(),
            SchemaStatus::Mismatch { stored }
        );
    }
}
