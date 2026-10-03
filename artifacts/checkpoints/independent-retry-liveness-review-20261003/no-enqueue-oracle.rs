//! Independent fixed acceptance oracle. No author tests are called.
use super::*;
use cc_model::config::ProjectConfig;
use cc_semantic::{
    providers::fake::{FakeProvider, FakeProviderConfig},
    queue::{drain_pending_parallel_with_lifecycle, TaskExit, WorkerLimits},
};
use std::sync::{atomic::AtomicUsize, Condvar};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Default)]
struct Block {
    state: Mutex<(usize, bool)>,
    cv: Condvar,
}
impl Block {
    fn enter(&self) {
        let mut s = self.state.lock().unwrap();
        s.0 += 1;
        self.cv.notify_all();
        while !s.1 {
            let (next, t) = self.cv.wait_timeout(s, Duration::from_secs(10)).unwrap();
            s = next;
            assert!(!t.timed_out(), "finite own provider timeout");
        }
    }
    fn wait(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        while s.0 < n {
            let (next, t) = self.cv.wait_timeout(s, Duration::from_secs(10)).unwrap();
            s = next;
            assert!(!t.timed_out(), "provider did not enter");
        }
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.cv.notify_all();
    }
}
struct OwnProvider {
    fake: FakeProvider,
    fail: Mutex<Option<String>>,
    calls: Mutex<Vec<String>>,
    block: Option<Arc<Block>>,
    errors: bool,
    active: AtomicUsize,
}
impl EmbeddingProvider for OwnProvider {
    fn space(&self) -> &cc_semantic::types::VectorSpace {
        self.fake.space()
    }
    fn embed_documents(&self, b: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        assert_eq!(b.len(), 1);
        let digest = b[0].input_digest.as_str().to_owned();
        self.calls.lock().unwrap().push(digest.clone());
        self.active.fetch_add(1, Ordering::SeqCst);
        if let Some(block) = &self.block {
            block.enter();
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        let mut fail = self.fail.lock().unwrap();
        if self.errors || fail.as_ref() == Some(&digest) {
            *fail = None;
            Err(ProviderError::ServerError)
        } else {
            self.fake.embed_documents(b)
        }
    }
    fn embed_queries(&self, _: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        panic!("no query/provider network in this oracle")
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    worker: Arc<SemanticRuntime>,
    provider: Arc<OwnProvider>,
    target: String,
}
fn fixture(
    n: usize,
    width: u32,
    position: usize,
    block: Option<Arc<Block>>,
    errors: bool,
) -> Fixture {
    let root = std::env::var("INDEPENDENT_FIXTURE_ROOT").expect("explicit own root required");
    std::fs::create_dir_all(&root).unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    let mut c = ProjectConfig::default();
    c.auto_index.enabled = false;
    c.semantic.enabled = true;
    c.semantic.model_id = "fake/fresh-retry-oracle-20261003".into();
    c.semantic.dimensions = Some(2);
    c.semantic.max_input_tokens = Some(8192);
    c.semantic.max_batch_items = Some(16);
    c.semantic.max_concurrent = 4;
    c.semantic.max_concurrent_per_project = width;
    c.semantic.endpoint = "https://semantic.invalid/v1".into();
    std::fs::write(
        dir.path().join(".codecortex.json"),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
    let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
    let sub = index.semantic_subsystem().unwrap();
    index
        .install_semantic_provider(Arc::new(FakeProvider::new(FakeProviderConfig::new(
            sub.space.clone(),
        ))))
        .unwrap();
    let old = index.semantic_runtime().unwrap();
    assert!(!old
        .run_round_with(old.resolve_provider().unwrap().as_ref())
        .unwrap());
    assert!(old.backfill_cursor.lock().unwrap().is_none());
    old.close();
    for i in 0..n {
        std::fs::write(
            dir.path().join(format!("fresh_{i:03}.rs")),
            format!("pub fn fresh_{i:03}() -> u64 {{ {} }}\n", i + 101),
        )
        .unwrap();
    }
    crate::handlers::core::build_index(Arc::new(std::sync::RwLock::new(index)), true).unwrap();
    let digests: Vec<String> = old
        .db
        .read_conn()
        .unwrap()
        .prepare("SELECT input_digest FROM semantic_outbox ORDER BY task_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(digests.len(), n);
    let target = digests[position].clone();
    let provider = Arc::new(OwnProvider {
        fake: FakeProvider::new(FakeProviderConfig::new(sub.space.clone())),
        fail: Mutex::new(if block.is_none() {
            Some(target.clone())
        } else {
            None
        }),
        calls: Mutex::new(Vec::new()),
        block,
        errors,
        active: AtomicUsize::new(0),
    });
    let worker =
        SemanticRuntime::new(old.db.clone(), sub, old.services.clone(), provider.clone()).unwrap();
    Fixture {
        _dir: dir,
        worker,
        provider,
        target,
    }
}
#[derive(Debug, PartialEq)]
struct Row {
    id: i64,
    state: String,
    attempt: i64,
    available: f64,
    token: Option<String>,
    digest: String,
    error: Option<String>,
}
fn rows(w: &SemanticRuntime) -> Vec<Row> {
    w.db.read_conn().unwrap().prepare("SELECT task_id,state,attempt_count,available_at,lease_token,input_digest,last_error FROM semantic_outbox ORDER BY task_id").unwrap().query_map([],|r|Ok(Row{id:r.get(0)?,state:r.get(1)?,attempt:r.get(2)?,available:r.get(3)?,token:r.get(4)?,digest:r.get(5)?,error:r.get(6)?})).unwrap().map(Result::unwrap).collect()
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
async fn idle(w: &Arc<SemanticRuntime>) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while w.running.load(Ordering::Acquire) || w.services.query_pins() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn acceptance(f: &Fixture, n: usize) -> bool {
    let r = rows(&f.worker);
    let time = now();
    let done = r.iter().filter(|x| x.state == "done").count();
    let ready = r
        .iter()
        .filter(|x| x.state == "pending" && x.available <= time)
        .count();
    let backoff = r
        .iter()
        .filter(|x| {
            x.state == "pending"
                && x.available > time
                && x.attempt == 1
                && x.digest == f.target
                && x.error.is_some()
                && x.token.is_none()
        })
        .count();
    let calls = f.provider.calls.lock().unwrap().len();
    let distinct: std::collections::HashSet<_> =
        r.iter().filter_map(|x| x.token.as_ref()).collect();
    let valid = r.len() == n
        && done == n - 1
        && ready == 0
        && backoff == 1
        && calls == n
        && r.iter().all(|x| x.attempt == 1 && x.state != "claimed")
        && distinct.len() == n - 1
        && f.worker
            .db
            .reads()
            .semantic_coverage()
            .unwrap()
            .coverage
            .published
            == (n - 1) as u64;
    println!("ORACLE n={n} done={done} backoff={backoff} ready={ready} calls={calls} accept={valid} rows={r:?}");
    valid
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_retry_matrix_same_acceptance_old_and_fixed() {
    let mut violations = Vec::new();
    for width in [0, 2] {
        for n in [4, 12, 33] {
            for position in [0, n / 2, n - 1] {
                let f =
                    tokio::task::spawn_blocking(move || fixture(n, width, position, None, false))
                        .await
                        .unwrap();
                assert!(f.worker.schedule());
                idle(&f.worker).await;
                println!("CASE width={width} n={n} failure_task_position={position}");
                if !acceptance(&f, n) {
                    violations.push((width, n, position));
                }
                let before = rows(&f.worker);
                let failed = before.iter().find(|x| x.digest == f.target).unwrap();
                assert!(failed.available > now(), "check before retry deadline");
                let count = f
                    .provider
                    .calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|x| **x == f.target)
                    .count();
                assert_eq!(count, 1);
                assert!(f.worker.schedule());
                idle(&f.worker).await;
                let after = rows(&f.worker);
                assert_eq!(
                    f.provider
                        .calls
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|x| **x == f.target)
                        .count(),
                    1,
                    "early schedule must not retry"
                );
                assert_eq!(
                    after.iter().find(|x| x.digest == f.target).unwrap(),
                    failed,
                    "retry row/deadline/token unchanged"
                );
                println!("EARLY_RETRY width={width} n={n} position={position} failed_calls=1 deadline_unchanged=true before_rows={}",before.len());
                f.worker.close();
            }
        }
    }
    assert!(
        violations.is_empty(),
        "same liveness oracle violations: {violations:?}"
    );
}
#[test]
fn fresh_retry_round_budget_16() {
    let mut failures = Vec::new();
    for width in [0, 2] {
        for position in [0, 8, 15, 32] {
            let f = fixture(33, width, position, None, false);
            let more = f.worker.run_round_with(f.provider.as_ref()).unwrap();
            let r = rows(&f.worker);
            let calls = f.provider.calls.lock().unwrap().len();
            let attempts: i64 = r.iter().map(|x| x.attempt).sum();
            println!("ROUND width={width} position={position} calls={calls} attempts={attempts} more={more}");
            if calls != 16 || attempts != 16 || !more {
                failures.push((width, position, calls, attempts, more));
            }
            assert!(r.iter().all(|x| x.attempt <= 1 && x.state != "claimed"));
            f.worker.close();
        }
    }
    assert!(
        failures.is_empty(),
        "finite batch budget violations: {failures:?}"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_close_join_with_success_and_server_error() {
    for errors in [false, true] {
        let b = Arc::new(Block::default());
        let b2 = b.clone();
        let f = tokio::task::spawn_blocking(move || fixture(20, 2, 0, Some(b2), errors))
            .await
            .unwrap();
        assert!(f.worker.schedule());
        let b2 = b.clone();
        tokio::task::spawn_blocking(move || b2.wait(2))
            .await
            .unwrap();
        assert_eq!(f.worker.services.query_pins(), 1);
        assert_eq!(f.provider.active.load(Ordering::SeqCst), 2);
        let db = f.worker.db.clone();
        tokio::task::spawn_blocking(move || {
            db.reads().read_generation().unwrap();
            db.enqueue_semantic_rebuild_plan(&[]).unwrap();
        })
        .await
        .unwrap();
        f.worker.close();
        assert!(!f.worker.schedule());
        assert!(f.worker.running.load(Ordering::Acquire));
        assert_eq!(f.worker.services.query_pins(), 1);
        b.release();
        idle(&f.worker).await;
        let r = rows(&f.worker);
        assert_eq!(f.provider.active.load(Ordering::SeqCst), 0);
        assert_eq!(f.provider.calls.lock().unwrap().len(), 2);
        assert_eq!(
            f.worker
                .db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .published,
            0
        );
        assert!(r.iter().all(|x| x.state == "pending" && x.token.is_none()));
        assert_eq!(r.iter().map(|x| x.attempt).sum::<i64>(), 2);
        println!("CLOSE errors={errors} calls=2 published=0 claims=0 pins_after_physical_join=0 retained_started_attempts=2");
    }
}
#[test]
fn fresh_unstarted_cancel_and_stale_token_fence() {
    for width in [0, 2] {
        let f = fixture(6, width, 0, None, false);
        let seen = Mutex::new(Vec::new());
        let limit = WorkerLimits::validated(16, 60.0, 30.0, 3).unwrap();
        let report = drain_pending_parallel_with_lifecycle(
            &f.worker.db,
            "fresh-cancel",
            &limit,
            Some(&f.worker.lifecycle),
            width as usize,
            &|g| {
                assert!(g.renew()?);
                assert!(!f.worker.db.renew_semantic_lease(
                    g.task().task_id,
                    "wrong-token",
                    60.0
                )?);
                seen.lock()
                    .unwrap()
                    .push((g.task().task_id, g.task().token.clone()));
                Ok(TaskExit::Cancelled { started: false })
            },
        )
        .unwrap();
        assert!(report.claimed >= 1 && report.claimed <= if width == 0 { 1 } else { 2 });
        assert!(rows(&f.worker)
            .iter()
            .all(|x| x.attempt == 0 && x.state == "pending" && x.token.is_none()));
        for (id, token) in seen.lock().unwrap().iter() {
            assert!(!f.worker.db.renew_semantic_lease(*id, token, 60.0).unwrap());
            assert!(!f
                .worker
                .db
                .hand_back_cancelled_semantic_task(*id, token, true)
                .unwrap());
        }
        println!(
            "CANCEL width={width} claims={} charged=0 stale_renew=false stale_handback=false",
            report.claimed
        );
        f.worker.close();
    }
}
#[test]
fn fresh_unhandled_error_waits_for_companion_join() {
    let f = fixture(6, 2, 0, None, false);
    let limit = WorkerLimits::validated(16, 60.0, 30.0, 3).unwrap();
    let b = Block::default();
    let ordinal = AtomicUsize::new(0);
    let returned = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|s| {
        let j = s.spawn(|| {
            let result = drain_pending_parallel_with_lifecycle(
                &f.worker.db,
                "fresh-error-join",
                &limit,
                Some(&f.worker.lifecycle),
                2,
                &|_| {
                    if ordinal.fetch_add(1, Ordering::SeqCst) == 0 {
                        b.wait(1);
                        tx.send(()).unwrap();
                        Err(cc_model::CcError::InvalidParams(
                            "own ordinary handler error".into(),
                        ))
                    } else {
                        b.enter();
                        Ok(TaskExit::Cancelled { started: true })
                    }
                },
            );
            returned.store(true, Ordering::SeqCst);
            result
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!returned.load(Ordering::SeqCst));
        b.release();
        assert!(j.join().unwrap().is_err());
    });
    assert!(rows(&f.worker).iter().all(|x| x.state != "claimed"));
    println!("ERROR_JOIN returned_after_physical_companion=true live_claims=0");
    f.worker.close();
}
#[test]
fn fresh_closed_needs_retry_hands_back_claims_without_continuation() {
    for width in [0, 2] {
        let f = fixture(20, width, 0, None, false);
        let b = Block::default();
        let limit = WorkerLimits::validated(16, 60.0, 30.0, 3).unwrap();
        let entered = AtomicUsize::new(0);
        std::thread::scope(|s| {
            let j = s.spawn(|| {
                drain_pending_parallel_with_lifecycle(
                    &f.worker.db,
                    "fresh-close-retry",
                    &limit,
                    Some(&f.worker.lifecycle),
                    width as usize,
                    &|_| {
                        entered.fetch_add(1, Ordering::SeqCst);
                        b.enter();
                        Ok(TaskExit::NeedsRetry {
                            reason: "own ordinary provider ServerError".into(),
                        })
                    },
                )
            });
            b.wait(if width == 0 { 1 } else { 2 });
            f.worker.close();
            b.release();
            let report = j.join().unwrap().unwrap();
            assert_eq!(report.retried, 0);
            assert_eq!(report.claimed, if width == 0 { 1 } else { 2 });
        });
        let r = rows(&f.worker);
        assert!(r
            .iter()
            .all(|x| x.state == "pending" && x.token.is_none() && x.available <= now()));
        assert_eq!(
            r.iter().map(|x| x.attempt).sum::<i64>(),
            entered.load(Ordering::SeqCst) as i64
        );
        println!("CLOSED_NEEDS_RETRY width={width} started={} retry_backoff=0 claimed_after_join=0 ready_handback=true",entered.load(Ordering::SeqCst));
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_minimal_12_document_reproduction() {
    for width in [0, 2] {
        let f = tokio::task::spawn_blocking(move || fixture(12, width, 0, None, false))
            .await
            .unwrap();
        assert!(f.worker.schedule());
        idle(&f.worker).await;
        println!("MINIMAL width={width} n=12 failure_task_position=0");
        let accepted = acceptance(&f, 12);
        f.worker.close();
        assert!(
            accepted,
            "ordinary ServerError must not strand other ready work (width={width})"
        );
    }
}
