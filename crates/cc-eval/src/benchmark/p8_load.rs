//! Bounded local mixed-load/soak evidence. The actual workload uses the existing
//! in-process MCP backend; an OS-process supervisor bounds uninterruptible calls.
//! This is not the subprocess-stdio product profile or a long-soak certificate.
use super::{
    invalid, manifest, mutations::Mutation, normalizer, oracle, report, statistics, Result,
};
use crate::runner::CodeIndexBackend;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadConfig {
    pub concurrency: usize,
    pub queue_capacity: usize,
    pub operations: usize,
    pub files: usize,
    pub offer_interval_ms: u64,
    pub request_deadline_ms: u64,
    pub run_deadline_ms: u64,
    pub drain_timeout_ms: u64,
    /// Deterministic cancellation injection measured from offering start,
    /// excluding fixture generation/initial build. None disables injection.
    pub cancel_after_ms: Option<u64>,
    pub artifact_budget_bytes: u64,
    pub seed: u64,
}
impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            queue_capacity: 16,
            operations: 24,
            files: 12,
            offer_interval_ms: 5,
            request_deadline_ms: 10_000,
            run_deadline_ms: 30_000,
            drain_timeout_ms: 5_000,
            cancel_after_ms: None,
            artifact_budget_bytes: 16 * 1024 * 1024,
            seed: 7,
        }
    }
}
impl LoadConfig {
    pub fn validate(&self) -> Result<()> {
        if ![1, 4, 8, 16].contains(&self.concurrency)
            || !(1..=256).contains(&self.queue_capacity)
            || !(2..=10_000).contains(&self.operations)
            || !(2..=256).contains(&self.files)
            || self.offer_interval_ms > 60_000
            || !(1..=3_600_000).contains(&self.run_deadline_ms)
            || self.request_deadline_ms == 0
            || self.request_deadline_ms > self.run_deadline_ms
            || self.drain_timeout_ms == 0
            || self.drain_timeout_ms > self.run_deadline_ms
            || !(1024..=64 * 1024 * 1024).contains(&self.artifact_budget_bytes)
        {
            return Err(invalid("p8 load bounds: C=1/4/8/16, queue=1..256, operations=2..10000, files=2..256, bounded positive deadlines/artifact budget"));
        }
        self.offer_interval_ms
            .checked_mul(self.operations as u64)
            .ok_or_else(|| invalid("offered schedule overflow"))?;
        Ok(())
    }
}

fn micros(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
}
fn cancelled(out: &Path) -> bool {
    out.join("cancel-requested").exists()
}
fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(2048).collect()
}
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const WORKER_STDERR_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Serialize)]
struct StderrReceipt {
    limit_bytes: u64,
    observed_bytes: u64,
    retained_bytes: u64,
    dropped_bytes: u64,
    read_to_eof: bool,
    error: Option<String>,
}

/// Drain even after the prefix limit, so a full pipe cannot conceal the
/// supervisor's stop signal. This helper never stores the discarded suffix.
fn drain_stderr(
    mut input: impl Read,
    mut output: impl Write,
    over_budget: &AtomicBool,
    failed: &AtomicBool,
) -> StderrReceipt {
    let mut receipt = StderrReceipt {
        limit_bytes: WORKER_STDERR_LIMIT,
        observed_bytes: 0,
        retained_bytes: 0,
        dropped_bytes: 0,
        read_to_eof: false,
        error: None,
    };
    let mut buffer = [0; 8192];
    loop {
        let count = match input.read(&mut buffer) {
            Ok(0) => {
                receipt.read_to_eof = true;
                break;
            }
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                receipt.error = Some(error_text(error));
                failed.store(true, Ordering::Release);
                break;
            }
        };
        receipt.observed_bytes = receipt.observed_bytes.saturating_add(count as u64);
        if receipt.observed_bytes > WORKER_STDERR_LIMIT {
            over_budget.store(true, Ordering::Release);
        }
        let keep = count.min((WORKER_STDERR_LIMIT - receipt.retained_bytes) as usize);
        let mut written = 0;
        while written < keep && receipt.error.is_none() {
            match output.write(&buffer[written..keep]) {
                Ok(0) => receipt.error = Some("worker stderr prefix writer stopped".into()),
                Ok(count) => {
                    written += count;
                    receipt.retained_bytes += count as u64;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => receipt.error = Some(error_text(error)),
            }
        }
        if receipt.error.is_some() {
            failed.store(true, Ordering::Release);
        }
    }
    if let Err(error) = output.flush() {
        receipt.error = Some(error_text(error));
        failed.store(true, Ordering::Release);
    }
    receipt.dropped_bytes = receipt.observed_bytes - receipt.retained_bytes;
    receipt
}

/// Runs this profile's trusted worker executable, never an arbitrary command.
/// The child receives only validated synthetic-workload configuration/new output.
/// Optional local programs are unavailable through a dedicated empty PATH.
/// Cancellation stops admission and permits a bounded drain/reconciliation;
/// expiry kills the worker process, including its in-process backend threads.
pub fn supervise(
    config: &LoadConfig,
    out: &Path,
    executable: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<Value> {
    config.validate()?;
    let executable = std::fs::canonicalize(executable)?;
    let binary_digest = manifest::file_digest(&executable)?;
    std::fs::create_dir(out)?; // Existing directories/symlinks are never reused.
    let absolute_out = std::fs::canonicalize(out)?;
    let out = absolute_out.as_path();
    let unavailable_programs = out.join("unavailable-programs");
    std::fs::create_dir(&unavailable_programs)?;
    report::json(&out.join("config.json"), config)?;
    report::json(
        &out.join("manifest.json"),
        &json!({"schema_version":1,
        "scope":"bounded_local_in_process_mcp_load_not_release_certification", "config":config,
        "compiled_module_digest":manifest::digest(include_bytes!("p8_load.rs")),
            "supervisor_binary_digest":binary_digest,
        "build_profile":if cfg!(debug_assertions) {"debug"} else {"release"},
        "initial_state":"running_unverified", "initial_exit_code":2,
        "external_network":"not_invoked_semantic_and_auto_index_disabled",
        "optional_local_programs":"unavailable_through_child_path_to_empty_directory",
        "inherited_codecortex_environment":"removed_including_external_cache_dir",
        "worker_stderr_prefix_limit_bytes":WORKER_STDERR_LIMIT,
        "not_run":["long_steady_state_rss","backfill","real_git_branch_switch","catalog_compaction","100k","subprocess_stdio_product_certification"]}),
    )?;
    let started = Instant::now();
    let log = File::create(out.join("worker.log"))?;
    let mut command = Command::new(&executable);
    // In particular, CODECORTEX_CACHE_DIR must not redirect IndexPaths outside
    // the generated fixtures. Other inherited knobs must not alter this profile.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CODECORTEX_") {
            command.env_remove(key);
        }
    }
    let spawned = command
        .arg("worker")
        .arg("--output")
        .arg(out)
        .current_dir(out)
        .env("PATH", &unavailable_programs)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => ChildGuard(child),
        Err(error) => {
            let result = json!({"schema_version":1,"status":"invalid_measurement","exit_code":2,"reason":error_text(error),"child_exit_code":null,"worker_summary":null});
            report::json(&out.join("supervisor.json"), &result)?;
            return Ok(result);
        }
    };
    let child_pid = child.0.id();
    let stderr_over_budget = Arc::new(AtomicBool::new(false));
    let stderr_failed = Arc::new(AtomicBool::new(false));
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| invalid("worker stderr pipe unavailable"))?;
    let over_budget = stderr_over_budget.clone();
    let failed = stderr_failed.clone();
    let reader = std::thread::Builder::new()
        .name("p8-load-stderr".into())
        .spawn(move || drain_stderr(stderr, log, &over_budget, &failed))?;
    let mut cancellation_started = None;
    let mut offering_observed = None;
    let mut forced_stop = None;
    let mut child_killed = false;
    let observed_status: Result<_> = (|| loop {
        if offering_observed.is_none() && out.join("offering-started").exists() {
            offering_observed = Some(Instant::now());
        }
        if config
            .cancel_after_ms
            .zip(offering_observed)
            .is_some_and(|(ms, at)| at.elapsed() >= Duration::from_millis(ms))
        {
            cancel.store(true, Ordering::Release);
        }
        if cancel.load(Ordering::Acquire) && !cancelled(out) {
            std::fs::write(
                out.join("cancel-requested"),
                b"supervisor user cancellation\n",
            )?;
        }
        if cancelled(out) && cancellation_started.is_none() {
            cancellation_started = Some(Instant::now());
        }
        if stderr_over_budget.load(Ordering::Acquire) {
            forced_stop = Some("worker_stderr_budget");
        } else if stderr_failed.load(Ordering::Acquire) {
            forced_stop = Some("worker_stderr_io_error");
        } else if started.elapsed() >= Duration::from_millis(config.run_deadline_ms) {
            forced_stop = Some("run_deadline");
        } else if cancellation_started
            .is_some_and(|time| time.elapsed() >= Duration::from_millis(config.drain_timeout_ms))
        {
            forced_stop = Some("cancellation_drain_deadline");
        }
        if let Some(status) = child.0.try_wait()? {
            break Ok(status);
        }
        if forced_stop.is_some() {
            // No worker subprocesses are launched by this profile. kill/wait
            // terminates all Rust threads rather than abandoning a blocked one.
            child_killed = child.0.kill().is_ok();
            break Ok(child.0.wait()?);
        }
        std::thread::sleep(Duration::from_millis(5));
    })();
    let mut errors = Vec::new();
    let status = match observed_status {
        Ok(status) => Some(status),
        Err(error) => {
            errors.push(error_text(error));
            child_killed = child.0.kill().is_ok();
            child.0.wait().ok()
        }
    };
    // This is a trusted self-worker with optional subprocess lookup disabled,
    // not a supervisor for arbitrary executables which may retain pipe owners.
    let stderr = match reader.join() {
        Ok(receipt) => Some(receipt),
        Err(_) => {
            errors.push("worker stderr reader panicked".into());
            None
        }
    };
    if stderr_over_budget.load(Ordering::Acquire) {
        forced_stop = Some("worker_stderr_budget");
    } else if stderr_failed.load(Ordering::Acquire) {
        forced_stop = Some("worker_stderr_io_error");
    }
    let binary_after = match manifest::file_digest(&executable) {
        Ok(digest) => Some(digest),
        Err(error) => {
            errors.push(format!(
                "final executable digest unavailable: {}",
                error_text(error)
            ));
            None
        }
    };
    let executable_unchanged = binary_after.as_ref() == Some(&binary_digest);
    if !executable_unchanged {
        errors.push("supervisor executable changed or disappeared during measurement".into());
    }
    let mut read_optional = |name: &str| -> Option<Value> {
        let path = out.join(name);
        if !path.exists() {
            return None;
        }
        match manifest::json_file(&path) {
            Ok(value) => Some(value),
            Err(error) => {
                errors.push(format!("invalid {name}: {}", error_text(error)));
                None
            }
        }
    };
    let worker = read_optional("worker-summary.json");
    let worker_binary = read_optional("worker-binary.json");
    let worker_binary_matches = worker_binary
        .as_ref()
        .is_some_and(|v| v["worker_binary_digest"] == binary_digest && v["matched"] == true);
    if worker_binary.is_some() && !worker_binary_matches {
        errors.push("worker binary self-check did not match supervisor digest".into());
    }
    let complete_worker = worker.as_ref().and_then(|v| v["exit_code"].as_i64());
    let exit_code = if !errors.is_empty()
        || stderr_over_budget.load(Ordering::Acquire)
        || stderr_failed.load(Ordering::Acquire)
    {
        2
    } else if cancellation_started.is_some() {
        3
    } else if forced_stop.is_some() {
        2
    } else if status.is_some_and(|s| s.success())
        && complete_worker == Some(0)
        && worker_binary_matches
    {
        0
    } else if status.and_then(|s| s.code()) == Some(1)
        && complete_worker == Some(1)
        && worker_binary_matches
    {
        1
    } else {
        2
    };
    let result = json!({"schema_version":1,"status":match exit_code {0=>"local_observations_recorded",1=>"local_profile_failed",3=>"cancelled",_=>"invalid_measurement"},
        "exit_code":exit_code,"child_pid":child_pid,"child_exit_code":status.and_then(|s| s.code()),"elapsed_us":micros(started),
        "forced_stop":forced_stop,"child_killed":child_killed,"cancellation_requested":cancellation_started.is_some(),
        "worker_summary":worker,"planned_operations":config.operations,
        "binary_binding":{"supervisor_before":binary_digest,"worker_self_check":worker_binary,"supervisor_after":binary_after,
            "supervisor_executable_unchanged":executable_unchanged,"worker_digest_verified":worker_binary_matches},
        "stderr":stderr,"errors":errors,
        "partial_artifacts_retained":true,"performance_certificate":"not_run"});
    report::json(&out.join("supervisor.json"), &result)?;
    Ok(result)
}

struct Artifacts {
    out: PathBuf,
    events: Mutex<File>,
    used: AtomicU64,
    limit: u64,
}
impl Artifacts {
    fn reserve(&self, bytes: usize) -> Result<()> {
        let mut used = self.used.load(Ordering::Acquire);
        loop {
            let next = used
                .checked_add(bytes as u64)
                .filter(|next| *next <= self.limit)
                .ok_or_else(|| {
                    invalid("p8 load artifact byte budget exhausted; prefix retained")
                })?;
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Ok(()),
                Err(observed) => used = observed,
            }
        }
    }
    fn event(&self, value: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&value)?;
        self.reserve(bytes.len() + 1)?;
        let mut file = self
            .events
            .lock()
            .map_err(|_| invalid("load event log poisoned"))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }
    fn json(&self, name: &str, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        self.reserve(bytes.len())?;
        std::fs::write(self.out.join(name), bytes)?;
        Ok(())
    }
}

#[derive(Clone)]
struct Work {
    id: usize,
    scheduled_us: u64,
    offered_us: u64,
    admitted_us: Option<u64>,
    build: bool,
}
#[derive(Default)]
struct QueueState {
    pending: VecDeque<Work>,
    closed: bool,
    peak: usize,
}
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}
impl Queue {
    fn offer(&self, mut work: Work, capacity: usize, started: Instant) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid("load queue poisoned"))?;
        if state.pending.len() == capacity {
            return Ok(false);
        }
        work.admitted_us = Some(micros(started));
        state.pending.push_back(work);
        state.peak = state.peak.max(state.pending.len());
        self.ready.notify_one();
        Ok(true)
    }
    fn take(&self) -> Result<Option<Work>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid("load queue poisoned"))?;
        while state.pending.is_empty() && !state.closed {
            state = self
                .ready
                .wait(state)
                .map_err(|_| invalid("load queue poisoned"))?;
        }
        Ok(state.pending.pop_front())
    }
    fn close(&self) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| invalid("load queue poisoned"))?
            .closed = true;
        self.ready.notify_all();
        Ok(())
    }
}

fn source_content(file: usize, revision: usize, seed: u64) -> String {
    format!(
        "def anchor_p8_{file}(value):\n    return value + {}\n",
        seed.wrapping_add(revision as u64)
    )
}
fn prepare_fixture(root: &Path, config: &LoadConfig) -> Result<()> {
    std::fs::create_dir(root)?;
    std::fs::create_dir(root.join("src"))?;
    report::json(
        &root.join(".codecortex.json"),
        &json!({"auto_index":{"enabled":false},"semantic":{"enabled":false},"indexing":{"max_concurrent_parse":2}}),
    )?;
    for file in 0..config.files {
        std::fs::write(
            root.join(format!("src/file_{file:04}.py")),
            source_content(file, 0, config.seed),
        )?;
    }
    Ok(())
}

/// Serial writer state keeps edits valid even if other scheduled writes were
/// rejected or timed out. Source files are overwritten, never grown forever.
fn mutation(revision: usize, seed: u64) -> Mutation {
    match revision % 6 {
        0 => Mutation::Write {
            path: "src/file_0000.py".into(),
            content: source_content(0, revision + 1, seed),
        },
        1 => Mutation::Write {
            path: "src/ephemeral.py".into(),
            content: "def ephemeral_p8():\n    return 1\n".into(),
        },
        2 => Mutation::Rename {
            from: "src/ephemeral.py".into(),
            to: "src/moved.py".into(),
        },
        3 => Mutation::Delete {
            path: "src/moved.py".into(),
        },
        4 => Mutation::Write {
            path: "src/file_0001.py".into(),
            content: format!(
                "def changed_api_p8(value, extra):\n    return value + extra + {revision}\n"
            ),
        },
        _ => Mutation::Write {
            path: "src/file_0001.py".into(),
            content: source_content(1, 0, seed),
        },
    }
}

struct Shared {
    backend: Arc<CodeIndexBackend>,
    writer: Mutex<usize>,
    queue: Queue,
    artifacts: Artifacts,
    config: LoadConfig,
    root: PathBuf,
    started: Instant,
    active_calls: AtomicUsize,
    peak_calls: AtomicUsize,
    active_reads: AtomicUsize,
    active_builds: AtomicUsize,
    read_build_overlap: AtomicBool,
}
fn operation_kind(work: &Work) -> &'static str {
    if work.build {
        "mutation_and_incremental_build"
    } else {
        "query"
    }
}
fn terminal(
    shared: &Shared,
    work: &Work,
    started_us: Option<u64>,
    status: &str,
    raw: Value,
) -> Result<()> {
    let finished_us = micros(shared.started);
    let status = if status == "success"
        && finished_us.saturating_sub(work.offered_us) >= shared.config.request_deadline_ms * 1000
    {
        "timeout"
    } else {
        status
    };
    shared.artifacts.event(json!({"schema_version":1,"event":"terminal","id":work.id,
        "operation":operation_kind(work),"scheduled_us":work.scheduled_us,"offered_us":work.offered_us,
        "admitted_us":work.admitted_us,"started_us":started_us,"finished_us":finished_us,
        "dispatch_lag_us":work.offered_us.saturating_sub(work.scheduled_us),
        "queue_us":work.admitted_us.map(|admitted| started_us.unwrap_or(finished_us).saturating_sub(admitted)),
        "service_us":started_us.map(|start| finished_us.saturating_sub(start)),
        "end_to_end_us":finished_us.saturating_sub(work.offered_us),
        "scheduled_to_end_us":finished_us.saturating_sub(work.scheduled_us),
        "status":status,"deadline_censored":status=="timeout","raw":raw}))
}
fn request_expired(shared: &Shared, work: &Work) -> bool {
    micros(shared.started).saturating_sub(work.offered_us)
        >= shared.config.request_deadline_ms * 1000
}
fn build_error(raw: &Value) -> Option<&'static str> {
    if raw.pointer("/resolution_freshness/complete") != Some(&json!(true)) {
        Some("build did not report complete resolution freshness")
    } else if !raw["parse_errors"]
        .as_array()
        .is_some_and(|errors| errors.is_empty())
    {
        Some("build reported parse errors or omitted parse error coverage")
    } else {
        None
    }
}
fn backend_call(
    shared: &Shared,
    work: &Work,
    call: impl FnOnce() -> cc_model::CcResult<Value>,
) -> Result<Value> {
    shared.artifacts.event(json!({"schema_version":1,"event":"backend_call_started","id":work.id,"operation":operation_kind(work),"at_us":micros(shared.started)}))?;
    let current = shared.active_calls.fetch_add(1, Ordering::SeqCst) + 1;
    shared.peak_calls.fetch_max(current, Ordering::SeqCst);
    let counter = if work.build {
        &shared.active_builds
    } else {
        &shared.active_reads
    };
    counter.fetch_add(1, Ordering::SeqCst);
    if shared.active_reads.load(Ordering::SeqCst) > 0
        && shared.active_builds.load(Ordering::SeqCst) > 0
    {
        shared.read_build_overlap.store(true, Ordering::SeqCst);
    }
    let result = call().map_err(|error| invalid(error_text(error)));
    counter.fetch_sub(1, Ordering::SeqCst);
    shared.active_calls.fetch_sub(1, Ordering::SeqCst);
    shared.artifacts.event(json!({"schema_version":1,"event":"backend_call_finished","id":work.id,"operation":operation_kind(work),"at_us":micros(shared.started)}))?;
    result
}
fn execute_work(shared: &Shared, work: &Work) -> Result<()> {
    if cancelled(&shared.artifacts.out) {
        return terminal(
            shared,
            work,
            None,
            "cancelled",
            json!({"stage":"before_service"}),
        );
    }
    if request_expired(shared, work) {
        return terminal(
            shared,
            work,
            None,
            "timeout",
            json!({"stage":"queue","deadline_censored":true}),
        );
    }
    let started_us = micros(shared.started);
    shared.artifacts.event(json!({"schema_version":1,"event":"started","id":work.id,"operation":operation_kind(work),"started_us":started_us}))?;
    let outcome = if work.build {
        let mut revision = shared
            .writer
            .lock()
            .map_err(|_| invalid("load writer poisoned"))?;
        // A queued writer may have waited behind a running writer while reads
        // continued. Do not start a mutation after its admission deadline.
        if cancelled(&shared.artifacts.out) {
            return terminal(
                shared,
                work,
                Some(started_us),
                "cancelled",
                json!({"stage":"writer_wait"}),
            );
        }
        if request_expired(shared, work) {
            return terminal(
                shared,
                work,
                Some(started_us),
                "timeout",
                json!({"stage":"writer_wait","deadline_censored":true}),
            );
        }
        let change = mutation(*revision, shared.config.seed);
        shared.artifacts.event(json!({"schema_version":1,"event":"mutation_started","id":work.id,
            "mutation":change,"revision":*revision,"writer_wait_us":micros(shared.started).saturating_sub(started_us)}))?;
        let applied = change.apply(&shared.root);
        if applied.is_ok() {
            *revision += 1;
        }
        applied.and_then(|_| {
            backend_call(shared, work, || shared.backend.build_index_report(false))
                .map(|raw| json!({"mutation":change,"result":raw}))
        })
    } else {
        backend_call(shared, work, || {
            shared.backend.call_tool("search", &json!({"query":format!("anchor_p8_{}", work.id % shared.config.files),"mode":"symbol","top_k":5}))
        })
    };
    let (status, raw) = match outcome {
        Ok(raw) => {
            let error = if work.build {
                build_error(&raw["result"]).map(str::to_owned)
            } else {
                match normalizer::mcp(&raw) {
                    Ok((
                        _,
                        super::schema::ResultStatus::Success | super::schema::ResultStatus::NoMatch,
                    )) => None,
                    Ok((_, status)) => Some(format!("query returned {status:?}")),
                    Err(error) => Some(error_text(error)),
                }
            };
            let status = if request_expired(shared, work) {
                "timeout"
            } else if error.is_some() {
                "error"
            } else {
                "success"
            };
            (
                status,
                json!({"response":raw,"error":error,"late_completion":status=="timeout"}),
            )
        }
        Err(error) => (
            if request_expired(shared, work) {
                "timeout"
            } else {
                "error"
            },
            json!({"error":error_text(error)}),
        ),
    };
    terminal(shared, work, Some(started_us), status, raw)
}

fn copy_final_sources(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir(destination)?;
    std::fs::create_dir(destination.join("src"))?;
    std::fs::copy(
        source.join(".codecortex.json"),
        destination.join(".codecortex.json"),
    )?;
    for entry in std::fs::read_dir(source.join("src"))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err(invalid("unexpected synthetic fixture object"));
        }
        std::fs::copy(
            entry.path(),
            destination.join("src").join(entry.file_name()),
        )?;
    }
    Ok(())
}
fn reconcile(shared: &Shared) -> Result<Value> {
    let full_root = shared.artifacts.out.join("full-worktree");
    copy_final_sources(&shared.root, &full_root)?;
    // Do not repair the incremental side before comparison: that could hide a
    // dropped update or an incomplete operation after admission stopped.
    let incremental = oracle::canonical(&shared.root)?;
    shared
        .artifacts
        .json("incremental-final.json", &incremental)?;
    let full_backend =
        CodeIndexBackend::new_unindexed(&full_root).map_err(|e| invalid(error_text(e)))?;
    let build = full_backend
        .build_index_report(true)
        .map_err(|e| invalid(error_text(e)))?;
    shared.artifacts.json("full-build.json", &build)?;
    if let Some(error) = build_error(&build) {
        return Err(invalid(error));
    }
    let full = oracle::canonical(&full_root)?;
    shared.artifacts.json("full-final.json", &full)?;
    let differences: Vec<_> = oracle::tables().iter().filter(|table| incremental.get(**table) != full.get(**table))
        .map(|table| json!({"table":table,"incremental_rows":incremental.get(*table).map(Vec::len),"full_rows":full.get(*table).map(Vec::len)})).collect();
    let mut probes = Vec::new();
    for query in ["anchor_p8_0", "ephemeral_p8", "changed_api_p8"] {
        let params = json!({"query":query,"mode":"symbol","top_k":10});
        let a = shared
            .backend
            .call_tool("search", &params)
            .map_err(|e| invalid(error_text(e)))?;
        let b = full_backend
            .call_tool("search", &params)
            .map_err(|e| invalid(error_text(e)))?;
        let locations = |raw: &Value| -> Result<_> {
            let (hits, status) = normalizer::mcp(raw)?;
            if !matches!(
                status,
                super::schema::ResultStatus::Success | super::schema::ResultStatus::NoMatch
            ) {
                return Err(invalid("reconciliation query incomplete"));
            }
            Ok(hits
                .into_iter()
                .map(|h| (h.path, h.start_line, h.end_line))
                .collect::<Vec<_>>())
        };
        probes.push(json!({"query":query,"equal":locations(&a)? == locations(&b)?,"incremental":a,"full":b}));
    }
    let equal = differences.is_empty() && probes.iter().all(|probe| probe["equal"] == true);
    let result = json!({"status":if equal {"equal"} else {"failed"},"equal":equal,"different_tables":differences,
        "canonical_tables":oracle::tables(),"raw_incremental":"incremental-final.json","raw_full":"full-final.json","probes":probes,
        "incremental_repaired_before_comparison":false});
    shared.artifacts.json("reconciliation.json", &result)?;
    Ok(result)
}

/// Child entry point. It must run outside another Tokio runtime because the
/// existing backend owns its real in-process MCP runtime. Call via supervise.
pub fn worker(out: &Path) -> Result<i32> {
    let config: LoadConfig = manifest::json_file(&out.join("config.json"))?;
    config.validate()?;
    let manifest: Value = manifest::json_file(&out.join("manifest.json"))?;
    if manifest["config"] != serde_json::to_value(&config)?
        || manifest["compiled_module_digest"] != manifest::digest(include_bytes!("p8_load.rs"))
    {
        return Err(invalid(
            "p8 load supervisor/worker config or implementation binding mismatch",
        ));
    }
    let artifacts = Artifacts {
        out: out.into(),
        events: Mutex::new(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out.join("events.jsonl"))?,
        ),
        used: AtomicU64::new(0),
        limit: config.artifact_budget_bytes,
    };
    let worker_binary_digest = manifest::file_digest(&std::env::current_exe()?)?;
    let binary_matches = manifest["supervisor_binary_digest"] == worker_binary_digest;
    artifacts.json(
        "worker-binary.json",
        &json!({"schema_version":1,"worker_binary_digest":worker_binary_digest,
            "matched":binary_matches}),
    )?;
    if !binary_matches {
        return Err(invalid("p8 load worker binary binding mismatch"));
    }
    if std::env::vars_os().any(|(key, _)| key.to_string_lossy().starts_with("CODECORTEX_")) {
        return Err(invalid(
            "p8 load worker rejects inherited CODECORTEX environment",
        ));
    }
    let unavailable_programs = std::fs::canonicalize(out)?.join("unavailable-programs");
    if std::env::var_os("PATH").as_deref() != Some(unavailable_programs.as_os_str())
        || std::fs::read_dir(&unavailable_programs)?.next().is_some()
    {
        return Err(invalid(
            "p8 load worker requires its empty optional-programs PATH",
        ));
    }
    if cancelled(out) {
        artifacts.json("worker-summary.json", &json!({"schema_version":1,"exit_code":3,"status":"cancelled","offered":0,"planned":config.operations,"reconciliation":{"status":"not_run_before_initialization"}}))?;
        return Ok(3);
    }
    let root = out.join("live-worktree");
    prepare_fixture(&root, &config)?;
    let backend =
        Arc::new(CodeIndexBackend::new_unindexed(&root).map_err(|e| invalid(error_text(e)))?);
    let initial_build = backend
        .build_index_report(true)
        .map_err(|e| invalid(error_text(e)))?;
    artifacts.json("initial-build.json", &initial_build)?;
    if let Some(error) = build_error(&initial_build) {
        return Err(invalid(error));
    }
    let shared = Arc::new(Shared {
        backend,
        writer: Mutex::new(0),
        queue: Queue::default(),
        artifacts,
        config: config.clone(),
        root,
        started: Instant::now(),
        active_calls: AtomicUsize::new(0),
        peak_calls: AtomicUsize::new(0),
        active_reads: AtomicUsize::new(0),
        active_builds: AtomicUsize::new(0),
        read_build_overlap: AtomicBool::new(false),
    });
    std::fs::write(out.join("offering-started"), b"offering clock started\n")?;
    let mut workers = Vec::new();
    for _ in 0..config.concurrency {
        let worker = shared.clone();
        workers.push(std::thread::spawn(move || -> Result<()> {
            while let Some(work) = worker.queue.take()? {
                execute_work(&worker, &work)?;
            }
            Ok(())
        }));
    }
    let offer_result = (|| -> Result<()> {
        for id in 0..config.operations {
            let scheduled_us = id as u64 * config.offer_interval_ms * 1000;
            while micros(shared.started) < scheduled_us && !cancelled(out) {
                if config
                    .cancel_after_ms
                    .is_some_and(|ms| shared.started.elapsed() >= Duration::from_millis(ms))
                {
                    std::fs::write(
                        out.join("cancel-requested"),
                        b"profile cancellation injection\n",
                    )?;
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            if config
                .cancel_after_ms
                .is_some_and(|ms| shared.started.elapsed() >= Duration::from_millis(ms))
            {
                std::fs::write(
                    out.join("cancel-requested"),
                    b"profile cancellation injection\n",
                )?;
            }
            if cancelled(out) {
                break;
            }
            let offered_us = micros(shared.started);
            let work = Work {
                id,
                scheduled_us,
                offered_us,
                admitted_us: None,
                build: id % 3 == 0,
            };
            shared.artifacts.event(json!({"schema_version":1,"event":"offered","id":id,"operation":operation_kind(&work),"scheduled_us":scheduled_us,"offered_us":offered_us}))?;
            if !shared
                .queue
                .offer(work.clone(), config.queue_capacity, shared.started)?
            {
                terminal(
                    &shared,
                    &work,
                    None,
                    "queue_rejected",
                    json!({"reason":"bounded_queue_full"}),
                )?;
            }
        }
        Ok(())
    })();
    shared.queue.close()?;
    let mut worker_error = None;
    for worker in workers {
        match worker.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => worker_error = Some(error),
            Err(_) => worker_error = Some(invalid("load worker panicked")),
        }
    }
    offer_result?;
    if let Some(error) = worker_error {
        return Err(error);
    }
    let load_elapsed_us = micros(shared.started);
    shared.artifacts.event(json!({"schema_version":1,"event":"admission_stopped_and_drained","elapsed_us":load_elapsed_us,"pending":0}))?;
    let reconciliation = match reconcile(&shared) {
        Ok(result) => result,
        Err(error) => {
            let value = json!({"status":"failed","equal":false,"error":error_text(error),"partial_canonical_artifacts_retained":true});
            shared.artifacts.json("reconciliation.json", &value)?;
            value
        }
    };
    let events: Vec<Value> = report::read_jsonl(&out.join("events.jsonl"))?;
    let terminals: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "terminal")
        .collect();
    let offered = events
        .iter()
        .filter(|event| event["event"] == "offered")
        .count();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for event in &terminals {
        *counts
            .entry(event["status"].as_str().unwrap_or("unknown").into())
            .or_default() += 1;
    }
    let elapsed: Vec<_> = terminals
        .iter()
        .filter_map(|event| event["end_to_end_us"].as_u64())
        .collect();
    let scheduled_elapsed: Vec<_> = terminals
        .iter()
        .filter_map(|event| event["scheduled_to_end_us"].as_u64())
        .collect();
    let successful = counts.get("success").copied().unwrap_or(0);
    let bad_outcomes = terminals.iter().any(|event| event["status"] != "success");
    let complete = offered == config.operations && terminals.len() == offered;
    let exit_code = if cancelled(out) {
        3
    } else if !complete || bad_outcomes || reconciliation["equal"] != true {
        1
    } else {
        0
    };
    let summary = json!({"schema_version":1,"status":if exit_code==0 {"local_observations_recorded"} else if exit_code==3 {"cancelled"} else {"failed"},"exit_code":exit_code,
        "scope":"bounded_local_in_process_mcp_load_not_release_certification","planned":config.operations,"offered":offered,"terminal_rows":terminals.len(),
        "not_offered":config.operations.saturating_sub(offered),"missing_terminal_rows":offered.saturating_sub(terminals.len()),"outcomes":counts,
        "concurrency":config.concurrency,"queue_capacity":config.queue_capacity,"queue_peak":shared.queue.state.lock().map_err(|_| invalid("queue poisoned"))?.peak,
        "backend_call_peak":shared.peak_calls.load(Ordering::SeqCst),"read_build_overlap_observed":shared.read_build_overlap.load(Ordering::SeqCst),
        "queue_drained":true,"completed_mutations":*shared.writer.lock().map_err(|_| invalid("writer poisoned"))?,
        "load_elapsed_us":load_elapsed_us,"offered_interval_ms":config.offer_interval_ms,
        "offered_per_second_observed":(load_elapsed_us>0).then(|| offered as f64*1_000_000.0/load_elapsed_us as f64),
        "successful_per_second_observed":(load_elapsed_us>0).then(|| successful as f64*1_000_000.0/load_elapsed_us as f64),
        "offered_schedule":"fixed target times independent of completions; dispatch lag and bounded-queue rejection retained",
        "all_attempt_elapsed":statistics::distribution(&elapsed),
        "all_scheduled_to_end_elapsed":statistics::distribution(&scheduled_elapsed),
        "request_deadline_origin":"offered_us; scheduled-to-end and dispatch lag are separately retained",
        "deadline_semantics":"queued expired work is not started; synchronous calls completing after request deadline remain timeout with late raw result; supervisor run/drain deadline kills a blocked process",
        "rss_bytes":null,"rss_reason":"not measured by this local profile; validated process attribution and long-run memory trend remain not_run",
        "reconciliation":reconciliation,"reserved_artifact_bytes_before_summary":shared.artifacts.used.load(Ordering::Acquire),
        "not_run":["long_steady_state_rss","backfill","real_git_branch_switch","catalog_compaction","100k","release_performance_certificate"]});
    shared.artifacts.json("worker-summary.json", &summary)?;
    Ok(exit_code)
}

#[cfg(test)]
mod artifact_budget_tests {
    use super::*;

    fn artifacts(used: u64, limit: u64) -> Artifacts {
        Artifacts {
            out: PathBuf::new(),
            events: Mutex::new(tempfile::tempfile().unwrap()),
            used: AtomicU64::new(used),
            limit,
        }
    }

    #[test]
    fn reservation_preserves_counter_on_overflow_or_budget_exhaustion() {
        let artifacts = artifacts(u64::MAX - 2, u64::MAX);
        assert!(artifacts.reserve(3).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), u64::MAX - 2);
        artifacts.reserve(2).unwrap();
        artifacts.reserve(0).unwrap();
        assert!(artifacts.reserve(1).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), u64::MAX);
    }

    #[test]
    fn contended_reservations_never_exceed_limit_or_lose_successes() {
        let artifacts = Arc::new(artifacts(0, 1024));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let artifacts = Arc::clone(&artifacts);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    (0..100).filter(|_| artifacts.reserve(3).is_ok()).count()
                })
            })
            .collect();
        let successes: usize = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        assert_eq!(successes, 341);
        assert_eq!(artifacts.used.load(Ordering::Acquire), successes as u64 * 3);
        assert!(artifacts.reserve(2).is_err());
        artifacts.reserve(1).unwrap();
        assert_eq!(artifacts.used.load(Ordering::Acquire), 1024);
    }

    #[test]
    fn concurrent_reservations_fill_only_whole_reservations_within_the_budget() {
        let artifacts = artifacts(0, 10_007);
        let successes = AtomicUsize::new(0);
        let start = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    start.wait();
                    for _ in 0..2_000 {
                        if artifacts.reserve(7).is_ok() {
                            successes.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let succeeded = successes.load(Ordering::Relaxed) as u64;
        assert_eq!(succeeded, artifacts.limit / 7);
        assert_eq!(artifacts.used.load(Ordering::Acquire), succeeded * 7);
        assert!(artifacts.reserve(7).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), succeeded * 7);
    }

    #[test]
    fn rejected_reservations_preserve_the_counter_at_limit_and_overflow_boundaries() {
        let artifacts = artifacts(63, 64);
        assert!(artifacts.reserve(2).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), 63);
        artifacts.reserve(1).unwrap();
        assert!(artifacts.reserve(1).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), 64);
        artifacts.reserve(0).unwrap();
        assert_eq!(artifacts.used.load(Ordering::Acquire), 64);

        let artifacts = self::artifacts(u64::MAX - 3, u64::MAX);
        artifacts.reserve(3).unwrap();
        assert!(artifacts.reserve(1).is_err());
        assert_eq!(artifacts.used.load(Ordering::Acquire), u64::MAX);
        artifacts.reserve(0).unwrap();
        assert_eq!(artifacts.used.load(Ordering::Acquire), u64::MAX);
    }
}

#[cfg(test)]
mod stderr_tests {
    use super::*;

    #[test]
    fn stderr_drain_retains_only_the_bounded_prefix_and_accounts_for_suffix() {
        let data: Vec<u8> = (0..WORKER_STDERR_LIMIT * 3 + 11)
            .map(|n| (n % 251) as u8)
            .collect();
        let over = AtomicBool::new(false);
        let failed = AtomicBool::new(false);
        let mut kept = Vec::new();
        let receipt = drain_stderr(std::io::Cursor::new(&data), &mut kept, &over, &failed);
        assert_eq!(kept, data[..WORKER_STDERR_LIMIT as usize]);
        assert_eq!(receipt.retained_bytes, WORKER_STDERR_LIMIT);
        assert_eq!(receipt.observed_bytes, data.len() as u64);
        assert_eq!(
            receipt.dropped_bytes,
            data.len() as u64 - WORKER_STDERR_LIMIT
        );
        assert!(receipt.read_to_eof);
        assert!(receipt.error.is_none());
        assert!(over.load(Ordering::Acquire));
        assert!(!failed.load(Ordering::Acquire));
    }

    #[test]
    fn stderr_write_failure_is_visible_and_input_is_still_drained() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("simulated full output device"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let over = AtomicBool::new(false);
        let failed = AtomicBool::new(false);
        let receipt = drain_stderr(std::io::Cursor::new([9; 9000]), Full, &over, &failed);
        assert!(receipt.read_to_eof);
        assert_eq!(receipt.observed_bytes, 9000);
        assert_eq!(receipt.retained_bytes, 0);
        assert_eq!(receipt.dropped_bytes, 9000);
        assert!(receipt.error.unwrap().contains("full output device"));
        assert!(failed.load(Ordering::Acquire));
        assert!(!over.load(Ordering::Acquire));
    }
}
