//! P6-011: artifact → manifest publish CAS over `semantic_manifest`.
//!
//! Invariants fixed here (TASK-BRIEFS P6-011 / ADR-0003 row P6-011):
//!
//! 1. five-way fencing (incarnation / lease token / doc version / input
//!    digest / space) — breaking any single fence rejects the publish with
//!    zero visible-set writes and zero epoch bumps;
//! 2. full agreement atomically upserts the visible-set row and acks the
//!    task in one transaction, bumping `semantic_epoch` exactly once (facade);
//! 3. Q4 (P6-004 hand-off): a duplicate ack of identical content acks the
//!    task but does NOT bump `semantic_epoch` — the visible-set diff is the
//!    publisher's duty and is implemented in the CAS;
//! 4. a slow old result cannot attach to a newer doc version of the same
//!    path; the rejection hands the task back through the P6-007 fenced
//!    retry (backoff → `pending`, exhaustion → terminal `failed`);
//! 5. an expired-and-reclaimed worker cannot publish with its stale token.
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{
    claim_next_on, reclaim_expired_on, supersede_and_enqueue_on, ClaimedTask, OutboxPlan,
    OutboxUpsert,
};
use cc_db::semantic_publish::{
    publish_and_ack_on, PublishOutcome, PublishRejection, PublishRequest,
};

// ── harness ──────────────────────────────────────────────────────────────

fn v22_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert!(matches!(
        cc_db::index_migrate::migrate_index_db(&conn).unwrap(),
        cc_db::index_migrate::SchemaStatus::Initialized
            | cc_db::index_migrate::SchemaStatus::UpToDate
    ));
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

fn seed_active_space(conn: &rusqlite::Connection, space_id: &str) {
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space_id],
    )
    .unwrap();
}

/// FK chain (files → chunks → document_manifest) with a minimal record_json
/// carrying the embedded-input hash the publish CAS fences on.
fn seed_document(conn: &rusqlite::Connection, input_hash: &str, encoding_key: Option<&str>) {
    conn.execute_batch(&format!(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
           VALUES('src/d1.rs','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
           VALUES('c-d1','src/d1.rs','rust',0,1,2,'body');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
           VALUES('d1','v1','src/d1.rs','c-d1',{},'{{}}','{{\"input\":{{\"input_hash\":\"{input_hash}\"}}}}');",
        match encoding_key {
            Some(k) => format!("'{k}'"),
            None => "NULL".to_string(),
        }
    ))
    .unwrap();
}

fn enqueue(conn: &rusqlite::Connection, doc_key: &str, doc_version: &str, input_digest: &str) {
    let stats = supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: doc_key.into(),
                doc_version: doc_version.into(),
                input_digest: input_digest.into(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
}

fn claim(conn: &rusqlite::Connection) -> ClaimedTask {
    claim_at(conn, 1000.0)
}

fn claim_at(conn: &rusqlite::Connection, now: f64) -> ClaimedTask {
    claim_next_on(conn, "sp", "worker", now, 60.0)
        .unwrap()
        .unwrap()
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

fn epoch(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .ok()
}

fn published_row(
    conn: &rusqlite::Connection,
) -> Option<(String, String, String, String, String, String)> {
    conn.query_row(
        "SELECT doc_version,file_path,encoding_key,input_digest,space_id,artifact_ref \
         FROM semantic_manifest WHERE doc_key='d1'",
        [],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        },
    )
    .ok()
}

fn task_row(conn: &rusqlite::Connection) -> (String, Option<String>, Option<f64>, Option<String>) {
    conn.query_row(
        "SELECT state,lease_token,available_at,last_error FROM semantic_outbox WHERE doc_key='d1'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

/// Run the CAS inside a transaction the way the facade does (committing the
/// fenced-retry write of a rejection, since nothing else was written).
fn cas(conn: &rusqlite::Connection, req: &PublishRequest<'_>) -> PublishOutcome {
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let outcome = publish_and_ack_on(conn, req).unwrap();
    conn.execute_batch("COMMIT;").unwrap();
    outcome
}

/// Seed space + document + task, claim it, and build a fully valid request.
/// Breaking exactly one fence per test isolates each rejection reason.
fn fenced_setup(input_hash: &str) -> (rusqlite::Connection, PublishRequest<'static>) {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_document(&conn, input_hash, Some("enc"));
    enqueue(&conn, "d1", "v1", input_hash);
    let task = claim(&conn);
    let incarnation = current_incarnation(&conn);
    let req = PublishRequest {
        task_id: task.task_id,
        lease_token: Box::leak(task.token.clone().into_boxed_str()),
        doc_key: Box::leak(task.doc_key.clone().into_boxed_str()),
        doc_version: Box::leak(task.doc_version.clone().into_boxed_str()),
        input_digest: Box::leak(task.input_digest.clone().into_boxed_str()),
        space_id: Box::leak("sp".to_string().into_boxed_str()),
        artifact_ref: Box::leak(format!("cas.v1:ns:sp:{}:spec:cksum", input_hash).into_boxed_str()),
        expected_incarnation: incarnation,
        retry_backoff_secs: 5.0,
        max_attempts: 3,
        now_unix: 1000.0,
    };
    (conn, req)
}

// ── happy path (facade): atomic upsert + ack + exactly-once semantic bump ─

#[test]
fn publish_cas_happy_path_bumps_semantic_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let conn = seed_conn(&db);
    seed_active_space(&conn, "sp");
    seed_document(&conn, "in-1", Some("enc"));
    enqueue(&conn, "d1", "v1", "in-1");
    let task = claim(&conn);
    let before = db.reads().read_generation().unwrap();
    assert_eq!(before.semantic_epoch, None, "None = semantic not ready");

    let req = PublishRequest {
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
    };
    let outcome = db.publish_semantic(&req).unwrap();
    assert!(outcome.published);
    assert!(outcome.visible_set_changed);

    // Visible-set row written with the request's verified fields, file_path
    // redundantly resolved from document_manifest, incarnation recorded.
    assert_eq!(
        published_row(&conn).unwrap(),
        (
            "v1".into(),
            "src/d1.rs".into(),
            "enc".into(),
            "in-1".into(),
            "sp".into(),
            "cas.v1:ns:sp:in-1:spec:cksum".into()
        )
    );
    let inc_hex: String = conn
        .query_row(
            "SELECT published_incarnation FROM semantic_manifest WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(inc_hex.len(), 32);

    // Task acked to terminal done; semantic bumped exactly once; the other
    // two clocks untouched (P6-004 typed effects).
    assert_eq!(task_row(&conn).0, "done");
    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.semantic_epoch, Some(1));
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    assert_eq!(after.incarnation, before.incarnation);
}

// ── Q4: duplicate ack of identical content — zero visible change ─────────

#[test]
fn duplicate_ack_of_identical_content_acks_without_bumping() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let conn = seed_conn(&db);
    seed_active_space(&conn, "sp");
    seed_document(&conn, "in-1", Some("enc"));
    enqueue(&conn, "d1", "v1", "in-1");

    // First publication: the visible set changes, the epoch moves.
    let first = claim(&conn);
    let req = PublishRequest {
        task_id: first.task_id,
        lease_token: &first.token,
        doc_key: &first.doc_key,
        doc_version: &first.doc_version,
        input_digest: &first.input_digest,
        space_id: "sp",
        artifact_ref: "cas.v1:ns:sp:in-1:spec:cksum",
        expected_incarnation: current_incarnation(&conn),
        retry_backoff_secs: 5.0,
        max_attempts: 3,
        now_unix: 1000.0,
    };
    let first_outcome = db.publish_semantic(&req).unwrap();
    assert!(first_outcome.visible_set_changed);
    let after_first = db.reads().read_generation().unwrap();
    assert_eq!(after_first.semantic_epoch, Some(1));

    // Replay: the same (doc, space) is re-enqueued and republished with the
    // identical artifact_ref — the Q4 judgment: the visible set did NOT
    // change, so the task is acked and the semantic epoch does NOT bump.
    enqueue(&conn, "d1", "v1", "in-1");
    let second = claim(&conn);
    assert_ne!(
        second.token, first.token,
        "every attempt carries a fresh token"
    );
    let replay_req = PublishRequest {
        task_id: second.task_id,
        lease_token: &second.token,
        doc_key: &second.doc_key,
        doc_version: &second.doc_version,
        input_digest: &second.input_digest,
        space_id: "sp",
        artifact_ref: "cas.v1:ns:sp:in-1:spec:cksum",
        expected_incarnation: current_incarnation(&conn),
        retry_backoff_secs: 5.0,
        max_attempts: 3,
        now_unix: 2000.0,
    };
    let second_outcome = db.publish_semantic(&replay_req).unwrap();
    assert!(second_outcome.published);
    assert!(
        !second_outcome.visible_set_changed,
        "identical republish is not a visible-set change"
    );

    // Manifest stays a single unchanged row; task done; clock frozen at the
    // last real publication (P6-004 钟读).
    assert_eq!(
        published_row(&conn).unwrap().5,
        "cas.v1:ns:sp:in-1:spec:cksum"
    );
    let count: i64 = conn
        .query_row("SELECT count(*) FROM semantic_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(task_row(&conn).0, "done");
    let after_second = db.reads().read_generation().unwrap();
    assert_eq!(
        after_second.semantic_epoch,
        Some(1),
        "duplicate ack of unchanged visible set must not bump"
    );
    assert_eq!(after_second.index_epoch, after_first.index_epoch);
}

// ── the five fences, each broken alone ───────────────────────────────────

#[test]
fn stale_incarnation_is_fenced_out() {
    let (conn, mut req) = fenced_setup("in-1");
    req.expected_incarnation = [7u8; 16]; // not the database's incarnation

    let outcome = cas(&conn, &req);
    assert_eq!(
        outcome.rejection,
        Some(PublishRejection::IncarnationMismatch)
    );
    assert!(!outcome.published);
    assert!(published_row(&conn).is_none(), "zero manifest writes");
    assert_eq!(epoch(&conn, "semantic_epoch"), None, "no epoch bump");
    // Handed back to the queue with backoff; the artifact side is untouched.
    let (state, token, available_at, last_error) = task_row(&conn);
    assert_eq!(state, "pending");
    assert_eq!(token, None, "lease cleared by the fenced retry");
    assert_eq!(available_at, Some(1005.0));
    assert_eq!(
        last_error.as_deref(),
        Some(PublishRejection::IncarnationMismatch.as_str())
    );
}

#[test]
fn lost_lease_is_fenced_out_without_touching_anything() {
    let (conn, mut req) = fenced_setup("in-1");
    req.lease_token = "forged-token";

    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::LeaseLost));
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);
    // The row keeps its real claimant: a forged/stale token wins nothing,
    // and the fenced retry inside the CAS is itself a no-op here.
    let (state, token, _available_at, last_error) = task_row(&conn);
    assert_eq!(state, "claimed");
    assert_ne!(token.as_deref(), Some("forged-token"));
    assert_eq!(last_error, None);
}

#[test]
fn stale_doc_version_cannot_attach_to_a_newer_document() {
    let (conn, req) = fenced_setup("in-1");
    // The document moved on after the task was claimed (rewrite raced in).
    conn.execute(
        "UPDATE document_manifest SET doc_version='v2' WHERE doc_key='d1'",
        [],
    )
    .unwrap();

    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::DocVersionStale));
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);
    assert_eq!(task_row(&conn).0, "pending");
}

#[test]
fn input_digest_mismatch_is_fenced_out() {
    // The task was enqueued for input "in-1" but the current record's
    // embedded input hash is different (and one variant: no input at all).
    let (conn, req) = fenced_setup("in-1");
    conn.execute(
        "UPDATE document_manifest SET record_json='{\"input\":{\"input_hash\":\"other\"}}' \
         WHERE doc_key='d1'",
        [],
    )
    .unwrap();

    let outcome = cas(&conn, &req);
    assert_eq!(
        outcome.rejection,
        Some(PublishRejection::InputDigestMismatch)
    );
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);

    // Variant: the record lost its embeddable input entirely.
    let (conn2, req2) = fenced_setup("in-1");
    conn2
        .execute(
            "UPDATE document_manifest SET record_json='{}', encoding_key=NULL WHERE doc_key='d1'",
            [],
        )
        .unwrap();
    let outcome2 = cas(&conn2, &req2);
    assert_eq!(
        outcome2.rejection,
        Some(PublishRejection::InputDigestMismatch)
    );
    assert!(published_row(&conn2).is_none());
}

#[test]
fn non_active_space_is_fenced_out() {
    let (conn, req) = fenced_setup("in-1");
    // Space switched/revoked between claim and publish (P6-017 window).
    conn.execute(
        "UPDATE semantic_spaces SET state='revoked' WHERE space_id='sp'",
        [],
    )
    .unwrap();

    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::SpaceNotActive));
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);
    assert_eq!(task_row(&conn).0, "pending");
}

#[test]
fn deleted_document_is_fenced_out() {
    let (conn, req) = fenced_setup("in-1");
    conn.execute("DELETE FROM document_manifest WHERE doc_key='d1'", [])
        .unwrap();

    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::DocumentMissing));
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);
}

// ── rejection bookkeeping: backoff retry, then dead letter ───────────────

#[test]
fn rejection_retries_with_backoff_then_dead_letters_on_exhaustion() {
    let (conn, mut req) = fenced_setup("in-1");
    req.max_attempts = 2;
    conn.execute(
        "UPDATE document_manifest SET doc_version='v2' WHERE doc_key='d1'",
        [],
    )
    .unwrap();

    // Attempt 1: backoff → pending, available_at = now + backoff.
    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::DocVersionStale));
    let (state, token, available_at, last_error) = task_row(&conn);
    assert_eq!(state, "pending");
    assert_eq!(token, None);
    assert_eq!(available_at, Some(1005.0));
    assert_eq!(
        last_error.as_deref(),
        Some(PublishRejection::DocVersionStale.as_str())
    );

    // Attempt 2 (after backoff): attempt budget exhausted → terminal failed.
    let task = claim_at(&conn, 1010.0);
    let req2 = PublishRequest {
        retry_backoff_secs: 5.0,
        max_attempts: 2,
        now_unix: 2000.0,
        lease_token: &task.token,
        ..req
    };
    assert_eq!(req2.task_id, task.task_id);
    let outcome2 = cas(&conn, &req2);
    assert_eq!(outcome2.rejection, Some(PublishRejection::DocVersionStale));
    let (state, _, _, last_error) = task_row(&conn);
    assert_eq!(state, "failed", "exhaustion dead-letters the task");
    assert_eq!(
        last_error.as_deref(),
        Some(PublishRejection::DocVersionStale.as_str())
    );
    assert!(
        claim_next_on(&conn, "sp", "worker", 3000.0, 60.0)
            .unwrap()
            .is_none(),
        "dead-lettered tasks are never claimable again"
    );
}

// ── lease expiry race: the old holder cannot publish ─────────────────────

#[test]
fn expired_reclaimed_lease_fences_the_old_publish() {
    let (conn, req) = fenced_setup("in-1");
    // The original worker stalls; a third party reclaims and worker B claims
    // the same task with a fresh token.
    assert_eq!(reclaim_expired_on(&conn, 1061.0).unwrap(), 1);
    let b = claim_at(&conn, 1062.0);
    assert_ne!(b.token, req.lease_token);

    // A's publish (stale token) is fenced out — zero writes, B untouched.
    let outcome = cas(&conn, &req);
    assert_eq!(outcome.rejection, Some(PublishRejection::LeaseLost));
    assert!(published_row(&conn).is_none());
    assert_eq!(epoch(&conn, "semantic_epoch"), None);

    // B's publish succeeds and acks.
    let req_b = PublishRequest {
        task_id: b.task_id,
        lease_token: &b.token,
        ..req
    };
    let outcome_b = cas(&conn, &req_b);
    assert_eq!(outcome_b.rejection, None);
    assert!(outcome_b.published && outcome_b.visible_set_changed);
    assert!(published_row(&conn).is_some());
    assert_eq!(task_row(&conn).0, "done");
}

// ── facade: a rejection is a committed no-visible-change (Auxiliary) ─────

#[test]
fn facade_rejection_commits_retry_without_any_epoch_bump() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let conn = seed_conn(&db);
    seed_active_space(&conn, "sp");
    seed_document(&conn, "in-1", Some("enc"));
    enqueue(&conn, "d1", "v1", "in-1");
    let task = claim(&conn);
    let before = db.reads().read_generation().unwrap();

    conn.execute(
        "UPDATE document_manifest SET doc_version='v2' WHERE doc_key='d1'",
        [],
    )
    .unwrap();
    let req = PublishRequest {
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
    };
    let outcome = db.publish_semantic(&req).unwrap();
    assert_eq!(outcome.rejection, Some(PublishRejection::DocVersionStale));

    // The fenced-retry hand-back persisted, but no clock moved at all.
    assert_eq!(task_row(&conn).0, "pending");
    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.semantic_epoch, None);
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
}

fn seed_conn(db: &IndexDb) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}
