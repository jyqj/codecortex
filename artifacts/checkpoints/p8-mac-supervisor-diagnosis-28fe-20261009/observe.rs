//! Causal observer for one fixed shell fixture, NOT run_supervised or certification.
//! Build with rustc 1.95.0 only. No Cargo, database, indexing, or measurement worker.
//! All timestamps are local monotonic observations; marker times are pipe receipts.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn kill(pid: i32, signal: i32) -> i32;
    fn getpgid(pid: i32) -> i32;
}
const DEADLINE: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(10);
const CONTAINMENT: Duration = Duration::from_millis(1500);
const STDERR_LIMIT: usize = 16 * 1024;
const EVENT_LIMIT: usize = 512;
const MARKERS: [&str; 5] = ["CCDIAG:parse_before", "CCDIAG:parse_after", "CCDIAG:sleep_before", "CCDIAG:sleep_after", "CCDIAG:summary_after"];

fn q(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""), '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"), '\r' => result.push_str("\\r"), '\t' => result.push_str("\\t"),
            c if c < ' ' => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"'); result
}
fn nullable<T: std::fmt::Display>(v: Option<T>) -> String { v.map(|x| x.to_string()).unwrap_or_else(|| "null".into()) }
fn err_code(e: &io::Error) -> String { format!("{{\"kind\":{},\"os_error\":{}}}", q(&format!("{:?}", e.kind())), nullable(e.raw_os_error())) }
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?; f.sync_all()
}
fn event(events: &mut Vec<String>, clock: Instant, name: &str, detail: &str) {
    if events.len() < EVENT_LIMIT {
        events.push(format!("{{\"at_ns\":{},\"event\":{},\"detail\":{}}}", clock.elapsed().as_nanos(), q(name), detail));
    }
}
fn exit_json(s: ExitStatus) -> String { format!("{{\"code\":{},\"signal\":{}}}", nullable(s.code()), nullable(s.signal())) }

// Owns only the fresh process group created by Command::process_group(0).
// Drop performs nonblocking last-resort signalling; it never waits or joins.
struct OwnedChild { child: Child, pid: i32, group_stop_attempted: bool, child_reaped: bool }
impl OwnedChild {
    fn stop(&mut self) -> String {
        // Signal a given owned group at most once; reaped numeric IDs may recycle.
        if self.group_stop_attempted { return "{\"already_attempted\":true}".into(); }
        self.group_stop_attempted = true;
        let group_rc = unsafe { kill(-self.pid, 9) };
        let group_errno = if group_rc == 0 { None } else { io::Error::last_os_error().raw_os_error() };
        let child_error = if self.child_reaped { None } else { self.child.kill().err() };
        format!("{{\"owned_pgid\":{},\"group_kill_return\":{},\"group_errno\":{},\"child_kill_error\":{}}}", self.pid, group_rc, nullable(group_errno), child_error.as_ref().map(err_code).unwrap_or_else(|| "null".into()))
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.group_stop_attempted { unsafe { kill(-self.pid, 9); } self.group_stop_attempted = true; }
        if !self.child_reaped { let _ = self.child.kill(); }
    }
}
#[derive(Default)]
struct Drain {
    bytes: Vec<u8>, chunks: Vec<String>, markers: Vec<String>, scanned: usize,
    eof: bool, error: Option<String>, frozen: bool,
}
fn observe_summary(path: &Path, first: &mut Option<u128>, events: &mut Vec<String>, clock: Instant) {
    if first.is_none() && path.is_file() {
        *first = Some(clock.elapsed().as_nanos());
        event(events, clock, "summary_first_observed", "{\"meaning\":\"filesystem visibility upper bound, not write timestamp\"}");
    }
}

fn run(helper: PathBuf, requested: PathBuf, case: &str) -> io::Result<i32> {
    if !matches!(case, "original" | "instrumented") { return Err(io::Error::other("invalid fixed case")); }
    // Directory and stderr creation precede the worker clock, as in A23.
    fs::create_dir(&requested)?;
    let out = requested.canonicalize()?;
    let tmp = out.join("owned-tmp"); fs::create_dir(&tmp)?;
    write_new(&out.join("plan.json"), b"{\"diagnostic_only\":true,\"deadline_ms\":250,\"indexing\":false}\n")?;
    let mut stderr_file = OpenOptions::new().write(true).create_new(true).open(out.join("worker.stderr"))?;
    let helper_before = fs::read(&helper)?;
    let mut command = Command::new(&helper);
    command.arg("--worker-plan").arg(out.join("plan.json")).arg("--output").arg(&out)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .env("RAYON_NUM_THREADS", "2").env("TMPDIR", &tmp).env("TMP", &tmp).env("TEMP", &tmp);
    for key in ["CODECORTEX_CACHE_DIR", "CODECORTEX_DIRTY_PROPAGATION", "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES", "CODECORTEX_MEMORY_BUDGET_FRACTION", "CODECORTEX_MAX_CONCURRENT_PARSE", "CODECORTEX_USE_DIRECT_WRITER"] { command.env_remove(key); }
    command.process_group(0);
    // The clock includes spawn and drain setup. No ready handshake is excluded.
    let started = Instant::now();
    let mut events = Vec::with_capacity(128);
    event(&mut events, started, "before_spawn", "{}");
    let spawned = command.spawn();
    event(&mut events, started, "spawn_returned", "{}");
    let child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let report = format!("{{\"schema_version\":1,\"diagnostic_only\":true,\"case\":{},\"status\":\"spawn_error\",\"exit_code\":2,\"worker_exit_code\":null,\"error\":{},\"events\":[{}],\"certification\":\"not_run\"}}\n", q(case), err_code(&e), events.join(","));
            write_new(&out.join("report.json"), report.as_bytes())?; return Ok(2);
        }
    };
    let pid = i32::try_from(child.id()).map_err(|_| io::Error::other("pid range"))?;
    let mut owned = OwnedChild { child, pid, group_stop_attempted: false, child_reaped: false };
    let pgid = unsafe { getpgid(pid) };
    let pgid_errno = if pgid < 0 { io::Error::last_os_error().raw_os_error() } else { None };
    // Allows outer containment to target only this explicitly created group.
    // A missing record after outer timeout means cleanup is unknown, never pass.
    let owner = format!("{{\"schema_version\":1,\"case\":{},\"observer_pid\":{},\"worker_pid\":{},\"fresh_process_group_requested\":true,\"observed_pgid\":{},\"getpgid_errno\":{}}}\n", q(case), std::process::id(), pid, pgid, nullable(pgid_errno));
    event(&mut events, started, "ownership_write_begin", "{}");
    // Ownership visibility is needed inside the clock; durability fsync is not.
    // Keep this exclusive write small and bracket its residual timing cost.
    {
        let mut f = OpenOptions::new().write(true).create_new(true).open(out.join("owned-child.json"))?;
        f.write_all(owner.as_bytes())?;
    }
    event(&mut events, started, "ownership_write_end", "{}");
    event(&mut events, started, "child_owned", &owner);
    let mut stderr = owned.child.stderr.take().ok_or_else(|| io::Error::other("stderr unavailable"))?;
    let state = Arc::new(Mutex::new(Drain::default()));
    let overflow = Arc::new(AtomicBool::new(false));
    let reader_state = Arc::clone(&state); let reader_overflow = Arc::clone(&overflow);
    event(&mut events, started, "before_drain_thread", "{}");
    let reader = std::thread::Builder::new().name("diagnostic-stderr".into()).spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            let read = stderr.read(&mut buffer);
            let at = started.elapsed().as_nanos();
            let mut d = reader_state.lock().unwrap_or_else(|p| p.into_inner());
            if d.frozen { break; }
            match read {
                Ok(0) => { d.eof = true; break; }
                Ok(n) => {
                    let offset = d.bytes.len(); let keep = n.min(STDERR_LIMIT.saturating_sub(offset));
                    d.bytes.extend_from_slice(&buffer[..keep]);
                    if keep < n { reader_overflow.store(true, Ordering::Release); }
                    if d.chunks.len() < 128 { d.chunks.push(format!("{{\"received_at_ns\":{at},\"offset\":{offset},\"bytes_kept\":{keep},\"bytes_read\":{n}}}")); }
                    let end = d.bytes.len();
                    while let Some(relative) = d.bytes[d.scanned..end].iter().position(|b| *b == b'\n') {
                        let newline = d.scanned + relative;
                        let line = std::str::from_utf8(&d.bytes[d.scanned..newline]).unwrap_or("");
                        if MARKERS.contains(&line) && d.markers.len() < 16 {
                            let line = q(line);
                            d.markers.push(format!("{{\"marker\":{line},\"received_at_ns\":{at},\"meaning\":\"pipe receipt upper bound\"}}"));
                        }
                        d.scanned = newline + 1;
                    }
                }
                Err(e) => { d.error = Some(err_code(&e)); break; }
            }
        }
    })?;
    event(&mut events, started, "after_drain_thread", "{}");
    let mut termination: Option<&str> = None;
    let mut actual_exit: Option<ExitStatus> = None;
    let mut wait_error: Option<String> = None;
    let mut summary_first = None;
    let summary_path = out.join("worker-summary.json");
    let mut polls = 0;
    loop {
        polls += 1;
        event(&mut events, started, "poll", &format!("{{\"index\":{polls}}}"));
        // Original order: output overflow, deadline, try_wait, then 10ms sleep.
        if overflow.load(Ordering::Acquire) { termination = Some("stderr_output_budget_exhausted"); break; }
        if started.elapsed() >= DEADLINE { termination = Some("deadline_exceeded"); break; }
        match owned.child.try_wait() {
            Ok(Some(exit)) => { owned.child_reaped = true; actual_exit = Some(exit); event(&mut events, started, "worker_exit_observed", &exit_json(exit)); break; }
            Ok(None) => {},
            Err(e) => { wait_error = Some(err_code(&e)); termination = Some("try_wait_error"); break; }
        }
        observe_summary(&summary_path, &mut summary_first, &mut events, started);
        std::thread::sleep(POLL);
    }
    observe_summary(&summary_path, &mut summary_first, &mut events, started);
    event(&mut events, started, "cleanup_signal_begin", "{}");
    let cleanup_signal = owned.stop();
    event(&mut events, started, "cleanup_signal_end", &cleanup_signal);
    // Original's blocking wait is replaced only by bounded diagnostic reaping.
    // This can never upgrade an expired original 250ms result to success.
    let mut cleanup_exit = actual_exit;
    while cleanup_exit.is_none() && started.elapsed() < CONTAINMENT {
        match owned.child.try_wait() {
            Ok(Some(exit)) => { owned.child_reaped = true; cleanup_exit = Some(exit); break; }
            Ok(None) => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => { wait_error = Some(err_code(&e)); break; }
        }
    }
    event(&mut events, started, "cleanup_reap_observed", &cleanup_exit.map(exit_json).unwrap_or_else(|| "null".into()));
    while !reader.is_finished() && started.elapsed() < DEADLINE {
        std::thread::sleep(DEADLINE.saturating_sub(started.elapsed()).min(POLL));
    }
    // Match A23's actual boundary: after the deadline-limited wait, inspect
    // is_finished once. Do not add a stricter elapsed-time predicate here.
    let drain_finished_at_original_check = reader.is_finished();
    // is_finished is checked again; join is never attempted on a live reader.
    let joined = if reader.is_finished() { reader.join().is_ok() } else { false };
    if !joined || !drain_finished_at_original_check { termination.get_or_insert("stderr_drain_deadline_exceeded"); }
    let mut d = state.lock().unwrap_or_else(|p| p.into_inner());
    d.frozen = true;
    stderr_file.write_all(&d.bytes)?; stderr_file.sync_all()?;
    let stderr_complete = joined && drain_finished_at_original_check && d.eof && d.error.is_none() && !overflow.load(Ordering::Acquire);
    let mut summary_bytes = Vec::new();
    let summary_error = match File::open(&summary_path) {
        Ok(f) => f.take(128 * 1024 + 1).read_to_end(&mut summary_bytes).err().map(|e| err_code(&e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => Some(err_code(&e)),
    };
    let summary_present = summary_path.is_file();
    let summary_passed = summary_bytes.len() <= 128 * 1024 && std::str::from_utf8(&summary_bytes).is_ok_and(|s| s.trim() == "{\"passed\":true}");
    let helper_unchanged = fs::read(&helper).is_ok_and(|bytes| bytes == helper_before);
    let cleanup_error = fs::remove_dir_all(&tmp).err().map(|e| err_code(&e));
    let exit_code = if termination.is_some() { 3 } else if actual_exit.and_then(|e| e.code()) == Some(0) && stderr_complete && summary_passed && summary_error.is_none() && helper_unchanged && cleanup_error.is_none() && cleanup_exit.is_some() && wait_error.is_none() { 0 } else { 2 };
    event(&mut events, started, "observer_result", &format!("{{\"exit_code\":{exit_code}}}"));
    let report = format!(concat!(
        "{{\"schema_version\":1,\"diagnostic_only\":true,\"certification\":\"not_run\",\"case\":{},",
        "\"status\":{},\"exit_code\":{},\"wall_ns\":{},\"worker_pid\":{},\"worker_exit\":{},\"worker_exit_code\":{},",
        "\"cleanup_reap_exit\":{},\"cleanup_signal\":{},\"cleanup_error\":{},\"wait_error\":{},",
        "\"stderr_complete\":{},\"drain_finished_at_original_check\":{},\"stderr_eof\":{},\"stderr_error\":{},\"stderr_overflow\":{},\"stderr_bytes_kept\":{},",
        "\"summary_present\":{},\"summary_passed_exact_fixture\":{},\"summary_first_observed_ns\":{},\"summary_error\":{},",
        "\"helper_unchanged\":{},\"polls\":{},\"events\":[{}],\"stderr_chunks\":[{}],\"markers\":[{}],",
        "\"limits\":{{\"worker_lifetime_ms\":250,\"poll_ms\":10,\"stderr_bytes\":16384,\"containment_reap_ms\":1500,\"outer_watchdog_ms\":2000}},",
        "\"differences\":[\"causal observer, not original run_supervised\",\"bounded memory stderr and filesystem summary observations perturb timing\",\"diagnostic plan ignored by fixed shell fixture\",\"bounded reaping replaces blocking wait solely for containment\",\"marker timestamps are receive upper bounds\"]}}\n"
    ), q(case), q(termination.unwrap_or(if exit_code == 0 { "diagnostic_case_complete" } else { "diagnostic_case_failed" })), exit_code, started.elapsed().as_nanos(), pid,
        actual_exit.map(exit_json).unwrap_or_else(|| "null".into()), nullable(actual_exit.and_then(|e| e.code())), cleanup_exit.map(exit_json).unwrap_or_else(|| "null".into()), cleanup_signal, cleanup_error.unwrap_or_else(|| "null".into()), wait_error.unwrap_or_else(|| "null".into()),
        stderr_complete, drain_finished_at_original_check, d.eof, d.error.as_deref().unwrap_or("null"), overflow.load(Ordering::Acquire), d.bytes.len(), summary_present, summary_passed, nullable(summary_first), summary_error.unwrap_or_else(|| "null".into()), helper_unchanged, polls, events.join(","), d.chunks.join(","), d.markers.join(","));
    write_new(&out.join("report.json"), report.as_bytes())?;
    Ok(exit_code)
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 { eprintln!("usage: observe HELPER NEW_RUN_DIR original|instrumented"); std::process::exit(2); }
    let case = args[3].to_str().unwrap_or("");
    match run(args[1].clone().into(), args[2].clone().into(), case) {
        Ok(code) => std::process::exit(code),
        Err(e) => { eprintln!("diagnostic_observer_error {}", err_code(&e)); std::process::exit(2); }
    }
}
