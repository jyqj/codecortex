//! Diagnostic reimplementation of the Command supervision boundary only.
//! This is NOT cc-eval, an original test result, a scale sample, or a repair.
//! The original library/test is run separately, byte-identically, before this.
//! Instrumentation is in-memory during the original 250ms deadline.
use std::{
    fs::{self, File}, io::{self, Read, Write}, os::unix::process::CommandExt,
    path::PathBuf, process::{Child, Command, Stdio},
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}},
    time::{Duration, Instant},
};
unsafe extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
const LIMIT: u64 = 256 * 1024;
const DEADLINE: Duration = Duration::from_millis(250);

fn q(s: &str) -> String {
    let mut v = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => v.push_str("\\\""), '\\' => v.push_str("\\\\"),
            '\n' => v.push_str("\\n"), '\r' => v.push_str("\\r"),
            '\t' => v.push_str("\\t"),
            c if c < ' ' => v.push_str(&format!("\\u{:04x}", c as u32)),
            c => v.push(c),
        }
    }
    v.push('"'); v
}
fn number(n: Option<u128>) -> String {
    n.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            if let Ok(pid) = i32::try_from(self.0.id()) {
                unsafe { kill(-pid, 9); }
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn stop(child: &mut OwnedChild, start: Instant, stops: &mut Vec<String>) {
    let begin = start.elapsed().as_nanos();
    let result = unsafe { kill(-(child.0.id() as i32), 9) };
    let group_errno = if result == -1 { io::Error::last_os_error().raw_os_error() } else { None };
    let direct_kill = child.0.kill();
    let waited = child.0.wait();
    let end = start.elapsed().as_nanos();
    stops.push(format!(
        "{{\"begin_ns\":{begin},\"end_ns\":{end},\"group_kill_return\":{result},\"group_kill_errno\":{},\"direct_kill_ok\":{},\"wait_ok\":{}}}",
        group_errno.map(|v|v.to_string()).unwrap_or_else(||"null".into()),
        direct_kill.is_ok(), waited.is_ok()));
}
fn main() {
    if let Err(e) = run() {
        eprintln!("diagnostic infrastructure error: {e}");
        std::process::exit(2);
    }
}
fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 { return Err(io::Error::other("usage: observer HELPER RUN_DIR WORKSPACE")); }
    let helper = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    let workspace = PathBuf::from(&args[3]);
    let total_started = Instant::now();
    fs::create_dir(&out)?;
    fs::create_dir(&workspace)?;
    let out = out.canonicalize()?;
    let workspace = workspace.canonicalize()?;
    // Exact original default ScalePlan with only deadline_ms=250, no shard.
    fs::write(out.join("plan.json"), concat!(
        "{\"schema_version\":1,\"profile\":\"smoke\",\"files\":[60],",
        "\"seed\":12648430,\"repetitions\":1,\"skip_fanout\":false,",
        "\"dirty_budget\":2,\"max_resume_builds\":64,\"batch_sizes\":[1,10],",
        "\"fanouts\":[2,8],\"deadline_ms\":250,\"max_output_bytes\":16777216}\n"))?;
    let log = File::create(out.join("worker.stderr"))?;
    let mut command = Command::new(&helper);
    command.arg("--worker-plan").arg(out.join("plan.json"))
        .arg("--output").arg(&out)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .env("RAYON_NUM_THREADS","2")
        .env("TMPDIR",&workspace).env("TMP",&workspace).env("TEMP",&workspace);
    for key in [
        "CODECORTEX_CACHE_DIR", "CODECORTEX_DIRTY_PROPAGATION",
        "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES", "CODECORTEX_MEMORY_BUDGET_FRACTION",
        "CODECORTEX_MAX_CONCURRENT_PARSE", "CODECORTEX_USE_DIRECT_WRITER",
    ] { command.env_remove(key); }
    command.process_group(0);
    let mut stops = Vec::with_capacity(3);
    let mut running_polls = Vec::with_capacity(32);
    let started = Instant::now();
    let spawned = command.spawn();
    let spawn_returned = started.elapsed().as_nanos();
    let raw_child = match spawned {
        Ok(c) => c,
        Err(e) => {
            fs::write(out.join("report.json"),format!(
                "{{\"diagnostic_only\":true,\"status\":\"spawn_error\",\"spawn_returned_ns\":{spawn_returned},\"error\":{}}}\n",q(&e.to_string())))?;
            return Err(e);
        }
    };
    let mut child = OwnedChild(raw_child);
    let pid = child.0.id();
    let mut stderr = child.0.stderr.take().ok_or_else(||io::Error::other("stderr absent"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let eof = Arc::new(AtomicU64::new(u64::MAX));
    let thread_eof = Arc::clone(&eof);
    let sink = Arc::new(Mutex::new(Some(log)));
    let thread_sink = Arc::clone(&sink);
    let drain = std::thread::spawn(move || -> io::Result<()> {
        let mut written = 0u64;
        let mut buffer = [0u8;8192];
        loop {
            let n = stderr.read(&mut buffer)?;
            if n == 0 {
                thread_eof.store(started.elapsed().as_nanos() as u64,Ordering::Release);
                break;
            }
            let mut output = thread_sink.lock().map_err(|_|io::Error::other("poisoned stderr sink"))?;
            let Some(log) = output.as_mut() else { break; };
            let keep = n.min(LIMIT.saturating_sub(written) as usize);
            log.write_all(&buffer[..keep])?;
            written += keep as u64;
            if keep < n { flag.store(true,Ordering::Release); }
        }
        Ok(())
    });
    let mut termination = None;
    let mut first_poll = None;
    let mut deadline_observed = None;
    let mut exit_observed = None;
    let mut wait_error = None;
    // Intentionally retain original deadline-before-try_wait ordering.
    let worker_exit = loop {
        first_poll.get_or_insert_with(|| started.elapsed().as_nanos());
        if overflow.load(Ordering::Acquire) {
            termination = Some("stderr_output_budget_exhausted");
            stop(&mut child,started,&mut stops);
            break None;
        }
        if started.elapsed() >= DEADLINE {
            deadline_observed = Some(started.elapsed().as_nanos());
            termination = Some("deadline_exceeded");
            stop(&mut child,started,&mut stops);
            break None;
        }
        match child.0.try_wait() {
            Ok(Some(status)) => {
                exit_observed = Some(started.elapsed().as_nanos());
                break status.code();
            }
            Ok(None) => running_polls.push(started.elapsed().as_nanos()),
            Err(e) => {
                wait_error = Some(e.to_string());
                termination = Some("try_wait_error");
                stop(&mut child,started,&mut stops);
                break None;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    stop(&mut child,started,&mut stops);
    while !drain.is_finished() && started.elapsed() < DEADLINE {
        let remaining = DEADLINE.saturating_sub(started.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    let drain_finished_observed = drain.is_finished().then(||started.elapsed().as_nanos());
    let stderr_ok = if drain_finished_observed.is_some() {
        matches!(drain.join(),Ok(Ok(())))
    } else {
        termination.get_or_insert("stderr_drain_deadline_exceeded");
        false
    };
    let sink_closed = match sink.lock() {
        Ok(mut s) => { s.take(); true },
        Err(p) => { p.into_inner().take(); false },
    };
    if termination.is_none() && overflow.load(Ordering::Acquire) {
        termination = Some("stderr_output_budget_exhausted");
    }
    let summary_path = out.join("worker-summary.json");
    let summary_raw = match fs::read(&summary_path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let cleanup_start = started.elapsed().as_nanos();
    // Conditional child cleanup and workspace removal finish before the report.
    drop(child);
    let cleanup_error = fs::remove_dir_all(&workspace).err().map(|e|e.to_string());
    let cleanup_end = started.elapsed().as_nanos();
    let wall = started.elapsed().as_nanos();
    let total = total_started.elapsed().as_nanos();
    let summary_passed = summary_raw.as_deref() == Some(b"{\"passed\":true}".as_slice());
    let exit_code = if termination.is_some() { 3 } else if
        !stderr_ok || !sink_closed || cleanup_error.is_some() || total >= 2_000_000_000 {
        2
    } else if worker_exit == Some(0) && summary_passed { 0 } else { 2 };
    let eof_value = eof.load(Ordering::Acquire);
    let raw_text = summary_raw.as_ref().map(|v|q(&String::from_utf8_lossy(v))).unwrap_or_else(||"null".into());
    let report = format!(concat!(
        "{{\"schema_version\":1,\"diagnostic_only\":true,",
        "\"original_test_result\":false,\"deadline_ms\":250,\"total_limit_ms\":2000,",
        "\"deadline_includes_spawn\":true,\"deadline_before_try_wait\":true,",
        "\"worker_pid\":{},\"spawn_returned_ns\":{},\"first_poll_ns\":{},",
        "\"last_try_wait_running_ns\":{},\"running_poll_ns\":[{}],",
        "\"exit_observed_ns\":{},\"deadline_observed_ns\":{},",
        "\"group_stop_calls\":[{}],\"drain_finished_observed_ns\":{},\"stderr_eof_ns\":{},",
        "\"status\":{},\"observer_exit_code\":{},\"worker_exit_code\":{},",
        "\"stderr_complete\":{},\"summary_raw_utf8\":{},\"summary_exact_fixture_passed\":{},",
        "\"try_wait_error\":{},\"cleanup_begin_ns\":{},\"cleanup_end_ns\":{},\"cleanup_error\":{},",
        "\"wall_ns\":{},\"total_elapsed_ns\":{},\"pre_report_two_second_bound_satisfied\":{},",
        "\"trace_clock\":\"parent Instant started immediately before Command.spawn; EOF is reader-thread observation\",",
        "\"limitations\":\"Independent std-only observer; total_elapsed_ns and the pre-report two-second check exclude report serialization/write and process startup. Instrumentation and different parent binary can change timing. No marker/child-clock, exact spawn implementation, original-host equivalence, scale or TODO claim.\"}}\n"),
        pid,spawn_returned,number(first_poll),number(running_polls.last().copied()),
        running_polls.iter().map(|v|v.to_string()).collect::<Vec<_>>().join(","),
        number(exit_observed),number(deadline_observed),stops.join(","),
        number(drain_finished_observed),number((eof_value!=u64::MAX).then_some(eof_value as u128)),
        q(termination.unwrap_or(if exit_code==0 {"diagnostic_observation_complete"} else {"diagnostic_observation_failed"})),
        exit_code,worker_exit.map(|v|v.to_string()).unwrap_or_else(||"null".into()),
        stderr_ok&&sink_closed&&!overflow.load(Ordering::Acquire),raw_text,summary_passed,
        wait_error.as_ref().map(|v|q(v)).unwrap_or_else(||"null".into()),
        cleanup_start,cleanup_end,cleanup_error.as_ref().map(|v|q(v)).unwrap_or_else(||"null".into()),
        wall,total,total<2_000_000_000);
    fs::write(out.join("report.json"),report)?;
    // All owned-process cleanup and evidence writes complete before this exit.
    std::process::exit(exit_code);
}
