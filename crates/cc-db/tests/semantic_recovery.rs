//! P6-015: the bounded recovery primitives over `IndexDb`
//! (`semantic_recovery.rs`) — page-capped reclaim, dead-letter census, and
//! the keyset-paginated desired-set projection (P6-014 deviation-7
//! hand-over).
//!
//! Invariants fixed here:
//!
//! 1. the bounded reclaim is row-for-row the frozen `reclaim_expired_on`
//!    transformation (state, `available_at`, lease clearing, attempts NOT
//!    consumed) at at most `limit` rows per call, with a transaction-consistent
//!    `exhausted` verdict — unexpired leases are never touched;
//! 2. the dead-letter census counts terminal `failed` rows of the active
//!    space only and is a pure read;
//! 3. the bounded desired-set projection pages in deterministic `doc_key`
//!    order and its pages concatenate to exactly the unbounded P6-014
//!    projection;
//! 4. every operation is Auxiliary: no clock ever moves (P6-004 taxonomy).
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::supersede_and_enqueue_on;

/// A real `IndexDb` plus a raw second connection for seeding and row-level
/// inspection (the semantic_queue test harness shape).
struct World {
    _dir: tempfile::TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
}

impl World {
    fn new(_tag: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let (db, _) = IndexDb::open(&dir.path().join("index.sqlite3")).expect("open index");
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        Self {
            _dir: dir,
            db,
            conn,
        }
    }

    fn seed_active_space(&self, space_id: &str) {
        self.conn
            .execute(
                "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
                [space_id],
            )
            .unwrap();
    }

    fn enqueue(&self, doc_key: &str) {
        let upsert = cc_db::semantic_outbox::OutboxUpsert {
            doc_key: doc_key.into(),
            doc_version: "v1".into(),
            input_digest: format!("in-{doc_key}"),
        };
        let stats = supersede_and_enqueue_on(
            &self.conn,
            &cc_db::semantic_outbox::OutboxPlan {
                upserts: &[upsert],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();
        assert_eq!(stats.enqueued, 1);
    }

    /// FK chain (files → chunks → document_manifest) with an explicit
    /// record_json so the desired-set projection has an input digest to peek.
    fn seed_document(&self, doc_key: &str, encoding_key: Option<&str>, record_json: &str) {
        self.conn
            .execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
                 VALUES(?1,'rust','hash',1.0,1,'2026-01-01')",
                [format!("src/{doc_key}.rs")],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
                 VALUES(?1,?2,'rust',0,1,2,'body')",
                rusqlite::params![format!("c-{doc_key}"), format!("src/{doc_key}.rs")],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,\
                 encoding_key,reference_json,record_json) \
                 VALUES(?1,'v1',?2,?3,?4,'{}',?5)",
                rusqlite::params![
                    doc_key,
                    format!("src/{doc_key}.rs"),
                    format!("c-{doc_key}"),
                    encoding_key,
                    record_json,
                ],
            )
            .unwrap();
    }

    fn generation(&self) -> cc_model::generation::ReadGeneration {
        self.db.reads().read_generation().unwrap()
    }
}

// ── bounded reclaim: pages through expired leases, never touches live ones ──

#[test]
fn bounded_reclaim_pages_through_expired_leases_and_reports_exhaustion() {
    let world = World::new("bounded-reclaim");
    let before = world.generation();
    world.seed_active_space("sp");
    for key in ["d1", "d2", "d3", "d4"] {
        world.enqueue(key);
        world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    }
    // Deterministic kill + time passage: three of the four leases move into
    // the past; d4's lease stays live.
    world
        .conn
        .execute(
            "UPDATE semantic_outbox SET lease_expires_at=1.0 WHERE doc_key IN ('d1','d2','d3')",
            [],
        )
        .unwrap();

    // Limit 0 is a caller bug, rejected up front.
    assert!(world
        .db
        .reclaim_expired_semantic_bounded(0)
        .unwrap_err()
        .to_string()
        .contains("limit >= 1"));

    // Page 1: exactly two rows (task_id order); d3 is still expired and
    // un-reclaimed, so the class is NOT exhausted yet.
    let page = world.db.reclaim_expired_semantic_bounded(2).unwrap();
    assert_eq!(page.reclaimed, 2, "one call takes at most `limit` rows");
    assert!(!page.exhausted, "d3's expired lease remains — keep driving");

    // The reclaimed rows are pending with cleared leases, attempts intact.
    let state = |key: &str| -> (String, Option<String>, i64) {
        world
            .conn
            .query_row(
                "SELECT state,lease_token,attempt_count FROM semantic_outbox WHERE doc_key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    };
    let (s1, t1, a1) = state("d1");
    assert_eq!(s1, "pending");
    assert_eq!(t1, None, "lease cleared");
    assert_eq!(a1, 1, "reclaim does not consume an attempt");
    assert_eq!(state("d4").0, "claimed", "unexpired lease is never touched");

    // There was exactly one more expired lease (d3): a second page takes it,
    // a third observes the drained class.
    let page2 = world.db.reclaim_expired_semantic_bounded(2).unwrap();
    assert_eq!(page2.reclaimed, 1);
    assert!(page2.exhausted);
    let page3 = world.db.reclaim_expired_semantic_bounded(2).unwrap();
    assert_eq!(page3.reclaimed, 0);
    assert!(page3.exhausted);
    assert_eq!(state("d4").0, "claimed");

    // Auxiliary audit: the whole bounded reclaim moved no clock.
    assert_eq!(world.generation(), before, "no clock moved (三钟全静)");
}

// ── dead-letter census: counts the active space's terminal failures ────────

#[test]
fn dead_letter_census_counts_only_the_active_space_failures() {
    let world = World::new("dead-letters");
    // Unconfigured semantic: honest zero.
    assert_eq!(world.db.semantic_dead_letter_count().unwrap(), 0);

    world.seed_active_space("sp");
    world.enqueue("d1");

    // No failures yet.
    assert_eq!(world.db.semantic_dead_letter_count().unwrap(), 0);

    // Exhaust the attempt budget → terminal `failed`.
    let task = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    assert!(world
        .db
        .retry_semantic_task(task.task_id, &task.token, "boom", 5.0, 1)
        .unwrap());
    assert_eq!(world.db.semantic_dead_letter_count().unwrap(), 1);

    // A revoked-space failure does not count against the active space.
    world
        .conn
        .execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('sp-old','{}','revoked')",
            [],
        )
        .unwrap();
    world.enqueue("d2");
    world
        .conn
        .execute(
            "UPDATE semantic_outbox SET space_id='sp-old' WHERE doc_key='d2'",
            [],
        )
        .unwrap();
    // d2 now belongs to the revoked space: claim it through the raw
    // space-scoped primitive (the facade only serves the active space).
    let task = cc_db::semantic_outbox::claim_next_on(&world.conn, "sp-old", "worker", 2000.0, 60.0)
        .unwrap()
        .expect("the revoked-space task");
    assert!(cc_db::semantic_outbox::retry_on(
        &world.conn,
        task.task_id,
        &task.token,
        "boom",
        2100.0,
        5.0,
        1,
    )
    .unwrap());
    assert_eq!(
        world.db.semantic_dead_letter_count().unwrap(),
        1,
        "only the active space counts"
    );
}

// ── hand-back primitive: fenced claimed→pending, attempt budget untouched ──

#[test]
fn hand_back_primitive_fences_and_never_consumes_an_attempt() {
    let world = World::new("hand-back");
    let before = world.generation();
    world.seed_active_space("sp");
    world.enqueue("d1");

    let task = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    assert_eq!(task.task_id, 1, "attempt_count = 1 after the claim");

    let row = |key: &str| -> (String, Option<String>, i64, String, f64) {
        world
            .conn
            .query_row(
                "SELECT state,lease_token,attempt_count,updated_at,available_at \
                 FROM semantic_outbox WHERE doc_key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
    };

    // Fence rejects: a stale token and an unknown task id both win nothing —
    // `Ok(false)` with a ZERO-WRITE row (byte-identical observation).
    let untouched = row("d1");
    assert!(!world
        .db
        .hand_back_semantic_task(task.task_id, "stale-token", "nope")
        .unwrap());
    assert!(!world
        .db
        .hand_back_semantic_task(999, &task.token, "nope")
        .unwrap());
    assert_eq!(row("d1"), untouched, "fence rejection wrote nothing");

    // The success path: claimed → pending, lease cleared, immediately
    // claimable, and the attempt budget untouched.
    assert!(world
        .db
        .hand_back_semantic_task(
            task.task_id,
            &task.token,
            "recovery: cache miss, requeued for the worker"
        )
        .unwrap());
    let (state, lease, attempts, _ts, available_at) = row("d1");
    assert_eq!(state, "pending");
    assert_eq!(lease, None, "lease cleared — the caller loses its token");
    assert_eq!(attempts, 1, "hand-back does not consume an attempt");
    assert!(
        available_at > 900.0,
        "immediately claimable (available_at = now, no backoff)"
    );
    let last_error: String = world
        .conn
        .query_row(
            "SELECT last_error FROM semantic_outbox WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(last_error.contains("cache miss"), "reason persisted for audit");

    // Terminal-state fence: once the task is done, no hand-back resurrects it.
    let task = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    assert!(cc_db::semantic_outbox::ack_done_on(
        &world.conn,
        task.task_id,
        &task.token,
        2_000.0
    )
    .unwrap());
    assert!(!world
        .db
        .hand_back_semantic_task(task.task_id, &task.token, "late")
        .unwrap());
    assert_eq!(row("d1").0, "done", "terminal state stays terminal");

    // Auxiliary audit: the whole hand-back moved no clock.
    assert_eq!(world.generation(), before, "no clock moved (三钟全静)");
}

// ── bounded desired-set projection: keyset pages == the unbounded set ──────

#[test]
fn desired_set_bounded_keyset_pagination_matches_the_unbounded_projection() {
    let world = World::new("desired-bounded");
    let record = |digest: &str| format!(r#"{{"input":{{"input_hash":"{digest}"}}}}"#);
    world.seed_document("d1", Some("enc"), &record("in-d1"));
    world.seed_document("d2", Some("enc"), &record("in-d2"));
    world.seed_document("d3", Some("enc"), "{}"); // render-failed: skipped
    world.seed_document("d4", Some("enc"), &record("in-d4"));
    world.seed_document("d5", None, &record("in-d5")); // not embeddable: skipped

    let unbounded = world.db.semantic_rebuild_desired_set().unwrap();
    assert_eq!(unbounded.len(), 3, "only embeddable rows are desired");

    // Keyset pages of two concatenate to exactly the unbounded projection.
    let page1 = world
        .db
        .semantic_rebuild_desired_set_bounded("", 2)
        .unwrap();
    assert_eq!(page1.len(), 2);
    let page2 = world
        .db
        .semantic_rebuild_desired_set_bounded(&page1.last().unwrap().doc_key, 2)
        .unwrap();
    assert_eq!(page2.len(), 1, "the tail page is partial");
    let page3 = world
        .db
        .semantic_rebuild_desired_set_bounded(&page2.last().unwrap().doc_key, 2)
        .unwrap();
    assert!(page3.is_empty(), "the scan terminates");

    let mut paged = page1;
    paged.extend(page2);
    assert_eq!(paged, unbounded, "pages == the unbounded projection");
    assert_eq!(paged[0].doc_key, "d1");
    assert_eq!(paged[2].doc_key, "d4");

    // Limit 0 is a caller bug, rejected instead of looping forever.
    assert!(world
        .db
        .semantic_rebuild_desired_set_bounded("", 0)
        .unwrap_err()
        .to_string()
        .contains("limit >= 1"));
}
