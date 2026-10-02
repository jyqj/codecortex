//! P7-006 integration: bounded call-layer retry × the outbox attempt
//! budget, admission batch flow, and the C11 wait window.
//!
//! Covers what the unit tests cannot see:
//!
//! 1. **layering (frozen 口径)**: one outbox attempt grants ONE fresh
//!    call-layer retry sequence; a sequence's provider calls never touch
//!    the outbox attempt counter — `attempt_count` moves by exactly one
//!    per fenced retry, however many provider calls happened inside;
//! 2. **budget interplay end to end**: call-layer success inside one
//!    attempt publishes; call-layer exhaustion hands back through the
//!    fenced retry and still dead-letters at the DB budget;
//! 3. **admission batch flow**: planned batches pass the retrying
//!    decorator with the shared gate attached; per-batch budgets are
//!    fresh and the 429 cooperation stays wired;
//! 4. **C11**: the retry waits happen with no DB transaction or lock
//!    held — an independent connection commits writes DURING the wait
//!    window, and the post-provider fenced write still lands.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert};
use cc_semantic::admission::{GateLimits, ProviderGate};
use cc_semantic::cache::ArtifactCache;
use cc_model::CcResult;
use cc_db::semantic_outbox::ClaimedTask;
use cc_semantic::ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput};
use cc_semantic::providers::openai_compatible::{
    BreakerLimits, CircuitBreaker, MockRetryClock, RetryPolicy, RetryingProvider,
    SystemRetryClock,
};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, WorkerLimits};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};

// ── harness (queue_worker.rs 的 World 形状的最小化复制) ─────────────────

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-p7006-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn space() -> VectorSpace {
    VectorSpace::new("fake/model-retry", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space: VectorSpace,
    spec: DocSpecDigest,
    incarnation: [u8; 16],
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let (db, _) = IndexDb::open(&dir.0.join("index.sqlite3")).expect("open index");
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let space = space();
        let spec = doc_spec(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space.digest().unwrap().as_str()],
        )
        .unwrap();
        let incarnation: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key='index_incarnation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut inc = [0u8; 16];
        for (i, byte) in inc.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&incarnation[i * 2..i * 2 + 2], 16).unwrap();
        }
        Self {
            _dir: dir,
            db,
            conn,
            cache,
            space,
            spec,
            incarnation: inc,
        }
    }

    fn seed_doc(&self, doc_key: &str, input: &InputDigest) {
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
                "INSERT OR REPLACE INTO document_manifest(doc_key,doc_version,file_path,chunk_id,\
                 encoding_key,reference_json,record_json) \
                 VALUES(?1,'v1',?2,?3,'enc','{}',?4)",
                rusqlite::params![
                    doc_key,
                    format!("src/{doc_key}.rs"),
                    format!("c-{doc_key}"),
                    format!("{{\"input\":{{\"input_hash\":\"{}\"}}}}", input.as_str())
                ],
            )
            .unwrap();
        let stats = supersede_and_enqueue_on(
            &self.conn,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: doc_key.into(),
                    doc_version: "v1".into(),
                    input_digest: input.as_str().into(),
                }],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();
        assert_eq!(stats.enqueued, 1);
    }

    fn row(&self, doc_key: &str) -> (String, i64, Option<String>) {
        self.conn
            .query_row(
                "SELECT state,attempt_count,last_error FROM semantic_outbox WHERE doc_key=?1",
                [doc_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
    }

    fn handler<'a>(
        &'a self,
        provider: &'a dyn EmbeddingProvider,
        resolve: &'a dyn Fn(&ClaimedTask) -> CcResult<Option<DocumentInput>>,
    ) -> EmbedHandler<'a> {
        let publisher = Publisher::new(&self.db, &self.cache, &self.space, &self.spec, self.incarnation)
            .expect("publisher");
        EmbedHandler::new(publisher, provider, resolve)
    }
}

/// Scripted inner provider (counts calls; sticks on the last outcome).
struct ScriptedProvider {
    space: VectorSpace,
    outcomes: Mutex<VecDeque<Result<Vec<Vec<f32>>, ProviderError>>>,
    calls: AtomicUsize,
}

impl ScriptedProvider {
    fn new(outcomes: Vec<Result<Vec<Vec<f32>>, ProviderError>>) -> Self {
        Self {
            space: space(),
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: AtomicUsize::new(0),
        }
    }
}

impl EmbeddingProvider for ScriptedProvider {
    fn space(&self) -> &VectorSpace {
        &self.space
    }

    fn embed_documents(&self, _batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("script exhausted"))
    }

    fn embed_queries(&self, _batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("script exhausted"))
    }
}

fn policy(max_attempts: u32) -> RetryPolicy {
    RetryPolicy::validated(
        max_attempts,
        Duration::from_millis(40),
        Duration::from_millis(200),
        true,
        Duration::from_secs(30),
        None,
    )
    .expect("valid policy")
}

// ── 1. call-layer success inside ONE outbox attempt ─────────────────────

#[test]
fn call_layer_retry_publishes_within_a_single_db_attempt() {
    let world = World::new("publish-in-one-attempt");
    let input = InputDigest::of_input(b"body").unwrap();
    world.seed_doc("d1", &input);

    // Two transient failures, then success — all inside the FIRST attempt.
    let inner = Arc::new(ScriptedProvider::new(vec![
        Err(ProviderError::ServerError),
        Err(ProviderError::Timeout),
        Ok(vec![vec![1.0_f32, 0.5_f32]]),
    ]));
    let provider = RetryingProvider::with_clock(
        inner.clone(),
        policy(3),
        Arc::new(CircuitBreaker::disabled()),
        None,
        Arc::new(SystemRetryClock),
    );

    let resolve = body_resolver();
    let report = drain_pending(&world.db, "worker-a", &limits(4), &mut |guard| {
        world.handler(&provider, &resolve).handle(guard)
    })
    .unwrap();
    assert_eq!(
        (report.claimed, report.completed, report.retried),
        (1, 1, 0),
        "the whole retry sequence lived inside one attempt"
    );
    assert_eq!(inner.calls.load(Ordering::SeqCst), 3);
    // Frozen layering: attempt_count == 1, task done, no last_error.
    let (state, attempts, last_error) = world.row("d1");
    assert_eq!(state, "done");
    assert_eq!(attempts, 1);
    assert_eq!(last_error, None);
}

fn body_resolver() -> impl Fn(&ClaimedTask) -> CcResult<Option<DocumentInput>> {
    |_task: &ClaimedTask| DocumentInput::from_bytes(b"body").map(Some)
}

/// max_batch is pinned to 1 in these tests (backoff 0 makes a handed-back
/// task immediately claimable, so a larger batch would re-claim it inside
/// the same drain — the layering assertions below want one claim per
/// drain).
fn limits(_max_batch: usize) -> WorkerLimits {
    WorkerLimits::validated(1, 60.0, 0.0, 2).expect("valid limits")
}

// ── 2. call-layer exhaustion → fenced retry → DB budget owns the rest ───

#[test]
fn call_layer_exhaustion_hands_back_and_the_db_budget_dead_letters() {
    let world = World::new("exhaustion-deadletters");
    let input = InputDigest::of_input(b"body").unwrap();
    world.seed_doc("d1", &input);

    // Every call fails retryably; each drain burns a FULL fresh call-layer
    // sequence (3 provider calls) but only ONE db attempt.
    let inner = Arc::new(ScriptedProvider::new(vec![
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
    ]));
    let provider = RetryingProvider::with_clock(
        inner.clone(),
        policy(3),
        Arc::new(CircuitBreaker::disabled()),
        None,
        Arc::new(SystemRetryClock),
    );

    let resolve = body_resolver();
    let handler = |world: &World, p: &RetryingProvider| {
        drain_pending(&world.db, "worker-a", &limits(4), &mut |guard| {
            world.handler(p, &resolve).handle(guard)
        })
        .unwrap()
    };

    let report = handler(&world, &provider);
    assert_eq!(report.retried, 1, "handed back through the fenced retry");
    assert_eq!(inner.calls.load(Ordering::SeqCst), 3, "fresh sequence, fully spent");
    let (state, attempts, last_error) = world.row("d1");
    assert_eq!(state, "pending");
    assert_eq!(attempts, 1, "one provider-call sequence == one db attempt");
    assert!(last_error.as_deref().unwrap().contains("provider server error"));

    // Second drain: same story, but the DB budget (max_attempts = 2) is
    // now exhausted → dead-letter, no further claims afterwards.
    let report = handler(&world, &provider);
    assert_eq!(report.retried, 1);
    assert_eq!(inner.calls.load(Ordering::SeqCst), 6);
    let (state, attempts, _) = world.row("d1");
    assert_eq!(state, "failed", "dead-lettered at the DB budget");
    assert_eq!(attempts, 2);
    assert!(
        world.db.claim_semantic("worker-a", 60.0).unwrap().is_none(),
        "dead-lettered tasks are never claimable through the queue again"
    );
}

// ── 3. admission batch flow through the retrying decorator ──────────────

#[test]
fn planned_batches_keep_fresh_retry_budgets_and_the_gate_wired() {
    let budget = cc_semantic::admission::InputBudget::validated(2, 1_000, 10_000).expect("budget");
    let rendered: Vec<(String, Vec<u8>)> = (0..4)
        .map(|i| (format!("k{i}"), vec![b'a'; 8]))
        .collect();
    let plan = cc_semantic::admission::plan_document_batches(
        &rendered,
        &budget,
        cc_model::chunk_policy::TOKEN_ESTIMATOR,
    )
    .expect("plan");
    let batches: Vec<Vec<DocumentInput>> = plan
        .iter()
        .filter_map(|entry| match entry {
            cc_semantic::admission::PlannedInput::Batch(batch) => Some(
                batch
                    .items
                    .iter()
                    .map(|(_, input)| input.clone())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(batches.len(), 2, "two planned batches");

    // Each batch: one transient failure, then success → per-batch fresh
    // sequence budgets; the shared gate rides along (429 cooperation).
    let gate = Arc::new(ProviderGate::new(GateLimits::permissive()));
    let inner = Arc::new(ScriptedProvider::new(vec![
        Err(ProviderError::ServerError),
        Ok(vec![vec![1.0_f32, 0.5_f32], vec![1.0_f32, 0.5_f32]]),
        Err(ProviderError::ServerError),
        Ok(vec![vec![1.0_f32, 0.5_f32], vec![1.0_f32, 0.5_f32]]),
    ]));
    let provider = RetryingProvider::with_clock(
        inner.clone(),
        policy(2),
        Arc::new(CircuitBreaker::disabled()),
        Some(gate.clone()),
        Arc::new(MockRetryClock::new(0)),
    );
    for batch in &batches {
        let out = provider.embed_documents(batch).expect("batch succeeds");
        assert_eq!(out.len(), batch.len());
    }
    assert_eq!(inner.calls.load(Ordering::SeqCst), 4, "1 retry per batch, fresh each time");
    assert!(gate.snapshot().suspended_for.is_none());
}

// ── 4. C11: retry waits never hold DB state ─────────────────────────────

#[test]
fn retry_waits_hold_no_db_lock_and_the_fenced_write_still_lands() {
    let world = World::new("c11-wait-window");
    let input = InputDigest::of_input(b"body").unwrap();
    world.seed_doc("d1", &input);

    let inner = Arc::new(ScriptedProvider::new(vec![
        Err(ProviderError::ServerError),
        Err(ProviderError::ServerError),
        Ok(vec![vec![1.0_f32, 0.5_f32]]),
    ]));
    let provider = RetryingProvider::with_clock(
        inner.clone(),
        policy(3),
        Arc::new(CircuitBreaker::disabled()),
        None,
        Arc::new(SystemRetryClock),
    );

    let guard = cc_semantic::queue::LeaseGuard::claim(&world.db, "worker-a", 60.0)
        .unwrap()
        .expect("claimed");
    assert!(guard.renew().unwrap(), "liveness gate before work");

    // An independent connection must be able to COMMIT writes DURING the
    // retry-wait window — proof that the waiting worker holds no
    // transaction or lock (C11).
    let wrote_during_window = Arc::new(AtomicBool::new(false));
    let flag = wrote_during_window.clone();
    let probe_conn = rusqlite::Connection::open(world.db.admin().db_path()).unwrap();
    probe_conn.busy_timeout(Duration::from_millis(30)).unwrap();
    let probe = std::thread::spawn(move || {
        for _ in 0..40 {
            if probe_conn
                .execute_batch("BEGIN IMMEDIATE; INSERT INTO metadata(key,value) \
                                VALUES('p7006_probe','1'); COMMIT;")
                .is_ok()
            {
                flag.store(true, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    // ~80ms of real waiting across two backoffs, no DB handle anywhere in
    // the decorator (structural: none exists on its surface).
    provider.embed_documents(&[DocumentInput::from_bytes(b"body").unwrap()])
        .expect("eventual success");
    probe.join().unwrap();
    assert!(
        wrote_during_window.load(Ordering::SeqCst),
        "an independent writer committed during the retry-wait window"
    );

    // The claim survived the window and the fenced write still lands.
    assert!(guard.renew().unwrap());
    world
        .db
        .retry_semantic_task(guard.task().task_id, guard.task().token.as_str(), "probe", 0.0, 2)
        .unwrap();
    let (state, attempts, _) = world.row("d1");
    assert_eq!(state, "pending");
    assert_eq!(attempts, 1);
}

// ── 5. breaker integration: a tripped process breaker fails fast ────────

#[test]
fn tripped_breaker_fails_fast_in_the_drain_without_provider_or_attempt_cost() {
    let world = World::new("breaker-fast-fail");
    let input = InputDigest::of_input(b"body").unwrap();
    world.seed_doc("d1", &input);

    // Trip the shared breaker BEFORE the drain: threshold 1.
    let clock = Arc::new(MockRetryClock::new(0));
    let breaker = Arc::new(
        CircuitBreaker::new(BreakerLimits::validated(1, Duration::from_secs(3_600)).unwrap())
    );
    breaker.record_failure(&*clock);

    let inner = Arc::new(ScriptedProvider::new(vec![Ok(vec![vec![1.0_f32, 0.5_f32]])]));
    let provider = RetryingProvider::with_clock(
        inner.clone(),
        policy(3),
        breaker.clone(),
        None,
        clock.clone(),
    );

    let resolve = body_resolver();
    let report = drain_pending(&world.db, "worker-a", &limits(4), &mut |guard| {
        world.handler(&provider, &resolve).handle(guard)
    })
    .unwrap();
    assert_eq!(report.retried, 1, "fast-fail handed back through the fenced retry");
    assert_eq!(inner.calls.load(Ordering::SeqCst), 0, "open circuit: zero provider contact");
    assert_eq!(world.row("d1").1, 1, "still exactly one db attempt spent");
    // The breaker picture is process-wide and observable.
    assert!(matches!(
        breaker.state(&*clock),
        cc_semantic::providers::openai_compatible::CircuitState::Open { .. }
    ));
}
