//! Finite synthetic acceptance tests. Every global-policy case runs in its own process.
use super::*;
use crate::{semantic_wiring, service_factory};
use cc_model::config::ProjectConfig;
use cc_model::{query::QueryControl, CcError};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Hold {
    state: Mutex<(usize, bool)>,
    wake: std::sync::Condvar,
}
impl Hold {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 += 1;
        self.wake.notify_all();
        let end = Instant::now() + Duration::from_secs(10);
        while !state.1 {
            let (next, timeout) = self
                .wake
                .wait_timeout(state, end.saturating_duration_since(Instant::now()))
                .unwrap();
            state = next;
            assert!(!timeout.timed_out(), "synthetic HTTP hold expired");
        }
    }
    fn wait(&self, count: usize) {
        let mut state = self.state.lock().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while state.0 < count {
            let (next, timeout) = self
                .wake
                .wait_timeout(state, end.saturating_duration_since(Instant::now()))
                .unwrap();
            state = next;
            assert!(!timeout.timed_out(), "synthetic HTTP calls did not enter");
        }
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
    fn calls(&self) -> usize {
        self.state.lock().unwrap().0
    }
}
struct Loopback {
    endpoint: String,
    hold: Arc<Hold>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Loopback {
    fn new() -> Self {
        Self::with_one_failure(false)
    }
    fn with_one_failure(fail_first: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let hold = Arc::new(Hold::default());
        let stop = Arc::new(AtomicBool::new(false));
        let worker_hold = hold.clone();
        let worker_stop = stop.clone();
        let sequence = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker = std::thread::spawn(move || {
            std::thread::scope(|scope| {
                let end = Instant::now() + Duration::from_secs(15);
                while !worker_stop.load(Ordering::Acquire) && Instant::now() < end {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let hold = worker_hold.clone();
                            let sequence = sequence.clone();
                            scope.spawn(move || {
                                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                                let mut bytes = vec![];
                                loop {
                                    let mut buf = [0; 2048];
                                    let n = stream.read(&mut buf).unwrap();
                                    assert!(n > 0);
                                    bytes.extend_from_slice(&buf[..n]);
                                    if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                                        let headers = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                                        let length: usize = headers.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                                        if bytes.len() >= split + 4 + length { break; }
                                    }
                                }
                                let split = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
                                let request: serde_json::Value = serde_json::from_slice(&bytes[split + 4..]).unwrap();
                                let inputs = request["input"].as_array().unwrap().len();
                                let first = sequence.fetch_add(1, Ordering::SeqCst) == 0;
                                hold.enter();
                                let status = if fail_first && first { "503 Service Unavailable" } else { "200 OK" };
                                let body = serde_json::json!({"model":"synthetic-gate", "data": (0..inputs).map(|index| serde_json::json!({"index":index,"embedding":[1.0,0.0]})).collect::<Vec<_>>()}).to_string();
                                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(1))
                        }
                        Err(error) => panic!("synthetic listener: {error}"),
                    }
                }
            });
        });
        Self {
            endpoint,
            hold,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Loopback {
    fn drop(&mut self) {
        self.hold.release();
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn config() -> ProjectConfig {
    let mut config = ProjectConfig::default();
    config.semantic.enabled = true;
    config.semantic.model_id = "synthetic-gate".into();
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(16);
    config.semantic.endpoint = "https://semantic.invalid/v1".into();
    config
}
fn assemble(
    dir: &std::path::Path,
    name: &str,
    config: &ProjectConfig,
) -> CcResult<(Arc<IndexDb>, Arc<SemanticSubsystem>)> {
    let db = Arc::new(IndexDb::open(&dir.join(format!("{name}.db")))?.0);
    let subsystem = semantic_wiring::assemble_with(
        name,
        config,
        db.clone(),
        |key| {
            (key == cc_semantic::cache::CACHE_ROOT_ENV)
                .then(|| dir.join("cache").to_string_lossy().into_owned())
        },
        false,
    )?
    .unwrap();
    Ok((db, Arc::new(subsystem)))
}
fn runtime(dir: &std::path::Path, name: &str, config: &ProjectConfig) -> Arc<SemanticRuntime> {
    let (db, subsystem) = assemble(dir, name, config).unwrap();
    from_config(
        db,
        subsystem,
        Arc::new(QueryServices::default()),
        &config.semantic,
    )
    .unwrap()
    .unwrap()
}
fn assert_project_init_rejects(dir: &std::path::Path, name: &str, config: &ProjectConfig) {
    let project = dir.join(name);
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join(".codecortex.json"),
        serde_json::to_vec(config).unwrap(),
    )
    .unwrap();
    let mut index = crate::engine::CodeIndex::empty();
    assert!(matches!(
        index.set_project(&project, false),
        Err(CcError::Config(_))
    ));
    assert!(
        index.semantic_subsystem().is_none(),
        "failed configuration cannot publish subsystem state"
    );
}

fn query_provider(runtime: &SemanticRuntime) -> Arc<dyn EmbeddingProvider> {
    let context = runtime.query_encoding.lock().unwrap().clone().unwrap();
    (context.factory)(
        QueryControl::new(Duration::from_secs(5)).unwrap(),
        runtime.cancellation.clone(),
    )
    .unwrap()
}
fn call(provider: &dyn EmbeddingProvider, query: bool) -> Result<Vec<Vec<f32>>, ProviderError> {
    if query {
        provider.embed_queries(&[QueryInput::from_bytes(b"synthetic query").unwrap()])
    } else {
        provider.embed_documents(&[DocumentInput::from_bytes(b"synthetic document").unwrap()])
    }
}

#[test]
fn shared_provider_gate_cases_in_fresh_processes() {
    for name in [
        "production_wrappers_adopt_and_enforce",
        "busy_config_errors_propagate",
        "concurrent_registry_init_and_acquire",
        "disabled_is_inert",
        "combined_queue_serial",
        "combined_queue_width_one",
        "combined_queue_width_two",
        "combined_production_close",
    ] {
        let name = format!("semantic_runtime::shared_provider_gate_tests::{name}");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name, "--ignored", "--nocapture"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("1 passed"),
            "child case did not execute: {stdout}"
        );
        print!("{stdout}");
    }
}

#[test]
#[ignore = "invoked by isolated parent harness"]
fn production_wrappers_adopt_and_enforce() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let server = Loopback::new();
    let key_path = dir.path().join("synthetic-key");
    std::fs::write(&key_path, "synthetic-only").unwrap();
    let mut default = config();
    default.semantic.network_opt_in = true;
    default.semantic.allow_query_network = true;
    default.semantic.allow_http = true;
    default.semantic.endpoint = server.endpoint.clone();
    default.semantic.api_key_ref = Some(format!("file:{}", key_path.display()));
    default.semantic.acquire_timeout_ms = 60;
    let old_a = runtime(dir.path(), "a", &default);
    let old_b = runtime(dir.path(), "b", &default);
    // Actual production doc and query factories, constructed BEFORE adoption.
    let a_doc = old_a.resolve_provider().unwrap();
    let a_query = query_provider(&old_a);
    let b_doc = old_b.resolve_provider().unwrap();
    let b_query = query_provider(&old_b);
    let shared = service_factory::semantic_provider_gate();
    assert_eq!(shared.snapshot().max_concurrent, usize::MAX);
    let mut explicit = default.clone();
    explicit.semantic.max_concurrent = 4;
    explicit.semantic.max_concurrent_per_project = 2;
    assemble(dir.path(), "explicit", &explicit).unwrap();
    let effective = service_factory::semantic_provider_gate();
    assert!(Arc::ptr_eq(&shared, &effective));
    assert_eq!(effective.snapshot().max_concurrent, 4);
    assert_eq!(effective.snapshot().max_concurrent_per_project, Some(2));
    assemble(dir.path(), "same", &explicit).unwrap();
    let after = runtime(dir.path(), "after-default", &default);
    let after_doc = after.resolve_provider().unwrap();
    assert_eq!(effective.snapshot().max_concurrent, 4);
    let mut conflict = explicit.clone();
    conflict.semantic.max_concurrent = 5;
    assert!(matches!(
        assemble(dir.path(), "conflict", &conflict),
        Err(CcError::Config(_))
    ));
    assert_project_init_rejects(dir.path(), "project-conflict", &conflict);
    std::thread::scope(|scope| {
        let a1 = scope.spawn(|| call(a_doc.as_ref(), false));
        let a2 = scope.spawn(|| call(a_query.as_ref(), true));
        server.hold.wait(2);
        assert_eq!(call(a_doc.as_ref(), false), Err(ProviderError::Timeout));
        assert_eq!(server.hold.calls(), 2, "per-project refusal precedes HTTP");
        let b1 = scope.spawn(|| call(b_query.as_ref(), true));
        server.hold.wait(3); // Neighbor receives a slot while A's share is full.
        let b2 = scope.spawn(|| call(b_doc.as_ref(), false));
        server.hold.wait(4);
        let snapshot = effective.snapshot();
        assert_eq!(snapshot.in_flight, 4);
        assert_eq!(
            snapshot.per_project_in_flight[&old_a.subsystem.namespace],
            2
        );
        assert_eq!(
            snapshot.per_project_in_flight[&old_b.subsystem.namespace],
            2
        );
        assert_eq!(call(after_doc.as_ref(), false), Err(ProviderError::Timeout));
        assert_eq!(server.hold.calls(), 4, "global refusal precedes HTTP");
        server.hold.release();
        for worker in [a1, a2, b1, b2] {
            worker.join().unwrap().unwrap();
        }
    });
    assert_eq!(effective.snapshot().in_flight, 0);
    assert!(effective.snapshot().per_project_in_flight.is_empty());
    call(after_doc.as_ref(), false).unwrap();
    assert_eq!(server.hold.calls(), 5);
    println!("PRODUCTION_GATE upper_global=4 upper_project=2 actual_http_held=4 old_default_doc_and_query=true neighbor=true default_after_explicit=true arc_identity=true released=0");
}

#[test]
#[ignore = "invoked by isolated parent harness"]
fn busy_config_errors_propagate() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let default = config();
    let (db, subsystem) = assemble(dir.path(), "default", &default).unwrap();
    let gate = service_factory::semantic_provider_gate();
    let permit = gate.try_acquire_permit("old", Duration::ZERO).unwrap();
    let mut explicit = default.clone();
    explicit.semantic.max_concurrent = 4;
    explicit.semantic.max_concurrent_per_project = 2;
    assert!(matches!(
        assemble(dir.path(), "busy", &explicit),
        Err(CcError::Config(_))
    ));
    assert_project_init_rejects(dir.path(), "project-busy", &explicit);
    explicit.semantic.network_opt_in = true;
    assert!(matches!(
        from_config(
            db,
            subsystem,
            Arc::new(QueryServices::default()),
            &explicit.semantic
        ),
        Err(CcError::Config(_))
    ));
    assert_eq!(gate.snapshot().in_flight, 1);
    assert_eq!(gate.snapshot().max_concurrent, usize::MAX);
    drop(permit);
    assemble(dir.path(), "idle", &explicit).unwrap();
    assert_eq!(gate.snapshot().in_flight, 0);
    assert_eq!(gate.snapshot().max_concurrent, 4);
    println!(
        "BUSY_CONFIG wiring_runtime_and_project_errors=true old_permit_release=true retry_adoption=true"
    );
}

#[test]
#[ignore = "invoked by isolated parent harness"]
fn concurrent_registry_init_and_acquire() {
    let gate = service_factory::semantic_provider_gate();
    let limits = cc_semantic::admission::GateLimits::validated(4, Some(2)).unwrap();
    let start = Arc::new(std::sync::Barrier::new(3));
    let finish = Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        let init = scope.spawn(|| {
            start.wait();
            service_factory::init_semantic_provider_gate(limits)
        });
        let acquire = scope.spawn(|| {
            start.wait();
            let permit = gate.try_acquire_permit("old", Duration::ZERO).unwrap();
            finish.wait();
            drop(permit);
        });
        start.wait();
        match init.join().unwrap() {
            Ok(configured) => {
                assert!(Arc::ptr_eq(&configured, &gate));
                assert_eq!(gate.snapshot().max_concurrent, 4);
            }
            Err(CcError::Config(_)) => {
                assert_eq!(gate.snapshot().in_flight, 1);
                assert_eq!(gate.snapshot().max_concurrent, usize::MAX);
            }
            Err(error) => panic!("unexpected {error}"),
        }
        finish.wait();
        acquire.join().unwrap();
    });
    let configured = service_factory::init_semantic_provider_gate(limits).unwrap();
    assert!(Arc::ptr_eq(&configured, &gate));
    assert_eq!(configured.snapshot().in_flight, 0);
    println!("REGISTRY_RACE same_arc=true linearized_adoption_or_busy=true released=0");
}

#[test]
#[ignore = "invoked by isolated parent harness"]
fn disabled_is_inert() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let db = Arc::new(IndexDb::open(&dir.path().join("disabled.db")).unwrap().0);
    let config = ProjectConfig::default();
    assert!(semantic_wiring::assemble_with(
        "disabled",
        &config,
        db,
        |_| panic!("disabled cache lookup"),
        false
    )
    .unwrap()
    .is_none());
    assert!(
        crate::semantic_provider_factory::build_embedding_provider_with(
            &config.semantic,
            |_| panic!("disabled credential lookup"),
            || panic!("disabled transport build")
        )
        .unwrap()
        .is_none()
    );
    assert!(!dir.path().join("cache").exists());
    println!(
        "DEFAULT_DISABLED cache_lookups=0 credential_lookups=0 transport_builds=0 provider_calls=0"
    );
}

// Self-authored finite combination: real production HTTP factories, shared gate,
// document store, FIFO queue and runtime round. Only synthetic /tmp data/cache.
fn wait_gate_waiters(gate: &cc_semantic::admission::ProviderGate, count: usize) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let waiting = gate.snapshot().waiting;
        if waiting == count {
            return;
        }
        assert!(waiting < count, "gate waiter bound exceeded");
        assert!(
            Instant::now() < end,
            "local handlers did not reach HTTP admission"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_claimed(worker: &SemanticRuntime, count: i64) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let conn = worker.db.read_conn().unwrap();
        let claimed: i64 = conn
            .query_row(
                "SELECT count(*) FROM semantic_outbox WHERE state='claimed'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if claimed == count {
            return;
        }
        assert!(claimed < count, "local attempt budget exceeded: {claimed}");
        assert!(
            Instant::now() < end,
            "local attempts did not enter: {claimed}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn combined_queue(width: u32) {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let server = Loopback::with_one_failure(true);
    let key = dir.path().join("synthetic-key");
    std::fs::write(&key, "synthetic-only").unwrap();
    let mut cfg = config();
    cfg.auto_index.enabled = false;
    cfg.semantic.network_opt_in = true;
    cfg.semantic.allow_query_network = true;
    cfg.semantic.allow_http = true;
    cfg.semantic.endpoint = server.endpoint.clone();
    cfg.semantic.api_key_ref = Some(format!("file:{}", key.display()));
    cfg.semantic.acquire_timeout_ms = 1000;
    cfg.semantic.retry_max_attempts = 1;
    // Initialize actual default wrappers first. Explicit caps are adopted later.
    let neighbor = runtime(dir.path(), "neighbor", &cfg);
    let neighbor_doc = neighbor.resolve_provider().unwrap();
    let neighbor_query = query_provider(&neighbor);
    cfg.semantic.max_concurrent_per_project = width;
    std::fs::write(
        dir.path().join(".codecortex.json"),
        serde_json::to_vec(&cfg).unwrap(),
    )
    .unwrap();
    for n in 0..20 {
        std::fs::write(
            dir.path().join(format!("combo_{n}.rs")),
            format!("pub fn combo_{n}() -> u32 {{ {n} }}\n"),
        )
        .unwrap();
    }
    let index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
    let retired = index.semantic_runtime().unwrap();
    retired.close();
    let index = Arc::new(std::sync::RwLock::new(index));
    crate::handlers::core::build_index(index, true).unwrap();
    // Only the overflow probe should exhaust its budget while HTTP is held.
    // With the worker's same 1 s budget, its two earlier gate waiters time
    // out before this later probe, introducing two unintended retry tasks.
    // Keep the worker and neighbor budgets unchanged and use the production
    // document factory with the same namespace and shared provider gate.
    let mut overflow_config = cfg.semantic.clone();
    overflow_config.acquire_timeout_ms = 20;
    let overflow = from_config(
        retired.db.clone(),
        retired.subsystem.clone(),
        Arc::new(QueryServices::default()),
        &overflow_config,
    )
    .unwrap()
    .unwrap();
    let doc = overflow.resolve_provider().unwrap();
    let worker = from_config(
        retired.db.clone(),
        retired.subsystem.clone(),
        Arc::new(QueryServices::default()),
        &cfg.semantic,
    )
    .unwrap()
    .unwrap();
    let query = query_provider(&worker);
    let mut explicit = cfg.clone();
    explicit.semantic.max_concurrent = 4;
    explicit.semantic.max_concurrent_per_project = 2;
    assemble(dir.path(), "adopt", &explicit).unwrap();
    let gate = service_factory::semantic_provider_gate();
    let actual_width = if width < 2 { 1 } else { 2 };
    std::thread::scope(|scope| {
        let round = scope.spawn(|| worker.run_round().unwrap());
        server.hold.wait(actual_width);
        let local_width = if width < 2 { 1 } else { 4 };
        wait_claimed(&worker, local_width);
        wait_gate_waiters(&gate, (local_width as usize).saturating_sub(actual_width));
        assert_eq!(
            gate.snapshot().per_project_in_flight[&worker.subsystem.namespace],
            actual_width
        );
        assert_eq!(
            server.hold.calls(),
            actual_width,
            "HTTP quota remains separate from local attempts"
        );
        let query_call = scope.spawn(|| {
            if width < 2 {
                call(query.as_ref(), true)
            } else {
                call(neighbor_query.as_ref(), true)
            }
        });
        server.hold.wait(actual_width + 1);
        let neighbor_call = scope.spawn(|| call(neighbor_doc.as_ref(), false));
        server.hold.wait(actual_width + 2);
        let extra = (width < 2).then(|| scope.spawn(|| call(neighbor_query.as_ref(), true)));
        server.hold.wait(4);
        let snap = gate.snapshot();
        assert_eq!(snap.max_concurrent, 4);
        assert_eq!(snap.max_concurrent_per_project, Some(2));
        assert_eq!(snap.in_flight, 4);
        assert!(snap.per_project_in_flight.values().all(|count| *count <= 2));
        assert_eq!(
            snap.waiting,
            (local_width as usize).saturating_sub(actual_width)
        );
        assert_eq!(call(doc.as_ref(), false), Err(ProviderError::Timeout));
        assert_eq!(server.hold.calls(), 4);
        assert_eq!(
            gate.snapshot().waiting,
            snap.waiting,
            "the overflow probe must leave the original worker waiters pending"
        );
        server.hold.release();
        assert!(
            round.join().unwrap(),
            "first batch fills the finite 16-claim budget"
        );
        query_call.join().unwrap().unwrap();
        neighbor_call.join().unwrap().unwrap();
        if let Some(extra) = extra {
            extra.join().unwrap().unwrap();
        }
    });
    let counts = || {
        let conn = worker.db.read_conn().unwrap();
        conn.query_row("SELECT count(*),sum(attempt_count),sum(state='done'),sum(state='pending' AND attempt_count=0),sum(state='pending' AND attempt_count=1 AND available_at>unixepoch('now')),sum(state='claimed') FROM semantic_outbox", [], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?))).unwrap()
    };
    assert_eq!(counts(), (20, 16, 15, 4, 1, 0));
    assert!(!worker.run_round().unwrap());
    assert_eq!(counts(), (20, 20, 19, 0, 1, 0));
    let before = server.hold.calls();
    assert!(!worker.run_round().unwrap());
    assert_eq!(server.hold.calls(), before, "backoff cannot be paid early");
    assert_eq!(gate.snapshot().in_flight, 0);
    worker.close();
    assert!(worker.cancellation.is_cancelled());
    assert!(!worker.run_round().unwrap());
    assert_eq!(server.hold.calls(), before);
    assert_eq!(counts(), (20, 20, 19, 0, 1, 0));
    let conn = worker.db.read_conn().unwrap();
    let bad: i64 = conn.query_row("SELECT count(*) FROM semantic_outbox WHERE state='pending' AND lease_token IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(bad, 0);
    println!("COMBINED width={width} actual_width={actual_width} global=4 project=2 first_claims=16 first_done=15 ready_after_retry=4 final_done=19 backoff=1 early_retry_calls=0 closed_calls=0 live_claims=0 gate_after_join=0");
}
#[test]
#[ignore = "invoked by isolated parent harness"]
fn combined_queue_serial() {
    combined_queue(0);
}
#[test]
#[ignore = "invoked by isolated parent harness"]
fn combined_queue_width_one() {
    combined_queue(1);
}
#[test]
#[ignore = "invoked by isolated parent harness"]
fn combined_queue_width_two() {
    combined_queue(2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "invoked by isolated parent harness"]
async fn combined_production_close() {
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let server = Loopback::new();
    let key = dir.path().join("synthetic-key");
    std::fs::write(&key, "synthetic-only").unwrap();
    let mut cfg = config();
    cfg.auto_index.enabled = false;
    cfg.semantic.network_opt_in = true;
    cfg.semantic.allow_query_network = true;
    cfg.semantic.allow_http = true;
    cfg.semantic.endpoint = server.endpoint.clone();
    cfg.semantic.api_key_ref = Some(format!("file:{}", key.display()));
    cfg.semantic.max_concurrent = 4;
    cfg.semantic.max_concurrent_per_project = 2;
    std::fs::write(
        dir.path().join(".codecortex.json"),
        serde_json::to_vec(&cfg).unwrap(),
    )
    .unwrap();
    for n in 0..4 {
        std::fs::write(
            dir.path().join(format!("close_{n}.rs")),
            format!("pub fn close_{n}() -> u32 {{ {n} }}\n"),
        )
        .unwrap();
    }
    let index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
    let retired = index.semantic_runtime().unwrap();
    retired.close();
    let index = Arc::new(std::sync::RwLock::new(index));
    tokio::task::spawn_blocking(move || crate::handlers::core::build_index(index, true))
        .await
        .unwrap()
        .unwrap();
    let worker = from_config(
        retired.db.clone(),
        retired.subsystem.clone(),
        Arc::new(QueryServices::default()),
        &cfg.semantic,
    )
    .unwrap()
    .unwrap();
    assert!(worker.schedule());
    let hold = server.hold.clone();
    tokio::task::spawn_blocking(move || hold.wait(2))
        .await
        .unwrap();
    let gate = service_factory::semantic_provider_gate();
    wait_claimed(&worker, 4);
    wait_gate_waiters(&gate, 2);
    assert_eq!(gate.snapshot().in_flight, 2);
    assert_eq!(server.hold.calls(), 2);
    assert!(worker.running.load(Ordering::Acquire));
    assert_eq!(worker.services.query_pins(), 1);
    worker.close();
    assert!(worker.cancellation.is_cancelled());
    assert!(!worker.schedule());
    server.hold.release();
    let end = Instant::now() + Duration::from_secs(5);
    while worker.running.load(Ordering::Acquire) {
        assert!(
            Instant::now() < end,
            "real provider workers did not physically join"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(gate.snapshot().in_flight, 0);
    assert_eq!(worker.services.query_pins(), 0);
    assert_eq!(server.hold.calls(), 2);
    let conn = worker.db.read_conn().unwrap();
    let counts: (i64,i64,i64,i64) = conn.query_row("SELECT sum(attempt_count),sum(state='claimed'),sum(state='done'),sum(state='pending' AND lease_token IS NOT NULL) FROM semantic_outbox", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(counts, (4, 0, 0, 0));
    assert_eq!(
        worker
            .db
            .reads()
            .semantic_coverage()
            .unwrap()
            .coverage
            .published,
        0
    );
    println!("COMBINED_CLOSE actual_http=2 cancel=true running_before=1 pins_before=1 after_join_running=0 pins=0 gate=0 local_attempts=4 started=4 claimed=0 done=0 pending_token=0 published=0");
}
