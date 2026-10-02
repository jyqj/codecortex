//! Independent production review: synthetic projects and loopback HTTP only.
#![cfg(feature = "semantic-http")]
use cc_model::{config::ProjectConfig, query::RetrievalStrategy, search::SearchRequest};
use cc_server::{
    engine::CodeIndex,
    handlers::{self, SharedCodeIndex},
    service_factory,
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, RwLock,
    },
    time::{Duration, Instant},
};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Call {
    model: String,
    query: bool,
    released: bool,
}
struct State {
    calls: Vec<Call>,
    held: bool,
}
struct Endpoint {
    url: String,
    state: Arc<(Mutex<State>, Condvar)>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Endpoint {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let state = Arc::new((
            Mutex::new(State {
                calls: vec![],
                held: false,
            }),
            Condvar::new(),
        ));
        let stop = Arc::new(AtomicBool::new(false));
        let s = state.clone();
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut workers = vec![];
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                };
                let s = s.clone();
                let stopped = stopped.clone();
                workers.push(std::thread::spawn(move || {
                    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut bytes = Vec::new(); let mut buf = [0; 8192];
                    let body = loop {
                        let n = socket.read(&mut buf).unwrap_or(0); if n == 0 { return } bytes.extend_from_slice(&buf[..n]);
                        if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let len: usize = headers.lines().find_map(|line| { let (k,v) = line.split_once(':')?; if k.eq_ignore_ascii_case("content-length") { v.trim().parse().ok() } else { None } }).unwrap();
                            if bytes.len() >= end + 4 + len { break serde_json::from_slice::<Value>(&bytes[end+4..end+4+len]).unwrap() }
                        }
                    };
                    let query = body["input"].as_array().unwrap().iter().any(|v| v.as_str().unwrap().starts_with("review-query"));
                    let mut lock = s.0.lock().unwrap(); let id = lock.calls.len(); let held = lock.held;
                    lock.calls.push(Call { model: body["model"].as_str().unwrap().into(), query, released: !held }); s.1.notify_all();
                    let end = Instant::now() + Duration::from_secs(5);
                    while !lock.calls[id].released && !stopped.load(Ordering::Acquire) {
                        assert!(Instant::now() < end, "independent fixture was not released within original five-second bound");
                        lock = s.1.wait_timeout(lock, Duration::from_millis(10)).unwrap().0;
                    }
                    drop(lock);
                    let data: Vec<_> = body["input"].as_array().unwrap().iter().enumerate().map(|(i,_)| json!({"index":i,"embedding":[1.0,0.0]})).collect();
                    let reply = json!({"model":body["model"],"data":data}).to_string();
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.len());
                    let _ = socket.write_all(header.as_bytes()); let _ = socket.write_all(reply.as_bytes());
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            url,
            state,
            stop,
            thread: Some(thread),
        }
    }
    fn hold(&self) -> usize {
        let mut s = self.state.0.lock().unwrap();
        s.held = true;
        s.calls.len()
    }
    fn release(&self, id: usize) {
        self.state.0.lock().unwrap().calls[id].released = true;
        self.state.1.notify_all();
    }
    fn release_all(&self) {
        let mut s = self.state.0.lock().unwrap();
        s.held = false;
        for call in &mut s.calls {
            call.released = true;
        }
        self.state.1.notify_all();
    }
    fn count(&self) -> usize {
        self.state.0.lock().unwrap().calls.len()
    }
    fn query(&self, id: usize) -> bool {
        self.state.0.lock().unwrap().calls[id].query
    }
    fn model(&self, id: usize) -> String {
        self.state.0.lock().unwrap().calls[id].model.clone()
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.release_all();
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
struct CacheEnv(Option<std::ffi::OsString>);
impl CacheEnv {
    fn new(path: &std::path::Path) -> Self {
        let old = std::env::var_os(cc_semantic::cache::CACHE_ROOT_ENV);
        std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, path);
        Self(old)
    }
}
impl Drop for CacheEnv {
    fn drop(&mut self) {
        match self.0.take() {
            Some(v) => std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, v),
            None => std::env::remove_var(cc_semantic::cache::CACHE_ROOT_ENV),
        }
    }
}
fn config(root: &std::path::Path, endpoint: &Endpoint, model: &str) {
    let key = root.join("synthetic-key.txt");
    std::fs::write(&key, "synthetic-loopback-only").unwrap();
    let mut c = ProjectConfig::default();
    c.auto_index.enabled = false;
    c.indexing.db_read_pool_size = Some(1);
    c.semantic.enabled = true;
    c.semantic.network_opt_in = true;
    c.semantic.allow_query_network = true;
    c.semantic.allow_http = true;
    c.semantic.endpoint = endpoint.url.clone();
    c.semantic.api_key_ref = Some(format!("file:{}", key.display()));
    c.semantic.model_id = model.into();
    c.semantic.dimensions = Some(2);
    c.semantic.max_input_tokens = Some(8192);
    c.semantic.max_batch_items = Some(16);
    c.semantic.max_concurrent = 2;
    c.semantic.acquire_timeout_ms = 5000;
    c.semantic.retry_total_deadline_ms = 5000;
    std::fs::write(
        root.join(".codecortex.json"),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
}
async fn wait(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("original five-second acceptance bound");
}
fn status(index: &SharedCodeIndex) -> Value {
    handlers::facade::handle_status(index.clone(), "capabilities").unwrap()
}
async fn ready(index: &SharedCodeIndex) {
    wait(|| status(index)["retrieval"]["semantic_state"] == "ready").await;
}
async fn build(index: &SharedCodeIndex) {
    let i = index.clone();
    tokio::task::spawn_blocking(move || handlers::core::build_index(i, false))
        .await
        .unwrap()
        .unwrap();
}
async fn project(endpoint: &Endpoint, model: &str) -> (tempfile::TempDir, SharedCodeIndex) {
    let root = tempfile::tempdir().unwrap();
    config(root.path(), endpoint, model);
    std::fs::write(
        root.path().join("fixture.rs"),
        "pub fn needle() -> u32 { 101 }\n",
    )
    .unwrap();
    let index = Arc::new(RwLock::new(CodeIndex::new(Some(root.path())).unwrap()));
    build(&index).await;
    ready(&index).await;
    (root, index)
}
fn search(
    index: SharedCodeIndex,
    label: &str,
) -> tokio::task::JoinHandle<cc_model::CcResult<Value>> {
    let query = format!("review-query {label} needle");
    tokio::spawn(async move {
        handlers::context::search_async(
            index,
            query,
            2,
            None,
            SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Semantic),
                ..Default::default()
            },
        )
        .await
    })
}
fn context(
    index: SharedCodeIndex,
    label: &str,
) -> tokio::task::JoinHandle<cc_model::CcResult<Value>> {
    let task = format!("review-query {label} needle");
    tokio::spawn(async move {
        handlers::context::context_async_with_strategy(
            index,
            task,
            Some(2),
            true,
            None,
            Some(RetrievalStrategy::Semantic),
        )
        .await
    })
}
async fn finish(job: tokio::task::JoinHandle<cc_model::CcResult<Value>>) {
    match tokio::time::timeout(Duration::from_secs(5), job)
        .await
        .unwrap()
        .unwrap()
    {
        Ok(_) => {}
        Err(cc_model::CcError::RetrievalChanged { attempts: 1 }) => {
            eprintln!("independent review: concurrent publication correctly rejected with RetrievalChanged(1)");
        }
        other => panic!("foreground must progress within five seconds, got {other:?}"),
    }
}
fn db(index: &SharedCodeIndex) -> Arc<cc_db::index_db::IndexDb> {
    index.read().unwrap().index_db().unwrap().clone()
}
fn gate() -> cc_semantic::admission::GateSnapshot {
    service_factory::semantic_provider_gate().snapshot()
}
fn change(root: &std::path::Path, n: u32) {
    std::fs::write(
        root.join("fixture.rs"),
        format!("pub fn needle() -> u32 {{ {n} }}\n"),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifo_foreground_load_does_not_starve_background_or_hold_db_cpu() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (a, ia) = project(&e, "review/fairness").await;
    let (b, ib) = project(&e, "review/fairness").await;
    let start = e.hold();
    let q1 = search(ia.clone(), "fg-one");
    let q2 = context(ib.clone(), "fg-two");
    wait(|| e.count() == start + 2).await;
    assert_eq!(gate().in_flight, 2);
    assert!(e.query(start));
    assert!(e.query(start + 1));
    change(a.path(), 102);
    build(&ia).await;
    wait(|| gate().waiting == 1).await;
    change(b.path(), 103);
    build(&ib).await;
    wait(|| gate().waiting == 2).await;
    // Single-connection read pool, writer and CPU pool stay usable while both
    // classes await network. SQL transaction rolls back without mutating data.
    assert_eq!(service_factory::query_pool().stats().cpu_in_flight, 0);
    let path = db(&ia).admin().db_path().to_owned();
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.busy_timeout(Duration::from_millis(100)).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
    })
    .await
    .unwrap();
    let local = handlers::context::search_async(
        ia.clone(),
        "needle".into(),
        2,
        None,
        SearchRequest {
            retrieval_strategy: Some(RetrievalStrategy::Local),
            ..Default::default()
        },
    );
    tokio::time::timeout(Duration::from_secs(1), local)
        .await
        .unwrap()
        .unwrap();
    e.release(start);
    wait(|| e.count() == start + 3).await;
    assert!(
        !e.query(start + 2),
        "FIFO background must beat a later foreground request"
    );
    // Completion may observe generation changes from the intentional build;
    // only the active remaining task is needed to occupy the second FG slot.
    let q3 = search(ia.clone(), "fg-later");
    wait(|| gate().waiting == 2).await;
    e.release(start + 1);
    wait(|| e.count() == start + 4).await;
    assert!(!e.query(start + 3));
    e.release(start + 2);
    wait(|| e.count() == start + 5).await;
    assert!(e.query(start + 4));
    e.release_all();
    finish(q1).await;
    finish(q2).await;
    finish(q3).await;
    ready(&ia).await;
    ready(&ib).await;
    wait(|| gate().in_flight == 0 && gate().waiting == 0).await;
    ia.write().unwrap().close();
    ib.write().unwrap().close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifo_background_load_does_not_starve_search_context_or_release_slots_early() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (a, ia) = project(&e, "review/fairness").await;
    let (b, ib) = project(&e, "review/fairness").await;
    let (c, ic) = project(&e, "review/fairness").await;
    let start = e.hold();
    change(a.path(), 104);
    build(&ia).await;
    change(b.path(), 105);
    build(&ib).await;
    wait(|| e.count() == start + 2).await;
    assert!(!e.query(start));
    assert!(!e.query(start + 1));
    let q1 = search(ia.clone(), "bg-one");
    wait(|| gate().waiting == 1).await;
    let q2 = context(ib.clone(), "bg-two");
    wait(|| gate().waiting == 2).await;
    change(c.path(), 106);
    build(&ic).await;
    assert_eq!(
        gate().waiting,
        2,
        "third background job must wait outside provider gate for its class slot"
    );
    e.release(start);
    wait(|| e.count() == start + 3).await;
    assert!(
        e.query(start + 2),
        "queued search must precede fresh background provider admission"
    );
    e.release(start + 1);
    wait(|| e.count() == start + 4).await;
    assert!(
        e.query(start + 3),
        "queued context must precede fresh background provider admission"
    );
    e.release_all();
    finish(q1).await;
    finish(q2).await;
    ready(&ia).await;
    ready(&ib).await;
    ready(&ic).await;
    wait(|| gate().in_flight == 0 && gate().waiting == 0).await;
    for i in [&ia, &ib, &ic] {
        i.write().unwrap().close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_config_change_requires_explicit_set_project_and_close_fences_old_publication() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (root, index) = project(&e, "review/model-a").await;
    let old = index.read().unwrap().semantic_runtime().unwrap();
    let database = db(&index);
    let old_space = database.semantic_active_space().unwrap().unwrap();
    config(root.path(), &e, "review/model-b");
    let start = e.hold();
    change(root.path(), 107);
    build(&index).await;
    wait(|| e.count() == start + 1).await;
    assert_eq!(e.model(start), "review/model-a");
    assert_eq!(
        database.semantic_active_space().unwrap().unwrap(),
        old_space
    );
    index
        .write()
        .unwrap()
        .set_project(root.path(), false)
        .unwrap();
    let new_space = index
        .read()
        .unwrap()
        .semantic_subsystem()
        .unwrap()
        .space
        .digest()
        .unwrap()
        .as_str()
        .to_owned();
    assert_ne!(new_space, old_space);
    let pending = status(&index);
    assert_eq!(pending["retrieval"]["semantic_state"], "backfilling");
    assert_eq!(pending["retrieval"]["dense_published"], 0);
    assert!(
        !old.schedule(),
        "retired configuration owner must not switch space"
    );
    let current = index.read().unwrap().semantic_runtime().unwrap();
    assert!(current.schedule());
    wait(|| e.count() == start + 2).await;
    assert_eq!(e.model(start + 1), "review/model-b");
    assert_eq!(
        database.semantic_active_space().unwrap().unwrap(),
        new_space
    );
    assert_eq!(
        database
            .semantic_space_state(&old_space)
            .unwrap()
            .unwrap()
            .0,
        cc_db::semantic_space_switch::SpaceState::Revoked
    );
    assert_eq!(
        database
            .semantic_space_state(&new_space)
            .unwrap()
            .unwrap()
            .0,
        cc_db::semantic_space_switch::SpaceState::Active
    );
    let audit = rusqlite::Connection::open(database.admin().db_path()).unwrap();
    let events: String = audit
        .query_row(
            "SELECT value FROM metadata WHERE key='semantic_space_switch_log'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let events: Value = serde_json::from_str(&events).unwrap();
    let transition = events.as_array().unwrap().last().unwrap();
    assert_eq!(transition["from"], old_space);
    assert_eq!(transition["to"], new_space);
    assert!(transition["revision"]
        .as_str()
        .unwrap()
        .starts_with("configured-doc-spec:"));
    assert_eq!(transition["pinned"], false);
    // Close while both old/new responses are held. Neither can publish late.
    index.write().unwrap().close();
    e.release_all();
    wait(|| gate().in_flight == 0 && index.read().unwrap().query_pins() == 0).await;
    let conn = rusqlite::Connection::open(database.admin().db_path()).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM semantic_manifest WHERE space_id=?1",
            [&new_space],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "new owner closed before provider return");
    let wrong: i64 = conn.query_row("SELECT COUNT(*) FROM semantic_manifest WHERE space_id=?1 AND doc_version IN (SELECT doc_version FROM document_manifest)", [&old_space], |r|r.get(0)).unwrap();
    assert_eq!(
        wrong, 0,
        "old owner cannot publish current document after retirement"
    );
    assert!(!old.schedule());
    assert!(!current.schedule());
    index.write().unwrap().reopen().unwrap();
    let recovered = index.read().unwrap().semantic_runtime().unwrap();
    assert!(recovered.schedule());
    ready(&index).await;
    assert_eq!(
        database.semantic_active_space().unwrap().unwrap(),
        new_space
    );
    index.write().unwrap().close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_during_begin_immediate_wait_prevents_configured_space_transition() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (root, index) = project(&e, "review/txn-a").await;
    let database = db(&index);
    let active = database.semantic_active_space().unwrap();
    config(root.path(), &e, "review/txn-b");
    index
        .write()
        .unwrap()
        .set_project(root.path(), false)
        .unwrap();
    let owner = index.read().unwrap().semantic_runtime().unwrap();
    let writer = rusqlite::Connection::open(database.admin().db_path()).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let before = e.count();
    assert!(owner.schedule());
    // Busy SQLite writer excludes COMMIT; wait long enough for actual worker
    // ownership, then close must complete without waiting on the SQL blocker.
    wait(|| index.read().unwrap().query_pins() == 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::time::timeout(
        Duration::from_millis(500),
        tokio::task::spawn_blocking({
            let index = index.clone();
            move || index.write().unwrap().close()
        }),
    )
    .await
    .unwrap()
    .unwrap();
    writer.execute_batch("ROLLBACK;").unwrap();
    wait(|| {
        !service_factory::semantic_provider_gate()
            .snapshot()
            .per_project_in_flight
            .values()
            .any(|&n| n > 0)
    })
    .await;
    wait(|| index.read().unwrap().query_pins() == 0).await;
    assert_eq!(database.semantic_active_space().unwrap(), active);
    assert_eq!(e.count(), before);
    assert!(!owner.schedule());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aborted_production_queries_retain_physical_slots_pins_and_provider_gate() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (_root, index) = project(&e, "review/physical-exit").await;
    wait(|| index.read().unwrap().query_pins() == 0).await;
    let subsystem = index.read().unwrap().semantic_subsystem().unwrap();
    let start = e.hold();
    let q1 = search(index.clone(), "aborted-one");
    let q2 = search(index.clone(), "aborted-two");
    wait(|| e.count() == start + 2).await;
    q1.abort();
    q2.abort();
    assert!(q1.await.unwrap_err().is_cancelled());
    assert!(q2.await.unwrap_err().is_cancelled());
    // Each detached blocking encoding retains its separate pin, gate permit,
    // and foreground slot although the public waiting future has exited.
    assert_eq!(index.read().unwrap().query_pins(), 2);
    assert_eq!(gate().in_flight, 2);
    let refused = tokio::time::timeout(
        Duration::from_secs(1),
        search(index.clone(), "while-detached"),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let lane = refused["evidence_summary"]["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["lane_id"] == "semantic")
        .unwrap();
    assert_eq!(lane["status"], "unavailable");
    assert_eq!(lane["truncation_reason"], "semantic_capacity");
    assert_eq!(e.count(), start + 2);
    assert_eq!(gate().in_flight, 2);
    assert_eq!(index.read().unwrap().query_pins(), 2);
    assert!(subsystem.query_cache.is_empty());
    e.release_all();
    wait(|| index.read().unwrap().query_pins() == 0 && gate().in_flight == 0).await;
    assert!(
        subsystem.query_cache.is_empty(),
        "abandoned HTTP responses cannot prime the query cache"
    );
    finish(search(index.clone(), "after-physical-exit")).await;
    assert_eq!(e.count(), start + 3);
    assert_eq!(subsystem.query_cache.len(), 1);
    index.write().unwrap().close();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closed_background_owner_retains_slot_and_pin_until_real_http_exit() {
    let _serial = SERIAL.lock().await;
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::new(cache.path());
    let e = Endpoint::new();
    let (a, ia) = project(&e, "review/background-exit").await;
    let (b, ib) = project(&e, "review/background-exit").await;
    let (c, ic) = project(&e, "review/background-exit").await;
    wait(|| {
        [&ia, &ib, &ic]
            .iter()
            .all(|i| i.read().unwrap().query_pins() == 0)
    })
    .await;
    let start = e.hold();
    change(a.path(), 108);
    build(&ia).await;
    wait(|| e.count() == start + 1).await;
    change(b.path(), 109);
    build(&ib).await;
    wait(|| e.count() == start + 2).await;
    ia.write().unwrap().close();
    assert_eq!(ia.read().unwrap().query_pins(), 1);
    assert_eq!(gate().in_flight, 2);
    change(c.path(), 110);
    build(&ic).await;
    wait(|| ic.read().unwrap().query_pins() == 1).await;
    // Give the scheduled async class waiter a chance to run. Premature class
    // release would appear as a third provider-gate waiter (or HTTP request).
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(e.count(), start + 2);
    assert_eq!(gate().waiting, 0);
    assert_eq!(ia.read().unwrap().query_pins(), 1);
    e.release(start);
    wait(|| e.count() == start + 3).await;
    assert!(!e.query(start + 2));
    wait(|| ia.read().unwrap().query_pins() == 0).await;
    e.release_all();
    ready(&ib).await;
    ready(&ic).await;
    wait(|| {
        [&ia, &ib, &ic]
            .iter()
            .all(|i| i.read().unwrap().query_pins() == 0)
            && gate().in_flight == 0
    })
    .await;
    ib.write().unwrap().close();
    ic.write().unwrap().close();
}
