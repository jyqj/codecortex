//! P6-007: worker-side atomic claim and lease fencing over `semantic_outbox`.
//!
//! Fencing invariants fixed here (TASK-BRIEFS P6-007 / ADR-0003 row 143:
//! short-transaction claim/renew/retry, one token per attempt, two processes
//! never hold the same lease, an expired worker cannot ack a new lease):
//!
//! 1. concurrent claim of one pending task — exactly one winner (CAS UPDATE,
//!    no SELECT-then-UPDATE window);
//! 2. expired lease → third-party reclaim → the original owner's
//!    renew/ack/retry with the stale token are all rejected;
//! 3. heartbeat renewal extends `lease_expires_at` only for the token holder;
//! 4. every attempt carries a fresh token — the previous holder can never
//!    advance the task again;
//! 5. retry backoff returns to `pending`; attempt exhaustion dead-letters to
//!    `failed` (never claimable again through this path);
//! 6. superseded tasks are neither claimable nor ackable;
//! 7. claim/renew/retry/reclaim/ack are Auxiliary: no epoch ever moves.
use std::time::Duration;

use cc_db::semantic_outbox::{
    ack_done_on, claim_next_on, reclaim_expired_on, renew_lease_on, retry_on,
    supersede_and_enqueue_on, OutboxPlan, OutboxUpsert,
};

/// Fresh database at the current schema with one active semantic space.
fn v22_conn(path: &str) -> rusqlite::Connection {
    let conn = if path.is_empty() {
        rusqlite::Connection::open_in_memory().unwrap()
    } else {
        rusqlite::Connection::open(path).unwrap()
    };
    assert!(matches!(
        cc_db::index_migrate::migrate_index_db(&conn).unwrap(),
        cc_db::index_migrate::SchemaStatus::Initialized
            | cc_db::index_migrate::SchemaStatus::UpToDate
    ));
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}

fn seed_active_space(conn: &rusqlite::Connection, space_id: &str) {
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space_id],
    )
    .unwrap();
}

fn seed_task(conn: &rusqlite::Connection, doc_key: &str, state: &str, available_at: f64) -> i64 {
    conn.execute(
        "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
           available_at,created_at,updated_at) \
         VALUES(?1,'v1','in','sp','embed',?2,?3,'2026-01-01','2026-01-01')",
        rusqlite::params![doc_key, state, available_at],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn row(conn: &rusqlite::Connection, task_id: i64) -> LeaseRow {
    conn.query_row(
        "SELECT state,lease_token,lease_expires_at,claim_owner,attempt_count,available_at,last_error \
         FROM semantic_outbox WHERE task_id=?1",
        [task_id],
        |r| {
            Ok(LeaseRow {
                state: r.get(0)?,
                token: r.get(1)?,
                expires_at: r.get(2)?,
                owner: r.get(3)?,
                attempts: r.get(4)?,
                available_at: r.get(5)?,
                last_error: r.get(6)?,
            })
        },
    )
    .unwrap()
}

#[derive(Debug)]
struct LeaseRow {
    state: String,
    token: Option<String>,
    expires_at: Option<f64>,
    owner: Option<String>,
    attempts: i64,
    available_at: f64,
    last_error: Option<String>,
}

fn upsert(doc_key: &str) -> OutboxUpsert {
    OutboxUpsert {
        doc_key: doc_key.into(),
        doc_version: "v1".into(),
        input_digest: "in".into(),
    }
}

// ── invariant 1: concurrent claim, exactly one winner ───────────────────

/// Two workers race to claim the one pending task: the CAS UPDATE
/// (`WHERE state='pending'`) admits exactly one — the loser observes `None`
/// with the row untouched by it, and duplicate claim of the same task is
/// structurally impossible (the second claim finds no `pending` row).
#[test]
fn concurrent_claim_exactly_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("i.sqlite3");
    let a = v22_conn(path.to_str().unwrap());
    let b = v22_conn(path.to_str().unwrap());
    seed_active_space(&a, "sp");
    seed_task(&a, "d1", "pending", 0.0);

    let ta = claim_next_on(&a, "sp", "worker-a", 1000.0, 60.0).unwrap();
    let tb = claim_next_on(&b, "sp", "worker-b", 1000.0, 60.0).unwrap();
    let a_won = ta.is_some();
    let claimed = [ta.is_some(), tb.is_some()];
    assert_eq!(
        claimed.iter().filter(|w| **w).count(),
        1,
        "exactly one worker may win the lease"
    );
    let winner = match (ta, tb) {
        (Some(w), _) | (None, Some(w)) => w,
        (None, None) => panic!("one pending task must be claimable"),
    };
    let r = row(&a, winner.task_id);
    assert_eq!(r.state, "claimed");
    assert_eq!(r.attempts, 1);
    assert_eq!(r.expires_at, Some(1060.0));
    assert_eq!(
        r.owner.as_deref(),
        Some(if a_won { "worker-a" } else { "worker-b" })
    );
    // The row is now claimed, so a third claim attempt yields nothing.
    let again = claim_next_on(&a, "sp", "worker-a", 1000.0, 60.0).unwrap();
    assert!(
        again.is_none(),
        "duplicate claim of a claimed task must fail"
    );
    // A different space claims nothing.
    seed_task(&a, "d-other", "pending", 0.0);
    assert!(claim_next_on(&a, "other-space", "worker-a", 1000.0, 60.0)
        .unwrap()
        .is_none());
}

/// Claim eats only `pending` rows at or before the caller's clock, oldest
/// first (`ORDER BY task_id`), and carries the task identity to the winner.
#[test]
fn claim_skips_future_and_serves_oldest_with_identity() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    seed_task(&conn, "d-future", "pending", 2000.0);
    seed_task(&conn, "d-first", "pending", 0.0);
    seed_task(&conn, "d-second", "pending", 0.0);
    seed_task(&conn, "d-claimed", "claimed", 0.0);
    let task = claim_next_on(&conn, "sp", "w", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    assert_eq!(task.doc_key, "d-first");
    assert_eq!(task.doc_version, "v1");
    assert_eq!(task.input_digest, "in");
    assert_eq!(task.op, cc_db::semantic_outbox::OutboxOp::Embed);
    assert_eq!(task.lease_expires_at, 1060.0);
    assert!(!task.token.is_empty());
    assert_eq!(
        row(&conn, task.task_id).token.as_deref(),
        Some(task.token.as_str()),
        "the RETURNING token is the persisted one"
    );
}

// ── invariant 2: expired lease, reclaim, stale holder rejected ──────────

/// Worker A's lease expires → a third party reclaims the task → worker B
/// claims it with a fresh token → A's renew, ack and retry (stale token) are
/// ALL rejected with zero writes: an expired worker can never ack a new lease.
#[test]
fn expired_lease_reclaimed_then_old_owner_fenced_out() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    let task_id = seed_task(&conn, "d1", "pending", 0.0);
    let a = claim_next_on(&conn, "sp", "worker-a", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    assert_eq!(a.token, row(&conn, task_id).token.unwrap());

    // Reclaim before expiry is a no-op: only expired leases are taken away.
    assert_eq!(reclaim_expired_on(&conn, 1059.9).unwrap(), 0);
    assert_eq!(row(&conn, task_id).state, "claimed");

    // Reclaim at expiry returns the task to `pending` and clears the lease.
    assert_eq!(reclaim_expired_on(&conn, 1060.1).unwrap(), 1);
    let r = row(&conn, task_id);
    assert_eq!(r.state, "pending");
    assert_eq!(r.token, None, "reclaim must clear the stale token");
    assert_eq!(r.expires_at, None);
    assert_eq!(r.owner, None);
    assert_eq!(r.attempts, 1, "reclaim does not consume an attempt");
    assert_eq!(r.available_at, 1060.1);

    // Worker B claims the reclaimed task and receives a fresh token.
    let b = claim_next_on(&conn, "sp", "worker-b", 1061.0, 60.0)
        .unwrap()
        .unwrap();
    assert_ne!(b.token, a.token, "each attempt must carry a fresh token");
    assert_eq!(row(&conn, task_id).attempts, 2);

    // The expired worker A can neither renew, ack, nor retry the new lease.
    assert!(!renew_lease_on(&conn, task_id, &a.token, 1062.0, 60.0).unwrap());
    assert!(!ack_done_on(&conn, task_id, &a.token, 1062.0).unwrap());
    assert!(!retry_on(&conn, task_id, &a.token, "late", 1062.0, 5.0, 3).unwrap());
    let r = row(&conn, task_id);
    assert_eq!(r.state, "claimed");
    assert_eq!(r.token.as_deref(), Some(b.token.as_str()));
    assert_eq!(r.last_error, None, "rejected stale writes change nothing");
}

/// A live-but-untouched lease cannot be renewed or acked by anyone but its
/// token holder (fencing judges by token, never by `claim_owner`).
#[test]
fn wrong_token_and_owner_mismatch_are_fenced() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    let task_id = seed_task(&conn, "d1", "pending", 0.0);
    claim_next_on(&conn, "sp", "worker-a", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    // `claim_owner` is diagnostics only — knowing it grants no rights.
    assert!(!renew_lease_on(&conn, task_id, "forged-token", 1001.0, 60.0).unwrap());
    assert!(!ack_done_on(&conn, task_id, "forged-token", 1001.0).unwrap());
    assert_eq!(row(&conn, task_id).state, "claimed");
    assert_eq!(row(&conn, task_id).expires_at, Some(1060.0));
}

// ── invariant 3: heartbeat renewal ──────────────────────────────────────

/// The holder's heartbeat extends `lease_expires_at`; a lost lease turns the
/// subsequent heartbeat into a `false` and the final ack is rejected too.
#[test]
fn heartbeat_renews_and_lost_lease_heartbeats_fail() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    let task_id = seed_task(&conn, "d1", "pending", 0.0);
    let task = claim_next_on(&conn, "sp", "worker-a", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    assert!(renew_lease_on(&conn, task_id, &task.token, 1030.0, 60.0).unwrap());
    assert_eq!(row(&conn, task_id).expires_at, Some(1090.0));
    assert_eq!(
        row(&conn, task_id).attempts,
        1,
        "renewal is not a new attempt"
    );

    // Expire, reclaim, re-claim: the old heartbeat must now fail.
    reclaim_expired_on(&conn, 1090.1).unwrap();
    let fresh = claim_next_on(&conn, "sp", "worker-b", 1091.0, 60.0)
        .unwrap()
        .unwrap();
    assert!(!renew_lease_on(&conn, task_id, &task.token, 1092.0, 60.0).unwrap());
    assert!(!ack_done_on(&conn, task_id, &task.token, 1092.0).unwrap());
    // The current holder still completes the task.
    assert!(ack_done_on(&conn, task_id, &fresh.token, 1092.0).unwrap());
    assert_eq!(row(&conn, task_id).state, "done");
}

// ── invariant 4: fresh token per attempt ────────────────────────────────

/// Claim → retry → claim: the second attempt token differs from the first and
/// the first holder's credentials stop working the moment the retry happens.
#[test]
fn fresh_token_per_attempt_and_old_token_dead_on_retry() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    let task_id = seed_task(&conn, "d1", "pending", 0.0);
    let first = claim_next_on(&conn, "sp", "w", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    assert!(retry_on(
        &conn,
        task_id,
        &first.token,
        "provider 500",
        1001.0,
        30.0,
        3
    )
    .unwrap());
    let r = row(&conn, task_id);
    assert_eq!(r.state, "pending");
    assert_eq!(r.token, None, "retry clears the lease");
    assert_eq!(r.available_at, 1031.0, "backoff moves availability");
    assert_eq!(r.last_error.as_deref(), Some("provider 500"));
    assert_eq!(r.attempts, 1);

    // Still in backoff: not claimable a hair before, claimable at the mark.
    assert!(claim_next_on(&conn, "sp", "w", 1030.9, 60.0)
        .unwrap()
        .is_none());
    let second = claim_next_on(&conn, "sp", "w", 1031.0, 60.0)
        .unwrap()
        .unwrap();
    assert_ne!(second.token, first.token);
    assert_eq!(row(&conn, task_id).attempts, 2);
    // The first attempt's token can no longer advance anything.
    assert!(!ack_done_on(&conn, task_id, &first.token, 1032.0).unwrap());
    assert!(!renew_lease_on(&conn, task_id, &first.token, 1032.0, 60.0).unwrap());
    assert!(ack_done_on(&conn, task_id, &second.token, 1032.0).unwrap());
}

// ── invariant 5: attempt exhaustion dead-letters ────────────────────────

/// `retry_on` at the attempt ceiling moves the task to terminal `failed`
/// (dead-letter): never claimable again, error persisted, lease cleared.
#[test]
fn retry_exhaustion_dead_letters_to_failed() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    let task_id = seed_task(&conn, "d1", "pending", 0.0);
    let task = claim_next_on(&conn, "sp", "w", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    // attempt_count is 1 after the claim; ceiling 1 means this retry terminal.
    assert!(retry_on(&conn, task_id, &task.token, "provider 429", 1001.0, 30.0, 1).unwrap());
    let r = row(&conn, task_id);
    assert_eq!(r.state, "failed");
    assert_eq!(r.token, None);
    assert_eq!(r.last_error.as_deref(), Some("provider 429"));
    assert_eq!(r.available_at, 0.0, "terminal rows keep their availability");
    // Dead-lettered tasks are not claimable and not ready.
    assert!(claim_next_on(&conn, "sp", "w", 9999.0, 60.0)
        .unwrap()
        .is_none());
    assert!(cc_db::semantic_outbox::ready_tasks_on(&conn, 10, 9999.0)
        .unwrap()
        .is_empty());
    // And a stale retry on the now-failed row wins nothing.
    assert!(!retry_on(&conn, task_id, &task.token, "again", 1002.0, 30.0, 1).unwrap());
}

// ── invariant 6: supersede fences both directions ───────────────────────

/// A claimed task superseded by the write path (P6-006 merge) is neither
/// claimable nor ackable by the still-running holder.
#[test]
fn superseded_task_unclaimable_and_unackable() {
    let conn = v22_conn("");
    seed_active_space(&conn, "sp");
    seed_task(&conn, "d1", "pending", 0.0);
    let task = claim_next_on(&conn, "sp", "worker-a", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    // The write path supersedes the claimed task (doc rewritten).
    let stats = supersede_and_enqueue_on(
        &conn,
        &OutboxPlan {
            upserts: &[upsert("d1")],
            removals: &[],
            now_unix: 1005.0,
        },
    )
    .unwrap();
    assert_eq!(stats.superseded_tasks, 1);
    assert!(!ack_done_on(&conn, task.task_id, &task.token, 1006.0).unwrap());
    assert!(!renew_lease_on(&conn, task.task_id, &task.token, 1006.0, 60.0).unwrap());
    assert!(!retry_on(&conn, task.task_id, &task.token, "x", 1006.0, 1.0, 3).unwrap());
    assert_eq!(row(&conn, task.task_id).state, "superseded");
    // The superseded task itself is gone from the race; what claim serves now
    // is the WRITE PATH'S fresh replacement task (P6-006 merge: supersede old
    // + enqueue one new pending), a different row.
    let replacement = claim_next_on(&conn, "sp", "w", 1006.0, 60.0)
        .unwrap()
        .unwrap();
    assert_ne!(replacement.task_id, task.task_id);
}

// ── invariant 7: lease ops are Auxiliary — no epoch ever moves ──────────

/// The whole lease lifecycle (claim/renew/retry/reclaim/re-claim/ack) through
/// a real `IndexDb` leaves all three clocks and the incarnation untouched:
/// queue reliability data is not retrieval content (ADR-0003 typed effects).
#[test]
fn lease_lifecycle_is_auxiliary_zero_epoch_bumps() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    // Enqueue through the production path, then run the lease lifecycle on a
    // raw connection, exactly as a worker consumer would.
    let (mut unit, _doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    db.writes()
        .write_reconciled_batch(
            &[],
            std::slice::from_ref(&unit),
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
            None,
        )
        .unwrap();
    let before = db.reads().read_generation().unwrap();

    let conn = seed_conn(&db);
    let task = claim_next_on(&conn, "sp", "w", now() + 1.0, 60.0)
        .unwrap()
        .unwrap();
    assert!(renew_lease_on(&conn, task.task_id, &task.token, now() + 2.0, 60.0).unwrap());
    assert!(retry_on(
        &conn,
        task.task_id,
        &task.token,
        "flaky",
        now() + 3.0,
        0.0,
        5
    )
    .unwrap());
    let again = claim_next_on(&conn, "sp", "w", now() + 4.0, 60.0)
        .unwrap()
        .unwrap();
    assert!(ack_done_on(&conn, again.task_id, &again.token, now() + 5.0).unwrap());
    assert_eq!(reclaim_expired_on(&conn, now() + 6.0).unwrap(), 0);

    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    assert_eq!(
        after.semantic_epoch, before.semantic_epoch,
        "claim/renew/retry/reclaim/ack never advance any epoch"
    );
    assert_eq!(after.incarnation, before.incarnation);
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

use cc_db::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
use cc_model::identity::{bytes_hash, DocumentBatch, DocumentDelta, DocumentRecord};
use cc_model::retrieval::EmbeddingInput;
use cc_model::source::{ByteSpan, ChunkSource, SourceEncoding, SourceIdentity};
use cc_model::{ChunkRecord, Language, ParseOutcome, ParserTier};

/// One valid source-chunk document through the real identity/validate path
/// (same fixture shape as `semantic_outbox.rs`).
fn document_unit(rel_path: &str, text: &str, content_hash: &str) -> (FileWriteUnit, String) {
    let snapshot_id = bytes_hash(b"snapshot");
    let slice_digest = bytes_hash(text.as_bytes());
    let content_digest = bytes_hash(content_hash.as_bytes());
    let source = ChunkSource {
        source: SourceIdentity {
            snapshot_id: snapshot_id.clone(),
            content_digest: content_digest.clone(),
            byte_len: text.len(),
            encoding: SourceEncoding::Utf8,
        },
        span: ByteSpan::new(0, text.len()).unwrap(),
        slice_digest: slice_digest.clone(),
        boundary: "chunk".into(),
        owner: None,
        signature: None,
    };
    let input = EmbeddingInput {
        format_version: 1,
        text: text.to_string(),
        input_hash: slice_digest.clone(),
        render_key: "test-render".into(),
        source_range: ByteSpan::new(0, text.len()).unwrap(),
        source_snapshot_id: snapshot_id,
        source_slice_digest: slice_digest,
        metadata_truncated: false,
        token_estimate: cc_model::approx_tokens(text),
        token_estimator: cc_model::chunk_policy::TOKEN_ESTIMATOR.into(),
    };
    let chunk = ChunkRecord {
        source: Some(source),
        chunk_id: format!("{rel_path}:0"),
        file_path: rel_path.to_string(),
        language: Language::Rust,
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        breadcrumb: String::new(),
        text: text.to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: cc_model::approx_tokens(text),
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    };
    let policy = bytes_hash(b"policy");
    let record = DocumentRecord::new(&chunk, &policy, 0, "{\"v\":1}", Ok(input)).unwrap();
    let doc_key = record.reference.doc_key.clone();
    let unit = FileWriteUnit {
        rel_path: rel_path.to_string(),
        language: Language::Rust,
        content_hash: content_digest,
        mtime: 1.0,
        size: text.len() as u64,
        outcome: ParseOutcome {
            document_spec: Some(cc_model::identity::hash(&record.encoding_spec).unwrap()),
            documents: Some(DocumentBatch {
                records: vec![record],
                delta: DocumentDelta {
                    upsert: Vec::new(),
                    removed: Vec::new(),
                    ..Default::default()
                },
            }),
            chunk_policy: Some(policy),
            chunks: vec![chunk],
            ..Default::default()
        },
    };
    (unit, doc_key)
}

/// delta 装配：批次投影需要显式 upsert 列表（照 semantic_outbox.rs fixture）。
fn with_delta(unit: &mut FileWriteUnit) {
    let batch = unit.outcome.documents.as_mut().unwrap();
    batch.delta.upsert = batch.records.iter().map(|r| r.reference.clone()).collect();
}

fn seed_conn(db: &IndexDb) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn
}
