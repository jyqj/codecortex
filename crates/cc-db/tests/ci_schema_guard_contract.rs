//! CI declaration is backed by the committed v21 schema, not a fabricated
//! current-schema database relabelled as v21.
use cc_db::{
    index_db::IndexDb,
    index_migrate::{migrate_index_db, SchemaStatus, CURRENT_SCHEMA_VERSION},
};
use rusqlite::Connection;

const HISTORICAL_V21: &str = include_str!("fixtures/p7-ci-schema-v21.sql");
// Byte-identical schema from v24 commit 88f2cf099c8b81f3acef485fd5ac9b01c63ce790.
// This predecessor has semantic tables but no source-bound parser identities.
const HISTORICAL_V24: &str = include_str!("fixtures/p7-ci-schema-v24.sql");
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
        "semantic_outbox_fifo_pending",
        "chunk_symbol_identity",
        "chunk_symbol_identity_path",
        "chunk_symbol_identity_doc_key",
        "chunk_symbol_identity_delete",
    ] {
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1, "missing required current-schema object: {name}");
    }
}

fn identity_cascade_plan(conn: &Connection) -> Vec<String> {
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.prepare("EXPLAIN QUERY PLAN DELETE FROM document_manifest WHERE chunk_id=?1")
        .unwrap()
        .query_map(["remove"], |row| row.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn assert_indexed_identity_cascade(conn: &Connection) {
    let plan = identity_cascade_plan(conn);
    assert!(
        plan.iter()
            .any(|step| step.contains("SEARCH chunk_symbol_identity")
                && step.contains("chunk_symbol_identity_doc_key")),
        "document deletion must use the child-key index: {plan:?}"
    );
    assert!(
        plan.iter()
            .all(|step| !step.contains("SCAN chunk_symbol_identity")),
        "document deletion must not scan all parser identities: {plan:?}"
    );
}

#[test]
fn fresh_index_really_initializes_current_schema() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("fresh.sqlite3");
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    assert_eq!(db.reads().schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let conn = Connection::open(&path).unwrap();
    check_semantic_objects(&conn);
    assert_indexed_identity_cascade(&conn);
    assert_ne!(db.reads().read_generation().unwrap().incarnation, [0; 16]);
}

/// Same semantic schema, missing both maintained physical access paths.
/// Normal writable open restores canonical definitions without replacing rows.
#[test]
fn current_schema_identity_index_preserves_rows_and_uses_indexed_fk_cascade() {
    fn snapshot(conn: &Connection) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        tables
            .into_iter()
            .map(|name| {
                let sql = format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""));
                let width = conn.prepare(&sql).unwrap().column_count();
                let order = (1..=width)
                    .map(|column| column.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let mut statement = conn.prepare(&format!("{sql} ORDER BY {order}")).unwrap();
                let rows = statement
                    .query_map([], |row| {
                        (0..width)
                            .map(|column| row.get(column))
                            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                (name, rows)
            })
            .collect()
    }

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("same-version.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    assert_eq!(migrate_index_db(&conn).unwrap(), SchemaStatus::Initialized);
    assert_indexed_identity_cascade(&conn);
    let schema_before = legacy_objects(&conn);
    // Schema fixtures only: these rows test preservation and FK behavior,
    // not the parser-authority validation of a SymbolIdentityRecord.
    conn.execute_batch(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('sentinel.rs','rust','hash',1,17,'fixture');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES
           ('keep','sentinel.rs','rust',0,1,1,'fn keep() {}'),
           ('remove','sentinel.rs','rust',1,2,2,'fn remove() {}');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,reference_json,record_json) VALUES
           ('doc-keep','v1','sentinel.rs','keep','{}','{}'),
           ('doc-remove','v1','sentinel.rs','remove','{}','{}');
         INSERT INTO chunk_symbol_identity(chunk_id,file_path,doc_key,doc_version,symbol_id,format_version,record_json) VALUES
           ('keep','sentinel.rs','doc-keep','v1','keep-symbol',1,'{}'),
           ('remove','sentinel.rs','doc-remove','v1','remove-symbol',1,'{}');
         INSERT INTO metadata(key,value) VALUES('index_epoch','17'),('evidence_epoch','23'),('semantic_epoch','31');
         DROP INDEX chunk_symbol_identity_doc_key;
         DROP INDEX semantic_outbox_fifo_pending;",
    )
    .unwrap();
    let before = snapshot(&conn);
    assert!(identity_cascade_plan(&conn)
        .iter()
        .any(|step| step.contains("SCAN chunk_symbol_identity")));
    // Missing physical indexes must not bypass the normal read-only refusal.
    conn.pragma_update(None, "query_only", true).unwrap();
    assert!(migrate_index_db(&conn).is_err());
    assert_eq!(snapshot(&conn), before);
    conn.pragma_update(None, "query_only", false).unwrap();
    drop(conn);

    for _ in 0..2 {
        let (db, status) = IndexDb::open(&path).unwrap();
        assert_eq!(status, SchemaStatus::UpToDate);
        assert_eq!(db.reads().schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
        drop(db);
        let conn = Connection::open(&path).unwrap();
        assert_eq!(snapshot(&conn), before);
        assert_eq!(legacy_objects(&conn), schema_before);
        assert_indexed_identity_cascade(&conn);
    }

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    conn.execute(
        "DELETE FROM document_manifest WHERE chunk_id=?1",
        ["remove"],
    )
    .unwrap();
    let identities: Vec<String> = conn
        .prepare("SELECT chunk_id FROM chunk_symbol_identity ORDER BY chunk_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(identities, ["keep"]);
    assert!(conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
fn committed_v21_upgrade_invalidates_legacy_rows_and_incarnation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(HISTORICAL_V21).unwrap();
    conn.pragma_update(None, "user_version", 21).unwrap();
    conn.execute_batch("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('sentinel.rs','rust','synthetic-legacy-hash',1,17,'2026-01-01'); INSERT INTO metadata(key,value) VALUES('index_incarnation','01010101010101010101010101010101'),('index_epoch','17'),('evidence_epoch','19');").unwrap();
    assert!(legacy_objects(&conn)
        .iter()
        .all(|(name, _, _)| name != "semantic_manifest"));
    drop(conn);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    assert_eq!(db.reads().schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let generation = db.reads().read_generation().unwrap();
    assert_ne!(generation.incarnation, [1; 16]);
    assert!(generation.index_epoch > 17 && generation.evidence_epoch > 19);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    check_semantic_objects(&conn);
    drop(conn);
    drop(db);
    let (reopened, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_eq!(reopened.reads().read_generation().unwrap(), generation);
}

#[test]
fn nonadjacent_versions_still_require_rebuild() {
    // The historical layout is only a mismatch fixture, never a current database.
    for stored in [20, 22, 23, CURRENT_SCHEMA_VERSION + 1] {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(HISTORICAL_V21).unwrap();
        conn.pragma_update(None, "user_version", stored).unwrap();
        assert_eq!(
            migrate_index_db(&conn).unwrap(),
            SchemaStatus::Mismatch { stored }
        );
    }
}

/// The real v24 predecessor cannot certify the new parser identity authority.
#[test]
fn committed_v24_requires_rebuild_and_invalidates_legacy_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("v24.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(HISTORICAL_V24).unwrap();
    conn.pragma_update(None, "user_version", 24).unwrap();
    conn.execute_batch("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('sentinel.rs','rust','synthetic-legacy-hash',1,17,'2026-01-01'); INSERT INTO metadata(key,value) VALUES('index_incarnation','01010101010101010101010101010101'),('index_epoch','17'),('evidence_epoch','19');").unwrap();
    let before = legacy_objects(&conn);
    assert!(before.iter().any(|(name, _, _)| name == "semantic_outbox"));
    assert!(before
        .iter()
        .all(|(name, _, _)| name != "chunk_symbol_identity"));
    assert_eq!(
        migrate_index_db(&conn).unwrap(),
        SchemaStatus::Mismatch { stored: 24 }
    );
    assert_eq!(legacy_objects(&conn), before);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        24
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);

    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    assert_eq!(db.reads().schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    let generation = db.reads().read_generation().unwrap();
    assert_ne!(generation.incarnation, [1; 16]);
    assert!(generation.index_epoch > 17 && generation.evidence_epoch > 19);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    check_semantic_objects(&conn);
    drop(conn);
    drop(db);
    let (reopened, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_eq!(reopened.reads().read_generation().unwrap(), generation);
}
