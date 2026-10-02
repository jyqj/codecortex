//! P6-006: atomic semantic-outbox writes inside the source-code transaction.
//!
//! Covers: the no-active-space no-op (default behavior zero change), removal
//! revoke (manifest + task supersede, never an embed), upsert supersede +
//! enqueue, fail-stop on merge conflicts, transaction rollback atomicity, the
//! typed state-machine transition guards, the `semantic_outbox_ready` read
//! path, and the `{Index, Semantic}` EffectSet wiring of the incremental file
//! batch (`write_reconciled_batch` / `replace_files_batch` /
//! `remove_files_batch`).
use cc_db::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
use cc_db::semantic_outbox::{
    supersede_and_enqueue_on, transition_state_on, OutboxPlan, OutboxState, OutboxUpsert,
};
use cc_model::identity::{bytes_hash, DocumentBatch, DocumentDelta, DocumentRecord};
use cc_model::retrieval::EmbeddingInput;
use cc_model::source::{ByteSpan, ChunkSource, SourceEncoding, SourceIdentity};
use cc_model::{ChunkRecord, Language, ParseOutcome, ParserTier};

/// Fresh in-memory database at the current schema, foreign keys enforced.
fn v22_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert_eq!(
        cc_db::index_migrate::migrate_index_db(&conn).unwrap(),
        cc_db::index_migrate::SchemaStatus::Initialized
    );
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

fn seed_manifest_row(conn: &rusqlite::Connection, doc_key: &str, file_path: &str) {
    conn.execute_batch(&format!(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('{file_path}','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c-{doc_key}','{file_path}','rust',0,1,2,'body');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES('{doc_key}','v1','{file_path}','c-{doc_key}',NULL,'{{}}','{{}}');
         INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
           space_id,artifact_ref,published_at,published_incarnation) \
         VALUES('{doc_key}','v1','{file_path}','enc','in','sp','art','2026-01-01','inc');"
    ))
    .unwrap();
}

fn seed_outbox_task(conn: &rusqlite::Connection, doc_key: &str, state: &str, available_at: f64) {
    conn.execute(
        "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
           available_at,created_at,updated_at) \
         VALUES(?1,'v1','in','sp','embed',?2,?3,'2026-01-01','2026-01-01')",
        rusqlite::params![doc_key, state, available_at],
    )
    .unwrap();
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn upsert(doc_key: &str, version: &str) -> OutboxUpsert {
    OutboxUpsert {
        doc_key: doc_key.into(),
        doc_version: version.into(),
        input_digest: format!("in-{version}"),
    }
}

// ── supersede_and_enqueue_on ────────────────────────────────────────────

/// No active `semantic_spaces` row = semantic not configured: the whole
/// function is a no-op and the default build path pays nothing (ADR Decision
/// Drivers: zero default behavior change).
#[test]
fn no_active_space_is_a_full_noop() {
    let conn = v22_conn();
    seed_manifest_row(&conn, "d1", "a.rs");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    let plan = OutboxPlan {
        upserts: &[upsert("d1", "v2")],
        removals: &["d1".to_string()],
        now_unix: 1000.0,
    };
    let stats = supersede_and_enqueue_on(&conn, &plan).unwrap();
    assert_eq!(stats.superseded_tasks, 0);
    assert_eq!(stats.enqueued, 0);
    assert_eq!(stats.manifest_revoked, 0);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='pending'"
        ),
        1
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM semantic_manifest"), 1);
}

/// Removals revoke the visible-set row and supersede live tasks; a deletion
/// never enqueues an embed task.
#[test]
fn removals_revoke_manifest_and_supersede_without_embed() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_manifest_row(&conn, "d1", "a.rs");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    let plan = OutboxPlan {
        upserts: &[],
        removals: &["d1".to_string()],
        now_unix: 1000.0,
    };
    let stats = supersede_and_enqueue_on(&conn, &plan).unwrap();
    assert_eq!(stats.manifest_revoked, 1);
    assert_eq!(stats.superseded_tasks, 1);
    assert_eq!(stats.enqueued, 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM semantic_manifest"), 0);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='pending'"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='superseded'"
        ),
        1
    );
    // A superseded formerly-claimed task is fenced out the same way.
    seed_outbox_task(&conn, "d1", "claimed", 0.0);
    let stats = supersede_and_enqueue_on(&conn, &plan).unwrap();
    assert_eq!(stats.superseded_tasks, 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='superseded'"
        ),
        2
    );
}

/// Upserts supersede the doc's live task and enqueue exactly one pending
/// embed carrying the new doc_version and input digest.
#[test]
fn upsert_supersedes_old_and_enqueues_one_pending_embed() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    let plan = OutboxPlan {
        upserts: &[upsert("d1", "v2")],
        removals: &[],
        now_unix: 1000.0,
    };
    let stats = supersede_and_enqueue_on(&conn, &plan).unwrap();
    assert_eq!(stats.superseded_tasks, 1);
    assert_eq!(stats.enqueued, 1);
    let row: (String, String, String, String, f64) = conn
        .query_row(
            "SELECT doc_version,input_digest,space_id,state,available_at FROM semantic_outbox \
             WHERE state='pending'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(row.0, "v2");
    assert_eq!(row.1, "in-v2");
    assert_eq!(row.2, "sp");
    assert_eq!(row.3, "pending");
    assert_eq!(row.4, 1000.0);
}

/// Same-batch delete + re-add of one doc_key: the supersede runs first, so
/// exactly one live embed survives (never two, never zero).
#[test]
fn rewritten_doc_keeps_exactly_one_live_embed() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    let plan = OutboxPlan {
        upserts: &[upsert("d1", "v2")],
        removals: &["d1".to_string()],
        now_unix: 1000.0,
    };
    let stats = supersede_and_enqueue_on(&conn, &plan).unwrap();
    assert_eq!(stats.superseded_tasks, 1);
    assert_eq!(stats.enqueued, 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM semantic_outbox"), 2);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='pending'"
        ),
        1
    );
}

/// The partial unique index (one live task per doc+space) is fail-stop: a
/// plan whose upserts contain the same doc_key twice hits the constraint and
/// the error propagates (never swallowed).
#[test]
fn duplicate_live_upsert_conflicts_fail_stop() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    let plan = OutboxPlan {
        upserts: &[upsert("d1", "v1"), upsert("d1", "v2")],
        removals: &[],
        now_unix: 1000.0,
    };
    let err = supersede_and_enqueue_on(&conn, &plan).unwrap_err();
    assert!(
        err.to_string()
            .contains("UNIQUE constraint failed: semantic_outbox.doc_key"),
        "unexpected error: {err}"
    );
}

/// Two simultaneously active spaces violate the single-active-space
/// invariant (P6-017) — fail-stop, not silent pick-one.
#[test]
fn two_active_spaces_fail_stop() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp1");
    seed_active_space(&conn, "sp2");
    let plan = OutboxPlan {
        upserts: &[upsert("d1", "v1")],
        removals: &[],
        now_unix: 1000.0,
    };
    let err = supersede_and_enqueue_on(&conn, &plan).unwrap_err();
    assert!(
        err.to_string().contains("active semantic spaces"),
        "unexpected error: {err}"
    );
}

/// Atomicity: everything the function wrote inside an uncommitted
/// transaction disappears on rollback — no half task can escape.
#[test]
fn rollback_leaves_no_outbox_or_manifest_trace() {
    let mut conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_manifest_row(&conn, "d1", "a.rs");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    {
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let plan = OutboxPlan {
            upserts: &[upsert("d1", "v2")],
            removals: &["d1".to_string()],
            now_unix: 1000.0,
        };
        let stats = supersede_and_enqueue_on(&tx, &plan).unwrap();
        assert_eq!(stats.enqueued, 1);
        assert_eq!(stats.manifest_revoked, 1);
        // Dropped without commit → rollback.
    }
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='pending'"
        ),
        1
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='superseded'"
        ),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM semantic_manifest"), 1);
}

// ── typed state machine ─────────────────────────────────────────────────

/// The legal-transition table, positive and negative.
#[test]
fn state_transition_table_is_exact() {
    use OutboxState::{Claimed, Done, Failed, Pending, Superseded};
    for (from, to, legal) in [
        (Pending, Claimed, true),    // claim (P6-007 leases it)
        (Pending, Superseded, true), // write-path merge
        (Claimed, Done, true),       // ack
        (Claimed, Failed, true),     // failure
        (Claimed, Superseded, true), // write-path merge
        (Claimed, Pending, true),    // lease reclaim / atomic retry (P6-007)
        (Failed, Pending, true),     // retry with backoff
        (Failed, Superseded, true),  // write-path merge
        (Pending, Done, false),      // work must be claimed first
        (Pending, Failed, false),
        (Done, Pending, false),
        (Done, Claimed, false),
        (Done, Failed, false),
        (Done, Superseded, false),
        (Superseded, Pending, false), // terminal
        (Superseded, Claimed, false),
        (Superseded, Done, false),
        (Superseded, Failed, false),
    ] {
        assert_eq!(from.can_transition_to(to), legal, "{from:?} -> {to:?}");
    }
}

/// Guarded transitions in the database: legal ones flip exactly the matching
/// row (state + updated_at), stale expected-state writes win nothing, illegal
/// ones are rejected, and the lease columns stay untouched (P6-007 domain).
#[test]
fn transition_guards_in_database() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_outbox_task(&conn, "d1", "pending", 0.0);
    let task_id: i64 = conn
        .query_row("SELECT task_id FROM semantic_outbox", [], |r| r.get(0))
        .unwrap();

    // Illegal transition is a caller bug: typed rejection, no write.
    let err = transition_state_on(
        &conn,
        task_id,
        OutboxState::Pending,
        OutboxState::Done,
        1000.0,
        None,
    )
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("illegal semantic outbox transition"));

    // pending → claimed flips the row; lease columns stay NULL (no fencing
    // logic in this round — P6-007 owns token issuance).
    assert!(transition_state_on(
        &conn,
        task_id,
        OutboxState::Pending,
        OutboxState::Claimed,
        1000.0,
        None
    )
    .unwrap());
    let (state, lease, updated): (String, Option<String>, String) = conn
        .query_row(
            "SELECT state,lease_token,updated_at FROM semantic_outbox WHERE task_id=?1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "claimed");
    assert!(lease.is_none());
    assert_ne!(updated, "2026-01-01");

    // A second writer expecting `pending` wins nothing (stale guard).
    assert!(!transition_state_on(
        &conn,
        task_id,
        OutboxState::Pending,
        OutboxState::Superseded,
        1001.0,
        None
    )
    .unwrap());
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='claimed'"
        ),
        1
    );

    // claimed → done records the ack; a repeat ack finds no claimed row.
    assert!(transition_state_on(
        &conn,
        task_id,
        OutboxState::Claimed,
        OutboxState::Done,
        1002.0,
        None
    )
    .unwrap());
    assert!(!transition_state_on(
        &conn,
        task_id,
        OutboxState::Claimed,
        OutboxState::Done,
        1003.0,
        None
    )
    .unwrap());
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='done'"
        ),
        1
    );

    // failed → pending is the retry transition and persists last_error.
    seed_outbox_task(&conn, "d2", "failed", 0.0);
    let failed_id: i64 = conn
        .query_row(
            "SELECT task_id FROM semantic_outbox WHERE doc_key='d2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(transition_state_on(
        &conn,
        failed_id,
        OutboxState::Failed,
        OutboxState::Pending,
        1004.0,
        Some("provider 500")
    )
    .unwrap());
    let (state, error): (String, String) = conn
        .query_row(
            "SELECT state,last_error FROM semantic_outbox WHERE doc_key='d2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(error, "provider 500");
}

// ── ready read path ─────────────────────────────────────────────────────

/// Only `pending` rows at or before the caller clock are ready; ordering is
/// (available_at, task_id) and the limit truncates deterministically.
#[test]
fn ready_query_filters_and_orders() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp");
    seed_outbox_task(&conn, "d-late", "pending", 2000.0); // future: not ready
    seed_outbox_task(&conn, "d-claimed", "claimed", 0.0); // claimed: not ready
    seed_outbox_task(&conn, "d-done", "done", 0.0); // done: not ready
    seed_outbox_task(&conn, "d-superseded", "superseded", 0.0);
    seed_outbox_task(&conn, "d-second", "pending", 500.0);
    seed_outbox_task(&conn, "d-first", "pending", 100.0);
    let ready = cc_db::semantic_outbox::ready_tasks_on(&conn, 10, 1000.0).unwrap();
    let keys: Vec<String> = ready.iter().map(|t| t.doc_key.clone()).collect();
    assert_eq!(keys, vec!["d-first".to_string(), "d-second".to_string()]);
    let limited = cc_db::semantic_outbox::ready_tasks_on(&conn, 1, 1000.0).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].doc_key, "d-first");
    assert_eq!(limited[0].op, cc_db::semantic_outbox::OutboxOp::Embed);
    assert_eq!(limited[0].space_id, "sp");
}

/// The ready query is served through `semantic_outbox_ready`
/// (state, available_at, space_id) — the planned hot consumer path.
#[test]
fn ready_query_uses_the_ready_index() {
    let conn = v22_conn();
    let plan: String = conn
        .query_row(
            "EXPLAIN QUERY PLAN SELECT task_id FROM semantic_outbox \
             WHERE state='pending' AND available_at<=?1 ORDER BY available_at,task_id LIMIT ?2",
            rusqlite::params![1000.0, 10],
            |r| r.get::<_, String>(3),
        )
        .unwrap();
    assert!(plan.contains("semantic_outbox_ready"), "query plan: {plan}");
}

// ── production batch wiring ({Index, Semantic} EffectSet) ──────────────

/// One valid source-chunk document, built through the real identity/validate
/// path so the batch writer's provenance checks pass.
fn document_unit(rel_path: &str, text: &str, content_hash: &str) -> (FileWriteUnit, String) {
    let snapshot_id = bytes_hash(b"snapshot");
    let slice_digest = bytes_hash(text.as_bytes());
    // Both must be 64-hex digests (ChunkSource::validate hash_ok gate); the
    // unit's content_hash must equal the source's content_digest.
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

fn with_delta(unit: &mut FileWriteUnit) {
    let batch = unit.outcome.documents.as_mut().unwrap();
    batch.delta.upsert = batch.records.iter().map(|r| r.reference.clone()).collect();
}

fn seed_conn(db: &IndexDb) -> rusqlite::Connection {
    rusqlite::Connection::open(db.admin().db_path()).unwrap()
}

/// An incremental batch that projects documents while a semantic space is
/// active commits its desired embed task in the SAME transaction and declares
/// `{Index, Semantic}`: index_epoch +1 AND semantic_epoch None→Some(1), and
/// the ready read path serves the row afterwards.
#[test]
fn incremental_batch_with_active_space_enqueues_embed_and_bumps_semantic_once() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    let (mut unit, doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    let before = db.reads().read_generation().unwrap();

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

    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    assert_eq!(
        after.semantic_epoch,
        Some(1),
        "the batch declared the Index+Semantic set: the semantic clock starts at 1"
    );
    let ready = db.reads().semantic_outbox_ready(10).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].doc_key, doc_key);
    assert_eq!(ready[0].space_id, "sp");
    // input digest = blake3 of the rendered input text (P6-003 InputDigest).
    assert_eq!(ready[0].input_digest, bytes_hash(b"fn alpha() {}"));
}

/// Default path zero change: the identical batch without an active semantic
/// space writes no outbox row and never creates the semantic_epoch key.
#[test]
fn incremental_batch_without_active_space_keeps_default_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    let (mut unit, _doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    let before = db.reads().read_generation().unwrap();

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

    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(
        after.semantic_epoch, None,
        "no Semantic effect: the key must stay absent"
    );
    assert!(db.reads().semantic_outbox_ready(10).unwrap().is_empty());
}

/// A file removal revokes the published manifest row and supersedes its live
/// task inside the removal batch — and never enqueues an embed for it.
#[test]
fn file_removal_revokes_manifest_and_supersedes_without_embed() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    let (mut unit, doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    db.writes()
        .write_reconciled_batch(
            &[],
            &[unit],
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
            None,
        )
        .unwrap();
    // Publish a visible-set row for the doc (as the P6-011 CAS would).
    seed_conn(&db)
        .execute(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
             space_id,artifact_ref,published_at,published_incarnation) \
             VALUES(?1,'v1','src/a.rs','enc','in','sp','art','2026-01-01','inc')",
            [&doc_key],
        )
        .unwrap();
    // A live task exists from the first batch; a second batch removes the file.
    let before = db.reads().read_generation().unwrap();
    db.writes()
        .write_reconciled_batch(
            &["src/a.rs".to_string()],
            &[],
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
            None,
        )
        .unwrap();

    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(
        after.semantic_epoch,
        Some(2),
        "revoke is a semantic visible-set change"
    );
    let seed = seed_conn(&db);
    let manifest: i64 = seed
        .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        manifest, 0,
        "the removed doc's visible-set row must be revoked"
    );
    let pending_for_doc: i64 = seed
        .query_row(
            "SELECT COUNT(*) FROM semantic_outbox WHERE state='pending' AND doc_key=?1",
            [&doc_key],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pending_for_doc, 0, "deletions never produce embed tasks");
    assert!(db.reads().semantic_outbox_ready(10).unwrap().is_empty());
}

/// Atomicity through the production path: a batch that fails mid-write (here:
/// document/chunk coverage mismatch) rolls back the outbox rows it already
/// wrote — no half task survives, and neither clock moves.
#[test]
fn failed_batch_rolls_back_no_half_task() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    let (unit, _doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    // Break the document/chunk coverage: one projected record, zero sourced
    // chunks → document_store rejects inside the transaction.
    let mut broken = unit;
    broken.outcome.chunks.clear();
    let before = db.reads().read_generation().unwrap();

    let err = db
        .writes()
        .write_reconciled_batch(
            &[],
            std::slice::from_ref(&broken),
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
            None,
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("coverage"),
        "unexpected error: {err}"
    );

    let after = db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(
        after.semantic_epoch, None,
        "rollback must not bump the semantic clock"
    );
    let seed = seed_conn(&db);
    assert_eq!(
        seed.query_row::<i64, _, _>("SELECT COUNT(*) FROM semantic_outbox", [], |r| r.get(0))
            .unwrap(),
        0,
        "no half task may survive a rolled-back batch"
    );
    assert_eq!(
        seed.query_row::<i64, _, _>("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

/// replace_files_batch (the single-batch replace writer) carries the same
/// outbox wiring as the incremental batch.
#[test]
fn replace_files_batch_enqueues_embed_with_semantic_effect() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    let (mut unit, doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    db.writes()
        .replace_files_batch(std::slice::from_ref(&unit))
        .unwrap();
    let ready = db.reads().semantic_outbox_ready(10).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].doc_key, doc_key);
    assert_eq!(
        db.reads().read_generation().unwrap().semantic_epoch,
        Some(1)
    );
}

/// remove_files_batch (the standalone delete path) revokes and supersedes.
#[test]
fn remove_files_batch_supersedes_without_embed() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _guard) = IndexDb::open(&dir.path().join("i.sqlite3")).unwrap();
    seed_active_space(&seed_conn(&db), "sp");
    let (mut unit, doc_key) = document_unit("src/a.rs", "fn alpha() {}", "hash-a-1");
    with_delta(&mut unit);
    db.writes()
        .replace_files_batch(std::slice::from_ref(&unit))
        .unwrap();
    db.writes()
        .remove_files_batch(&["src/a.rs".to_string()])
        .unwrap();
    let seed = seed_conn(&db);
    assert_eq!(
        seed.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM semantic_outbox WHERE doc_key=?1 AND state='superseded'",
            [&doc_key],
            |r| r.get(0)
        )
        .unwrap(),
        1
    );
    assert!(db.reads().semantic_outbox_ready(10).unwrap().is_empty());
    assert_eq!(
        db.reads().read_generation().unwrap().semantic_epoch,
        Some(2)
    );
}
