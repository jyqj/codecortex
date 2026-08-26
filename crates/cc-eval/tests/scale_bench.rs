//! Synthetic scale benchmark matrix: 1k / 10k / 50k files.
//!
//! All tests are `#[ignore]`d — run explicitly:
//!
//! ```sh
//! cargo test -p cc-eval --test scale_bench bench_synthetic_1k -- --ignored --nocapture
//! cargo test -p cc-eval --test scale_bench bench_synthetic_10k -- --ignored --nocapture
//! CODECORTEX_BENCH_50K=1 cargo test -p cc-eval --test scale_bench bench_synthetic_50k -- --ignored --nocapture
//! ```
//!
//! Each run measures, per scale: cold full index wall time + index DB size,
//! incremental rebuild latency (single file and a 5% batch), and cold/warm
//! tool latency (µs) for search / find_symbol / impact / graph_query /
//! trace — all through the real MCP dispatch path. Cold = a fresh MCP
//! session per iteration (new IndexDb identity → cold graph/page caches,
//! empty search LRUs; OS file cache retained); warm = repeated identical
//! calls in one session (cache-hit path). The generator's ground-truth
//! facts double as scale-correctness assertions (needle ranks top-5, hub
//! impact surfaces known callers, the call chain traces, cycles close).
//!
//! Reports go to stderr; set `CODECORTEX_WRITE_BENCHMARK=1` to also persist
//! `docs/benchmarks/synthetic_<scale>_latest.md` (existing env convention).

use cc_eval::bench::{
    generate_scale_markdown, measure_tool_scenario, sample_rss, summarize_incremental_reports,
    CorrectnessCheck, ScaleBenchReport, ScenarioLatency,
};
use cc_eval::runner::CodeIndexBackend;
use cc_eval::synth::{generate, GroundTruth, SynthRepo, SynthSpec};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

const SCALE_BENCH_SEED: u64 = 0x00C0_FFEE;
const TOOL_ITERATIONS: usize = 7;
/// Fewer cold iterations than warm: each one rebuilds a full MCP session,
/// which bounds total bench time while still giving a p50 over 3 samples.
const COLD_TOOL_ITERATIONS: usize = 3;
const INCREMENTAL_ITERATIONS: usize = 3;
/// Every 20th code file → 5% incremental batch.
const BATCH_STRIDE: usize = 20;

// ── Helpers ────────────────────────────────────────────────────────

fn report_usize(report: &Value, field: &str) -> usize {
    report
        .get(field)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("IndexReport field `{}` should be a u64", field)) as usize
}

/// Total on-disk size of the index DB (main file + WAL/SHM if present).
fn index_db_bytes(project: &Path) -> u64 {
    let dir = project.join(".codecortex");
    ["index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm"]
        .iter()
        .map(|name| {
            std::fs::metadata(dir.join(name))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum()
}

/// Append a body-only comment edit (per-language marker) to one file.
fn touch_file(root: &Path, rel_path: &str, marker: usize) {
    let path = root.join(rel_path);
    let mut source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} for mutation: {}", rel_path, e));
    let comment = if rel_path.ends_with(".py") { "#" } else { "//" };
    source.push_str(&format!("{} scale bench edit marker {}\n", comment, marker));
    std::fs::write(&path, source).unwrap_or_else(|e| panic!("write {}: {}", rel_path, e));
}

/// Run 1 warmup + `iterations` measured incremental builds, applying
/// `mutate(iteration)` before each build. Returns measured reports only.
fn run_incremental_iterations(
    backend: &CodeIndexBackend,
    label: &str,
    iterations: usize,
    mut mutate: impl FnMut(usize),
) -> Vec<Value> {
    let mut reports = Vec::with_capacity(iterations);
    for iteration in 0..=iterations {
        mutate(iteration);
        // Boundary marker so sub-phase debug logs can be attributed to a
        // specific scenario/iteration (iteration 0 is the warmup).
        eprintln!(
            "[scale-bench] incremental build start: scenario={} iteration={}",
            label, iteration
        );
        let report = backend
            .build_index_report(false)
            .expect("incremental index build should succeed");
        if iteration >= 1 {
            reports.push(report);
        }
    }
    reports
}

fn check(name: &str, passed: bool, detail: String) -> CorrectnessCheck {
    CorrectnessCheck {
        check: name.to_string(),
        passed,
        detail,
    }
}

/// Watcher-parity targeted incremental: 1 warmup + `iterations` measured
/// single-file builds through `BuildScope::Targeted` (the production watcher
/// scan), driven directly on a fresh `Indexer` over the existing index.
/// `elapsed_ms` in each returned report is overwritten with the harness wall
/// time across prepare+commit so it is comparable to the MCP-path scenarios.
fn run_targeted_iterations(root: &Path, rel_path: &str, iterations: usize) -> Vec<Value> {
    let db_path = root.join(".codecortex").join("index.sqlite3");
    let db = Arc::new(
        cc_db::index_db::IndexDb::open(&db_path)
            .expect("open existing index for targeted bench")
            .0,
    );
    let config = cc_model::config::IndexingConfig::default();
    let indexer = cc_index::Indexer::new(db, root, &config);

    let mut reports = Vec::with_capacity(iterations);
    for iteration in 0..=iterations {
        touch_file(root, rel_path, 1000 + iteration);
        eprintln!(
            "[scale-bench] targeted incremental build start: iteration={}",
            iteration
        );
        let scope = cc_index::BuildScope::Targeted(cc_index::TargetedChanges {
            changed: vec![rel_path.to_string()],
            removed: Vec::new(),
        });
        let start = Instant::now();
        let prepared = indexer
            .prepare_build(root, false, None, scope)
            .expect("targeted prepare should succeed");
        let report = indexer
            .commit_build(root, false, None, prepared)
            .expect("targeted commit should succeed");
        let wall_ms = start.elapsed().as_millis() as u64;
        if iteration >= 1 {
            let mut value = serde_json::to_value(&report).expect("serialize IndexReport");
            value["elapsed_ms"] = json!(wall_ms);
            reports.push(value);
        }
    }
    reports
}

/// `graph_query` result rows: collect every string cell for membership tests.
fn graph_result_values(output: &Value) -> Vec<String> {
    output
        .get("results")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_object())
                .flat_map(|row| row.values())
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Rank (1-based) of `name` in a symbol-search result array.
fn symbol_rank(output: &Value, name: &str) -> Option<usize> {
    output
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .position(|row| row.get("name").and_then(|v| v.as_str()) == Some(name))
        })
        .map(|idx| idx + 1)
}

// ── Ground-truth correctness checks ────────────────────────────────

fn run_correctness_checks(backend: &CodeIndexBackend, gt: &GroundTruth) -> Vec<CorrectnessCheck> {
    let mut checks = Vec::new();
    let call = |tool: &str, params: Value| backend.call_tool(tool, &params);

    // 1. Symbol search must rank the unique needle top-5.
    match call(
        "search",
        json!({ "query": gt.needle_symbol, "mode": "symbol", "exact": true, "top_k": 5 }),
    ) {
        Ok(output) => {
            let rank = symbol_rank(&output, &gt.needle_symbol);
            checks.push(check(
                "search_symbol_needle_top5",
                rank.is_some_and(|r| r <= 5),
                format!("rank = {:?}", rank),
            ));
        }
        Err(e) => checks.push(check("search_symbol_needle_top5", false, e.to_string())),
    }

    // 2. Hybrid search on the needle's distinctive body phrase.
    match call(
        "search",
        json!({ "query": gt.needle_phrase, "mode": "hybrid", "top_k": 5 }),
    ) {
        Ok(output) => {
            let serialized = serde_json::to_string(&output).unwrap_or_default();
            let hit =
                serialized.contains(&gt.needle_symbol) || serialized.contains(&gt.needle_file);
            checks.push(check(
                "search_hybrid_needle_phrase",
                hit,
                format!("phrase '{}' surfaced needle: {}", gt.needle_phrase, hit),
            ));
        }
        Err(e) => checks.push(check("search_hybrid_needle_phrase", false, e.to_string())),
    }

    // 3. Impact of the hub file must include known hub callers.
    match call(
        "impact",
        json!({ "scope": "changes", "files": [gt.hub_file], "limit": 100 }),
    ) {
        Ok(output) => {
            let serialized = serde_json::to_string(&output).unwrap_or_default();
            let probed = gt.hub_callers.len().min(10);
            let found = gt.hub_callers[..probed]
                .iter()
                .filter(|fact| serialized.contains(&fact.caller))
                .count();
            checks.push(check(
                "impact_hub_known_callers",
                found >= 3usize.min(probed),
                format!("{}/{} probed hub callers in blast radius", found, probed),
            ));
        }
        Err(e) => checks.push(check("impact_hub_known_callers", false, e.to_string())),
    }

    // 4. Var-length CALLS traversal reaches 2 hops down the chain.
    let chain_from = &gt.chain[0].caller;
    let two_hops = &gt.chain[1].callee;
    match call(
        "graph_query",
        json!({ "query": format!(
            "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = '{}' RETURN DISTINCT b.name LIMIT 25",
            chain_from
        ) }),
    ) {
        Ok(output) => {
            let values = graph_result_values(&output);
            checks.push(check(
                "graph_query_varlen_chain",
                values.iter().any(|v| v == two_hops),
                format!(
                    "{} rows; expect {} reachable from {}",
                    values.len(),
                    two_hops,
                    chain_from
                ),
            ));
        }
        Err(e) => checks.push(check("graph_query_varlen_chain", false, e.to_string())),
    }

    // 5. The 3-cycle closes: cyc_a reaches itself within 3 CALLS hops.
    let cyc_a = &gt.cycle_symbols[0];
    match call(
        "graph_query",
        json!({ "query": format!(
            "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = '{}' RETURN DISTINCT b.name LIMIT 10",
            cyc_a
        ) }),
    ) {
        Ok(output) => {
            let values = graph_result_values(&output);
            checks.push(check(
                "graph_query_cycle_closes",
                values.iter().any(|v| v == cyc_a),
                format!("cycle legs reachable: {:?}", values),
            ));
        }
        Err(e) => checks.push(check("graph_query_cycle_closes", false, e.to_string())),
    }

    // 6. Trace finds the 4-hop cross-file chain path.
    let chain_to = &gt.chain[3].callee;
    let mid_hop = &gt.chain[1].callee;
    match call(
        "trace",
        json!({ "from": chain_from, "to": chain_to, "max_depth": 6 }),
    ) {
        Ok(output) => {
            let has_paths = output
                .get("paths")
                .and_then(|v| v.as_array())
                .is_some_and(|paths| !paths.is_empty());
            let serialized = serde_json::to_string(&output).unwrap_or_default();
            checks.push(check(
                "trace_chain_path",
                has_paths && serialized.contains(mid_hop.as_str()),
                format!("paths found: {}, via {}", has_paths, mid_hop),
            ));
        }
        Err(e) => checks.push(check("trace_chain_path", false, e.to_string())),
    }

    // 7. Python cross-file resolution: hub callers visible via relations.
    let py_caller = &gt.py_hub_callers[0].caller;
    match call(
        "relations",
        json!({ "symbol": gt.py_hub_symbol, "kind": "callers", "limit": 50 }),
    ) {
        Ok(output) => {
            let serialized = serde_json::to_string(&output).unwrap_or_default();
            checks.push(check(
                "relations_py_hub_callers",
                serialized.contains(py_caller.as_str()),
                format!("expect caller {}", py_caller),
            ));
        }
        Err(e) => checks.push(check("relations_py_hub_callers", false, e.to_string())),
    }

    // 8. Rust same-file fan-out edge exists in the graph.
    match call(
        "graph_query",
        json!({ "query": format!(
            "MATCH (a:Function)-[:CALLS]->(b:Function) WHERE a.name = '{}' RETURN b.name LIMIT 10",
            gt.rs_intra.caller
        ) }),
    ) {
        Ok(output) => {
            let values = graph_result_values(&output);
            checks.push(check(
                "graph_query_rs_intra_edge",
                values.iter().any(|v| v == &gt.rs_intra.callee),
                format!("expect {} -> {}", gt.rs_intra.caller, gt.rs_intra.callee),
            ));
        }
        Err(e) => checks.push(check("graph_query_rs_intra_edge", false, e.to_string())),
    }

    checks
}

// ── Tool latency scenarios ─────────────────────────────────────────

fn run_tool_scenarios(
    backend: &CodeIndexBackend,
    root: &Path,
    gt: &GroundTruth,
) -> Vec<ScenarioLatency> {
    let chain_from = &gt.chain[0].caller;
    let chain_to = &gt.chain[3].callee;
    let fuzzy_prefix = &gt.needle_symbol[..10]; // "needle_fn_"
    let scenarios: Vec<(&str, &str, Value)> = vec![
        (
            "search_hybrid_needle_phrase",
            "search",
            json!({ "query": gt.needle_phrase, "mode": "hybrid", "top_k": 10 }),
        ),
        (
            "search_hybrid_mixed_terms",
            "search",
            json!({ "query": "dispatch payload registry bridge", "mode": "hybrid", "top_k": 10 }),
        ),
        (
            "find_symbol_exact_needle",
            "search",
            json!({ "query": gt.needle_symbol, "mode": "symbol", "exact": true, "top_k": 5 }),
        ),
        (
            "find_symbol_fuzzy_prefix",
            "search",
            json!({ "query": fuzzy_prefix, "mode": "symbol", "top_k": 10 }),
        ),
        (
            "impact_changes_hub_file",
            "impact",
            json!({ "scope": "changes", "files": [gt.hub_file], "limit": 50 }),
        ),
        (
            "graph_query_calls_varlen",
            "graph_query",
            json!({ "query": format!(
                "MATCH (a:Function)-[:CALLS*1..3]->(b:Function) WHERE a.name = '{}' RETURN DISTINCT b.name LIMIT 25",
                chain_from
            ) }),
        ),
        (
            "trace_chain_4_hops",
            "trace",
            json!({ "from": chain_from, "to": chain_to, "max_depth": 6 }),
        ),
    ];

    scenarios
        .into_iter()
        .map(|(scenario, tool, params)| {
            measure_tool_scenario(
                backend,
                root,
                scenario,
                tool,
                &params,
                COLD_TOOL_ITERATIONS,
                TOOL_ITERATIONS,
            )
            .unwrap_or_else(|e| panic!("tool scenario failed: {}", e))
        })
        .collect()
}

// ── Matrix driver ──────────────────────────────────────────────────

fn run_scale_bench(scale_label: &str, target_files: usize) {
    // Sub-phase attribution: with RUST_LOG set (e.g. `cc_index=debug,cc_db=debug`)
    // the `time_step` debug events from cc-index / cc-db reach stderr; without
    // it the bench stays silent as before.
    if std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }
    let spec = SynthSpec {
        target_files,
        seed: SCALE_BENCH_SEED,
    };
    let tmp = tempfile::tempdir().expect("create tempdir for scale benchmark");
    let root = tmp.path();

    // Generate the synthetic repo (deterministic from seed + size).
    let mut rss_samples = Vec::new();
    let gen_start = Instant::now();
    let repo: SynthRepo = generate(root, &spec).expect("synthetic repo generation");
    let generate_ms = gen_start.elapsed().as_millis() as u64;
    assert_eq!(repo.files_written, target_files);
    rss_samples.push(sample_rss("after repo generation"));

    // Cold full index through the real `index` tool.
    let backend =
        CodeIndexBackend::new_unindexed(root).expect("backend should initialize without building");
    let cold_start = Instant::now();
    let full = backend
        .build_index_report(true)
        .expect("cold full index build should succeed");
    let cold_index_ms = cold_start.elapsed().as_millis() as u64;
    let db_bytes = index_db_bytes(root);
    rss_samples.push(sample_rss("after cold full index"));

    assert_eq!(
        report_usize(&full, "files_added"),
        repo.files_written,
        "cold build should index every generated file"
    );
    let parse_errors = full
        .get("parse_errors")
        .and_then(|v| v.as_array())
        .expect("IndexReport parse_errors present");
    assert!(
        parse_errors.is_empty(),
        "synthetic sources must parse cleanly: {:?}",
        parse_errors
    );
    let symbols_total = report_usize(&full, "symbols_total");
    assert!(
        symbols_total >= repo.functions_planned,
        "index should at least cover the {} planned functions, got {}",
        repo.functions_planned,
        symbols_total
    );

    // Ground-truth correctness + tool latency on the pristine index.
    let correctness = run_correctness_checks(&backend, &repo.ground_truth);
    let tools = run_tool_scenarios(&backend, root, &repo.ground_truth);
    rss_samples.push(sample_rss("after tool scenarios"));

    // Incremental: body-only edit of a single chain-middle file.
    let single_target = repo.ground_truth.chain[1].callee_file.clone();
    let single_reports = run_incremental_iterations(
        &backend,
        "single_file",
        INCREMENTAL_ITERATIONS,
        |iteration| {
            touch_file(root, &single_target, iteration);
        },
    );
    for report in &single_reports {
        assert_eq!(report_usize(report, "files_parsed"), 1);
        assert_eq!(report_usize(report, "files_updated"), 1);
    }
    let incremental_single =
        summarize_incremental_reports("single_file", repo.files_written, &single_reports);

    // Incremental: body-only edits across 5% of code files per iteration.
    let batch: Vec<String> = repo
        .code_file_paths
        .iter()
        .step_by(BATCH_STRIDE)
        .cloned()
        .collect();
    let batch_reports = run_incremental_iterations(
        &backend,
        "five_percent_batch",
        INCREMENTAL_ITERATIONS,
        |iteration| {
            for rel_path in &batch {
                touch_file(root, rel_path, iteration);
            }
        },
    );
    for report in &batch_reports {
        assert_eq!(report_usize(report, "files_parsed"), batch.len());
        assert_eq!(report_usize(report, "files_updated"), batch.len());
    }
    let incremental_batch =
        summarize_incremental_reports("five_percent_batch", repo.files_written, &batch_reports);
    rss_samples.push(sample_rss("after incremental scenarios"));

    // Watcher-parity targeted scan on the same single file. The MCP session
    // is dropped first so the direct Indexer is the only writer.
    drop(backend);
    let targeted_reports = run_targeted_iterations(root, &single_target, INCREMENTAL_ITERATIONS);
    for report in &targeted_reports {
        assert_eq!(report_usize(report, "files_parsed"), 1);
        assert_eq!(report_usize(report, "files_updated"), 1);
    }
    let incremental_targeted = summarize_incremental_reports(
        "targeted_single_file (watcher parity)",
        repo.files_written,
        &targeted_reports,
    );
    rss_samples.push(sample_rss("after targeted scenario"));

    let report = ScaleBenchReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        scale_label: scale_label.to_string(),
        seed: spec.seed,
        file_count: repo.files_written,
        symbols_total,
        generate_ms,
        cold_index_ms,
        db_bytes,
        incremental_single,
        incremental_batch,
        incremental_targeted: Some(incremental_targeted),
        batch_touched_files: batch.len(),
        tools,
        correctness,
        rss_samples,
    };
    let md = generate_scale_markdown(&report);
    eprintln!("{}", md);

    // Persist only behind the existing env-flag convention.
    if std::env::var("CODECORTEX_WRITE_BENCHMARK").is_ok() {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        if let Some(workspace_root) = crate_dir.parent().and_then(|p| p.parent()) {
            let bench_dir = workspace_root.join("docs").join("benchmarks");
            std::fs::create_dir_all(&bench_dir).expect("create docs/benchmarks");
            let path = bench_dir.join(format!("synthetic_{}_latest.md", scale_label));
            std::fs::write(&path, &md).expect("write synthetic scale benchmark report");
            eprintln!("Scale benchmark report written to {}", path.display());
        }
    }

    let failed: Vec<&CorrectnessCheck> = report.correctness.iter().filter(|c| !c.passed).collect();
    assert!(
        failed.is_empty(),
        "ground-truth correctness checks failed at {} scale: {:?}",
        scale_label,
        failed
            .iter()
            .map(|c| format!("{}: {}", c.check, c.detail))
            .collect::<Vec<_>>()
    );
}

// ── Cold-build scaling profiler ─────────────────────────────────────

/// Phase fields of `IndexReport.phase_timing`, in pipeline order.
const COLD_PROFILE_PHASES: &[&str] = &[
    "scan_diff_ms",
    "parse_ms",
    "resolve_ms",
    "write_ms",
    "postprocess_ms",
    "analysis_ms",
];

/// Lean cold-build phase-timing profiler across a scale sweep, for localizing
/// the superlinear cold-index cost (issue: 5× files → 24× wall at 50k). Unlike
/// `run_scale_bench` this skips incremental/tool/correctness work and only
/// drives `index(full=true)` at each scale, capturing the report's per-phase
/// `phase_timing`. Set `RUST_LOG=cc_index=debug` to additionally surface the
/// `time_step` sub-phase events (synthesis_round / louvain / test_edges_apply /
/// full_rebuild_direct_writer …) so the dominant phase can be split further.
///
/// Run (default sweep 1k→16k):
/// ```sh
/// cargo test -p cc-eval --test scale_bench profile_cold_build_scaling -- --ignored --nocapture
/// ```
/// Override scales (comma-separated file counts):
/// ```sh
/// CODECORTEX_PROFILE_SCALES=2000,8000,32000 RUST_LOG=cc_index=debug \
///   cargo test -p cc-eval --test scale_bench profile_cold_build_scaling -- --ignored --nocapture
/// ```
#[test]
#[ignore = "cold-build scaling profiler; run explicitly"]
fn profile_cold_build_scaling() {
    if std::env::var("RUST_LOG").is_ok() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }

    let scales: Vec<usize> = std::env::var("CODECORTEX_PROFILE_SCALES")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|tok| tok.trim().parse::<usize>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec![1_000, 2_000, 4_000, 8_000, 16_000]);

    // (files, symbols, wall_ms, [phase_ms; 6]) per scale.
    let mut samples: Vec<(usize, usize, u64, [u64; 6])> = Vec::new();

    for &target in &scales {
        let spec = SynthSpec {
            target_files: target,
            seed: SCALE_BENCH_SEED,
        };
        let tmp = tempfile::tempdir().expect("create tempdir for profile scale");
        let root = tmp.path();
        let repo: SynthRepo = generate(root, &spec).expect("synthetic repo generation");
        let backend = CodeIndexBackend::new_unindexed(root)
            .expect("backend should initialize without building");

        eprintln!("[profile] cold build start: files={}", repo.files_written);
        let cold_start = Instant::now();
        let full = backend
            .build_index_report(true)
            .expect("cold full index build should succeed");
        let wall = cold_start.elapsed().as_millis() as u64;

        let mut phases = [0u64; 6];
        for (slot, field) in COLD_PROFILE_PHASES.iter().enumerate() {
            phases[slot] = full
                .get("phase_timing")
                .and_then(|timing| timing.get(*field))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }
        let symbols = report_usize(&full, "symbols_total");
        eprintln!(
            "[profile] files={} symbols={} wall={}ms phases(scan_diff/parse/resolve/write/postprocess/analysis)={:?}",
            repo.files_written, symbols, wall, phases
        );
        samples.push((repo.files_written, symbols, wall, phases));
        drop(backend);
    }

    // Phase × scale table.
    eprintln!("\n## Cold Build Phase Scaling\n");
    eprint!("| files | symbols | wall");
    for field in COLD_PROFILE_PHASES {
        eprint!(" | {}", field.trim_end_matches("_ms"));
    }
    eprintln!(" |");
    eprint!("|------|------|------");
    for _ in COLD_PROFILE_PHASES {
        eprint!("|------");
    }
    eprintln!("|");
    for (files, symbols, wall, phases) in &samples {
        eprint!("| {} | {} | {}ms", files, symbols, wall);
        for ph in phases {
            eprint!(" | {}ms", ph);
        }
        eprintln!(" |");
    }

    // Per-phase superlinearity factor between the smallest and largest scale:
    // (t_max / t_min) / (files_max / files_min). ~1 ⇒ linear; >1 ⇒ superlinear.
    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let (files_lo, _, wall_lo, ph_lo) = first;
        let (files_hi, _, wall_hi, ph_hi) = last;
        let file_ratio = *files_hi as f64 / (*files_lo).max(1) as f64;
        eprintln!(
            "\n## Superlinearity Factor (files {}→{}, ratio {:.1}×)\n",
            files_lo, files_hi, file_ratio
        );
        eprintln!("| phase | t_lo | t_hi | time_ratio | superlinearity |");
        eprintln!("|-------|------|------|------------|----------------|");
        let factor = |lo: u64, hi: u64| -> (f64, f64) {
            let time_ratio = hi as f64 / (lo.max(1)) as f64;
            (time_ratio, time_ratio / file_ratio)
        };
        let (wr, wf) = factor(*wall_lo, *wall_hi);
        eprintln!(
            "| **wall** | {}ms | {}ms | {:.1}× | {:.2} |",
            wall_lo, wall_hi, wr, wf
        );
        for (slot, field) in COLD_PROFILE_PHASES.iter().enumerate() {
            let (tr, sf) = factor(ph_lo[slot], ph_hi[slot]);
            eprintln!(
                "| {} | {}ms | {}ms | {:.1}× | {:.2} |",
                field.trim_end_matches("_ms"),
                ph_lo[slot],
                ph_hi[slot],
                tr,
                sf
            );
        }
        eprintln!(
            "\nSuperlinearity > 1.3 flags a phase whose per-file cost grows with corpus size."
        );
    }
}

// ── Matrix entry points ────────────────────────────────────────────

#[test]
#[ignore = "synthetic 1k-file scale benchmark; run explicitly"]
fn bench_synthetic_1k() {
    run_scale_bench("1k", 1_000);
}

#[test]
#[ignore = "synthetic 10k-file scale benchmark; run explicitly"]
fn bench_synthetic_10k() {
    run_scale_bench("10k", 10_000);
}

#[test]
#[ignore = "synthetic 50k-file scale benchmark; additionally gated behind CODECORTEX_BENCH_50K=1"]
fn bench_synthetic_50k() {
    if std::env::var("CODECORTEX_BENCH_50K").as_deref() != Ok("1") {
        eprintln!("Skipping bench_synthetic_50k: set CODECORTEX_BENCH_50K=1 to run");
        return;
    }
    run_scale_bench("50k", 50_000);
}
