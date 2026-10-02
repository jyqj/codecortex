//! P6-012: semantic coverage reads and the epoch-consistent read discipline.
//!
//! Facade-level invariants fixed here (module docs of
//! `cc_db::semantic_coverage` / TASK-BRIEFS P6-012 / ADR-0003 row P6-012):
//!
//! 1. the consistent-read facade pairs every coverage reading with the
//!    strict `ReadGeneration` it was computed under (before/after compare on
//!    one pooled connection) — `semantic_epoch == None` ("semantic not
//!    ready") is never folded to 0;
//! 2. an unwired database (no active space) reports honest zeros with the
//!    `SemanticNotConfigured` reason even when embeddable documents exist —
//!    no inflated denominator;
//! 3. coverage numbers agree with the P6-011 publish CAS outcomes: a real
//!    publication flips published/uncovered and the snapshot's epoch matches
//!    a fresh strict read.
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_coverage::{coverage_on, SemanticCoverage, ZeroEligibleReason};
use cc_db::semantic_outbox::{claim_next_on, supersede_and_enqueue_on, OutboxPlan, OutboxUpsert};
use cc_db::semantic_publish::PublishRequest;

fn seed_conn(db: &IndexDb) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}

fn seed_document(conn: &rusqlite::Connection, input_hash: &str) {
    conn.execute_batch(&format!(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
           VALUES('src/d1.rs','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
           VALUES('c-d1','src/d1.rs','rust',0,1,2,'body');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
           VALUES('d1','v1','src/d1.rs','c-d1','enc','{{}}','{{\"input\":{{\"input_hash\":\"{input_hash}\"}}}}');"
    ))
    .unwrap();
}

fn enqueue(conn: &rusqlite::Connection, input_digest: &str) {
    let stats = supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d1".into(),
                doc_version: "v1".into(),
                input_digest: input_digest.into(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
}

fn current_incarnation(conn: &rusqlite::Connection) -> [u8; 16] {
    let hex: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_incarnation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    bytes
}

#[test]
fn unwired_facade_reports_honest_zero_and_none_epoch() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    // Embeddable documents exist — but semantic was never wired (no space
    // row). The facade must not inflate the denominator and must not fold
    // the absent epoch key to 0: the snapshot carries `None`, meaning
    // "semantic not ready" (strict reads only, ADR-0003).
    let conn = seed_conn(&db);
    seed_document(&conn, "in-1");

    let snapshot = db.reads().semantic_coverage().unwrap();
    assert_eq!(
        snapshot.coverage,
        SemanticCoverage {
            eligible: 0,
            published: 0,
            uncovered: 0,
            failed: 0,
            stale: 0,
            reason: Some(ZeroEligibleReason::SemanticNotConfigured),
        }
    );
    assert_eq!(
        snapshot.generation.semantic_epoch, None,
        "None = not ready, never 0"
    );
    assert_eq!(
        snapshot.generation,
        db.reads().read_generation().unwrap(),
        "snapshot pairs with the live strict generation"
    );
    assert!(db.reads().semantic_uncovered("", 10).unwrap().is_empty());
}

#[test]
fn coverage_facade_pairs_a_strict_generation_snapshot_with_publication() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let conn = seed_conn(&db);
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('sp','{}','active')",
        [],
    )
    .unwrap();
    seed_document(&conn, "in-1");
    enqueue(&conn, "in-1");
    let task = claim_next_on(&conn, "sp", "worker", 1000.0, 60.0)
        .unwrap()
        .unwrap();

    // Pre-publication: eligible but not covered, epoch key still absent.
    let before = db.reads().semantic_coverage().unwrap();
    assert_eq!(before.coverage.eligible, 1);
    assert_eq!(before.coverage.published, 0);
    assert_eq!(before.coverage.uncovered, 1);
    assert_eq!(before.generation.semantic_epoch, None);
    let uncovered = db.reads().semantic_uncovered("", 10).unwrap();
    assert_eq!(uncovered.len(), 1);
    assert_eq!(uncovered[0].doc_key, "d1");

    // Real publication through the P6-011 facade (只调用 P6-011 交付物).
    let outcome = db
        .publish_semantic(&PublishRequest {
            task_id: task.task_id,
            lease_token: &task.token,
            doc_key: &task.doc_key,
            doc_version: &task.doc_version,
            input_digest: &task.input_digest,
            space_id: "sp",
            artifact_ref: "cas.v1:ns:sp:in-1:spec:cksum",
            expected_incarnation: current_incarnation(&conn),
            retry_backoff_secs: 5.0,
            max_attempts: 3,
            now_unix: 1000.0,
        })
        .unwrap();
    assert!(outcome.published && outcome.visible_set_changed);

    // Post-publication: the gap closed and the snapshot's epoch is the one
    // the publication bumped — paired with the same generation a fresh
    // strict read returns (single-connection consistency held).
    let after = db.reads().semantic_coverage().unwrap();
    assert_eq!(after.coverage.eligible, 1);
    assert_eq!(after.coverage.published, 1);
    assert_eq!(after.coverage.uncovered, 0);
    assert_eq!(after.coverage.reason, None);
    assert_eq!(after.generation.semantic_epoch, Some(1));
    assert_eq!(after.generation, db.reads().read_generation().unwrap());
    assert!(db.reads().semantic_uncovered("", 10).unwrap().is_empty());
}

#[test]
fn primitive_counts_agree_with_facade_on_a_stable_database() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let conn = seed_conn(&db);
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('sp','{}','active')",
        [],
    )
    .unwrap();
    seed_document(&conn, "in-1");

    // Same database, same answer: the raw `*_on` primitive (one connection,
    // one moment) and the facade (generation-guarded) agree when the DB is
    // stable — the retry discipline only filters cross-write races.
    let direct = coverage_on(&conn).unwrap();
    let snapshot = db.reads().semantic_coverage().unwrap();
    assert_eq!(direct, snapshot.coverage);
}
