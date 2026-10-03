//! P5-D capabilities and real session lifecycle; fake recall tests orchestration only.
use cc_model::{
    query::{QueryControl, RetrievalStrategy},
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus},
    semantic::{SemanticRecall, SemanticRequest},
    CcResult,
};
use cc_server::{
    engine::CodeIndex,
    handlers::{self, SharedCodeIndex},
    project_session::ProjectSession,
    query_handle::QueryHandle,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tokio::sync::Notify;

fn project(strategy: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".codecortex.json"),json!({"auto_index":{"enabled":false},"query":{"strategy":strategy,"deadline_ms":60000,"semantic_timeout_ms":60000},"indexing":{"db_read_pool_size":1}}).to_string()).unwrap();
    std::fs::write(
        d.path().join("a.py"),
        "def needle():\n    return 'original project'\n",
    )
    .unwrap();
    d
}
fn runtime(d: &tempfile::TempDir) -> SharedCodeIndex {
    let mut idx = CodeIndex::new(Some(d.path())).unwrap();
    idx.build_index(true).unwrap();
    Arc::new(RwLock::new(idx))
}
fn caps(rt: &SharedCodeIndex) -> Value {
    handlers::core::index_capabilities(rt.clone()).unwrap()
}
struct SlowFake {
    calls: AtomicUsize,
    started: Notify,
    release: Notify,
}
impl SlowFake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        })
    }
}
impl SemanticRecall for SlowFake {
    fn recall(
        &self,
        _request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.notified().await;
            control.check()?;
            Ok(LaneOutcome {
                status: LaneStatus::Complete,
                coverage: LaneCoverage::complete(Some(0), 0),
                ..LaneOutcome::disabled("semantic", 1.0)
            })
        })
    }
}
#[test]
fn capability_readiness_distinguishes_absence_empty_error_and_semantic_port() {
    let rt = Arc::new(RwLock::new(CodeIndex::empty()));
    assert_eq!(caps(&rt)["retrieval"]["index_state"], "no_project");
    let d = project("auto");
    rt.write().unwrap().set_project(d.path(), false).unwrap();
    let c = caps(&rt);
    assert_eq!(c["retrieval"]["index_state"], "empty");
    assert_eq!(c["retrieval"]["semantic_state"], "not_configured");
    assert_eq!(c["retrieval"]["dense_state"], "disabled");
    assert_eq!(c["retrieval"]["default_effective_strategy"], "local");
    assert_eq!(c["retrieval"]["query_coverage"]["state"], "not_measured");
    rt.write().unwrap().build_index(true).unwrap();
    let fake = SlowFake::new();
    rt.read().unwrap().set_semantic_recall(Some(fake.clone()));
    let c = caps(&rt);
    assert_eq!(c["retrieval"]["index_state"], "available");
    assert_eq!(c["retrieval"]["semantic_state"], "port_attached_unverified");
    assert_eq!(c["retrieval"]["dense_state"], "disabled");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let db_path = rt
        .read()
        .unwrap()
        .index_db()
        .unwrap()
        .admin()
        .db_path()
        .to_path_buf();
    rusqlite::Connection::open(db_path)
        .unwrap()
        .execute(
            "UPDATE metadata SET value='broken' WHERE key='index_incarnation'",
            [],
        )
        .unwrap();
    let c = caps(&rt);
    assert_eq!(c["has_index"], false);
    assert_eq!(c["retrieval"]["index_state"], "error");
    assert!(c["retrieval"]["error"].is_object());
    assert!(c["indexed_files"].is_null());
    rt.write().unwrap().close();
    assert_eq!(caps(&rt)["retrieval"]["index_state"], "closed");
}
#[test]
fn explicit_unconfigured_semantic_is_never_reported_ready() {
    let d = project("semantic");
    let rt = runtime(&d);
    let c = caps(&rt);
    assert_eq!(
        c["retrieval"]["default_query_state"],
        "semantic_unavailable"
    );
    assert!(c["retrieval"]["default_effective_strategy"].is_null());
    assert_eq!(c["retrieval"]["local_state"], "available");
    assert_eq!(c["retrieval"]["dense_state"], "disabled");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_cold_routes_share_one_instance_and_build_gate() {
    let d = project("local");
    let s = ProjectSession::new(None).unwrap();
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let s = s.clone();
        let p = d.path().to_str().unwrap().to_owned();
        let b = barrier.clone();
        jobs.spawn(async move {
            b.wait().await;
            s.index_for_project_path(Some(&p)).await.unwrap()
        });
    }
    let mut all = Vec::new();
    while let Some(r) = jobs.join_next().await {
        all.push(r.unwrap());
    }
    for index in &all {
        assert!(Arc::ptr_eq(index, &all[0]));
        assert!(Arc::ptr_eq(
            &index.read().unwrap().build_gate(),
            &all[0].read().unwrap().build_gate()
        ));
    }
    s.shutdown().await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inflight_recall_survives_idle_and_lru_then_reclaims_resources() {
    let d = project("local");
    let s = ProjectSession::new(Some(d.path())).unwrap();
    let rt = s.active_index().await;
    rt.write().unwrap().build_index(true).unwrap();
    let id = rt.read().unwrap().index_db().unwrap().admin().instance_id();
    let weak_runtime = Arc::downgrade(&rt);
    let weak_db = Arc::downgrade(rt.read().unwrap().index_db().unwrap());
    let fake = SlowFake::new();
    rt.read().unwrap().set_semantic_recall(Some(fake.clone()));
    let call_rt = rt.clone();
    let job = tokio::spawn(async move {
        handlers::context::search_async(
            call_rt,
            "needle".into(),
            3,
            None,
            cc_model::search::SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Semantic),
                ..Default::default()
            },
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), fake.started.notified())
        .await
        .unwrap();
    assert_eq!(caps(&rt)["retrieval"]["query_pins"], 1);
    assert_eq!(s.evict_idle_now().await, 0);
    assert!(!rt.read().unwrap().is_closed());
    let other = project("local");
    s.set_active_project(other.path().to_path_buf())
        .await
        .unwrap();
    drop(rt);
    let mut projects = Vec::new();
    for _ in 0..18 {
        let p = project("local");
        s.index_for_project_path(Some(p.path().to_str().unwrap()))
            .await
            .unwrap();
        projects.push(p);
    }
    let again = s
        .index_for_project_path(Some(d.path().to_str().unwrap()))
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&again, &weak_runtime.upgrade().unwrap()));
    assert_eq!(
        again
            .read()
            .unwrap()
            .index_db()
            .unwrap()
            .admin()
            .instance_id(),
        id
    );
    fake.release.notify_one();
    let value = tokio::time::timeout(Duration::from_secs(10), job)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(value["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["text"].as_str().unwrap().contains("original project")));
    assert_eq!(again.read().unwrap().query_pins(), 0);
    assert!(s.evict_idle_now().await > 0);
    assert!(again.read().unwrap().is_closed());
    drop(again);
    s.shutdown().await;
    drop(s);
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak_db.upgrade().is_some() || weak_runtime.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p5d-lifecycle.json"),json!({"scope":"one real session, 18 LRU pressure projects, delayed fake recall","same_runtime_after_lru":true,"idle_skips_pin":true,"db_reclaimed_after_release":true,"fake_calls":fake.calls.load(Ordering::SeqCst)}).to_string()).unwrap();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn idle_sweep_never_waits_for_write_or_build_lock() {
    let d = project("local");
    let s = ProjectSession::new(Some(d.path())).unwrap();
    let rt = s.active_index().await;
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locked = rt.clone();
    let worker = std::thread::spawn(move || {
        let _guard = locked.write().unwrap();
        start_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    start_rx.await.unwrap();
    let sweep = tokio::time::timeout(Duration::from_millis(500), s.evict_idle_now()).await;
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(sweep.unwrap(), 0);
    let gate = rt.read().unwrap().build_gate();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _guard = gate.lock().unwrap();
        start_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    start_rx.await.unwrap();
    let sweep = tokio::time::timeout(Duration::from_millis(500), s.evict_idle_now()).await;
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(sweep.unwrap(), 0);
    assert_eq!(s.evict_idle_now().await, 1);
    s.shutdown().await;
}
#[tokio::test]
async fn dropping_session_stops_recurring_tasks_and_releases_cached_db() {
    let d = project("local");
    let s = ProjectSession::new(Some(d.path())).unwrap();
    let rt = s.active_index().await;
    let weak = Arc::downgrade(&rt);
    drop(rt);
    s.start_idle_eviction().await;
    s.start_idle_eviction().await;
    drop(s);
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn explicit_context_policy_cannot_bypass_optional_port_via_direct_symbols() {
    let d = project("local");
    std::fs::write(d.path().join("named.py"),"def AlphaNeedle():\n    return 1\ndef BetaNeedle():\n    return 2\ndef GammaNeedle():\n    return 3\n").unwrap();
    let rt = runtime(&d);
    let fake = SlowFake::new();
    rt.read().unwrap().set_semantic_recall(Some(fake.clone()));
    let task = "AlphaNeedle BetaNeedle GammaNeedle";
    let legacy = handlers::context::context_async(rt.clone(), task.into(), Some(10), true, None)
        .await
        .unwrap();
    assert!(legacy["matched_symbols"]
        .as_array()
        .is_some_and(|s| s.len() >= 3));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    fake.release.notify_one();
    let value = handlers::context::context_async_with_strategy(
        rt.clone(),
        task.into(),
        Some(10),
        true,
        None,
        Some(RetrievalStrategy::Semantic),
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        value["evidence_summary"]["retrieval"]["policy"]["requested"],
        "semantic"
    );
    handlers::context::context_async_with_strategy(
        rt,
        task.into(),
        Some(10),
        true,
        None,
        Some(RetrievalStrategy::Local),
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_blocking_work_keeps_pin_until_actual_exit() {
    let d = project("local");
    let session = ProjectSession::new(Some(d.path())).unwrap();
    let rt = session.active_index().await;
    let handle = QueryHandle::capture(&rt).unwrap();
    let control = QueryControl::new(Duration::from_secs(10)).unwrap();
    let inside = control.clone();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let job = tokio::spawn(async move {
        cc_server::service_factory::query_pool()
            .run_cpu(inside, move || {
                let _owned = handle;
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            })
            .await
    });
    started_rx.await.unwrap();
    control.cancel();
    assert!(matches!(
        job.await.unwrap(),
        Err(cc_model::CcError::QueryCancelled)
    ));
    assert_eq!(rt.read().unwrap().query_pins(), 1);
    assert_eq!(session.evict_idle_now().await, 0);
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while rt.read().unwrap().query_pins() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(session.evict_idle_now().await, 1);
    session.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_prevents_late_watcher_and_idle_restart() {
    // Native subscription took 7.5s in a recorded parallel Mac run. This
    // 20s startup watchdog tests liveness, not latency. The separate two-second
    // shutdown/reclamation checks and all source assertions remain unchanged.
    let d = project("local");
    std::fs::write(
        d.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":true}}"#,
    )
    .unwrap();
    let session = ProjectSession::new(Some(d.path())).unwrap();
    assert!(
        cc_model::config::load_project_config(d.path())
            .auto_index
            .enabled
    );
    assert_eq!(
        session
            .active_index()
            .await
            .read()
            .unwrap()
            .project_path
            .as_deref(),
        Some(d.path().canonicalize().unwrap().as_path())
    );
    session.start_watcher(d.path().canonicalize().unwrap());
    session.start_idle_eviction().await;
    let native_start = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(20), async {
        while !session.watcher_ready() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("p5d-native-startup.json"),json!({"native_startup_ms":native_start.elapsed().as_millis(),"startup_watchdog_seconds":20,"scope":"native subscription availability only; not a latency gate"}).to_string()).unwrap();
    }
    session.shutdown().await;
    session.start_watcher(d.path().canonicalize().unwrap());
    session.start_idle_eviction().await;
    assert!(!session.watcher_ready());
    let weak = Arc::downgrade(&session.active_index().await);
    drop(session);
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn cloned_views_hold_one_lease_until_last_drop() {
    let d = project("local");
    let rt = runtime(&d);
    let handle = QueryHandle::capture(&rt).unwrap();
    let clone = handle.clone();
    assert_eq!(rt.read().unwrap().query_pins(), 1);
    drop(handle);
    assert_eq!(rt.read().unwrap().query_pins(), 1);
    drop(clone);
    assert_eq!(rt.read().unwrap().query_pins(), 0);
}
