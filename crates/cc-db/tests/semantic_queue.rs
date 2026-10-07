//! P6-013: the semantic queue write facade over `IndexDb`
//! (`semantic_queue.rs`, the P6-007 deviation-8 hand-over).
//!
//! Invariants fixed here:
//!
//! 1. claim/renew/retry/reclaim each run as one `IMMEDIATE` short
//!    transaction on the real `IndexDb` write connection and keep every
//!    fenced primitive's semantics (token-only credential, `Ok(false)` =
//!    lease lost, backoff then dead-letter);
//! 2. unconfigured semantic (no `active` space) and an empty queue are both
//!    a clean `Ok(None)` — the default build path pays nothing;
//! 3. the whole facade is Auxiliary: no clock ever moves across any
//!    combination of its operations (P6-004 effect taxonomy).
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::supersede_and_enqueue_on;

/// A real `IndexDb` plus a raw second connection for seeding and row-level
/// inspection (the publish-CAS test harness shape).
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

    fn enqueue(&self, doc_key: &str, doc_version: &str) {
        let upsert = cc_db::semantic_outbox::OutboxUpsert {
            doc_key: doc_key.into(),
            doc_version: doc_version.into(),
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

    fn row(&self, doc_key: &str) -> TaskRow {
        self.conn
            .query_row(
                "SELECT state,lease_token,claim_owner,attempt_count,available_at,last_error \
                 FROM semantic_outbox WHERE doc_key=?1",
                [doc_key],
                |r| {
                    Ok(TaskRow {
                        state: r.get(0)?,
                        token: r.get(1)?,
                        owner: r.get(2)?,
                        attempts: r.get(3)?,
                        available_at: r.get(4)?,
                        last_error: r.get(5)?,
                    })
                },
            )
            .unwrap()
    }

    /// The full three-clock snapshot (P6-004 钟读) for the Auxiliary audit.
    fn generation(&self) -> cc_model::generation::ReadGeneration {
        self.db.reads().read_generation().unwrap()
    }
}

#[derive(Debug)]
struct TaskRow {
    state: String,
    token: Option<String>,
    owner: Option<String>,
    attempts: i64,
    available_at: f64,
    last_error: Option<String>,
}

// ── facade happy path: claim → renew → retry, fenced end to end ──────────

#[test]
fn facade_claim_renew_and_retry_keep_the_fenced_semantics() {
    let world = World::new("facade");
    let before = world.generation();

    // Unconfigured semantic: claim is a clean None, not an error.
    assert_eq!(world.db.claim_semantic("worker", 60.0).unwrap(), None);

    world.seed_active_space("sp");

    // Empty queue: still a clean None.
    assert_eq!(world.db.claim_semantic("worker", 60.0).unwrap(), None);

    // Future task (in backoff / not yet available): not claimable.
    world.enqueue("d1", "v1");
    world
        .conn
        .execute("UPDATE semantic_outbox SET available_at=1e12", [])
        .unwrap();
    assert_eq!(world.db.claim_semantic("worker", 60.0).unwrap(), None);
    world
        .conn
        .execute("UPDATE semantic_outbox SET available_at=0", [])
        .unwrap();

    // Claim through the facade: fresh token, owner recorded, attempt +1.
    let task = world
        .db
        .claim_semantic("worker-a", 60.0)
        .unwrap()
        .expect("a ready task");
    assert_eq!(task.doc_key, "d1");
    assert_eq!(task.op, cc_db::semantic_outbox::OutboxOp::Embed);
    let row = world.row("d1");
    assert_eq!(row.state, "claimed");
    assert_eq!(row.owner.as_deref(), Some("worker-a"));
    assert_eq!(row.attempts, 1);
    assert_eq!(row.token.as_deref(), Some(task.token.as_str()));

    // Heartbeat for the token holder succeeds…
    assert!(world
        .db
        .renew_semantic_lease(task.task_id, &task.token, 60.0)
        .unwrap());
    // …a forged token is refused with zero writes.
    assert!(!world
        .db
        .renew_semantic_lease(task.task_id, "not-the-token", 60.0)
        .unwrap());

    // Retry through the facade: back to pending with backoff, lease cleared,
    // reason persisted.
    assert!(world
        .db
        .retry_semantic_task(task.task_id, &task.token, "provider timeout", 5.0, 3)
        .unwrap());
    let row = world.row("d1");
    assert_eq!(row.state, "pending");
    assert_eq!(row.token, None);
    assert_eq!(row.last_error.as_deref(), Some("provider timeout"));
    assert!(row.available_at > 900.0, "backoff pushed availability");

    // A stale token cannot retry a task it no longer holds.
    assert!(!world
        .db
        .retry_semantic_task(task.task_id, &task.token, "again", 5.0, 3)
        .unwrap());

    // Auxiliary audit: the whole facade moved no clock.
    assert_eq!(world.generation(), before, "no clock moved (三钟全静)");
}

// ── reclaim through the facade ───────────────────────────────────────────

#[test]
fn facade_reclaim_returns_only_expired_leases_to_pending() {
    let world = World::new("reclaim");
    world.seed_active_space("sp");
    world.enqueue("d1", "v1");

    let task = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();

    // Unexpired lease: reclaim finds nothing.
    assert_eq!(world.db.reclaim_expired_semantic().unwrap(), 0);
    assert_eq!(world.row("d1").state, "claimed");

    // Force expiry (deterministic: move the deadline into the past).
    world
        .conn
        .execute("UPDATE semantic_outbox SET lease_expires_at=1.0", [])
        .unwrap();
    assert_eq!(world.db.reclaim_expired_semantic().unwrap(), 1);
    let row = world.row("d1");
    assert_eq!(row.state, "pending");
    assert_eq!(row.token, None);
    assert_eq!(row.attempts, 1, "reclaim does not consume an attempt");

    // The task is immediately claimable again — with a fresh token.
    let successor = world.db.claim_semantic("worker-b", 60.0).unwrap().unwrap();
    assert_eq!(successor.task_id, task.task_id);
    assert_ne!(successor.token, task.token);
    assert_eq!(world.row("d1").attempts, 2);
}

// ── claim fairness (P7-005; 接线轮待办 8: ORDER BY 注入) ─────────────────

use cc_db::semantic_outbox::{ack_done_on, retry_on, ClaimFairness, OutboxUpsert};

/// Seed one embed task for `doc_key` whose created_at/updated_at carry the
/// caller's clock (the rotation policy orders by those timestamps).
fn enqueue_at(world: &World, doc_key: &str, now_unix: f64) {
    let upsert = OutboxUpsert {
        doc_key: doc_key.into(),
        doc_version: format!("v-{doc_key}"),
        input_digest: format!("in-{doc_key}"),
    };
    let stats = supersede_and_enqueue_on(
        &world.conn,
        &cc_db::semantic_outbox::OutboxPlan {
            upserts: &[upsert],
            removals: &[],
            now_unix,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
}

#[test]
fn claim_fairness_doc_rotation_serves_the_least_recently_touched_doc_first() {
    let world = World::new("fairness");
    world.seed_active_space("sp");
    enqueue_at(&world, "doc-a", 100.0); // oldest task_id AND oldest updated_at
    enqueue_at(&world, "doc-b", 500.0);

    // Default claim: arrival FIFO — doc-a (task_id 1) first. Then hand it
    // back through a retry at a LATER clock tick: the row keeps its old
    // task_id but gets a FRESH updated_at. That divergence between arrival
    // order and touch order is exactly the signal rotation uses.
    let first = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    assert_eq!(first.doc_key, "doc-a");
    assert!(retry_on(
        &world.conn,
        first.task_id,
        &first.token,
        "flaky",
        900.0,
        0.0,
        3
    )
    .unwrap());

    // FIFO ignores the fresh retry timestamp: doc-a again (task_id order).
    let fifo = world.db.claim_semantic("worker", 60.0).unwrap().unwrap();
    assert_eq!(fifo.doc_key, "doc-a");
    assert!(retry_on(
        &world.conn,
        fifo.task_id,
        &fifo.token,
        "flaky",
        950.0,
        0.0,
        3
    )
    .unwrap());

    // Rotation serves the least recently touched doc: doc-b (updated_at
    // 500) before doc-a (950), despite doc-a's strictly older task_id.
    let rotated = world
        .db
        .claim_semantic_ordered("worker", 60.0, ClaimFairness::DocRoundRobin)
        .unwrap()
        .expect("rotation candidate");
    assert_eq!(rotated.doc_key, "doc-b");
    assert!(ack_done_on(&world.conn, rotated.task_id, &rotated.token, 960.0).unwrap());

    // Rotation round 2: doc-b is done; doc-a is the only candidate left.
    let rotated = world
        .db
        .claim_semantic_ordered("worker", 60.0, ClaimFairness::DocRoundRobin)
        .unwrap()
        .expect("rotation candidate");
    assert_eq!(rotated.doc_key, "doc-a");

    // The closed enum is the whole surface: Fifo behaves like the default.
    let world = World::new("fairness-fifo-default");
    world.seed_active_space("sp");
    enqueue_at(&world, "doc-a", 100.0);
    enqueue_at(&world, "doc-b", 500.0);
    let fifo = world
        .db
        .claim_semantic_ordered("worker", 60.0, ClaimFairness::Fifo)
        .unwrap()
        .unwrap();
    assert_eq!(
        fifo.doc_key, "doc-a",
        "explicit Fifo == default claim order"
    );
}
