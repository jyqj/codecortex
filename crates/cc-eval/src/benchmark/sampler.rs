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

#[derive(Debug, Clone, Serialize)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub resident_bytes: u64,
    pub resident_method: &'static str,
    pub threads: u64,
    pub cpu_user_raw: u64,
    pub cpu_system_raw: u64,
    pub cpu_time_unit: &'static str,
    pub cpu_user_ns: Option<u64>,
    pub cpu_system_ns: Option<u64>,
    pub timebase_numer: u32,
    pub timebase_denom: u32,
    pub kernel_release: String,
}

/// PID-attributed current resident-size/thread/CPU snapshot, not a tree or
/// absolute peak. Linux CPU units come from sysconf(_SC_CLK_TCK); macOS CPU
/// conversion is qualified to the calibrated kernel/arch.
pub fn process_snapshot(pid: u32) -> Option<ProcessSnapshot> {
    if std::env::var("CODECORTEX_BENCH_PROCESS_PROBE").as_deref() == Ok("0") {
        return None;
    }
    PROCESS_PROBE
        .get_or_init(ProbeGate::default)
        .run(Duration::from_millis(250), move || {
            #[cfg(target_os = "macos")]
            {
                native_macos_snapshot(pid)
            }
            #[cfg(target_os = "linux")]
            {
                native_linux_snapshot(pid)
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            {
                let _ = pid;
                None
            }
        })
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn ticks_to_ns(raw: u64, numer: u32, denom: u32) -> Option<u64> {
    if denom == 0 || numer == 0 {
        return None;
    }
    u64::try_from(u128::from(raw) * u128::from(numer) / u128::from(denom)).ok()
}

#[cfg(target_os = "linux")]
fn native_linux_snapshot(pid: u32) -> Option<ProcessSnapshot> {
    // Read all per-process fields from one stat record, not separate files
    // which could describe different processes after exit and PID reuse.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // SAFETY: sysconf takes a constant selector and no pointers. Nonpositive
    // or unrepresentable results are unavailable, never guessed defaults.
    let (page_size, clock_ticks) = unsafe {
        (
            libc::sysconf(libc::_SC_PAGESIZE),
            libc::sysconf(libc::_SC_CLK_TCK),
        )
    };
    let page_size = u64::try_from(page_size).ok()?;
    let clock_ticks = u32::try_from(clock_ticks).ok()?;
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    linux_stat_snapshot(pid, &stat, page_size, clock_ticks, kernel.trim().into())
}

#[cfg(target_os = "linux")]
fn linux_stat_snapshot(
    pid: u32,
    stat: &str,
    page_size: u64,
    clock_ticks: u32,
    kernel_release: String,
) -> Option<ProcessSnapshot> {
    if pid == 0 || page_size == 0 || clock_ticks == 0 || kernel_release.is_empty() {
        return None;
    }
    // comm (field 2) may contain whitespace, newlines and ')' characters.
    // The final ')' terminates it; only the numeric suffix is tokenized.
    // Linux proc_pid_stat(5): utime/stime=14/15, threads=20, rss=24.
    let (identity, suffix) = stat.rsplit_once(") ")?;
    let (reported_pid, _) = identity.split_once(" (")?;
    if reported_pid.parse::<u32>().ok()? != pid {
        return None;
    }
    let fields: Vec<_> = suffix.split_whitespace().collect();
    let state = *fields.first()?;
    if state.len() != 1 || matches!(state, "Z" | "X" | "x") {
        return None;
    }
    let user_ticks = fields.get(11)?.parse::<u64>().ok()?;
    let system_ticks = fields.get(12)?.parse::<u64>().ok()?;
    let threads = fields.get(17)?.parse::<u64>().ok()?;
    if threads == 0 {
        return None;
    }
    let rss_pages = u64::try_from(fields.get(21)?.parse::<i64>().ok()?).ok()?;
    Some(ProcessSnapshot {
        pid,
        resident_bytes: rss_pages.checked_mul(page_size)?,
        resident_method: "Linux /proc/PID/stat RSS pages * sysconf page size; kernel estimate; PID only; not a process tree",
        threads,
        cpu_user_raw: user_ticks,
        cpu_system_raw: system_ticks,
        cpu_time_unit: "linux_proc_stat_clock_ticks",
        cpu_user_ns: ticks_to_ns(user_ticks, 1_000_000_000, clock_ticks),
        cpu_system_ns: ticks_to_ns(system_ticks, 1_000_000_000, clock_ticks),
        timebase_numer: 1_000_000_000,
        timebase_denom: clock_ticks,
        kernel_release,
    })
}

/// Optional thread-count snapshot. Shares the single admitted probe worker
/// with RSS sampling, including its disable policy and late-worker fence.
/// Missing/disabled samples are unavailable, never a zero-thread result.
pub fn thread_count(pid: u32) -> Option<u64> {
    if std::env::var("CODECORTEX_BENCH_PROCESS_PROBE").as_deref() == Ok("0") {
        return None;
    }
    PROCESS_PROBE
        .get_or_init(ProbeGate::default)
        .run(Duration::from_millis(250), move || {
            #[cfg(target_os = "linux")]
            {
                let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("Threads:")?.trim().parse::<u64>().ok())
            }
            #[cfg(target_os = "macos")]
            {
                native_macos_thread_count(pid)
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                let _ = pid;
                None
            }
        })
}

#[cfg(target_os = "macos")]
fn native_macos_thread_count(pid: u32) -> Option<u64> {
    Some(native_macos_snapshot(pid)?.threads)
}

#[cfg(target_os = "macos")]
fn native_macos_snapshot(pid: u32) -> Option<ProcessSnapshot> {
    // SDK sys/proc_info.h: proc_taskinfo has six uint64_t values followed
    // by twelve int32_t values; pti_threadnum is the tenth int32_t.
    // This avoids invoking ps while retaining the shared deadline/late fence.
    #[repr(C)]
    #[derive(Default)]
    struct TaskInfo {
        times_and_memory: [u64; 6],
        counters: [i32; 12],
    }
    #[link(name = "proc")]
    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            buffersize: i32,
        ) -> i32;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    extern "C" {
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }
    let native_pid = i32::try_from(pid).ok()?;
    let mut info = TaskInfo::default();
    let size = i32::try_from(std::mem::size_of::<TaskInfo>()).ok()?;
    // SAFETY: a correctly aligned writable C-layout buffer of the complete
    // SDK proc_taskinfo size is passed for PROC_PIDTASKINFO (flavor 4).
    let read = unsafe { proc_pidinfo(native_pid, 4, 0, (&mut info as *mut TaskInfo).cast(), size) };
    if read != size || info.counters[9] <= 0 {
        return None;
    }
    let mut timebase = Timebase::default();
    let mut kernel = std::mem::MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: writable correctly typed SDK buffers; success checked before read.
    let (clock_ok, kernel_ok) = unsafe {
        (
            mach_timebase_info(&mut timebase) == 0,
            libc::uname(kernel.as_mut_ptr()) == 0,
        )
    };
    if !clock_ok || !kernel_ok || timebase.denom == 0 {
        return None;
    }
    // SAFETY: uname initializes release as a nul-terminated C string.
    let kernel_release = unsafe {
        std::ffi::CStr::from_ptr(kernel.assume_init_ref().release.as_ptr())
            .to_string_lossy()
            .into_owned()
    };
    // Independent CPU busy/getrusage calibration proved this exact runtime.
    // Unknown kernels retain raw values, never falsely report nanoseconds.
    let calibrated = cfg!(target_arch = "aarch64") && kernel_release == "25.6.0";
    Some(ProcessSnapshot {
        pid,
        resident_bytes: info.times_and_memory[1],
        resident_method: "libproc proc_taskinfo resident-size bytes; PID only; not a process tree",
        threads: info.counters[9] as u64,
        cpu_user_raw: info.times_and_memory[2],
        cpu_system_raw: info.times_and_memory[3],
        cpu_time_unit: if calibrated {
            "mach_ticks_calibrated_Darwin25.6_aarch64"
        } else {
            "uncalibrated_kernel_counter"
        },
        cpu_user_ns: if calibrated {
            ticks_to_ns(info.times_and_memory[2], timebase.numer, timebase.denom)
        } else {
            None
        },
        cpu_system_ns: if calibrated {
            ticks_to_ns(info.times_and_memory[3], timebase.numer, timebase.denom)
        } else {
            None
        },
        timebase_numer: timebase.numer,
        timebase_denom: timebase.denom,
        kernel_release,
    })
}

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
    let mut result = ps_resources(stage, std::process::id(), pid, process_table.as_deref());
    result.runner_native_rss_bytes = cc_index::process_rss_bytes_opt();
    if disabled {
        result.method = "ps explicitly disabled by CODECORTEX_BENCH_PROCESS_PROBE=0; process-tree RSS null, not zero; native runner RSS separate".into();
    }
    result
}

/// Parse one complete process-table observation. Missing or overflowing child
/// RSS invalidates its tree total instead of silently dropping that child.
/// A malformed topology line or duplicate PID makes all tree totals partial;
/// directly attributed, unambiguous PID observations remain usable.
pub fn ps_resources(stage: &str, runner: u32, pid: Option<u32>, table: Option<&str>) -> Resources {
    let mut rows: BTreeMap<u32, (u32, Option<u64>)> = BTreeMap::new();
    let mut complete_topology = table.is_some();
    for line in table
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let parts: Vec<_> = line.split_whitespace().collect();
        let parsed = (|| {
            let p = parts.first()?.parse::<u32>().ok().filter(|p| *p > 0)?;
            let parent = parts.get(1)?.parse::<u32>().ok()?;
            if parts.len() != 3 || p == parent {
                return None;
            }
            let rss = parts[2]
                .parse::<u64>()
                .ok()
                .and_then(|kib| kib.checked_mul(1024));
            Some((p, parent, rss))
        })();
        if let Some((p, parent, rss)) = parsed {
            if let std::collections::btree_map::Entry::Vacant(entry) = rows.entry(p) {
                entry.insert((parent, rss));
            } else {
                complete_topology = false;
                if let Some(row) = rows.get_mut(&p) {
                    row.1 = None;
                }
            }
        } else {
            complete_topology = false;
        }
    }
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
    let cyclic = pid
        .and_then(|p| rows.get(&p))
        .is_some_and(|(parent, _)| members.contains(parent));
    let server_tree_rss_bytes = if complete_topology && !members.is_empty() && !cyclic {
        members
            .iter()
            .try_fold(0_u64, |total, p| total.checked_add(rows.get(p)?.1?))
    } else {
        None
    };
    Resources {
        stage: stage.into(),
        runner_pid: runner,
        server_pid: pid,
        runner_rss_bytes: rows.get(&runner).and_then(|x| x.1),
        runner_native_rss_bytes: None,
        server_rss_bytes: pid.and_then(|p| rows.get(&p).and_then(|x| x.1)),
        server_tree_rss_bytes,
        external_service_rss_bytes: None,
        method: if table.is_some() && complete_topology && (pid.is_none() || server_tree_rss_bytes.is_some()) {
            "ps RSS KiB stage snapshots (250ms caller budget, one worker); native runner RSS separate; transient peaks may be missed"
        } else if table.is_some() {
            "ps partial/missing/ambiguous/overflowing process-tree observation; tree RSS unavailable, not partial sum; native runner RSS separate"
        } else {
            "ps unavailable/failed/timeout/busy; process-tree RSS null, not zero; native runner RSS separate"
        }.into(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRole {
    pub role: &'static str,
    /// Tree entries list observed root PIDs, not a fabricated member inventory.
    pub root_pids: Vec<u32>,
    pub available_samples: usize,
    pub unavailable_samples: usize,
    pub peak_observed_bytes: Option<u64>,
    pub status: &'static str,
}
/// These peaks can occur at different stages. They are never added to each
/// other; neither are runner ps/native alternatives or server PID/tree values.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryLedger {
    pub snapshots: usize,
    pub sampling_interval_ms: Option<u64>,
    pub measurement: &'static str,
    pub total_rss_bytes: Option<u64>,
    pub total_reason: &'static str,
    pub roles: Vec<MemoryRole>,
}
pub fn memory_ledger(resources: &[Resources]) -> MemoryLedger {
    let role = |name, read: fn(&Resources) -> Option<u64>, pid: fn(&Resources) -> Option<u32>| {
        let values: Vec<_> = resources.iter().filter_map(read).collect();
        MemoryRole {
            role: name,
            root_pids: resources
                .iter()
                .filter_map(pid)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            available_samples: values.len(),
            unavailable_samples: resources.len() - values.len(),
            peak_observed_bytes: values.iter().copied().max(),
            status: if values.is_empty() {
                "unavailable"
            } else if values.len() != resources.len() {
                "partial"
            } else {
                "observed"
            },
        }
    };
    MemoryLedger { snapshots: resources.len(), sampling_interval_ms: None,
        measurement: "stage_snapshots_not_continuous_peak; no sampling interval recorded",
        total_rss_bytes: None,
        total_reason: "role overlap and simultaneous disjoint coverage are not proven; do not add runner ps/native or server PID/tree or per-role peaks",
        roles: vec![
            role("client_runner_ps", |r| r.runner_rss_bytes, |r| Some(r.runner_pid)),
            role("client_runner_native_alternative", |r| r.runner_native_rss_bytes, |r| Some(r.runner_pid)),
            role("server_process_included_in_tree", |r| r.server_rss_bytes, |r| r.server_pid),
            role("server_process_tree", |r| r.server_tree_rss_bytes, |r| r.server_pid),
            role("external_service_unspecified", |r| r.external_service_rss_bytes, |_| None),
            // The legacy sampler never observed these services separately.
            role("lsp_or_model_service", |_| None, |_| None),
            role("oce_external_container", |_| None, |_| None),
        ] }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskComponent {
    Index,
    Fts,
    ParseCache,
    Vector,
    ArtifactCache,
    Shared,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskPartition {
    /// Identity of one physical, disjoint storage object (for example an inode),
    /// not a directory overlapping other entries. A SQLite file containing both
    /// FTS and index tables belongs to Shared, not to two component totals.
    pub storage_id: String,
    pub component: DiskComponent,
    pub bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DiskLedger {
    pub status: &'static str,
    pub complete_layout: bool,
    pub duplicate_observations: usize,
    pub measured_subtotal_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub partitions: Vec<DiskPartition>,
}
/// This is pure accounting; it does not scan a live index during a benchmark.
/// Exact duplicate observations count once. Conflicting identities are rejected
/// because choosing one would invent a storage layout or silently lose bytes.
pub fn disk_ledger(
    partitions: &[DiskPartition],
    complete_layout: bool,
) -> super::Result<DiskLedger> {
    let mut unique = BTreeMap::new();
    for partition in partitions {
        if partition.storage_id.trim().is_empty() {
            return Err(super::invalid("empty disk storage identity"));
        }
        if let Some(previous) = unique.insert(partition.storage_id.clone(), partition.clone()) {
            if previous != *partition {
                return Err(super::invalid("conflicting disk storage identity"));
            }
        }
    }
    let measured: Vec<_> = unique.values().filter_map(|p| p.bytes).collect();
    let measured_subtotal_bytes = (!measured.is_empty())
        .then(|| {
            measured
                .iter()
                .try_fold(0_u64, |total, bytes| total.checked_add(*bytes))
        })
        .flatten();
    let total_bytes = (complete_layout && !unique.is_empty() && measured.len() == unique.len())
        .then_some(measured_subtotal_bytes)
        .flatten();
    Ok(DiskLedger {
        status: if partitions.is_empty() || measured.is_empty() {
            "unavailable"
        } else if measured_subtotal_bytes.is_none() {
            "overflow"
        } else if total_bytes.is_none() {
            "partial"
        } else {
            "observed"
        },
        complete_layout,
        duplicate_observations: partitions.len() - unique.len(),
        measured_subtotal_bytes,
        total_bytes,
        partitions: unique.into_values().collect(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    Reported,
    Estimated,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostReceipt {
    pub receipt_id: String,
    pub basis: CostBasis,
    pub currency: String,
    /// Integer millionths of the declared currency; never binary-float money.
    pub amount_microunits: Option<u64>,
    pub requests: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_hits: Option<u64>,
    pub duplicate_charge_uncertain: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct CostTotal {
    pub basis: CostBasis,
    pub currency: String,
    pub receipts: usize,
    pub amount_microunits: Option<u64>,
    pub requests: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_hits: Option<u64>,
    pub duplicate_charge_uncertain_receipts: usize,
    pub status: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct CostLedger {
    pub status: &'static str,
    pub duplicate_observations: usize,
    pub totals: Vec<CostTotal>,
}
/// No model calls, prices or bills are inferred from retrieval-work counters.
/// Reported and estimated money stay in separate currency/basis buckets; an
/// estimate never becomes a real charge by being added to reported usage.
pub fn cost_ledger(receipts: &[CostReceipt]) -> super::Result<CostLedger> {
    let mut unique = BTreeMap::new();
    for receipt in receipts {
        if receipt.receipt_id.trim().is_empty()
            || receipt.currency.len() != 3
            || !receipt.currency.bytes().all(|c| c.is_ascii_uppercase())
        {
            return Err(super::invalid(
                "cost receipt requires identity and uppercase three-letter currency",
            ));
        }
        if let Some(previous) = unique.insert(receipt.receipt_id.clone(), receipt.clone()) {
            if previous != *receipt {
                return Err(super::invalid("conflicting cost receipt identity"));
            }
        }
    }
    let duplicate_observations = receipts.len() - unique.len();
    let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for receipt in unique.into_values() {
        groups
            .entry((receipt.basis, receipt.currency.clone()))
            .or_default()
            .push(receipt);
    }
    let totals = groups
        .into_iter()
        .map(|((basis, currency), rows)| {
            let sum = |read: fn(&CostReceipt) -> Option<u64>| {
                rows.iter()
                    .try_fold(0_u64, |total, row| total.checked_add(read(row)?))
            };
            let amount_microunits = sum(|r| r.amount_microunits);
            let duplicate_charge_uncertain_receipts =
                rows.iter().filter(|r| r.duplicate_charge_uncertain).count();
            CostTotal {
                basis,
                currency,
                receipts: rows.len(),
                amount_microunits,
                requests: sum(|r| r.requests),
                input_tokens: sum(|r| r.input_tokens),
                output_tokens: sum(|r| r.output_tokens),
                cache_hits: sum(|r| r.cache_hits),
                duplicate_charge_uncertain_receipts,
                status: if amount_microunits.is_none() {
                    "unavailable_or_overflow"
                } else if duplicate_charge_uncertain_receipts > 0 {
                    "duplicate_charge_uncertain"
                } else {
                    "recorded"
                },
            }
        })
        .collect();
    Ok(CostLedger {
        status: if receipts.is_empty() {
            "unavailable"
        } else {
            "recorded_by_currency_and_basis_not_a_combined_bill"
        },
        duplicate_observations,
        totals,
    })
}

#[cfg(all(test, unix))]
mod process_probe_tests {
    use super::*;

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;

        fn stat(comm: &str) -> String {
            let mut fields = vec!["0"; 22];
            fields[0] = "S";
            fields[11] = "124";
            fields[12] = "35";
            fields[17] = "3";
            fields[21] = "512";
            format!("42 ({comm}) {}", fields.join(" "))
        }

        fn parse(stat: &str) -> Option<ProcessSnapshot> {
            linux_stat_snapshot(42, stat, 4096, 100, "test-kernel".into())
        }

        #[test]
        fn proc_stat_handles_parentheses_whitespace_and_documented_units() {
            let snapshot = parse(&stat("name ) ( with\nspaces")).unwrap();
            assert_eq!(snapshot.pid, 42);
            assert_eq!(snapshot.resident_bytes, 2_097_152);
            assert_eq!(snapshot.threads, 3);
            assert_eq!(snapshot.cpu_user_raw, 124);
            assert_eq!(snapshot.cpu_system_raw, 35);
            assert_eq!(snapshot.cpu_user_ns, Some(1_240_000_000));
            assert_eq!(snapshot.cpu_system_ns, Some(350_000_000));
            assert_eq!(snapshot.timebase_numer, 1_000_000_000);
            assert_eq!(snapshot.timebase_denom, 100);
            assert_eq!(snapshot.cpu_time_unit, "linux_proc_stat_clock_ticks");
            assert_eq!(snapshot.kernel_release, "test-kernel");
            // Valid zero CPU is measured zero, unlike a missing record.
            let zero = stat("zero").replacen("124 35", "0 0", 1);
            assert_eq!(parse(&zero).unwrap().cpu_user_ns, Some(0));
        }

        #[test]
        fn malformed_dead_or_mismatched_process_is_unavailable() {
            let valid = stat("name");
            for invalid in [
                String::new(),
                "42 (truncated) S 0".into(),
                valid.replacen("42 (", "43 (", 1),
                valid.replacen(") S", ") Z", 1),
                valid.replacen(") S", ") X", 1),
                valid.replacen("124 35", "not-a-number 35", 1),
                valid.strip_suffix("512").unwrap().to_owned() + "-1",
                valid.replacen("3 0 0 0 512", "0 0 0 0 512", 1),
            ] {
                assert!(parse(&invalid).is_none(), "accepted {invalid:?}");
            }
            assert!(native_linux_snapshot(u32::MAX).is_none());
            assert!(native_linux_snapshot(0).is_none());
        }

        #[test]
        fn invalid_units_and_rss_overflow_are_unavailable_cpu_overflow_stays_raw() {
            let valid = stat("name");
            for (page_size, hz) in [(0, 100), (4096, 0), (u64::MAX, 100)] {
                assert!(linux_stat_snapshot(42, &valid, page_size, hz, "test".into()).is_none());
            }
            let overflow = valid.replacen("124 35", &format!("{} 35", u64::MAX), 1);
            let snapshot = parse(&overflow).unwrap();
            assert_eq!(snapshot.cpu_user_raw, u64::MAX);
            assert_eq!(snapshot.cpu_user_ns, None);
            assert_eq!(snapshot.cpu_system_ns, Some(350_000_000));
            assert_eq!(snapshot.resident_bytes, 2_097_152);
            // _SC_CLK_TCK is not universally 100.
            assert_eq!(
                linux_stat_snapshot(42, &valid, 16384, 250, "test".into())
                    .unwrap()
                    .cpu_user_ns,
                Some(496_000_000)
            );
        }

        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        fn wait_snapshot(pid: u32) -> ProcessSnapshot {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(snapshot) = process_snapshot(pid) {
                    return snapshot;
                }
                assert!(Instant::now() < deadline, "live child snapshot unavailable");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        fn live_child_snapshot_is_attributed_monotonic_and_disappears() {
            const READY_ENV: &str = "CODECORTEX_SAMPLER_TEST_CHILD_READY";
            if let Some(ready) = std::env::var_os(READY_ENV) {
                // Re-exec only this test: isolated child CPU/RSS, no shared-env
                // mutation, external interpreter or unbounded helper lifetime.
                let mut allocation = vec![1_u8; 16 * 1024 * 1024];
                std::hint::black_box(&mut allocation);
                std::fs::write(ready, b"ready").unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut value = 1_u64;
                while Instant::now() < deadline {
                    value = std::hint::black_box(value.wrapping_mul(13).wrapping_add(17));
                }
                std::hint::black_box((value, allocation));
                return;
            }
            let directory = tempfile::tempdir().unwrap();
            let ready = directory.path().join("ready");
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "benchmark::sampler::process_probe_tests::linux::live_child_snapshot_is_attributed_monotonic_and_disappears",
                        "--test-threads=1",
                    ])
                    .env(READY_ENV, &ready)
                    .env_remove("CODECORTEX_BENCH_PROCESS_PROBE")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let pid = child.0.id();
            let deadline = Instant::now() + Duration::from_secs(3);
            while !ready.exists() {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child exited before ready"
                );
                assert!(Instant::now() < deadline, "child did not become ready");
                std::thread::sleep(Duration::from_millis(5));
            }
            let before = wait_snapshot(pid);
            assert_ne!(pid, std::process::id());
            assert_eq!(before.pid, pid);
            assert!(before.resident_bytes >= 16 * 1024 * 1024);
            assert!(before.threads > 0);
            assert!(!before.kernel_release.is_empty());
            let mut previous_user = before.cpu_user_raw;
            let mut previous_system = before.cpu_system_raw;
            let deadline = Instant::now() + Duration::from_secs(3);
            let after = loop {
                let current = wait_snapshot(pid);
                assert_eq!(current.pid, pid);
                assert!(current.cpu_user_raw >= previous_user);
                assert!(current.cpu_system_raw >= previous_system);
                previous_user = current.cpu_user_raw;
                previous_system = current.cpu_system_raw;
                if current.cpu_user_ns.unwrap() - before.cpu_user_ns.unwrap() >= 50_000_000 {
                    break current;
                }
                assert!(Instant::now() < deadline, "child did not accumulate CPU");
                std::thread::sleep(Duration::from_millis(5));
            };
            println!(
                "linux child before: {}",
                serde_json::to_string(&before).unwrap()
            );
            println!(
                "linux child after: {}",
                serde_json::to_string(&after).unwrap()
            );
            child.0.kill().unwrap();
            child.0.wait().unwrap();
            assert!(native_linux_snapshot(pid).is_none());
            assert!(process_snapshot(pid).is_none());
        }

        #[test]
        fn disabled_probe_is_unavailable_in_an_isolated_process() {
            const CHILD_ENV: &str = "CODECORTEX_SAMPLER_DISABLED_TEST_CHILD";
            if std::env::var_os(CHILD_ENV).is_some() {
                assert!(process_snapshot(std::process::id()).is_none());
                assert!(thread_count(std::process::id()).is_none());
                let resources = sample("disabled", Some(std::process::id()));
                assert!(resources.runner_rss_bytes.is_none());
                assert!(resources.server_rss_bytes.is_none());
                assert!(resources.server_tree_rss_bytes.is_none());
                assert!(resources.method.contains("explicitly disabled"));
                return;
            }
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "benchmark::sampler::process_probe_tests::linux::disabled_probe_is_unavailable_in_an_isolated_process",
                        "--test-threads=1",
                    ])
                    .env(CHILD_ENV, "1")
                    .env("CODECORTEX_BENCH_PROCESS_PROBE", "0")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    assert!(status.success(), "disabled child failed: {status}");
                    break;
                }
                assert!(Instant::now() < deadline, "disabled child did not exit");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn native_thread_count_uses_sdk_task_info_and_rejects_invalid_pid() {
        assert!(native_macos_thread_count(std::process::id()).is_some_and(|count| count > 0));
        assert_eq!(native_macos_thread_count(u32::MAX), None);
    }

    #[test]
    fn cpu_tick_conversion_is_checked_and_preserves_units() {
        assert_eq!(ticks_to_ns(24_000_000, 125, 3), Some(1_000_000_000));
        assert_eq!(ticks_to_ns(1, 1, 0), None);
        assert_eq!(ticks_to_ns(1, 0, 1), None);
        assert_eq!(ticks_to_ns(u64::MAX, u32::MAX, 1), None);
        assert_eq!(ticks_to_ns(u64::MAX, 1, 1), Some(u64::MAX));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn converted_native_cpu_matches_getrusage_on_the_calibrated_kernel() {
        fn usage_ns() -> u64 {
            let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
            // SAFETY: getrusage initializes the complete writable SDK struct.
            assert_eq!(
                unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
                0
            );
            // SAFETY: success checked above.
            let usage = unsafe { usage.assume_init() };
            (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as u64 * 1_000_000_000
                + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as u64 * 1000
        }
        let before = native_macos_snapshot(std::process::id()).unwrap();
        if before.cpu_user_ns.is_none() {
            assert_eq!(before.cpu_time_unit, "uncalibrated_kernel_counter");
            return;
        }
        let usage_before = usage_ns();
        let started = Instant::now();
        let mut n = 1_u64;
        while started.elapsed() < Duration::from_millis(300) {
            n = std::hint::black_box(n.wrapping_mul(13).wrapping_add(17));
        }
        std::hint::black_box(n);
        let after = native_macos_snapshot(std::process::id()).unwrap();
        let usage_after = usage_ns();
        let measured = (after.cpu_user_ns.unwrap() + after.cpu_system_ns.unwrap())
            - (before.cpu_user_ns.unwrap() + before.cpu_system_ns.unwrap());
        let reference = usage_after - usage_before;
        assert!(reference > 0);
        let ratio = measured as f64 / reference as f64;
        assert!(
            (0.9..=1.1).contains(&ratio),
            "CPU clock conversion disagrees with getrusage: {ratio}"
        );
        assert!(after.resident_bytes > 0);
    }
}
