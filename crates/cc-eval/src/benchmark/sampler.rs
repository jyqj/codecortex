//! Process-attributed RSS snapshots, not a claim of continuous peak coverage.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, OnceLock,
    },
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    pub stage: String,
    pub runner_pid: u32,
    pub server_pid: Option<u32>,
    pub runner_rss_bytes: Option<u64>,
    /// Native current-process RSS from the same reader used by the indexer's
    /// memory controller. Kept separate from `ps` so disagreement is visible.
    pub runner_native_rss_bytes: Option<u64>,
    pub server_rss_bytes: Option<u64>,
    pub server_tree_rss_bytes: Option<u64>,
    pub external_service_rss_bytes: Option<u64>,
    pub method: String,
}
/// Extract only public, versioned originating-work receipts. Missing diagnostics
/// on an older binary are unavailable, not a zero-cost query. No raw hint paths.
pub fn retrieval_work(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let cost = payload.pointer("/evidence_summary/retrieval/cost")?;
    if cost
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return None;
    }
    let cost: cc_model::retrieval_cost::RetrievalCost =
        serde_json::from_value(cost.clone()).ok()?;
    let grep: Option<cc_model::retrieval::GrepDiagnostics> = payload
        .pointer("/evidence_summary/retrieval/grep")
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    Some(
        serde_json::json!({"kind":"originating_work_not_current_cache_hit_work","cost":cost,"grep":grep}),
    )
}

/// ru_maxrss is bytes on macOS, KiB on Linux. None is never converted to zero.
pub fn high_water_bytes(platform: &str, value: u64) -> Option<u64> {
    match platform {
        "macos" => Some(value),
        "linux" => value.checked_mul(1024),
        _ => None,
    }
}
/// A subprocess may stall even in spawn or reap, outside its polling timeout.
/// Isolate optional introspection from the benchmark with one admitted worker.
/// A late/stalled worker retains its slot; subsequent samples return unavailable
/// rather than accumulating more threads/processes. No thread is forcibly killed.
#[derive(Clone, Default)]
struct ProbeGate(Arc<AtomicBool>);
struct ProbeLease(Arc<AtomicBool>);
impl Drop for ProbeLease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl ProbeGate {
    fn run<T: Send + 'static>(
        &self,
        timeout: Duration,
        work: impl FnOnce() -> Option<T> + Send + 'static,
    ) -> Option<T> {
        if self
            .0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        let (send, receive) = mpsc::sync_channel(1);
        let lease = ProbeLease(self.0.clone());
        match std::thread::Builder::new()
            .name("cc-eval-resource-probe".into())
            .spawn(move || {
                let _lease = lease;
                let _ = send.send(work());
            }) {
            Ok(worker) => drop(worker),
            Err(_) => return None,
        }
        receive.recv_timeout(timeout).ok().flatten()
    }
}
static PROCESS_PROBE: OnceLock<ProbeGate> = OnceLock::new();

/// Process introspection is optional evidence, never an unbounded benchmark
/// dependency. A file-backed pipe avoids deadlock when the process table is big.
fn bounded_output(command: &mut Command, timeout: Duration) -> Option<String> {
    let mut output = tempfile::tempfile().ok()?;
    let mut child = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::from(output.try_clone().ok()?))
        .spawn()
        .ok()?;
    let started = Instant::now();
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !success || output.metadata().ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    output.seek(SeekFrom::Start(0)).ok()?;
    let mut text = String::new();
    output.read_to_string(&mut text).ok()?;
    Some(text)
}

pub fn sample(stage: &str, pid: Option<u32>) -> Resources {
    let mut rows = BTreeMap::new();
    let disabled = std::env::var("CODECORTEX_BENCH_PROCESS_PROBE").as_deref() == Ok("0");
    let process_table = if disabled {
        None
    } else {
        PROCESS_PROBE
            .get_or_init(ProbeGate::default)
            .run(Duration::from_millis(250), || {
                bounded_output(
                    Command::new("ps").args(["-axo", "pid=,ppid=,rss="]),
                    Duration::from_millis(250),
                )
            })
    };
    if let Some(table) = &process_table {
        for line in table.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 3 {
                continue;
            }
            if let (Ok(p), Ok(parent), Ok(kib)) = (
                parts[0].parse::<u32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u64>(),
            ) {
                if let Some(bytes) = kib.checked_mul(1024) {
                    rows.insert(p, (parent, bytes));
                }
            }
        }
    }
    let runner = std::process::id();
    let mut members = BTreeSet::new();
    if let Some(p) = pid {
        if rows.contains_key(&p) {
            members.insert(p);
        }
    }
    loop {
        let n = members.len();
        for (p, (parent, _)) in &rows {
            if members.contains(parent) {
                members.insert(*p);
            }
        }
        if members.len() == n {
            break;
        }
    }
    Resources {
        stage: stage.into(),
        runner_pid: runner,
        server_pid: pid,
        runner_rss_bytes: rows.get(&runner).map(|x| x.1),
        runner_native_rss_bytes: cc_index::process_rss_bytes_opt(),
        server_rss_bytes: pid.and_then(|p| rows.get(&p).map(|x| x.1)),
        server_tree_rss_bytes: (!members.is_empty()).then(|| {
            members
                .iter()
                .filter_map(|p| rows.get(p).map(|x| x.1))
                .sum()
        }),
        external_service_rss_bytes: None,
        method: if disabled {
            "ps explicitly disabled by CODECORTEX_BENCH_PROCESS_PROBE=0; process-tree RSS null, not zero; native runner RSS separate"
        } else if process_table.is_some() {
            "ps RSS KiB stage snapshots (250ms caller budget, one worker); native runner RSS separate; transient peaks may be missed"
        } else {
            "ps unavailable/failed/timeout/busy; process-tree RSS null, not zero; native runner RSS separate"
        }.into(),
    }
}

#[cfg(all(test, unix))]
mod process_probe_tests {
    use super::*;
    #[test]
    fn unresponsive_optional_probe_cannot_block_or_accumulate_workers() {
        let gate = ProbeGate::default();
        let (release, wait) = mpsc::sync_channel(1);
        let started = Instant::now();
        assert!(gate
            .run(Duration::from_millis(20), move || {
                wait.recv_timeout(Duration::from_secs(3)).unwrap();
                Some("late")
            })
            .is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(gate.0.load(Ordering::Acquire));
        assert!(gate
            .run::<()>(Duration::from_secs(1), || panic!(
                "a second probe was admitted"
            ))
            .is_none());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while gate.0.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(gate.run(Duration::from_secs(1), || Some(7)), Some(7));
    }

    #[test]
    fn stalled_resource_probe_is_unavailable_not_zero_or_infinite_wait() {
        let started = Instant::now();
        assert!(bounded_output(
            Command::new("sh").args(["-c", "exec sleep 2"]),
            Duration::from_millis(20)
        )
        .is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            bounded_output(Command::new("printf").arg("123"), Duration::from_secs(1)).as_deref(),
            Some("123")
        );
    }
}
