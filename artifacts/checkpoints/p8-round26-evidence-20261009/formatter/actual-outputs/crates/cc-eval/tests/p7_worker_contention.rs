//! P7-015: fixed-seed measurements at the actual post-index worker boundary.
//! Fake vectors establish scheduling and fencing only. The existing two-second
//! query/five-second progress watchdogs are functional controls, not a new SLA.
#![cfg(feature = "semantic")]

use cc_db::index_db::IndexDb;
use cc_model::{
    config::ProjectConfig,
    query::{QueryControl, RetrievalStrategy},
    search::SearchRequest,
};
use cc_semantic::{
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::fake::{FakeProvider, FakeProviderConfig},
    types::VectorSpace,
};
use cc_server::{
    engine::CodeIndex,
    handlers::{core, SharedCodeIndex},
    query_handle::capture_for_request,
    service_factory,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Condvar, Mutex, RwLock},
    time::{Duration, Instant},
};
use tracing::Instrument;

const SEEDS: [u64; 3] = [7, 19, 43];
const SAMPLES_PER_CELL: usize = 32;
const CONCURRENCIES: [usize; 4] = [1, 4, 8, 16];
const LOCAL_QUERY_BOUND: Duration = Duration::from_secs(2);
const PROGRESS_BOUND: Duration = Duration::from_secs(5);
const LOCAL_ATTEMPT_WIDTH: usize = 4;

#[derive(Default)]
struct GateState {
    held: bool,
    entered: usize,
    active: usize,
    waiting: usize,
    maximum_active: usize,
    held_inputs: BTreeSet<String>,
}

struct SlowProvider {
    inner: FakeProvider,
    gate: (Mutex<GateState>, Condvar),
}
impl SlowProvider {
    fn new(space: VectorSpace) -> Self {
        let mut config = FakeProviderConfig::new(space);
        config.delay_per_call = Duration::from_millis(10);
        Self {
            inner: FakeProvider::new(config),
            gate: (Mutex::new(GateState::default()), Condvar::new()),
        }
    }
    fn hold(&self) {
        let mut state = self.gate.0.lock().unwrap();
        assert_eq!(state.active, 0);
        state.held = true;
    }
    fn release(&self) {
        self.gate.0.lock().unwrap_or_else(|p| p.into_inner()).held = false;
        self.gate.1.notify_all();
    }
    fn snapshot(&self) -> Value {
        let state = self.gate.0.lock().unwrap();
        json!({"entered":state.entered,"active":state.active,"waiting":state.waiting,
               "maximum_active":state.maximum_active,"held_inputs":state.held_inputs,
               "fake_completed_calls":self.inner.call_count()})
    }
}
impl EmbeddingProvider for SlowProvider {
    fn space(&self) -> &VectorSpace {
        self.inner.space()
    }
    fn embed_documents(&self, inputs: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let mut state = self.gate.0.lock().unwrap();
        state.entered += 1;
        state.active += 1;
        state.maximum_active = state.maximum_active.max(state.active);
        if state.held {
            state.waiting += 1;
            state.held_inputs.extend(
                inputs
                    .iter()
                    .map(|input| input.input_digest.as_str().to_owned()),
            );
            self.gate.1.notify_all();
            state = self.gate.1.wait_while(state, |state| state.held).unwrap();
            state.waiting -= 1;
        }
        drop(state);
        let result = self.inner.embed_documents(inputs);
        self.gate.0.lock().unwrap().active -= 1;
        self.gate.1.notify_all();
        result
    }
    fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.inner.embed_queries(inputs)
    }
}

struct CacheEnv(Option<std::ffi::OsString>);
impl CacheEnv {
    fn new(path: &Path) -> Self {
        let previous = std::env::var_os(cc_semantic::cache::CACHE_ROOT_ENV);
        std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, path);
        Self(previous)
    }
}
impl Drop for CacheEnv {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, value),
            None => std::env::remove_var(cc_semantic::cache::CACHE_ROOT_ENV),
        }
    }
}

struct Cleanup {
    index: SharedCodeIndex,
    provider: Arc<SlowProvider>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.index
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .close();
        self.provider.release();
    }
}

fn write(root: &Path, path: &str, value: impl AsRef<[u8]>) {
    std::fs::write(root.join(path), value).unwrap();
}

fn save(seed: u64, name: &str, value: &Value) {
    if let Some(directory) = std::env::var_os("CODECORTEX_WORKER_CONTENTION_EVIDENCE_DIR") {
        let directory = std::path::PathBuf::from(directory).join(format!("seed-{seed}"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

async fn wait(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(PROGRESS_BOUND, async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("original five-second progress watchdog");
}

async fn build(index: SharedCodeIndex, full: bool) -> u64 {
    let start = Instant::now();
    tokio::time::timeout(
        PROGRESS_BOUND,
        tokio::task::spawn_blocking(move || core::build_index(index, full)),
    )
    .await
    .expect("local indexing progresses while actual worker awaits provider")
    .unwrap()
    .unwrap();
    start.elapsed().as_micros() as u64
}

fn queue(db: &IndexDb) -> Value {
    let conn = db.read_conn().unwrap();
    let (total, pending, claimed, attempts): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(state='pending'),0), COALESCE(SUM(state='claimed'),0), COALESCE(SUM(attempt_count),0) FROM semantic_outbox",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).unwrap();
    drop(conn);
    let coverage = db.reads().semantic_coverage().unwrap().coverage;
    json!({"total":total,"pending":pending,"claimed":claimed,"attempt_count":attempts,
           "published":coverage.published,"uncovered":coverage.uncovered})
}

fn resources(stage: &str) -> Value {
    let pid = std::process::id();
    let raw = cc_eval::benchmark::sampler::sample(stage, None);
    let snapshot = cc_eval::benchmark::sampler::process_snapshot(pid);
    let usage = cc_eval::benchmark::sampler::current_process_usage();
    // RUSAGE_SELF identifies this process directly, independently of procfs's
    // PID namespace. All three components share this one resource owner. Keep
    // raw optional PID probes distinct and never sum shared-owner measurements.
    json!({
        "stage":stage,"topology":"runner, CodeIndex and fake provider share one process",
        "shared_process_owner":{"components":["test_runner","CodeIndex","fake_provider"],"usage":usage},
        "server":{"separate_pid":null,"status":"included in shared_process_owner; not separately summed"},
        "server_tree":{"separate_server_or_provider_process":false,
            "child_process_usage":null,"status":"server/provider share caller; transient git/probe children are not measured by RUSAGE_SELF"},
        "raw_sampler":raw,"raw_process_snapshot":snapshot,
        "current_executable":std::env::current_exe().ok(),
        "proc_executable":std::fs::read_link(format!("/proc/{pid}/exe")).ok(),
        "resource_gate":if usage.is_some(){"attributed_combined_process"}else{"unavailable"},
        "cpu_pool_slots":service_factory::query_pool().stats(),
        "no_zero_fill":true,"no_current_rss_or_component_utilization_claim":true,
    })
}

async fn db_control(db: Arc<IndexDb>) -> Value {
    tokio::time::timeout(LOCAL_QUERY_BOUND, tokio::task::spawn_blocking(move || {
        let generation = db.reads().read_generation().unwrap();
        let checkout_start = Instant::now();
        let reader = db.read_conn().unwrap();
        let read_checkout_us = checkout_start.elapsed().as_micros() as u64;
        let read_start = Instant::now();
        let count: i64 = reader.query_row("SELECT COUNT(*) FROM document_manifest", [], |row| row.get(0)).unwrap();
        let read_statement_us = read_start.elapsed().as_micros() as u64;
        drop(reader);
        let writer = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        // Original independent production fairness control: no provider wait
        // holds a writer transaction; the probe itself rolls back every time.
        writer.busy_timeout(Duration::from_millis(100)).unwrap();
        let writer_start = Instant::now();
        writer.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
        let writer_acquire_and_rollback_us = writer_start.elapsed().as_micros() as u64;
        let after = db.reads().read_generation().unwrap();
        assert_eq!(after, generation);
        json!({"single_read_pool_checkout_us":read_checkout_us,"read_statement_us":read_statement_us,
               "read_rows":count,"writer_acquire_and_rollback_us":writer_acquire_and_rollback_us,
               "writer_busy_timeout_ms":100,"generation_before":generation,"generation_after":after,
               "generation_unchanged":true})
    })).await.expect("single read connection and writer stay available during provider wait").unwrap()
}

async fn samples(index: SharedCodeIndex, seed: u64, phase: &'static str) -> Value {
    let mut rows = Vec::new();
    for concurrency in CONCURRENCIES {
        for first in (0..SAMPLES_PER_CELL).step_by(concurrency) {
            let mut tasks = Vec::new();
            for offset in 0..concurrency {
                let index = index.clone();
                let ordinal = first + offset;
                let diagnostic_span = cc_db::runtime_diagnostics::local_span(
                    seed,
                    phase,
                    ordinal as u64,
                    concurrency as u64,
                );
                let offered = Instant::now();
                let control = QueryControl::new(LOCAL_QUERY_BOUND).unwrap();
                tasks.push((ordinal, tokio::spawn(async move {
                    let start = Instant::now();
                    tokio::time::timeout(LOCAL_QUERY_BOUND, async {
                        let (handle, control) = capture_for_request(index, Some(control)).await.unwrap();
                        let captured = Instant::now();
                        let envelope = handle.search_async("stable_signal".into(), 4, None, SearchRequest {
                            retrieval_strategy:Some(RetrievalStrategy::Local),control:Some(control),..Default::default()
                        }).await.unwrap();
                        let retrieved = Instant::now();
                        let hits = envelope.machine_pack["hits"].as_array().unwrap();
                        assert!(hits.iter().any(|hit|hit["file_path"]=="stable.rs"), "real local source hit must survive held backfill");
                        json!({"ordinal":ordinal,"concurrency":concurrency,
                            "caller_schedule_us":start.duration_since(offered).as_micros() as u64,
                            "capture_admission_us":captured.duration_since(start).as_micros() as u64,
                            "retrieval_us":retrieved.duration_since(captured).as_micros() as u64,
                            "offered_to_api_return_us":retrieved.duration_since(offered).as_micros() as u64,
                            "originating_work":cc_eval::benchmark::sampler::retrieval_work(&envelope.machine_pack),
                            "hits":hits,"source_freshness":envelope.machine_pack.pointer("/evidence_summary/source_freshness"),
                            "scope":"real local API; no MCP transport or separated backend queue/service timing"})
                    }).await.expect("unchanged two-second local-query progress bound")
                }.instrument(diagnostic_span))));
            }
            let mut failures = Vec::new();
            for (ordinal, task) in tasks {
                match task.await {
                    Ok(row) => rows.push(row),
                    Err(error) => failures.push(json!({"ordinal":ordinal,"concurrency":concurrency,
                        "join_error":error.to_string(),"partial_stage_timings":"unavailable after task panic/cancellation"})),
                }
            }
            // Join the whole wave before asserting, so another task's failure
            // does not discard rows that have already completed successfully.
            save(seed, &format!("{phase}-requests"), &json!(rows));
            if !failures.is_empty() {
                save(seed, &format!("{phase}-request-failures"), &json!(failures));
            }
            assert!(
                failures.is_empty(),
                "local request wave failed: {failures:?}"
            );
        }
    }
    let cells: Vec<_> = CONCURRENCIES.into_iter().map(|concurrency| {
        let mut times:Vec<u64>=rows.iter().filter(|r|r["concurrency"]==concurrency)
            .map(|r|r["offered_to_api_return_us"].as_u64().unwrap()).collect();
        times.sort_unstable();
        assert_eq!(times.len(),SAMPLES_PER_CELL);
        let percentile=|p:usize|times[(p*times.len()).div_ceil(100)-1];
        json!({"concurrency":concurrency,"n":times.len(),"p50_us":percentile(50),"p95_us":percentile(95),"p99_us":percentile(99),"max_us":times.last(),
               "method":"all fixed samples; nearest rank; cache behavior not overridden; no CI or certified tail-latency gate"})
    }).collect();
    json!(cells)
}

async fn run(seed: u64) {
    save(
        seed,
        "protocol",
        &json!({"seed":seed,"mutable_files":24,"concurrency_cells":CONCURRENCIES,
        "samples_per_cell":SAMPLES_PER_CELL,"phases":["quiet","held"],"fake_delay_ms":10,
        "query_watchdog_ms":2000,"progress_watchdog_ms":5000,"writer_busy_timeout_ms":100,
        "worker_local_attempt_width":LOCAL_ATTEMPT_WIDTH,"worker_claim_round_cap":16,
        "operations":["ready backfill","quiet local queries","hold actual provider calls after changed build",
                      "held local queries and DB transaction control","write/delete","retire model","release and drain"],
        "performance_sla":null,"resource_method":"direct kernel SELF CPU and lifetime RSS high-water; combined in-process owner",
        "full_task_acceptance":false,"review_status":"independent closeout review required"}),
    );
    let root = tempfile::tempdir().unwrap();
    let mut config = ProjectConfig::default();
    config.auto_index.enabled = false;
    config.indexing.db_read_pool_size = Some(1);
    config.semantic.enabled = true;
    config.semantic.endpoint = "https://synthetic.invalid/v1".into();
    config.semantic.model_id = format!("fake/contention-old-{seed}");
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(16);
    config.semantic.max_concurrent = 4;
    config.semantic.max_concurrent_per_project = 2;
    write(
        root.path(),
        ".codecortex.json",
        serde_json::to_vec(&config).unwrap(),
    );
    write(
        root.path(),
        "stable.rs",
        format!("pub fn stable_signal() -> u64 {{ {seed} }}\n"),
    );
    write(
        root.path(),
        "delete.rs",
        format!("pub fn deleted_signal() -> u64 {{ {} }}\n", seed + 1),
    );
    for i in 0..24 {
        write(
            root.path(),
            &format!("mutable_{i:02}.rs"),
            format!("pub fn before_{i:02}() -> u64 {{ {} }}\n", seed * 100 + i),
        );
    }
    let mut local = CodeIndex::new(Some(root.path())).unwrap();
    let db = local.index_db().unwrap().clone();
    let old_space = local.semantic_subsystem().unwrap().space.clone();
    let provider = Arc::new(SlowProvider::new(old_space.clone()));
    local.install_semantic_provider(provider.clone()).unwrap();
    let index = Arc::new(RwLock::new(local));
    let _cleanup = Cleanup {
        index: index.clone(),
        provider: provider.clone(),
    };
    let initial_build_us = build(index.clone(), true)
        .instrument(cc_db::runtime_diagnostics::local_span(
            seed,
            "initial_build",
            0,
            1,
        ))
        .await;
    wait(|| {
        let pins = index.read().unwrap().query_pins();
        if pins != 0 {
            return false;
        }
        let coverage = db.reads().semantic_coverage().unwrap().coverage;
        coverage.published > 0 && coverage.uncovered == 0
    })
    .await;
    let quiet_resources = resources("quiet-before");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert_eq!(
        quiet_resources["resource_gate"],
        "attributed_combined_process"
    );
    let quiet = samples(index.clone(), seed, "quiet").await;
    provider.hold();
    for i in 0..24 {
        write(
            root.path(),
            &format!("mutable_{i:02}.rs"),
            format!("pub fn after_{i:02}() -> u64 {{ {} }}\n", seed * 1000 + i),
        );
    }
    let held_build_us = build(index.clone(), false)
        .instrument(cc_db::runtime_diagnostics::local_span(
            seed,
            "held_build",
            0,
            1,
        ))
        .await;
    wait(|| provider.gate.0.lock().unwrap().waiting == LOCAL_ATTEMPT_WIDTH).await;
    let before = queue(&db);
    save(
        seed,
        "held-before",
        &json!({"queue":before,"provider":provider.snapshot(),"resources":resources("held-before")}),
    );
    assert_eq!(before["claimed"], LOCAL_ATTEMPT_WIDTH);
    assert!(before["pending"].as_i64().unwrap() > 0);
    assert!(before["published"].as_u64().unwrap() > 0 && before["uncovered"].as_u64().unwrap() > 0);
    assert_eq!(service_factory::query_pool().stats().cpu_in_flight, 0);
    assert_eq!(service_factory::query_pool().stats().cpu_admitted, 0);
    let held_provider = provider.snapshot();
    let db_availability = db_control(db.clone())
        .instrument(cc_db::runtime_diagnostics::local_span(
            seed,
            "db_control",
            0,
            1,
        ))
        .await;
    let held = samples(index.clone(), seed, "held").await;
    assert_eq!(
        provider.snapshot(),
        held_provider,
        "local requests must not start or complete additional provider work"
    );
    assert_eq!(
        queue(&db),
        before,
        "held requests cannot consume outbox attempts or advance publications"
    );
    assert_eq!(service_factory::query_pool().stats().cpu_in_flight, 0);
    assert_eq!(service_factory::query_pool().stats().cpu_admitted, 0);
    let after_resources = resources("held-after");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let before = &quiet_resources["shared_process_owner"]["usage"];
        let after = &after_resources["shared_process_owner"]["usage"];
        assert!(after["user_cpu_ns"].as_u64().unwrap() >= before["user_cpu_ns"].as_u64().unwrap());
        assert!(
            after["system_cpu_ns"].as_u64().unwrap() >= before["system_cpu_ns"].as_u64().unwrap()
        );
        assert!(after["peak_resident_bytes"].as_u64().unwrap() > 0);
    }
    write(
        root.path(),
        "mutable_00.rs",
        format!("pub fn latest_signal() -> u64 {{ {} }}\n", seed * 10000),
    );
    std::fs::remove_file(root.path().join("delete.rs")).unwrap();
    let write_delete_us = build(index.clone(), false)
        .instrument(cc_db::runtime_diagnostics::local_span(
            seed,
            "write_delete",
            0,
            1,
        ))
        .await;
    {
        let local = index
            .try_read()
            .expect("provider wait holds no CodeIndex lock");
        for stale in ["before_00", "after_00", "deleted_signal"] {
            assert!(local
                .graph()
                .find_symbol(stale, true, 8, false)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty());
        }
        assert!(!local
            .graph()
            .find_symbol("latest_signal", true, 8, false)
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
    }
    let retired = index.read().unwrap().semantic_runtime().unwrap();
    let held_inputs = provider.gate.0.lock().unwrap().held_inputs.clone();
    assert_eq!(held_inputs.len(), LOCAL_ATTEMPT_WIDTH);
    config.semantic.model_id = format!("fake/contention-new-{seed}");
    write(
        root.path(),
        ".codecortex.json",
        serde_json::to_vec(&config).unwrap(),
    );
    let switch_started = Instant::now();
    let replacement = {
        let mut local = index
            .try_write()
            .expect("model switch cannot wait for old provider");
        local.set_project(root.path(), false).unwrap();
        let new_space = local.semantic_subsystem().unwrap().space.clone();
        assert_ne!(new_space, old_space);
        let replacement = Arc::new(FakeProvider::new(FakeProviderConfig::new(new_space)));
        local
            .install_semantic_provider(replacement.clone())
            .unwrap();
        replacement
    };
    assert!(!retired.schedule(), "retired worker cannot claim new work");
    build(index.clone(), false)
        .instrument(cc_db::runtime_diagnostics::local_span(
            seed,
            "post_retirement_build",
            0,
            1,
        ))
        .await;
    provider.release();
    wait(|| {
        let active = provider.gate.0.lock().unwrap().active;
        if active != 0 {
            return false;
        }
        let pins = index.read().unwrap().query_pins();
        if pins != 0 {
            return false;
        }
        let coverage = db.reads().semantic_coverage().unwrap().coverage;
        coverage.published > 0 && coverage.uncovered == 0
    })
    .await;
    let switch_and_drain_us = switch_started.elapsed().as_micros() as u64;
    let stale_publications: i64 = {
        let reader = db.read_conn().unwrap();
        held_inputs
            .iter()
            .map(|input| {
                reader.query_row(
            "SELECT COUNT(*) FROM semantic_manifest WHERE space_id=?1 AND input_digest=?2",
            rusqlite::params![old_space.digest().unwrap().as_str(),input],|row|row.get::<_,i64>(0),
        ).unwrap()
            })
            .sum()
    };
    assert_eq!(
        stale_publications, 0,
        "held old-space responses must not publish after retirement"
    );
    assert!(replacement.call_count() > 0);
    assert!(provider.gate.0.lock().unwrap().maximum_active <= LOCAL_ATTEMPT_WIDTH);
    save(
        seed,
        "summary",
        &json!({
            "seed":seed,"quiet":quiet,"held":held,"db_availability":db_availability,
            "initial_build_us":initial_build_us,"held_build_us":held_build_us,"write_delete_us":write_delete_us,
            "switch_and_drain_us":switch_and_drain_us,"old_held_input_publications":stale_publications,
            "old_provider":provider.snapshot(),"new_provider_calls":replacement.call_count(),"queue_final":queue(&db),
            "quiet_resources":quiet_resources,"held_after_resources":after_resources,"final_resources":resources("after-model-drain"),
            "fixed_watchdogs":{"local_query_ms":2000,"progress_ms":5000},
            "performance_delta_gate":"not evaluated: no new SLA, stable-device baseline or confidence interval invented",
            "resource_attribution_gate":after_resources["resource_gate"],
            "full_p7_015_complete":false,"review_status":"independent closeout review required",
            "scope":"actual CodeIndex/post-index worker; synthetic vectors; existing HTTP fairness and original lifecycle tests are separate controls",
        }),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_slow_backfill_preserves_local_progress_and_records_every_request() {
    let _diagnostic_session = (std::env::var("CODECORTEX_RUNTIME_DIAGNOSTICS").as_deref()
        == Ok("1"))
    .then(cc_server::runtime_diagnostics::install);
    let cache = tempfile::tempdir().unwrap();
    let _cache = CacheEnv::new(cache.path());
    for seed in SEEDS {
        run(seed).await;
    }
}
