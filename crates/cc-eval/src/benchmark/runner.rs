use super::{
    adapters::Backend,
    gate::Gate,
    manifest::{self, LoadedSuite},
    normalizer,
    readiness::State,
    report::{self, RunManifest},
    sampler,
    schema::*,
    statistics, BenchError, Result,
};
use serde_json::{json, Value};
use std::{path::Path, process::Command, time::Instant};
pub fn engine_provenance(binary: Option<&Path>) -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let dirty = git(&["status", "--porcelain", "--untracked-files=all"]);
    let diff = git(&["diff", "--binary"]);
    let mut source_entries = Vec::new();
    fn walk(path: &Path, base: &Path, entries: &mut Vec<(String, String)>) -> Result<()> {
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_symlink() {
                continue;
            }
            if p.is_dir() {
                walk(&p, base, entries)?;
            } else if matches!(p.extension().and_then(|s| s.to_str()), Some("rs" | "toml")) {
                entries.push((
                    p.strip_prefix(base)
                        .map_err(|_| super::invalid("source path"))?
                        .to_string_lossy()
                        .to_string(),
                    manifest::file_digest(&p)?,
                ));
            }
        }
        Ok(())
    }
    walk(&root.join("crates"), &root, &mut source_entries)?;
    source_entries.sort();
    let source_digest = manifest::digest(&serde_json::to_vec(&source_entries)?);
    let hardware = Command::new("sysctl")
        .args(["-n", "hw.memsize", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    Ok(
        json!({"engine_head_observed":git(&["rev-parse","HEAD"]),"dirty_observed":dirty,
        "tracked_diff_digest":diff.as_ref().map(|d|manifest::digest(d.as_bytes())),
        "binary_digest":binary.map(manifest::file_digest).transpose()?,
        "binary_source_binding":"recorded build receipt required; path/HEAD alone do not prove binding",
        "eval_version":env!("CARGO_PKG_VERSION"),"source_files_digest":source_digest,"cargo_lock_digest":manifest::file_digest(&root.join("Cargo.lock"))?,"eval_debug_assertions":cfg!(debug_assertions),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "sdkroot":std::env::var("SDKROOT").ok(),"rustflags":std::env::var("RUSTFLAGS").ok(),
        "cpu_parallelism":std::thread::available_parallelism().ok().map(|v|v.get()),"hardware":hardware,
        "rustc":Command::new("rustc").arg("--version").output().ok().filter(|o|o.status.success()).map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string())}),
    )
}
pub async fn run<B: Backend>(
    loaded: &LoadedSuite,
    mut backend: B,
    work: &Path,
    out: &Path,
    engine: Value,
    profile: &str,
) -> Result<Gate> {
    std::fs::create_dir(out.join("raw"))?;
    report::jsonl(&out.join("queries.jsonl"), &loaded.queries)?;
    let mut manifest=RunManifest{schema_version:1,suite:loaded.suite.clone(),input:loaded.input.clone(),adapter:backend.name().into(),adapter_version:ADAPTER_VERSION.into(),engine,
        query_snapshot_digest:manifest::file_digest(&out.join("queries.jsonl"))?,normalized_digest:String::new(),measurement_profile:profile.into(),
        source_verification:"public result source ranges checked against isolated admitted input; missing source fields marked unverified".into(),infrastructure_failure:None};
    report::json(&out.join("manifest.json"), &manifest)?;
    let mut normalized_log = std::fs::File::create(out.join("normalized.jsonl"))?;
    let mut resources = vec![sampler::sample("before_index", backend.pid())];
    let mut rows = Vec::new();
    let started = Instant::now();
    match backend.prepare(&loaded.input.files).await {
        Ok(value) => {
            report::json(
                &out.join("prepare.json"),
                &json!({"elapsed_us":started.elapsed().as_micros(),"result":value}),
            )?;
        }
        Err(e) => manifest.infrastructure_failure = Some(e.to_string()),
    }
    if manifest.infrastructure_failure.is_none() {
        match backend.readiness(&loaded.input.files).await {
            Ok(state) => {
                report::json(&out.join("readiness.json"), &state)?;
                if state.state != State::Ready {
                    manifest.infrastructure_failure = Some("input coverage not ready".into());
                }
            }
            Err(e) => manifest.infrastructure_failure = Some(e.to_string()),
        }
    }
    resources.push(sampler::sample("after_index", backend.pid()));
    if manifest.infrastructure_failure.is_none() {
        for _ in 0..loaded.suite.warmup {
            for q in &loaded.queries {
                if let Err(e) = backend.search(&q.input(loaded.suite.top_k)).await {
                    manifest.infrastructure_failure = Some(format!("warmup: {e}"));
                    break;
                }
            }
            if manifest.infrastructure_failure.is_some() {
                break;
            }
        }
    }
    if manifest.infrastructure_failure.is_none() {
        for repetition in 0..loaded.suite.repetitions {
            for idx in statistics::order(
                loaded.queries.len(),
                loaded.suite.seed.wrapping_add(repetition as u64),
            ) {
                let q = &loaded.queries[idx];
                let input = q.input(loaded.suite.top_k);
                let started = Instant::now();
                let result = backend.search(&input).await;
                let elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                let (raw, mut hits, mut status, mut diagnostic) = match result {
                    Ok(raw) => {
                        let normalized = if backend.name() == "oce-http" {
                            normalizer::oce(&raw)
                        } else {
                            normalizer::mcp(&raw)
                        };
                        match normalized {
                            Ok((hits, status)) => (raw, hits, status, None),
                            Err(e) => (
                                raw,
                                vec![],
                                ResultStatus::ProtocolError,
                                Some(e.to_string()),
                            ),
                        }
                    }
                    Err(e) => {
                        let status = match &e {
                            BenchError::Timeout(_) => ResultStatus::Timeout,
                            BenchError::Tool(_) => ResultStatus::ToolError,
                            _ => ResultStatus::ProtocolError,
                        };
                        (
                            json!({"error_kind":format!("{status:?}"),"message":e.to_string()}),
                            vec![],
                            status,
                            Some(e.to_string()),
                        )
                    }
                };
                for h in &mut hits {
                    normalizer::verify_source(h, work)?;
                    if input.path_prefix.as_ref().is_some_and(|p| {
                        let base = p.trim_end_matches('/');
                        !base.is_empty()
                            && h.path != base
                            && !h.path.starts_with(&format!("{base}/"))
                    }) {
                        h.evidence_valid = Some(false);
                        diagnostic = Some("returned path violates explicit prefix".into());
                    }
                }
                if status == ResultStatus::Success && hits.is_empty() {
                    status = ResultStatus::NoMatch;
                }
                let raw_path = format!("raw/{:06}.json", rows.len());
                report::json(&out.join(&raw_path), &raw)?;
                rows.push(Row {
                    case_id: q.id.clone(),
                    repetition,
                    status,
                    elapsed_us,
                    hits,
                    raw_digest: manifest::file_digest(&out.join(&raw_path))?,
                    raw_path,
                    diagnostic,
                });
                // Incremental artifact persistence survives a later process interruption.
                use std::io::Write;
                serde_json::to_writer(
                    &mut normalized_log,
                    rows.last().expect("row just inserted"),
                )?;
                normalized_log.write_all(b"\n")?;
                normalized_log.flush()?;
            }
            resources.push(sampler::sample(
                &format!("after_repetition_{repetition}"),
                backend.pid(),
            ));
        }
    }
    if let Err(e) = backend.close().await {
        manifest.infrastructure_failure = Some(format!("close: {e}"));
    }
    report::jsonl(&out.join("normalized.jsonl"), &rows)?;
    report::jsonl(&out.join("resources.jsonl"), &resources)?;
    report::jsonl(&out.join("latency.jsonl"),&rows.iter().map(|r|json!({"case_id":r.case_id,"repetition":r.repetition,"status":r.status,"elapsed_us":r.elapsed_us})).collect::<Vec<_>>())?;
    report::jsonl(
        &out.join("failures.jsonl"),
        &rows
            .iter()
            .filter(|r| {
                !matches!(r.status, ResultStatus::Success | ResultStatus::NoMatch)
                    || r.hits.iter().any(|h| h.evidence_valid == Some(false))
            })
            .collect::<Vec<_>>(),
    )?;
    manifest.normalized_digest = manifest::file_digest(&out.join("normalized.jsonl"))?;
    report::json(&out.join("manifest.json"), &manifest)?;
    report::finish(out, &manifest, &loaded.queries, &rows)
}
