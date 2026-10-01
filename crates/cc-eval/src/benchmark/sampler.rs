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
/// absolute peak. CPU conversion is qualified to the calibrated kernel/arch.
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
            #[cfg(not(target_os = "macos"))]
            {
                let _ = pid;
                None
            }
        })
}

#[cfg(any(target_os = "macos", test))]
fn ticks_to_ns(raw: u64, numer: u32, denom: u32) -> Option<u64> {
    if denom == 0 || numer == 0 {
        return None;
    }
    u64::try_from(u128::from(raw) * u128::from(numer) / u128::from(denom)).ok()
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
