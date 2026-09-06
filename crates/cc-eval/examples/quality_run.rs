//! cargo run -p cc-eval --example quality_run -- MANIFEST OUTPUT_DIR [REPETITIONS] [CONFIG_JSON]
//! Append-only raw JSONL before scoring. Never indexes the caller's source tree.
use cc_eval::quality::{self, Header, Manifest, Sample, Task};
use cc_eval::runner::CodeIndexBackend;
use serde_json::{json, Value};
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;
use tempfile::TempDir;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn command(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn git_blob(bytes: &[u8]) -> Result<String> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing git stdin")?
        .write_all(bytes)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("git hash-object failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
fn write_line(writer: &mut BufWriter<File>, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}
fn measure(
    backend: &CodeIndexBackend,
    task: &Task,
    mode: &str,
    iteration: usize,
) -> Result<Sample> {
    let start = Instant::now();
    let result = backend.call_tool(&task.tool, &task.params);
    let elapsed_us = start.elapsed().as_micros().try_into()?;
    let (output, error) = match result {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let output_bytes = match &output {
        Some(v) => serde_json::to_vec(v)?.len(),
        None => 0,
    };
    Ok(Sample {
        task_id: task.id.clone(),
        mode: mode.into(),
        iteration,
        elapsed_us,
        output_bytes,
        output,
        error,
    })
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if !(3..=5).contains(&args.len()) {
        return Err("usage: quality_run MANIFEST OUTPUT_DIR [REPETITIONS] [CONFIG_JSON]".into());
    }
    let bytes = fs::read(&args[1])?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    let repetitions: usize = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(3);
    if !(1..=1000).contains(&repetitions) {
        return Err("repetitions must be 1..1000".into());
    }
    let mut config: Value = if let Some(path) = args.get(4) {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        json!({})
    };
    if !config.is_object() {
        return Err("config must be a JSON object".into());
    }
    config["auto_index"] = json!({"enabled":false});
    let effective: cc_model::ProjectConfig = serde_json::from_value(config)?;
    let effective_config = serde_json::to_value(effective)?;
    let output_dir = Path::new(&args[2]);
    fs::create_dir_all(output_dir)?;
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(output_dir.join("raw.jsonl"))?;
    let mut writer = BufWriter::new(file);
    let header = Header {
        schema_version: 1,
        implementation_commit: command("git", &["rev-parse", "HEAD"])?,
        rustc: command("rustc", &["--version"])?,
        provenance: json!({
            "commit_tree": command("git", &["rev-parse", "HEAD^{tree}"] )?,
            "tracked_diff_git_blob": git_blob(command("git", &["diff", "HEAD", "--"])?.as_bytes())?,
            "worktree_status": command("git", &["status", "--porcelain"] )?,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "available_parallelism": std::thread::available_parallelism().map(|n|n.get()).ok(),
            "rustflags": std::env::var("RUSTFLAGS").unwrap_or_default(),
            "git_diff_note": "Baseline instrumentation may change eval-only files; inspect the recorded worktree status."
        }),
        manifest_git_blob: git_blob(&bytes)?,
        repetitions,
        variant: args.get(4).cloned().unwrap_or_else(|| "default".into()),
        effective_config,
        manifest,
    };
    write_line(&mut writer, &json!({"kind":"header","data":header}))?;
    for repo in &header.manifest.repositories {
        let temp = TempDir::new()?;
        for (path, text) in &repo.files {
            let target = temp.path().join(path);
            fs::create_dir_all(target.parent().ok_or("file without parent")?)?;
            fs::write(target, text)?;
        }
        fs::write(
            temp.path().join(".codecortex.json"),
            serde_json::to_vec_pretty(&header.effective_config)?,
        )?;
        let backend = CodeIndexBackend::new_unindexed(temp.path())?;
        let start = Instant::now();
        let build = backend.build_index_report(true)?;
        let index_elapsed_us = start.elapsed().as_micros();
        write_line(
            &mut writer,
            &json!({"kind":"index","repo":repo.id,"revision":repo.revision,
            "snapshot_git_blob":git_blob(&serde_json::to_vec(&repo.files)?)?,
            "elapsed_us":index_elapsed_us,"report":build}),
        )?;
        for task in header
            .manifest
            .tasks
            .iter()
            .filter(|task| task.repo == repo.id)
        {
            for iteration in 0..repetitions {
                let cold = CodeIndexBackend::open_existing(temp.path())?;
                let sample = measure(&cold, task, "cold_session", iteration)?;
                write_line(&mut writer, &json!({"kind":"sample","data":sample}))?;
            }
            let warmup = measure(&backend, task, "warmup", 0)?;
            write_line(&mut writer, &json!({"kind":"warmup","data":warmup}))?;
            for iteration in 0..repetitions {
                let sample = measure(&backend, task, "warm_cache", iteration)?;
                write_line(&mut writer, &json!({"kind":"sample","data":sample}))?;
            }
        }
    }
    // No accumulated response payload competes with index memory during timing.
    drop(writer);
    let (saved_header, samples) =
        quality::read_raw(BufReader::new(File::open(output_dir.join("raw.jsonl"))?))?;
    let report = quality::report(&saved_header, &samples)?;
    fs::write(
        output_dir.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report["passed"] != true {
        return Err("quality gate failed; raw observations retained".into());
    }
    Ok(())
}
