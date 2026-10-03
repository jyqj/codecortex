//! P6-005 semantic persistence schema contracts (v22 additive tables).
//!
//! Covers: schema version assertion, old-database in-place upgrade through
//! `IndexDb::open` (data untouched), and positive/negative constraint tests
//! (PK / FK cascade / CHECK domains / partial unique outbox merging) for
//! `semantic_manifest`, `semantic_outbox` and `semantic_spaces`.
use cc_db::index_db::IndexDb;
use cc_db::index_migrate::{migrate_index_db, SchemaStatus, CURRENT_SCHEMA_VERSION};

/// Fresh in-memory database at the current schema, foreign keys enforced.
fn v22_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert_eq!(migrate_index_db(&conn).unwrap(), SchemaStatus::Initialized);
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

/// One published document: files -> chunks -> document_manifest fixture.
fn seed_document(conn: &rusqlite::Connection) {
    conn.execute_batch(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('a.rs','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c1','a.rs','rust',0,1,2,'body');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES('d1','v1','a.rs','c1',NULL,'{}','{}');",
    )
    .unwrap();
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn fresh_database_carries_semantic_tables_at_current_version() {
    let conn = v22_conn();
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    for object in [
        "semantic_manifest",
        "semantic_outbox",
        "semantic_spaces",
        "semantic_outbox_live_per_doc",
    ] {
        let found: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name=?1",
                [object],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(found, 1, "{object} missing from fresh schema");
    }
}

/// Historical v21 rows must be rebuilt under the corrected parser/resolver.
#[test]
fn v21_file_database_requires_reindex() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("fixtures/p7-ci-schema-v21.sql"))
        .unwrap();
    conn.pragma_update(None, "user_version", 21).unwrap();
    drop(conn);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    assert_eq!(db.reads().schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
    drop(db);
    let (_, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
}

#[test]
fn semantic_manifest_pk_fk_and_cascade() {
    let conn = v22_conn();
    seed_document(&conn);

    let published = "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
         input_digest,space_id,artifact_ref,published_at,published_incarnation) \
         VALUES('d1','v1','a.rs','ek','in','sp','ar','t','inc')";
    conn.execute_batch(published).unwrap();

    // PK: one visible row per doc_key.
    assert!(conn.execute_batch(published).is_err());

    // FK: unknown doc_key is rejected under foreign_keys=ON.
    let ghost = published.replace("'d1','v1'", "'ghost','v1'");
    assert!(conn.execute_batch(&ghost).is_err());

    // Deleting the document cascades the visible manifest row away.
    conn.execute("DELETE FROM document_manifest WHERE doc_key='d1'", [])
        .unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM semantic_manifest"), 0);
}

#[test]
fn semantic_outbox_op_and_state_domains_are_typed() {
    let conn = v22_conn();
    let base = "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
                available_at,created_at,updated_at) \
                VALUES('d1','v1','in','sp',";
    let tail = ",0.0,'t','t')";
    conn.execute_batch(&format!("{base}'embed','pending'{tail}"))
        .unwrap();

    // CHECK(op IN ('embed','revoke'))
    assert!(conn
        .execute_batch(&format!("{base}'purge','pending'{tail}"))
        .is_err());
    // CHECK(state IN ('pending','claimed','done','failed','superseded'))
    assert!(conn
        .execute_batch(&format!("{base}'embed','queued'{tail}"))
        .is_err());

    // done rows coexist with a fresh pending row for the same document.
    conn.execute_batch(&format!(
        "{base}'embed','done'{tail}" /* distinct task_id */
    ))
    .unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM semantic_outbox"), 2);
}

#[test]
fn semantic_outbox_at_most_one_live_task_per_doc_and_space() {
    let conn = v22_conn();
    let row = |doc: &str, space: &str, state: &str| {
        format!(
            "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
             available_at,created_at,updated_at) \
             VALUES('{doc}','v1','in','{space}','embed','{state}',0.0,'t','t');"
        )
    };
    conn.execute_batch(&row("d1", "sp", "pending")).unwrap();

    // Second live task (pending or claimed) for the same (doc, space) fails.
    assert!(conn.execute_batch(&row("d1", "sp", "pending")).is_err());
    assert!(conn.execute_batch(&row("d1", "sp", "claimed")).is_err());

    // Terminal states do not count as live.
    conn.execute_batch(&row("d1", "sp", "superseded")).unwrap();
    conn.execute_batch(&row("d1", "sp", "done")).unwrap();
    conn.execute_batch(&row("d1", "sp", "failed")).unwrap();

    // Same document in another space is independent.
    conn.execute_batch(&row("d1", "sp2", "pending")).unwrap();
    // Another document in the same space is independent.
    conn.execute_batch(&row("d2", "sp", "pending")).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM semantic_outbox WHERE state='pending'"
        ),
        3
    );
}

#[test]
fn semantic_spaces_state_domain_and_active_read_pattern() {
    let conn = v22_conn();
    let row = |id: &str, state: &str, activated: &str| {
        format!(
            "INSERT INTO semantic_spaces(space_id,spec_json,state,activated_at) \
             VALUES('{id}','{{}}','{state}',{activated});"
        )
    };
    conn.execute_batch(&row("s-back", "backfilling", "NULL"))
        .unwrap();
    conn.execute_batch(&row("s-active", "active", "'t'"))
        .unwrap();
    conn.execute_batch(&row("s-revoked", "revoked", "NULL"))
        .unwrap();

    // CHECK(state IN ('backfilling','active','revoked'))
    assert!(conn
        .execute_batch(&row("s-bad", "retired", "NULL"))
        .is_err());
    // PK: space_id unique.
    assert!(conn
        .execute_batch(&row("s-active", "active", "'t'"))
        .is_err());

    // The active-space read pattern is a plain single-row query; no
    // metadata-key duplication exists to drift against.
    let active: Vec<String> = conn
        .prepare("SELECT space_id FROM semantic_spaces WHERE state='active'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(active, vec!["s-active".to_string()]);
}
