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
//! incremental rebuild latency (single file and a 5% batch), and p50/p95
//! tool latency for search / find_symbol / impact / graph_query / trace —
//! all through the real MCP dispatch path. The generator's ground-truth
//! facts double as scale-correctness assertions (needle ranks top-5, hub
//! impact surfaces known callers, the call chain traces, cycles close).
//!
//! Reports go to stderr; set `CODECORTEX_WRITE_BENCHMARK=1` to also persist
//! `docs/benchmarks/synthetic_<scale>_latest.md` (existing env convention).

use cc_eval::bench::{
    generate_scale_markdown, measure_tool_scenario, summarize_incremental_reports,
    CorrectnessCheck, ScaleBenchReport, ScenarioLatency,
};
use cc_eval::runner::CodeIndexBackend;
use cc_eval::synth::{generate, GroundTruth, SynthRepo, SynthSpec};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

const SCALE_BENCH_SEED: u64 = 0x00C0_FFEE;
const TOOL_ITERATIONS: usize = 7;
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
        Err(e) => checks.push(check("search_symbol_needle_top5", false, e)),
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
        Err(e) => checks.push(check("search_hybrid_needle_phrase", false, e)),
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
        Err(e) => checks.push(check("impact_hub_known_callers", false, e)),
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
        Err(e) => checks.push(check("graph_query_varlen_chain", false, e)),
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
        Err(e) => checks.push(check("graph_query_cycle_closes", false, e)),
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
        Err(e) => checks.push(check("trace_chain_path", false, e)),
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
        Err(e) => checks.push(check("relations_py_hub_callers", false, e)),
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
        Err(e) => checks.push(check("graph_query_rs_intra_edge", false, e)),
    }

    checks
}

// ── Tool latency scenarios ─────────────────────────────────────────

fn run_tool_scenarios(backend: &CodeIndexBackend, gt: &GroundTruth) -> Vec<ScenarioLatency> {
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
            measure_tool_scenario(backend, scenario, tool, &params, TOOL_ITERATIONS)
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
    let gen_start = Instant::now();
    let repo: SynthRepo = generate(root, &spec).expect("synthetic repo generation");
    let generate_ms = gen_start.elapsed().as_millis() as u64;
    assert_eq!(repo.files_written, target_files);

    // Cold full index through the real `index` tool.
    let backend =
        CodeIndexBackend::new_unindexed(root).expect("backend should initialize without building");
    let cold_start = Instant::now();
    let full = backend
        .build_index_report(true)
        .expect("cold full index build should succeed");
    let cold_index_ms = cold_start.elapsed().as_millis() as u64;
    let db_bytes = index_db_bytes(root);

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
    let tools = run_tool_scenarios(&backend, &repo.ground_truth);

    // Incremental: body-only edit of a single chain-middle file.
    let single_target = repo.ground_truth.chain[1].callee_file.clone();
    let single_reports =
        run_incremental_iterations(&backend, "single_file", INCREMENTAL_ITERATIONS, |iteration| {
            touch_file(root, &single_target, iteration);
        });
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
        batch_touched_files: batch.len(),
        tools,
        correctness,
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
