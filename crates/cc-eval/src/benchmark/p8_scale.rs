//! Bounded local P8 scale preparation, using the existing generator, actual MCP
//! index dispatch, and the shared full-table diagnostic oracle. No release claim.
use super::{
    invalid, manifest, mutation_case, mutations::Mutation, oracle, runner, sampler, BenchError,
    Result,
};
use crate::{runner::CodeIndexBackend, synth};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const RELEASE_SCALES: [usize; 5] = [1_000, 5_000, 10_000, 50_000, 100_000];
const CONTROL_RESERVE: u64 = 768 * 1024;
const STDERR_LIMIT: u64 = 256 * 1024;
const ORACLE_ROWS: u64 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Smoke,
    Release,
}

/// An additive, explicitly selected capacity contract. Existing smoke/release
/// plans retain their original work budgets and default oracle capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapacityProfile {
    #[serde(rename = "scale_capacity_v1")]
    ScaleCapacityV1,
    #[serde(rename = "scale_wide_dirty_v1")]
    ScaleWideDirtyV1,
}

/// An explicitly separate stage study. Absence preserves the original plan
/// serialization and executes every registered incremental and fanout stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageScope {
    #[serde(rename = "cold_only_v1")]
    ColdOnlyV1,
    #[serde(rename = "profile_isolated_v1")]
    ProfileIsolatedV1,
    #[serde(rename = "profile_task_descriptive_v1")]
    ProfileTaskDescriptiveV1,
}

/// Identity of the separate cold study, bound into both native plan and raw header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColdStudy {
    pub run_id: String,
    pub run_attempt: u32,
}

/// A separately registered, fresh-history P8-006 cell. This is not a slice
/// of the original all-stage five-hour worker population.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileStudy {
    pub run_id: String,
    pub run_attempt: u32,
    pub mutation_profile: String,
    pub fanout: Option<usize>,
}

pub const MUTATION_PROFILES: [&str; 8] = [
    "no_op",
    "body",
    "api",
    "config",
    "batch_1",
    "batch_10",
    "batch_100",
    "batch_1000",
];

/// A disjoint slice of the registered repetition population. A successful
/// slice is never evidence that the omitted repetitions or scales ran.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScaleShard {
    pub index: usize,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalePlan {
    pub schema_version: u32,
    pub profile: Profile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_profile: Option<CapacityProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_scope: Option<StageScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_study: Option<ColdStudy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_study: Option<ProfileStudy>,
    pub files: Vec<usize>,
    pub seed: u64,
    pub repetitions: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shard: Option<ScaleShard>,
    #[serde(default)]
    pub skip_fanout: bool,
    pub dirty_budget: usize,
    pub max_resume_builds: usize,
    pub batch_sizes: Vec<usize>,
    pub fanouts: Vec<usize>,
    pub deadline_ms: u64,
    pub max_output_bytes: u64,
}

impl Default for ScalePlan {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: Profile::Smoke,
            capacity_profile: None,
            stage_scope: None,
            cold_study: None,
            profile_study: None,
            files: vec![60],
            seed: 0x00c0_ffee,
            repetitions: 1,
            shard: None,
            skip_fanout: false,
            dirty_budget: 2,
            max_resume_builds: 64,
            batch_sizes: vec![1, 10],
            fanouts: vec![2, 8],
            deadline_ms: 120_000,
            max_output_bytes: 16 * 1024 * 1024,
        }
    }
}

impl ScalePlan {
    pub fn validate(&self) -> Result<()> {
        let unique = |xs: &[usize]| xs.iter().collect::<BTreeSet<_>>().len() == xs.len();
        if self.schema_version != 1
            || self.files.is_empty()
            || self.files.len() > 5
            || !unique(&self.files)
            || self.files.iter().any(|n| !(60..=100_000).contains(n))
            || !(1..=200).contains(&self.repetitions)
            || self
                .shard
                .is_some_and(|s| s.count == 0 || s.count > self.repetitions || s.index >= s.count)
            || (self.skip_fanout && self.shard.is_none())
            || !(1..=10_000).contains(&self.dirty_budget)
            || !(1..=1024).contains(&self.max_resume_builds)
            || self.batch_sizes.is_empty()
            || self.batch_sizes.len() > 4
            || !unique(&self.batch_sizes)
            || self.batch_sizes.iter().any(|n| !(1..=1000).contains(n))
            || self.fanouts.is_empty()
            || self.fanouts.len() > 5
            || !unique(&self.fanouts)
            || self.fanouts.iter().any(|n| !(1..=128).contains(n))
            || !(1..=86_400_000).contains(&self.deadline_ms)
            || !(1024 * 1024..=512 * 1024 * 1024).contains(&self.max_output_bytes)
        {
            return Err(invalid(
                "invalid/duplicate scale, repetition, fanout, deadline or output bound",
            ));
        }
        if self.capacity_profile == Some(CapacityProfile::ScaleCapacityV1)
            && (self.profile != Profile::Release
                || self.files.len() != 1
                || self.dirty_budget != 200
                || self.max_resume_builds != 1024
                || self.shard.is_none_or(|s| s.count != self.repetitions))
        {
            return Err(invalid(
                "scale_capacity_v1 requires release, one scale, one repetition per shard, dirty budget 200 and resume budget 1024; fanout keeps 8/128",
            ));
        }
        if self.capacity_profile == Some(CapacityProfile::ScaleWideDirtyV1)
            && (self.profile != Profile::Release
                || self.files.len() != 1
                || self.dirty_budget != 4096
                || self.max_resume_builds != 1024
                || self.shard.is_none_or(|s| s.count != self.repetitions))
        {
            return Err(invalid(
                "scale_wide_dirty_v1 requires release, one scale, one repetition per shard, dirty budget 4096 and resume budget 1024; fanout keeps 8/128",
            ));
        }
        // This separate task protocol never changes the registered N30 study.
        // Its release cells are explicitly N1 and retain every work/resource bound.
        if self.stage_scope == Some(StageScope::ProfileTaskDescriptiveV1)
            && (self.profile != Profile::Release
                || self.capacity_profile != Some(CapacityProfile::ScaleCapacityV1)
                || self.files.len() != 1
                || self.files.iter().any(|n| !RELEASE_SCALES.contains(n))
                || self.repetitions != 1
                || self.shard.is_none_or(|s| s.index != 0 || s.count != 1)
                || self.seed != 0x00c0_ffee
                || self.deadline_ms != 18_000_000
                || self.max_output_bytes != 512 * 1024 * 1024
                || self.batch_sizes != [1, 10, 100, 1000]
                || self.fanouts != [1, 4, 16, 64, 128])
        {
            return Err(invalid(
                "profile_task_descriptive_v1 requires a release N1 cell at shard 0/1 with the fixed seed, scales, capacity and original budgets",
            ));
        }
        if self.profile == Profile::Release
            && (cfg!(debug_assertions)
                || (self.repetitions < 30
                    && self.stage_scope != Some(StageScope::ProfileTaskDescriptiveV1))
                || self.files.iter().any(|n| !RELEASE_SCALES.contains(n)))
        {
            return Err(invalid(
                "release measurement requires a release eval build, >=30 repetitions, and explicit canonical scales; this does not grant release certification",
            ));
        }
        if (self.stage_scope == Some(StageScope::ColdOnlyV1)) != self.cold_study.is_some()
            || self.cold_study.as_ref().is_some_and(|study| {
                study.run_id.is_empty()
                    || !study.run_id.bytes().all(|b| b.is_ascii_digit())
                    || study.run_id.parse::<u64>().map_or(true, |id| id == 0)
                    || study.run_attempt == 0
            })
        {
            return Err(invalid(
                "cold study identity must accompany only the explicit cold scope",
            ));
        }
        if self.stage_scope == Some(StageScope::ColdOnlyV1)
            && (!self.skip_fanout
                || self.shard.is_none()
                || (self.profile == Profile::Release
                    && (self.capacity_profile != Some(CapacityProfile::ScaleCapacityV1)
                        || self.repetitions != 30
                        || self.seed != 0x00c0_ffee
                        || self.deadline_ms != 18_000_000
                        || self.max_output_bytes != 512 * 1024 * 1024
                        || self.batch_sizes != [1, 10, 100, 1000]
                        || self.fanouts != [1, 4, 16, 64, 128])))
        {
            return Err(invalid(
                "cold_only_v1 requires an explicit shard without fanout; release keeps the fixed seed, N30, capacity and original time/output/work budgets",
            ));
        }
        if matches!(
            self.stage_scope,
            Some(StageScope::ProfileIsolatedV1 | StageScope::ProfileTaskDescriptiveV1)
        ) != self.profile_study.is_some()
        {
            return Err(invalid(
                "isolated profile identity requires its own explicit scope",
            ));
        }
        if let Some(study) = &self.profile_study {
            let fanout = study.mutation_profile == "fanout";
            if study.run_id.is_empty()
                || !study.run_id.bytes().all(|b| b.is_ascii_digit())
                || study.run_id.parse::<u64>().map_or(true, |id| id == 0)
                || study.run_attempt == 0
                || self.files.len() != 1
                || self.shard.is_none_or(|s| s.count != self.repetitions)
                || self.skip_fanout == fanout
                || (!fanout
                    && (!MUTATION_PROFILES.contains(&study.mutation_profile.as_str())
                        || study.fanout.is_some()))
                || (fanout
                    && (!study
                        .fanout
                        .is_some_and(|n| [1, 4, 16, 64, 128].contains(&n))
                        || self.files != [1000]))
                || (self.profile == Profile::Release
                    && (self.capacity_profile != Some(CapacityProfile::ScaleCapacityV1)
                        || (self.stage_scope == Some(StageScope::ProfileIsolatedV1)
                            && self.repetitions != 30)
                        || self.seed != 0x00c0_ffee
                        || self.deadline_ms != 18_000_000
                        || self.max_output_bytes != 512 * 1024 * 1024
                        || self.batch_sizes != [1, 10, 100, 1000]
                        || self.fanouts != [1, 4, 16, 64, 128]))
            {
                return Err(invalid(
                    "invalid isolated profile, fanout, identity or fixed release population/budget",
                ));
            }
        }
        Ok(())
    }

    pub fn repetition_range(&self) -> Result<std::ops::Range<usize>> {
        self.validate()?;
        Ok(match self.shard {
            Some(shard) => {
                self.repetitions * shard.index / shard.count
                    ..self.repetitions * (shard.index + 1) / shard.count
            }
            None => 0..self.repetitions,
        })
    }

    pub fn fanout_budgets(&self) -> (usize, usize) {
        match self.capacity_profile {
            Some(CapacityProfile::ScaleCapacityV1 | CapacityProfile::ScaleWideDirtyV1) => (8, 128),
            None => (self.dirty_budget, self.max_resume_builds),
        }
    }

    // The work profile is in the plan. Both named profiles use the unchanged
    // physical oracle limits; parity records this separate capacity identity.
    fn oracle_capacity_profile(&self) -> Option<&'static str> {
        self.capacity_profile.map(|_| "scale_capacity_v1")
    }
}

fn create_file(path: &Path) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}

fn control_json(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() > 128 * 1024 {
        return Err(invalid("control report exceeds its reserved output bound"));
    }
    create_file(path)?.write_all(&bytes)?;
    Ok(())
}

struct Raw {
    file: File,
    written: u64,
    limit: u64,
    clock: Instant,
}

impl Raw {
    fn emit(&mut self, event: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&event)?;
        bytes.push(b'\n');
        if self.written.saturating_add(bytes.len() as u64) > self.limit {
            return Err(BenchError::Timeout("raw_output_budget_exhausted".into()));
        }
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

struct ChildGuard(Child);
impl ChildGuard {
    fn stop(&mut self) {
        #[cfg(unix)]
        {
            // The worker alone creates this fresh process group. No shared
            // shell/session group is signalled, including on error paths.
            if let Ok(pid) = i32::try_from(self.0.id()) {
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            self.stop();
        }
    }
}

/// Execute the exact supplied eval binary in a disposable worker process. The
/// parent enforces a wall deadline even while a synchronous index build blocks.
/// All output files use create_new; no old raw run is overwritten.
pub fn run_supervised(plan: &ScalePlan, out: &Path, binary: &Path) -> Result<Value> {
    plan.validate()?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(out)?;
    let out = out.canonicalize()?;
    control_json(&out.join("plan.json"), &json!(plan))?;
    let stderr_file = create_file(&out.join("worker.stderr"))?;
    let binary_before = manifest::file_digest(binary).ok();
    // The supervisor owns all fixture directories, including those allocated
    // by the existing mutation runner; a deadline cannot orphan 100k trees.
    let workspace = tempfile::tempdir()?;
    let cold_temporary_environment = plan
        .stage_scope
        .is_some()
        .then(|| json!({"parent_root":std::env::temp_dir(),"worker_root":workspace.path()}));
    let mut command = Command::new(binary);
    command
        .arg("--worker-plan")
        .arg(out.join("plan.json"))
        .arg("--output")
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("RAYON_NUM_THREADS", "2")
        .env("TMPDIR", workspace.path())
        .env("TMP", workspace.path())
        .env("TEMP", workspace.path());
    for key in [
        "CODECORTEX_CACHE_DIR",
        "CODECORTEX_DIRTY_PROPAGATION",
        "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES",
        "CODECORTEX_MEMORY_BUDGET_FRACTION",
        "CODECORTEX_MAX_CONCURRENT_PARSE",
        "CODECORTEX_USE_DIRECT_WRITER",
    ] {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let started = Instant::now();
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(child) => ChildGuard(child),
        Err(error) => {
            let result = json!({"schema_version":1,"status":"infrastructure_error","error":error.to_string(),"exit_code":2,"release_certification":"not_run","full_100k_certification":"not_run"});
            control_json(&out.join("report.json"), &result)?;
            return Ok(result);
        }
    };
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| invalid("worker stderr unavailable"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let stderr_sink = Arc::new(Mutex::new(Some(stderr_file)));
    let sink = Arc::clone(&stderr_sink);
    let drain = std::thread::spawn(move || -> std::io::Result<()> {
        let mut written = 0u64;
        let mut buffer = [0u8; 8192];
        loop {
            let n = stderr.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            let mut output = sink
                .lock()
                .map_err(|_| std::io::Error::other("stderr sink poisoned"))?;
            let Some(log) = output.as_mut() else {
                break;
            };
            let keep = n.min(STDERR_LIMIT.saturating_sub(written) as usize);
            log.write_all(&buffer[..keep])?;
            written += keep as u64;
            if keep < n {
                flag.store(true, Ordering::Release);
            }
        }
        Ok(())
    });
    let mut termination = None;
    let exit = loop {
        if overflow.load(Ordering::Acquire) {
            termination = Some("stderr_output_budget_exhausted");
            child.stop();
            break None;
        }
        if started.elapsed() >= Duration::from_millis(plan.deadline_ms) {
            termination = Some("deadline_exceeded");
            child.stop();
            break None;
        }
        if let Some(exit) = child.0.try_wait()? {
            break exit.code();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // A completed worker may leave a descendant holding its stderr pipe.
    // Close our fresh process group before waiting for EOF, and never join a
    // thread that has not completed within the original worker deadline.
    child.stop();
    while !drain.is_finished() && started.elapsed() < Duration::from_millis(plan.deadline_ms) {
        let remaining = Duration::from_millis(plan.deadline_ms).saturating_sub(started.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    let stderr_ok = if drain.is_finished() {
        matches!(drain.join(), Ok(Ok(())))
    } else {
        termination.get_or_insert("stderr_drain_deadline_exceeded");
        false
    };
    // Even an abnormal descendant outside our process group cannot cause a
    // detached reader to mutate this run's stderr after its report is sealed.
    let stderr_sink_closed = match stderr_sink.lock() {
        Ok(mut sink) => {
            sink.take();
            true
        }
        Err(poisoned) => {
            poisoned.into_inner().take();
            false
        }
    };
    if termination.is_none() && overflow.load(Ordering::Acquire) {
        termination = Some("stderr_output_budget_exhausted");
    }
    let summary_path = out.join("worker-summary.json");
    let summary = if summary_path.is_file() {
        let mut bytes = Vec::new();
        File::open(&summary_path)?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            return Err(invalid("oversized worker summary"));
        }
        Some(serde_json::from_slice::<Value>(&bytes)?)
    } else {
        None
    };
    let binary_after = manifest::file_digest(binary).ok();
    let binary_unchanged = binary_before.is_some() && binary_before == binary_after;
    let cleanup_started = Instant::now();
    let cleanup_error = workspace.close().err().map(|e| e.to_string());
    let cleanup_ms = cleanup_started.elapsed().as_millis();
    let exit_code = if termination.is_some() {
        3
    } else if !stderr_ok || !stderr_sink_closed || !binary_unchanged || cleanup_error.is_some() {
        2
    } else {
        match exit {
            Some(0) if summary.as_ref().is_some_and(|s| s["passed"] == true) => 0,
            Some(1..=3) => exit.unwrap_or(2),
            _ => 2,
        }
    };
    let mut result = json!({
        "schema_version":1,"profile":plan.profile,"plan_digest":manifest::file_digest(&out.join("plan.json"))?,
        "worker_pid":child.0.id(),"worker_binary_digest_before":binary_before,"worker_binary_digest_after":binary_after,"worker_binary_unchanged":binary_unchanged,
        "status":termination.unwrap_or(if exit_code==0 {"measurement_complete"} else {"measurement_failed"}),
        "exit_code":exit_code,"worker_exit_code":exit,"wall_ms":started.elapsed().as_millis(),
        "stderr_complete":stderr_ok&&stderr_sink_closed&&!overflow.load(Ordering::Acquire),"summary":summary,
        "fixture_cleanup":{"elapsed_ms":cleanup_ms,"error":cleanup_error},
        "raw_digest":out.join("raw.jsonl").is_file().then(|| manifest::file_digest(&out.join("raw.jsonl"))).transpose()?,
        "release_certification":"not_run","release_prerequisites":"P7-020/P8-001/P8-004 and source/binary release lock are separate gates",
        "full_100k_certification":"not_run","max_output_bytes":plan.max_output_bytes,
        "shard_only":plan.shard.is_some(),
        "registered_repetitions":plan.repetitions,"executed_repetition_range":plan.repetition_range()?,
        "deadline_scope":"entire child lifetime and stderr drain, including generation, indexing, parity and evidence; parent polling interval 10ms"
    });
    if let Some(environment) = cold_temporary_environment {
        let key = if plan.profile_study.is_some() {
            "profile_temporary_environment"
        } else {
            "cold_temporary_environment"
        };
        result[key] = environment;
    }
    control_json(&out.join("report.json"), &result)?;
    Ok(result)
}

fn input_files(root: &Path) -> Result<Vec<manifest::FileRecord>> {
    fn walk(root: &Path, dir: &Path, entries: &mut Vec<manifest::FileRecord>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with(".codecortex") {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, entries)?;
            } else if entry.file_type()?.is_file() {
                let bytes = fs::read(&path)?;
                entries.push(manifest::FileRecord {
                    path: path
                        .strip_prefix(root)
                        .map_err(|_| invalid("fixture path"))?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    bytes: bytes.len() as u64,
                    digest: manifest::digest(&bytes),
                });
            } else {
                return Err(invalid("nonregular generated input"));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn counts(root: &Path) -> Result<Value> {
    let conn = Connection::open_with_flags(
        root.join(".codecortex/index.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| BenchError::Protocol(format!("scale DB open: {e}")))?;
    let mut tables = BTreeMap::new();
    for table in oracle::tables() {
        let n: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |r| {
                r.get(0)
            })
            .map_err(|e| BenchError::Protocol(format!("scale count {table}: {e}")))?;
        tables.insert(
            *table,
            u64::try_from(n).map_err(|_| invalid("negative SQLite count"))?,
        );
    }
    let db_bytes: u64 = ["index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm"]
        .iter()
        .filter_map(|p| fs::metadata(root.join(".codecortex").join(p)).ok())
        .map(|m| m.len())
        .sum();
    Ok(
        json!({"files":tables["files"],"symbols":tables["symbols"],"chunks":tables["chunks"],
        "edges":{"call_edges":tables["call_edges"],"semantic_edges":tables["semantic_edges"],"test_edges":tables["test_edges"]},
        "vectors":{"state":"disabled","count":Value::Null,"reason":"semantic explicitly disabled; no provider installed"},
        "tables":tables,"index_db_with_wal_shm_bytes":db_bytes}),
    )
}

fn payload_differences(
    incremental: Option<&Value>,
    full: Option<&Value>,
    pointer: &str,
    count: &mut usize,
    examples: &mut Vec<Value>,
) {
    if incremental == full {
        return;
    }
    match (incremental, full) {
        (Some(Value::Object(a)), Some(Value::Object(b))) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                payload_differences(
                    a.get(key),
                    b.get(key),
                    &format!("{pointer}/{escaped}"),
                    count,
                    examples,
                );
            }
        }
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            for index in 0..a.len().max(b.len()) {
                payload_differences(
                    a.get(index),
                    b.get(index),
                    &format!("{pointer}/{index}"),
                    count,
                    examples,
                );
            }
        }
        _ => {
            *count += 1;
            if examples.len() < 32 {
                examples.push(json!({
                    "pointer": pointer,
                    "incremental_present": incremental.is_some(),
                    "full_present": full.is_some(),
                    "incremental": incremental,
                    "full": full,
                }));
            }
        }
    }
}

// The oracle sorts complete JSON rows. A changed digest can move a manifest to
// another position, so diagnostics must join by file_path before comparing its
// payload. This never changes the full-row equality used by the oracle gate.
fn manifest_difference_examples(a: &[Value], b: &[Value]) -> Result<(usize, Vec<Value>)> {
    fn by_file(rows: &[Value]) -> Result<BTreeMap<&str, &Value>> {
        let mut result = BTreeMap::new();
        for row in rows {
            let file = row["file_path"]
                .as_str()
                .ok_or_else(|| invalid("manifest diagnostic missing file_path"))?;
            if result.insert(file, row).is_some() {
                return Err(invalid("manifest diagnostic duplicate file_path"));
            }
        }
        Ok(result)
    }
    fn payload(row: Option<&Value>) -> Result<Option<Value>> {
        row.map(|row| {
            let payload = row["payload"]
                .as_str()
                .ok_or_else(|| invalid("manifest diagnostic missing payload"))?;
            Ok(serde_json::from_str(payload)?)
        })
        .transpose()
    }
    let a = by_file(a)?;
    let b = by_file(b)?;
    let keys: BTreeSet<_> = a.keys().chain(b.keys()).copied().collect();
    let changed: Vec<_> = keys
        .into_iter()
        .filter(|file| a.get(file) != b.get(file))
        .collect();
    let mut examples = Vec::new();
    for file in changed.iter().take(3) {
        let incremental = a.get(file).copied();
        let full = b.get(file).copied();
        let incremental_payload = payload(incremental)?;
        let full_payload = payload(full)?;
        let mut count = 0;
        let mut differences = Vec::new();
        payload_differences(
            incremental_payload.as_ref(),
            full_payload.as_ref(),
            "/payload",
            &mut count,
            &mut differences,
        );
        examples.push(json!({
            "file_path": file,
            "incremental_present": incremental.is_some(),
            "full_present": full.is_some(),
            "incremental": incremental,
            "full": full,
            "payload_difference_count": count,
            "payload_difference_examples": differences,
            "payload_difference_examples_limit": 32,
        }));
    }
    Ok((changed.len(), examples))
}

fn parity(a: &Path, b: &Path, complete: bool, plan: &ScalePlan) -> Result<Value> {
    let ac = counts(a)?;
    let bc = counts(b)?;
    let over = [&ac, &bc].iter().any(|c| {
        c["tables"].as_object().is_some_and(|ts| {
            ts.values()
                .any(|n| n.as_u64().unwrap_or(u64::MAX) > ORACLE_ROWS)
        })
    });
    if over {
        let mut comparison = if plan.oracle_capacity_profile().is_some() {
            oracle::compare_streaming_scale_capacity_v1(a, b)?
        } else {
            oracle::compare_streaming(a, b, oracle::StreamingLimits::default())?
        };
        if !complete {
            comparison["status"] = json!("incomplete_not_certified");
            comparison["equal"] = json!(false);
        }
        comparison["incremental_counts"] = ac;
        comparison["full_counts"] = bc;
        return Ok(comparison);
    }
    let ca = oracle::canonical(a)?;
    let cb = oracle::canonical(b)?;
    let mut table_evidence = Vec::new();
    let mut different = Vec::new();
    for table in oracle::tables() {
        let same = ca[*table] == cb[*table];
        if !same {
            different.push(*table);
        }
        let (different_row_count, difference_alignment, different_rows) = if same {
            (0, "sorted_row", vec![])
        } else if *table == "resolution_manifests" {
            let (count, examples) = manifest_difference_examples(&ca[*table], &cb[*table])?;
            (count, "file_path", examples)
        } else {
            let positions: Vec<_> = (0..ca[*table].len().max(cb[*table].len()))
                .filter(|&i| ca[*table].get(i) != cb[*table].get(i))
                .collect();
            let examples = positions.iter().take(3)
                .map(|&i|json!({"sorted_row":i,"incremental":ca[*table].get(i),"full":cb[*table].get(i)})).collect::<Vec<_>>();
            (positions.len(), "sorted_row", examples)
        };
        table_evidence.push(json!({"table":table,"equal":same,"incremental_rows":ca[*table].len(),"full_rows":cb[*table].len(),
            "incremental_digest":manifest::digest(&serde_json::to_vec(&ca[*table])?),"full_digest":manifest::digest(&serde_json::to_vec(&cb[*table])?),
            "different_row_count":different_row_count,"difference_alignment":difference_alignment,
            "different_row_examples":different_rows}));
    }
    Ok(
        json!({"status":if !complete{"incomplete_not_certified"}else if different.is_empty(){"equal"}else{"different"},
        "equal":complete&&different.is_empty(),"different_tables":different,"tables":table_evidence,
        "comparison":"complete unchanged oracle rows compared before hashing; only exported differing examples are bounded",
        "incremental_counts":ac,"full_counts":bc,"capacity_profile":plan.oracle_capacity_profile()}),
    )
}

fn build(backend: &CodeIndexBackend, full: bool, label: &str, raw: &mut Raw) -> Result<Value> {
    let start = raw.clock.elapsed().as_micros();
    raw.emit(json!({"event":"build_started","label":label,"full":full,"start_us":start}))?;
    let result = backend.build_index_report(full);
    let end = raw.clock.elapsed().as_micros();
    match result {
        Ok(report) => {
            raw.emit(json!({"event":"build_finished","label":label,"full":full,"start_us":start,"end_us":end,"wall_us":end-start,
                "report":report,"process_snapshot":sampler::process_snapshot(std::process::id()),
                "resource_scope":"single worker including in-process MCP; snapshot, not peak or process-tree proof"}))?;
            Ok(report)
        }
        Err(error) => {
            raw.emit(json!({"event":"build_failed","label":label,"full":full,"start_us":start,"end_us":end,"wall_us":end-start,"error":error.to_string()}))?;
            Err(BenchError::Tool(error.to_string()))
        }
    }
}

fn has_no_parse_error(report: &Value) -> bool {
    report["parse_errors"].as_array().is_some_and(Vec::is_empty)
}

fn summed_elapsed_ms<'a>(reports: impl IntoIterator<Item = &'a Value>) -> Option<u64> {
    let mut count = 0;
    let total = reports.into_iter().try_fold(0u64, |sum, report| {
        count += 1;
        sum.checked_add(report["elapsed_ms"].as_u64()?)
    })?;
    (count > 0).then_some(total)
}

fn apply_both(a: &Path, b: &Path, mutation: &Mutation) -> Result<()> {
    mutation.apply(a)?;
    mutation.apply(b)
}

fn config_source(target: &str) -> String {
    json!({"compilerOptions":{"baseUrl":".","paths":{"@p8/config":[format!("./{target}")]}}})
        .to_string()
}

fn config_fact(root: &Path, consumer: &str, target: &str) -> Result<Value> {
    let conn = Connection::open_with_flags(
        root.join(".codecortex/index.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| BenchError::Protocol(e.to_string()))?;
    let mut statement=conn.prepare("SELECT target_file_path FROM call_edges WHERE file_path=?1 AND callee_symbol='p8_config_target'")
        .map_err(|e|BenchError::Protocol(e.to_string()))?;
    let actual = statement
        .query_map([consumer], |r| r.get::<_, Option<String>>(0))
        .map_err(|e| BenchError::Protocol(e.to_string()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    Ok(
        json!({"consumer":consumer,"expected_target":target,"actual_targets":actual,"passed":actual==vec![Some(target.into())]}),
    )
}

fn scale_sample(
    plan: &ScalePlan,
    n: usize,
    repetition: usize,
    raw: &mut Raw,
) -> Result<Vec<Value>> {
    let a = tempfile::tempdir()?;
    let b = tempfile::tempdir()?;
    let spec = synth::SynthSpec {
        target_files: n,
        seed: plan.seed,
    };
    let generated = synth::generate(a.path(), &spec).map_err(invalid)?;
    synth::generate(b.path(), &spec).map_err(invalid)?;
    let runtime_config = json!({"auto_index":{"enabled":false},"indexing":{"dirty_propagation":true,"dirty_propagation_max_files":plan.dirty_budget,"db_read_pool_size":1,"max_concurrent_parse":2},
        "semantic":{"enabled":false,"network_opt_in":false,"allow_query_network":false}});
    let config_files: Vec<_> = generated
        .code_file_paths
        .iter()
        .filter(|p| p.ends_with(".ts"))
        .take(3)
        .cloned()
        .collect();
    if config_files.len() != 3 {
        return Err(invalid("synthetic TS config fixture needs three files"));
    }
    for root in [a.path(), b.path()] {
        fs::write(
            root.join(".codecortex.json"),
            serde_json::to_vec(&runtime_config)?,
        )?;
        Mutation::Write {
            path: "tsconfig.json".into(),
            content: config_source(&config_files[0]),
        }
        .apply(root)?;
        for path in &config_files[..2] {
            let old = fs::read_to_string(root.join(path))?;
            Mutation::Write {
                path: path.clone(),
                content: format!(
                    "{old}\nexport function p8_config_target(x:number):number{{return x;}}\n"
                ),
            }
            .apply(root)?;
        }
        let old = fs::read_to_string(root.join(&config_files[2]))?;
        Mutation::Write{path:config_files[2].clone(),content:format!("{old}\nimport {{p8_config_target}} from '@p8/config';\nexport function p8_config_user(x:number):number{{return p8_config_target(x);}}\n")}.apply(root)?;
    }
    let files = input_files(a.path())?;
    if files != input_files(b.path())? {
        return Err(invalid("independent synthetic trees differ"));
    }
    // Preserve the existing ordinary-code order for small batches. The
    // original benchmark requires changed files, not exclusively ordinary
    // code slots: the 1k corpus also contains route TS and YAML inputs. Add
    // those existing generated inputs only when the requested batch needs
    // them; never create filler files or silently shrink a 1000-file batch.
    let code_files: BTreeSet<_> = generated
        .code_file_paths
        .iter()
        .map(String::as_str)
        .collect();
    let mut batch_files = generated.code_file_paths.clone();
    batch_files.extend(
        files
            .iter()
            .filter(|f| f.path != "tsconfig.json" && !code_files.contains(f.path.as_str()))
            .map(|f| f.path.clone()),
    );
    let prefix = format!("scale-{n}/repetition-{repetition}");
    raw.emit(
        json!({"event":"input","sample":prefix,"seed":plan.seed,"synthetic_files_requested":n,
        "synthetic_files_written":generated.files_written,"auxiliary_visible_config_files":1,
        "generated_named_functions":generated.functions_planned,"runtime_config":runtime_config,
        "fixture_version":"p8-synth-config-fixture-v1","batch_target_policy":"generated_code_then_existing_route_and_yaml_v2",
        "config_fixture_augmented_code_files":config_files,"config_fixture_added_functions":3,
        "input_digest":manifest::digest(&serde_json::to_vec(&files)?),"files":files,
        "cold_definition":"fresh generated projects and empty indexes; OS page cache is retained"}),
    )?;
    let aa =
        CodeIndexBackend::new_unindexed(a.path()).map_err(|e| BenchError::Tool(e.to_string()))?;
    let bb =
        CodeIndexBackend::new_unindexed(b.path()).map_err(|e| BenchError::Tool(e.to_string()))?;
    let first = build(&aa, true, &format!("{prefix}/cold_incremental"), raw)?;
    let full = build(&bb, true, &format!("{prefix}/cold_full_control"), raw)?;
    let parity_started = Instant::now();
    let initial = parity(
        a.path(),
        b.path(),
        first["resolution_freshness"]["complete"] == true
            && full["resolution_freshness"]["complete"] == true,
        plan,
    )?;
    let parity_wall_us = parity_started.elapsed().as_micros();
    let initial_fact = config_fact(a.path(), &config_files[2], &config_files[0])?;
    raw.emit(json!({"event":"cold_parity","sample":prefix,"parity":initial,"parity_wall_us":parity_wall_us,"independent_config_fact":initial_fact}))?;
    let mut summaries = vec![
        json!({"sample":prefix,"stage":"cold","passed":initial["equal"]==true&&initial_fact["passed"]==true&&has_no_parse_error(&first)&&has_no_parse_error(&full),"parity_status":initial["status"]}),
    ];
    if plan.stage_scope == Some(StageScope::ColdOnlyV1) {
        return Ok(summaries);
    }
    let mut stages = vec![
        ("no_op".to_owned(), 0),
        ("body".to_owned(), 1),
        ("api".to_owned(), 1),
        ("config".to_owned(), 1),
    ];
    stages.extend(plan.batch_sizes.iter().map(|&n| (format!("batch_{n}"), n)));
    if let Some(study) = &plan.profile_study {
        stages.retain(|(stage, _)| stage == &study.mutation_profile);
        if stages.len() != 1 {
            return Err(invalid(
                "isolated scale cell must execute exactly one registered mutation",
            ));
        }
    }
    for (stage, requested) in stages {
        let label = format!("{prefix}/{stage}");
        let mut ops = Vec::new();
        match stage.as_str() {
            "no_op" => {}
            "body" => {
                let p = &generated.ground_truth.needle_file;
                let old = fs::read_to_string(a.path().join(p))?;
                if !old.contains("  return ") {
                    return Err(invalid("synthetic body shape drift"));
                }
                let new = old.replacen(
                    "  return ",
                    &format!(
                        "  const p8_body_{repetition} = {};\n  return ",
                        plan.seed.wrapping_add(repetition as u64)
                    ),
                    1,
                );
                ops.push(Mutation::Write {
                    path: p.clone(),
                    content: new,
                });
            }
            "api" => {
                let p = &generated.ground_truth.hub_file;
                let old = fs::read_to_string(a.path().join(p))?;
                let before = format!("{}(input: number)", generated.ground_truth.hub_symbol);
                if !old.contains(&before) {
                    return Err(invalid("synthetic API shape drift"));
                }
                let after = format!(
                    "{}(input: number, p8_offset: number = {})",
                    generated.ground_truth.hub_symbol,
                    repetition + 1
                );
                ops.push(Mutation::Write {
                    path: p.clone(),
                    content: old.replacen(&before, &after, 1),
                });
            }
            "config" => ops.push(Mutation::Write {
                path: "tsconfig.json".into(),
                content: config_source(&config_files[1]),
            }),
            _ => {
                if requested > batch_files.len() {
                    let item = json!({"sample":prefix,"stage":stage,"passed":false,"status":"not_run_batch_exceeds_generated_files","requested":requested,"available":batch_files.len()});
                    raw.emit(json!({"event":"stage_not_run","detail":item}))?;
                    summaries.push(item);
                    continue;
                }
                for p in batch_files.iter().take(requested) {
                    let comment = if p.ends_with(".py") || p.ends_with(".yaml") {
                        "#"
                    } else {
                        "//"
                    };
                    let old = fs::read_to_string(a.path().join(p))?;
                    ops.push(Mutation::Write {
                        path: p.clone(),
                        content: format!(
                            "{old}\n{comment} p8 batch {requested} repetition {repetition}\n"
                        ),
                    });
                }
            }
        }
        let mutation_receipts=ops.iter().map(|op|match op {Mutation::Write{path,content}=>Ok(json!({"path":path,"before_digest":manifest::file_digest(&a.path().join(path))?,"after_digest":manifest::digest(content.as_bytes()),"bytes":content.len()})),_=>Err(invalid("unexpected scale mutation"))}).collect::<Result<Vec<_>>>()?;
        raw.emit(json!({"event":"mutation","label":label,"operations":ops,"receipts":mutation_receipts,"requested_files":requested}))?;
        for op in &ops {
            apply_both(a.path(), b.path(), op)?;
        }
        let started = Instant::now();
        let mut reports = vec![build(&aa, false, &format!("{label}/incremental-0"), raw)?];
        for resume in 1..=plan.max_resume_builds {
            if reports
                .last()
                .is_some_and(|r| r["resolution_freshness"]["complete"] == true)
            {
                break;
            }
            reports.push(build(
                &aa,
                false,
                &format!("{label}/incremental-{resume}"),
                raw,
            )?);
        }
        let incremental_wall_us = started.elapsed().as_micros();
        let full = build(&bb, true, &format!("{label}/full_control"), raw)?;
        let complete = reports
            .last()
            .is_some_and(|r| r["resolution_freshness"]["complete"] == true);
        let full_complete = full["resolution_freshness"]["complete"] == true;
        let parity_started = Instant::now();
        let comparison = parity(a.path(), b.path(), complete && full_complete, plan)?;
        let parity_wall_us = parity_started.elapsed().as_micros();
        let fact = config_fact(
            a.path(),
            &config_files[2],
            if stage == "config" || (plan.profile_study.is_none() && stage.starts_with("batch_")) {
                &config_files[1]
            } else {
                &config_files[0]
            },
        )?;
        let no_op_unchanged = stage != "no_op"
            || reports.first().is_some_and(|r| {
                r["files_added"] == 0 && r["files_updated"] == 0 && r["files_removed"] == 0
            });
        let passed = comparison["equal"] == true
            && fact["passed"] == true
            && no_op_unchanged
            && reports.iter().all(has_no_parse_error)
            && has_no_parse_error(&full);
        raw.emit(json!({"event":"stage_finished","label":label,"incremental_wall_us":incremental_wall_us,
            "incremental_builds":reports.len(),"complete":complete,"full_complete":full_complete,"parity":comparison,"parity_wall_us":parity_wall_us,"passed":passed,"independent_config_fact":fact,"no_op_unchanged":no_op_unchanged,
            "phase_count_definition":"raw IndexReport files_*, dirty_plan, document_changes and project_model are preserved for every actual build"}))?;
        summaries.push(json!({"sample":prefix,"stage":stage,"passed":passed,"parity_status":comparison["status"],"incremental_builds":reports.len(),"incremental_wall_us":incremental_wall_us,
            "incremental_engine_elapsed_ms":summed_elapsed_ms(&reports)}));
    }
    Ok(summaries)
}

/// Hand-authored import/call expectations accompany the unchanged full oracle.
/// The fanout fixture is a separate workload; its file count is never presented
/// as the requested 1k–100k synthetic corpus count.
pub fn fanout_case(
    fanout: usize,
    seed: u64,
    dirty_budget: usize,
    max_resume_builds: usize,
) -> Result<mutation_case::MutationCase> {
    if !(1..=128).contains(&fanout) {
        return Err(invalid("fanout outside 1..128"));
    }
    let mut initial = BTreeMap::from([(
        "api.ts".into(),
        "export function scale_ping(x:number):number{return x;}\n".into(),
    )]);
    let mut assertions = Vec::new();
    for n in 0..fanout {
        let p = format!("use_{n:03}.ts");
        initial.insert(p.clone(),format!("import {{scale_ping}} from './api';\nexport function run_{n}(x:number):number{{return scale_ping(x);}}\n"));
        assertions.push(mutation_case::FactAssertion {
            id: format!("caller_{n}"),
            table: "call_edges".into(),
            matches: [
                ("file_path".into(), json!(p)),
                ("callee_symbol".into(), json!("scale_ping")),
                ("target_file_path".into(), json!("api.ts")),
            ]
            .into(),
            count: 1,
        });
    }
    let case=mutation_case::MutationCase{schema_version:1,name:format!("p8-fanout-{fanout}-{seed}"),initial,
        stages:vec![mutation_case::Stage{mutation:Mutation::Write{path:"api.ts".into(),content:format!("export function scale_ping(x:number,offset:number={}):number{{return x+offset;}}\n",seed%1000+1)},reopen:false,settle:true,assertions}],dirty_budget,max_resume_builds};
    case.validate()?;
    Ok(case)
}

fn distribution(samples: &[&Value], field: &str) -> Value {
    let values: Vec<_> = samples.iter().filter_map(|s| s[field].as_u64()).collect();
    let total: u128 = values.iter().map(|&v| u128::from(v)).sum();
    json!({"n":values.len(),"min":values.iter().min(),"max":values.iter().max(),
        "mean":if values.is_empty(){None}else{Some(total as f64/values.len() as f64)}})
}

fn worker_measure(plan: &ScalePlan, raw: &mut Raw) -> Result<Value> {
    let mut started = json!({"event":"run_started","plan":plan,"engine":runner::engine_provenance(Some(&std::env::current_exe()?))?,
        "evidence_layer":"actual native parser/SQLite; synthetic builds use existing in-process MCP; fanout uses existing mutation_case",
        "semantic":"disabled","release_certification":"not_run","tail_latency_estimate":Value::Null,
        "shard_only":plan.shard.is_some(),"registered_repetitions":plan.repetitions,"executed_repetition_range":plan.repetition_range()?,
        "statistics":"all repetitions retained; no best-of selection, speedup, stable p95/p99 or independent-sample quality inference"});
    if plan.stage_scope.is_some() {
        let seed_cap = std::env::var("CODECORTEX_SEED_CACHE_MAX_SYMBOLS");
        let parsed = seed_cap.as_ref().ok().and_then(|v| v.parse::<usize>().ok());
        let key = if plan.profile_study.is_some() {
            "profile_environment"
        } else {
            "cold_environment"
        };
        started[key] = json!({
            "seed_cache_max_symbols": {
                "raw": seed_cap.as_ref().ok(),
                "read_status": match &seed_cap {
                    Ok(_) => "unicode",
                    Err(std::env::VarError::NotPresent) => "absent",
                    Err(std::env::VarError::NotUnicode(_)) => "non_unicode",
                },
                "parsed_usize": parsed,
                "effective": cc_db::seed_cache_max_symbols(),
            },
            "runtime_environment": {
                "RUSTFLAGS": std::env::var("RUSTFLAGS").ok(),
                "CARGO_ENCODED_RUSTFLAGS": std::env::var("CARGO_ENCODED_RUSTFLAGS").ok(),
                "TMPDIR": std::env::var("TMPDIR").ok(),
                "TMP": std::env::var("TMP").ok(),
                "TEMP": std::env::var("TEMP").ok(),
            },
            "scope": "observed inputs; no cache, capacity or engine-setting override",
        });
    }
    raw.emit(started)?;
    let mut summaries = Vec::new();
    for &files in plan.files.iter().filter(|_| {
        plan.profile_study
            .as_ref()
            .is_none_or(|s| s.mutation_profile != "fanout")
    }) {
        for repetition in plan.repetition_range()? {
            summaries.extend(scale_sample(plan, files, repetition, raw)?);
        }
    }
    for &fanout in plan.fanouts.iter().filter(|&&n| {
        !plan.skip_fanout
            && plan
                .profile_study
                .as_ref()
                .is_none_or(|s| s.fanout == Some(n))
    }) {
        for repetition in plan.repetition_range()? {
            let (dirty_budget, max_resume_builds) = plan.fanout_budgets();
            let case = fanout_case(fanout, plan.seed, dirty_budget, max_resume_builds)?;
            raw.emit(json!({"event":"fanout_started","fanout":fanout,"repetition":repetition,"case":case}))?;
            let started = Instant::now();
            let result = if plan.profile_study.is_some() {
                mutation_case::evaluate_with_initial_evidence(&case)?
            } else {
                mutation_case::evaluate(&case)?
            };
            let passed = result["passed"] == true;
            let first_incomplete = result
                .pointer("/checkpoints/0/reports/0/resolution_freshness/complete")
                == Some(&json!(false));
            let replay_wall_us = started.elapsed().as_micros();
            let reports: Vec<_> = result["checkpoints"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|checkpoint| checkpoint["reports"].as_array())
                .flatten()
                .collect();
            let elapsed_ms = summed_elapsed_ms(reports.iter().copied());
            let builds = reports.len();
            raw.emit(json!({"event":"fanout_finished","fanout":fanout,"fixture_files":case.initial.len(),"repetition":repetition,
                "wall_us":replay_wall_us,"incremental_engine_elapsed_ms":elapsed_ms,"incremental_builds":builds,
                "result":result,"passed":passed,"first_build_incomplete":first_incomplete,
                "measurement_scope":if plan.profile_study.is_some() {
                    "whole fixture replay includes fresh setup and initial/final parity; incremental and full-control timings are retained original reports"
                } else {
                    "whole fixture replay wall time; incremental phase counts/timings are the original reports; full rebuild timing is unavailable"
                }}))?;
            summaries.push(json!({"stage":"fanout","fanout":fanout,"repetition":repetition,"passed":passed,"first_build_incomplete":first_incomplete,
                "fanout_replay_wall_us":replay_wall_us,"incremental_engine_elapsed_ms":elapsed_ms,"incremental_builds":builds}));
        }
    }
    let passed = summaries.iter().all(|s| s["passed"] == true);
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for sample in &summaries {
        let key = if sample["stage"] == "fanout" {
            format!("fanout-{}", sample["fanout"])
        } else {
            format!(
                "{}/{}",
                sample["sample"]
                    .as_str()
                    .unwrap_or("unknown")
                    .split('/')
                    .next()
                    .unwrap_or("unknown"),
                sample["stage"].as_str().unwrap_or("unknown")
            )
        };
        groups.entry(key).or_default().push(sample);
    }
    let groups:Vec<_>=groups.into_iter().map(|(name,samples)|{
        json!({"group":name,"samples":samples.len(),"passed":samples.iter().filter(|s|s["passed"]==true).count(),
            "failed_or_not_compared":samples.iter().filter(|s|s["passed"]!=true).count(),
            "outer_incremental_us_including_evidence":distribution(&samples,"incremental_wall_us"),
            "incremental_engine_elapsed_ms_sum_across_resumes":distribution(&samples,"incremental_engine_elapsed_ms"),
            "whole_fanout_fixture_replay_us":distribution(&samples,"fanout_replay_wall_us"),
            "first_build_incomplete_samples":samples.iter().filter(|s|s["first_build_incomplete"]==true).count()})
    }).collect();
    let mut summary = json!({"schema_version":1,"passed":passed,"sample_count":summaries.len(),"groups":groups,"raw_bytes":raw.written,
        "release_certification":"not_run","full_100k_certification":"not_run","prerequisite_gates":"not_evaluated",
        "shard_only":plan.shard.is_some(),"registered_repetitions":plan.repetitions,"executed_repetition_range":plan.repetition_range()?,
        "not_selected_scales":RELEASE_SCALES.iter().filter(|n|!plan.files.contains(n)).collect::<Vec<_>>(),
        "oracle_row_limit_unchanged":ORACLE_ROWS,"stable_tail_latency":"not_established"});
    if let Some(scope) = plan.stage_scope {
        summary["stage_scope"] = serde_json::to_value(scope)?;
    }
    Ok(summary)
}

/// Internal worker entry. The CLI supervisor is required for the hard deadline.
pub fn run_worker(plan: &ScalePlan, out: &Path) -> Result<i32> {
    plan.validate()?;
    let mut raw = Raw {
        file: create_file(&out.join("raw.jsonl"))?,
        written: 0,
        limit: plan.max_output_bytes - CONTROL_RESERVE,
        clock: Instant::now(),
    };
    let (summary, exit) = match worker_measure(plan, &mut raw) {
        Ok(summary) => {
            let exit = if summary["passed"] == true { 0 } else { 1 };
            (summary, exit)
        }
        Err(error) => {
            let exit = if matches!(error, BenchError::Timeout(_)) {
                3
            } else {
                2
            };
            (
                json!({"schema_version":1,"passed":false,"error":error.to_string(),"raw_bytes":raw.written,"release_certification":"not_run","full_100k_certification":"not_run"}),
                exit,
            )
        }
    };
    control_json(&out.join("worker-summary.json"), &summary)?;
    Ok(exit)
}
