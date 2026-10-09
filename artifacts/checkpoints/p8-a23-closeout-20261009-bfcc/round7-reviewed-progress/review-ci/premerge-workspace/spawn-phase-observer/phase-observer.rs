use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use serde_json::json;
extern "C" { fn kill(pid: i32, sig: i32) -> i32; }
const STDERR_LIMIT: u64 = 256 * 1024;
struct ChildGuard(Child);
impl ChildGuard {
    fn stop(&mut self) {
        if let Ok(pid) = i32::try_from(self.0.id()) { unsafe { kill(-pid, 9); } }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) { self.stop(); }
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let binary = Path::new(&args[1]);
    let out = Path::new(&args[2]).canonicalize().unwrap();
    let stderr_file = OpenOptions::new().create_new(true).write(true).open(out.join("worker.stderr")).unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut command = Command::new(binary);
    command.arg("--worker-plan").arg(out.join("plan.json"))
        .arg("--output").arg(&out)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .env("RAYON_NUM_THREADS", "2")
        .env("TMPDIR", workspace.path()).env("TMP", workspace.path()).env("TEMP", workspace.path());
    for key in [
        "CODECORTEX_CACHE_DIR", "CODECORTEX_DIRTY_PROPAGATION",
        "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES", "CODECORTEX_MEMORY_BUDGET_FRACTION",
        "CODECORTEX_MAX_CONCURRENT_PARSE", "CODECORTEX_USE_DIRECT_WRITER"
    ] { command.env_remove(key); }
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut events = Vec::new();
    let started = Instant::now();
    let spawned = command.spawn();
    let spawn_elapsed_ns = started.elapsed().as_nanos();
    let mut child = match spawned {
        Ok(child) => ChildGuard(child),
        Err(error) => {
            fs::write(out.join("phase-report.json"), json!({
                "schema_version":1, "spawn_elapsed_ns":spawn_elapsed_ns,
                "spawn_error":error.to_string(),"original_deadline_ms":250,
                "diagnostic_only":true
            }).to_string()).unwrap();
            panic!("diagnostic spawn failed: {error}");
        }
    };
    let worker_pid = child.0.id();
    events.push(json!({"stage":"spawn_return","elapsed_ns":spawn_elapsed_ns,
        "worker_pid":worker_pid,"summary_exists":out.join("worker-summary.json").exists()}));
    let mut stderr = child.0.stderr.take().unwrap();
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let stderr_sink = Arc::new(Mutex::new(Some(stderr_file)));
    let sink = Arc::clone(&stderr_sink);
    let drain = std::thread::spawn(move || -> std::io::Result<()> {
        let mut written = 0u64; let mut buffer = [0u8; 8192];
        loop {
            let n = stderr.read(&mut buffer)?;
            if n == 0 { break; }
            let mut output = sink.lock().map_err(|_| std::io::Error::other("stderr sink poisoned"))?;
            let Some(log) = output.as_mut() else { break; };
            let keep = n.min(STDERR_LIMIT.saturating_sub(written) as usize);
            log.write_all(&buffer[..keep])?; written += keep as u64;
            if keep < n { flag.store(true, Ordering::Release); }
        }
        Ok(())
    });
    let mut termination = None;
    let exit = loop {
        if overflow.load(Ordering::Acquire) {
            termination = Some("stderr_output_budget_exhausted");child.stop();break None;
        }
        let at = started.elapsed();
        if at >= Duration::from_millis(250) {
            // Diagnostic-only observation at the same original deadline branch;
            // budget is not extended to wait for a successful worker.
            let status = child.0.try_wait().unwrap();
            events.push(json!({"stage":"deadline_boundary_try_wait","elapsed_ns":at.as_nanos(),
                "probe_return_ns":started.elapsed().as_nanos(),
                "worker_exited":status.is_some(),"worker_exit_code":status.and_then(|s|s.code()),
                "summary_exists":out.join("worker-summary.json").exists()}));
            termination = Some("deadline_exceeded");child.stop();break None;
        }
        let status = child.0.try_wait().unwrap();
        events.push(json!({"stage":"poll","elapsed_ns":at.as_nanos(),
            "probe_return_ns":started.elapsed().as_nanos(),
            "worker_exited":status.is_some(),"worker_exit_code":status.and_then(|s|s.code()),
            "summary_exists":out.join("worker-summary.json").exists()}));
        if let Some(exit) = status { break exit.code(); }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.stop();
    events.push(json!({"stage":"fresh_group_stopped","elapsed_ns":started.elapsed().as_nanos()}));
    while !drain.is_finished() && started.elapsed() < Duration::from_millis(250) {
        let remaining = Duration::from_millis(250).saturating_sub(started.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    let stderr_ok = if drain.is_finished() {matches!(drain.join(),Ok(Ok(())))}
        else {termination.get_or_insert("stderr_drain_deadline_exceeded");false};
    let stderr_sink_closed = match stderr_sink.lock() {
        Ok(mut sink) => {sink.take();true}
        Err(poisoned) => {poisoned.into_inner().take();false}
    };
    let summary_path = out.join("worker-summary.json");
    let summary = fs::read_to_string(&summary_path).ok();
    let cleanup = workspace.close();
    let total_ns=started.elapsed().as_nanos();
    let report=json!({"schema_version":1,"diagnostic_only":true,
        "method":"Direct exact Command configuration with separately recorded spawn return and an extra try_wait/summary existence probe at the unchanged original deadline branch; memory-only trace during deadline.",
        "original_deadline_ms":250,"original_elapsed_limit_seconds":2,
        "spawn_elapsed_ns":spawn_elapsed_ns,"spawn_exceeded_deadline":spawn_elapsed_ns>=250_000_000,
        "total_elapsed_ns":total_ns,"elapsed_under_two_seconds":total_ns<2_000_000_000,
        "worker_pid":worker_pid,"termination":termination,"worker_exit_code":exit,
        "stderr_complete":stderr_ok&&stderr_sink_closed&&!overflow.load(Ordering::Acquire),
        "summary_text":summary,"summary_exists":summary_path.exists(),
        "fixture_cleanup_error":cleanup.err().map(|e|e.to_string()),"events":events});
    fs::write(out.join("phase-report.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("{report}");
    assert!(total_ns<2_000_000_000,"original 2s elapsed assertion failed");
    if termination.is_some() {std::process::exit(3);}
}
