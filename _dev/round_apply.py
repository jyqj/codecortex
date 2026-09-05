from pathlib import Path


def replace(path,old,new,count=1):
 p=Path(path);s=p.read_text()
 if s.count(old)!=count:raise RuntimeError(f'{path}: wanted {count}, got {s.count(old)} for {old!r}')
 p.write_text(s.replace(old,new))

replace('crates/cc-index/src/indexer_phases/analysis.rs',
 '.phase_analysis_compute(project, false, &[], &[], &[], None, None, build_explain)',
 '.phase_analysis_compute(AnalysisInputs { project_path: project, full: false, write_units: &[], route_nodes: &[], walk_manifest: None, scope_hints: None }, build_explain)')
replace('crates/cc-eval/src/quality.rs','report(&header, &[s.clone()])','report(&header, std::slice::from_ref(&s))')
replace('crates/cc-eval/src/quality.rs','    pub rustc: String,','    pub rustc: String,\n    #[serde(default)]\n    pub provenance: Value,')
replace('crates/cc-eval/src/quality.rs','            rustc: "test".into(),','            rustc: "test".into(),\n            provenance: json!({}),')
replace('crates/cc-eval/src/quality.rs','"implementation_commit":header.implementation_commit,"variant":header.variant,','"implementation_commit":header.implementation_commit,"variant":header.variant,\n        "provenance":header.provenance,"rustc":header.rustc,\n        "ndcg_policy":"maximum-new-label-gain-per-hit-v1",')
p=Path('crates/cc-eval/src/quality.rs')
p.write_text(p.read_text()+'''
/// Read saved wire observations after measurement. Shared by run and replay;
/// during measurement only one response is retained at a time.
pub fn read_raw(reader: impl std::io::BufRead) -> Result<(Header, Vec<Sample>), String> {
    let mut header = None;
    let mut samples = Vec::new();
    for line in reader.lines() {
        let row: Value = serde_json::from_str(&line.map_err(|e|e.to_string())?)
            .map_err(|e|e.to_string())?;
        match row["kind"].as_str() {
            Some("header") if header.is_none() && samples.is_empty() => {
                header = Some(serde_json::from_value(row["data"].clone()).map_err(|e|e.to_string())?);
            }
            Some("sample") if header.is_some() => {
                samples.push(serde_json::from_value(row["data"].clone()).map_err(|e|e.to_string())?);
            }
            Some("index" | "warmup") if header.is_some() => {},
            _ => return Err("unexpected or duplicate raw record".into()),
        }
    }
    Ok((header.ok_or("missing raw header")?, samples))
}
''')
path='crates/cc-eval/examples/quality_run.rs'
replace(path,'use std::io::{BufWriter, Write};','use std::io::{BufReader, BufWriter, Write};')
replace(path,'        rustc: command("rustc", &["--version"])?,','''        rustc: command("rustc", &["--version"])?,
        provenance: json!({
            "tracked_diff_git_blob": git_blob(command("git", &["diff", "HEAD", "--"]).unwrap_or_default().as_bytes())?,
            "worktree_status": command("git", &["status", "--porcelain"] )?,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "available_parallelism": std::thread::available_parallelism().map(|n|n.get()).ok(),
            "rustflags": std::env::var("RUSTFLAGS").unwrap_or_default(),
            "git_diff_note": "Baseline instrumentation may change eval-only files; inspect the recorded worktree status."
        }),''')
replace(path,'    let mut samples = Vec::new();\n','')
replace(path,'        let build = backend.build_index_report(true)?;','        let build = backend.build_index_report(true)?;\n        let index_elapsed_us = start.elapsed().as_micros();')
replace(path,'"elapsed_us":start.elapsed().as_micros(),"report":build','"elapsed_us":index_elapsed_us,"report":build')
replace(path,'                samples.push(sample);\n','',2)
replace(path,'    let report = quality::report(&header, &samples)?;','''    // No accumulated response payload competes with index memory during timing.
    drop(writer);
    let (saved_header, samples) = quality::read_raw(BufReader::new(File::open(output_dir.join("raw.jsonl"))?))?;
    let report = quality::report(&saved_header, &samples)?;''')
print('Fixed test callsite; streamed benchmark observations and recorded provenance')
