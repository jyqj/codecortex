//! Benchmark runner: measures per-tool latency and output size across eval cases.
//!
//! Tool latency is measured in microseconds and split into two columns:
//!
//! - **cold**: each iteration times the FIRST call of a brand-new MCP session
//!   attached to the existing index ([`CodeIndexBackend::open_existing`]). A
//!   new session means new IndexDb connections → a new `db_identity`, so the
//!   generation-keyed graph adjacency caches miss, the SQLite per-connection
//!   page caches start cold, and the per-session SearchEngine LRUs are empty.
//!   The OS file cache is intentionally retained.
//! - **warm**: repeated identical calls within one shared session (1 warmup
//!   discarded), exercising the cache-hit path.
//!
//! Corpus-case results are aggregated per-tool; named scale scenarios keep
//! full p50/p95/max for both columns.

use crate::runner::CodeIndexBackend;
use crate::types::EvalCase;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

// ── Benchmark report types ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkReport {
    pub generated_at: String,
    pub dataset_name: String,
    pub fixture_files: usize,
    pub total_cases: usize,
    pub per_tool: Vec<ToolBenchmark>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolBenchmark {
    pub tool: String,
    pub cases: usize,
    pub cold_p50_us: u64,
    pub cold_max_us: u64,
    pub warm_p50_us: u64,
    pub warm_p95_us: u64,
    pub warm_max_us: u64,
    pub warm_samples: usize,
    pub avg_output_bytes: usize,
}

// ── Single-case measurement ────────────────────────────────────────

#[derive(Debug, Clone)]
struct CaseMeasurement {
    tool: String,
    cold_us: u64,
    warm_us: Vec<u64>,
    output_bytes: usize,
}

/// Measure one eval case cold and warm.
///
/// Cold: one timed call on a brand-new MCP session attached to the existing
/// index (see module docs for which cache layers this invalidates). Warm:
/// 1 warmup + 2 measured calls on the shared session, retaining both samples.
fn measure_case(
    backend: &CodeIndexBackend,
    project_path: &Path,
    case: &EvalCase,
) -> CaseMeasurement {
    // Cold: first call of a fresh session. Errors are timed like the warm
    // path (a failing case fails its assertions in the eval run, not here).
    let cold_backend = CodeIndexBackend::open_existing(project_path).unwrap_or_else(|e| {
        panic!(
            "cold session for case '{}' failed to open: {}",
            case.name, e
        )
    });
    let start = Instant::now();
    let _ = cold_backend.call_tool(&case.tool, &case.params);
    let cold_us = start.elapsed().as_micros() as u64;
    drop(cold_backend);

    // Warm: shared session, 1 warmup + 2 measured.
    let mut durations = Vec::with_capacity(2);
    let mut last_output_bytes: usize = 0;

    for iteration in 0..3 {
        let start = Instant::now();
        let result = backend.call_tool(&case.tool, &case.params);
        let elapsed_us = start.elapsed().as_micros() as u64;

        let output_bytes = match &result {
            Ok(output) => serde_json::to_string(output).unwrap_or_default().len(),
            Err(_) => 0,
        };

        if iteration >= 1 {
            // Measured runs (skip iteration 0 = warmup)
            durations.push(elapsed_us);
            last_output_bytes = output_bytes;
        }
    }

    CaseMeasurement {
        tool: case.tool.clone(),
        cold_us,
        warm_us: durations,
        output_bytes: last_output_bytes,
    }
}

// ── Percentile helper ──────────────────────────────────────────────

fn percentile(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64) * pct).ceil() as usize;
    let idx = idx.min(sorted.len()).saturating_sub(1);
    sorted[idx]
}

// ── Run benchmark ──────────────────────────────────────────────────

/// Run benchmarks for all cases and aggregate results per tool.
///
/// `project_path` is the indexed project root, used to open fresh MCP
/// sessions for the cold column. `fixture_files` is the number of source
/// files in the fixture project (for display in the report header).
pub fn run_benchmark(
    backend: &CodeIndexBackend,
    project_path: &Path,
    cases: &[EvalCase],
    fixture_files: usize,
) -> BenchmarkReport {
    run_benchmark_named(backend, project_path, cases, fixture_files, "fixture")
}

/// Run benchmarks with a dataset label for the generated report.
pub fn run_benchmark_named(
    backend: &CodeIndexBackend,
    project_path: &Path,
    cases: &[EvalCase],
    fixture_files: usize,
    dataset_name: &str,
) -> BenchmarkReport {
    // Measure each case
    let measurements: Vec<CaseMeasurement> = cases
        .iter()
        .map(|c| measure_case(backend, project_path, c))
        .collect();

    // Group by tool
    let mut by_tool: HashMap<String, Vec<&CaseMeasurement>> = HashMap::new();
    for m in &measurements {
        by_tool.entry(m.tool.clone()).or_default().push(m);
    }

    // Aggregate per-tool
    let mut per_tool: Vec<ToolBenchmark> = by_tool
        .into_iter()
        .map(|(tool, group)| {
            let cases_count = group.len();

            let mut cold: Vec<u64> = group.iter().map(|m| m.cold_us).collect();
            cold.sort_unstable();
            let mut warm: Vec<u64> = group
                .iter()
                .flat_map(|m| m.warm_us.iter().copied())
                .collect();
            warm.sort_unstable();

            let total_output: usize = group.iter().map(|m| m.output_bytes).sum();
            let avg_output = total_output.checked_div(cases_count).unwrap_or(0);

            ToolBenchmark {
                tool,
                cases: cases_count,
                cold_p50_us: percentile(&cold, 0.50),
                cold_max_us: cold.last().copied().unwrap_or(0),
                warm_p50_us: percentile(&warm, 0.50),
                warm_p95_us: percentile(&warm, 0.95),
                warm_max_us: warm.last().copied().unwrap_or(0),
                warm_samples: warm.len(),
                avg_output_bytes: avg_output,
            }
        })
        .collect();

    per_tool.sort_by(|a, b| a.tool.cmp(&b.tool));

    BenchmarkReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        dataset_name: dataset_name.to_string(),
        fixture_files,
        total_cases: cases.len(),
        per_tool,
    }
}

// ── Incremental indexing latency ───────────────────────────────────

/// p50/p95/max over a series of per-iteration durations.
#[derive(Debug, Clone, Serialize)]
pub struct LatencyStats {
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

impl LatencyStats {
    pub fn from_durations(durations: &[u64]) -> Self {
        let mut sorted = durations.to_vec();
        sorted.sort_unstable();
        Self {
            p50_ms: percentile(&sorted, 0.50),
            p95_ms: percentile(&sorted, 0.95),
            max_ms: sorted.last().copied().unwrap_or(0),
        }
    }
}

/// Latency stats for one `IndexReport.phase_timing` entry.
#[derive(Debug, Clone, Serialize)]
pub struct PhaseLatency {
    pub phase: String,
    pub stats: LatencyStats,
}

/// Aggregated latency for one incremental indexing scenario.
#[derive(Debug, Clone, Serialize)]
pub struct IncrementalBenchReport {
    pub scenario: String,
    pub fixture_files: usize,
    pub iterations: usize,
    /// Total build latency from `IndexReport.elapsed_ms`.
    pub elapsed: LatencyStats,
    /// Per-phase breakdown, sorted by p50 descending (dominant phase first).
    pub phases: Vec<PhaseLatency>,
}

/// `IndexReport.phase_timing` field names, mirroring `cc_index::indexer::PhaseTiming`.
const PHASE_TIMING_FIELDS: &[&str] = &[
    "scan_diff_ms",
    "parse_ms",
    "resolve_ms",
    "write_ms",
    "postprocess_ms",
    "analysis_ms",
];

/// Aggregate per-iteration serialized `IndexReport` values (`elapsed_ms` +
/// `phase_timing`) into a latency summary for one incremental scenario.
pub fn summarize_incremental_reports(
    scenario: &str,
    fixture_files: usize,
    reports: &[serde_json::Value],
) -> IncrementalBenchReport {
    let elapsed: Vec<u64> = reports
        .iter()
        .map(|report| {
            report
                .get("elapsed_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        })
        .collect();

    let mut phases: Vec<PhaseLatency> = PHASE_TIMING_FIELDS
        .iter()
        .map(|field| {
            let durations: Vec<u64> = reports
                .iter()
                .map(|report| {
                    report
                        .get("phase_timing")
                        .and_then(|timing| timing.get(*field))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                })
                .collect();
            PhaseLatency {
                phase: field.trim_end_matches("_ms").to_string(),
                stats: LatencyStats::from_durations(&durations),
            }
        })
        .collect();
    phases.sort_by(|a, b| {
        b.stats
            .p50_ms
            .cmp(&a.stats.p50_ms)
            .then(a.phase.cmp(&b.phase))
    });

    IncrementalBenchReport {
        scenario: scenario.to_string(),
        fixture_files,
        iterations: reports.len(),
        elapsed: LatencyStats::from_durations(&elapsed),
        phases,
    }
}

/// Generate a Markdown section for one incremental indexing latency scenario.
pub fn generate_incremental_markdown(report: &IncrementalBenchReport) -> String {
    let mut md = String::new();

    md.push_str(&format!("## Incremental Latency: {}\n\n", report.scenario));
    md.push_str(&format!(
        "Files: {} | Measured iterations: {}\n\n",
        report.fixture_files, report.iterations
    ));
    md.push_str("| Phase | p50 | p95 | Max |\n");
    md.push_str("|-------|-----|-----|-----|\n");
    md.push_str(&format!(
        "| total elapsed | {}ms | {}ms | {}ms |\n",
        report.elapsed.p50_ms, report.elapsed.p95_ms, report.elapsed.max_ms
    ));
    for phase in &report.phases {
        md.push_str(&format!(
            "| {} | {}ms | {}ms | {}ms |\n",
            phase.phase, phase.stats.p50_ms, phase.stats.p95_ms, phase.stats.max_ms
        ));
    }
    md.push('\n');

    md
}

// ── Synthetic scale benchmark (1k/10k/50k matrix) ──────────────────

/// p50/p95/max over per-iteration durations in microseconds (tool latency).
#[derive(Debug, Clone, Serialize)]
pub struct LatencyStatsUs {
    pub p50_us: u64,
    pub p95_us: u64,
    pub max_us: u64,
}

impl LatencyStatsUs {
    pub fn from_durations(durations: &[u64]) -> Self {
        let mut sorted = durations.to_vec();
        sorted.sort_unstable();
        Self {
            p50_us: percentile(&sorted, 0.50),
            p95_us: percentile(&sorted, 0.95),
            max_us: sorted.last().copied().unwrap_or(0),
        }
    }
}

/// Format a microsecond duration human-readably: `450µs`, `1.23ms`, `2.05s`.
pub fn format_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.2}ms", us as f64 / 1_000.0)
    } else {
        format!("{}µs", us)
    }
}

/// Latency for one named tool scenario through the real MCP dispatch path,
/// split into a cold column (fresh MCP session per iteration) and a warm
/// column (repeated identical calls in one session, 1 warmup discarded).
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioLatency {
    pub scenario: String,
    pub tool: String,
    pub cold_iterations: usize,
    pub warm_iterations: usize,
    pub cold: LatencyStatsUs,
    pub warm: LatencyStatsUs,
    pub avg_output_bytes: usize,
}

/// Measure one tool scenario cold and warm.
///
/// Cold: each of `cold_iterations` opens a NEW MCP session against the
/// existing index at `project_path` and times the first call (see module
/// docs for which cache layers this invalidates). Warm: 1 warmup +
/// `warm_iterations` repeated calls on the shared `backend` session. Any
/// call failure aborts the scenario with context.
pub fn measure_tool_scenario(
    backend: &CodeIndexBackend,
    project_path: &Path,
    scenario: &str,
    tool: &str,
    params: &serde_json::Value,
    cold_iterations: usize,
    warm_iterations: usize,
) -> Result<ScenarioLatency, String> {
    let mut cold_durations = Vec::with_capacity(cold_iterations);
    for _ in 0..cold_iterations {
        let cold_backend = CodeIndexBackend::open_existing(project_path)
            .map_err(|e| format!("scenario '{}' ({}) cold session: {}", scenario, tool, e))?;
        let start = Instant::now();
        cold_backend
            .call_tool(tool, params)
            .map_err(|e| format!("scenario '{}' ({}) cold call failed: {}", scenario, tool, e))?;
        cold_durations.push(start.elapsed().as_micros() as u64);
    }

    let mut warm_durations = Vec::with_capacity(warm_iterations);
    let mut total_output = 0usize;
    for iteration in 0..=warm_iterations {
        let start = Instant::now();
        let output = backend
            .call_tool(tool, params)
            .map_err(|e| format!("scenario '{}' ({}) failed: {}", scenario, tool, e))?;
        let elapsed_us = start.elapsed().as_micros() as u64;
        if iteration >= 1 {
            warm_durations.push(elapsed_us);
            total_output += serde_json::to_string(&output).unwrap_or_default().len();
        }
    }
    Ok(ScenarioLatency {
        scenario: scenario.to_string(),
        tool: tool.to_string(),
        cold_iterations,
        warm_iterations,
        cold: LatencyStatsUs::from_durations(&cold_durations),
        warm: LatencyStatsUs::from_durations(&warm_durations),
        avg_output_bytes: total_output / warm_iterations.max(1),
    })
}

/// One ground-truth correctness check result at scale.
#[derive(Debug, Clone, Serialize)]
pub struct CorrectnessCheck {
    pub check: String,
    pub passed: bool,
    pub detail: String,
}

/// A labeled process-RSS reading taken at a bench milestone.
#[derive(Debug, Clone, Serialize)]
pub struct RssSample {
    pub label: String,
    pub rss_bytes: u64,
}

/// Sample the current process RSS under a milestone label (0 bytes when the
/// platform reader fails; the report shows the raw value either way).
pub fn sample_rss(label: &str) -> RssSample {
    RssSample {
        label: label.to_string(),
        rss_bytes: cc_index::process_rss_bytes(),
    }
}

/// Full scale benchmark report for one synthetic repo size.
#[derive(Debug, Clone, Serialize)]
pub struct ScaleBenchReport {
    pub generated_at: String,
    pub scale_label: String,
    pub seed: u64,
    pub file_count: usize,
    pub symbols_total: usize,
    pub generate_ms: u64,
    pub cold_index_ms: u64,
    pub db_bytes: u64,
    pub incremental_single: IncrementalBenchReport,
    /// Same single-file edit driven through the event-scoped path
    /// (`changed_paths` on the `index` tool → BuildScope): stat-only
    /// scan/diff, no tree walk — the watcher-tick latency.
    pub incremental_single_scoped: IncrementalBenchReport,
    pub incremental_batch: IncrementalBenchReport,
    /// Watcher-parity incremental: same single-file edit, but scanned with
    /// `BuildScope::Targeted` (the production watcher path) instead of the
    /// full-tree walk the MCP `index` tool performs.
    pub incremental_targeted: Option<IncrementalBenchReport>,
    pub batch_touched_files: usize,
    pub tools: Vec<ScenarioLatency>,
    pub correctness: Vec<CorrectnessCheck>,
    /// Process RSS at bench milestones (this process runs generator, MCP
    /// server, and harness together — read as an upper bound, not a serving
    /// footprint).
    pub rss_samples: Vec<RssSample>,
}

/// Generate a Markdown report for one synthetic scale benchmark, mirroring
/// the existing benchmark report style.
pub fn generate_scale_markdown(report: &ScaleBenchReport) -> String {
    let mut md = String::new();

    md.push_str(&format!(
        "# Synthetic Scale Benchmark: {}\n\n",
        report.scale_label
    ));
    md.push_str(&format!("Generated: {}\n", report.generated_at));
    md.push_str(&format!(
        "Dataset: synthetic {} (seed {:#x})\n",
        report.scale_label, report.seed
    ));
    md.push_str(&format!(
        "Files: {} | Symbols: {}\n\n",
        report.file_count, report.symbols_total
    ));

    md.push_str("## Cold Full Index\n\n");
    md.push_str("| Metric | Value |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!("| generate wall | {}ms |\n", report.generate_ms));
    md.push_str(&format!(
        "| cold full index wall | {}ms |\n",
        report.cold_index_ms
    ));
    md.push_str(&format!(
        "| index db size | {} |\n\n",
        format_bytes(report.db_bytes as usize)
    ));

    md.push_str(&generate_incremental_markdown(&report.incremental_single));
    md.push_str(&generate_incremental_markdown(
        &report.incremental_single_scoped,
    ));
    md.push_str(&generate_incremental_markdown(&report.incremental_batch));
    if let Some(targeted) = &report.incremental_targeted {
        md.push_str(&generate_incremental_markdown(targeted));
        md.push_str(
            "Targeted = watcher-parity `BuildScope::Targeted` scan (event-reported paths \
             only), driven directly through `Indexer::prepare_build`/`commit_build`; total \
             elapsed is harness wall time across both halves.\n\n",
        );
    }

    md.push_str("## Per-Tool Latency\n\n");
    md.push_str(LATENCY_METHODOLOGY_NOTE);
    md.push_str(
        "| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |\n",
    );
    md.push_str(
        "|----------|------|-------------------|----------|----------|----------|----------|----------|------------|\n",
    );
    for tool in &report.tools {
        md.push_str(&format!(
            "| {} | {} | {}/{} | {} | {} | {} | {} | {} | {} |\n",
            tool.scenario,
            tool.tool,
            tool.cold_iterations,
            tool.warm_iterations,
            format_us(tool.cold.p50_us),
            format_us(tool.cold.max_us),
            format_us(tool.warm.p50_us),
            format_us(tool.warm.p95_us),
            format_us(tool.warm.max_us),
            format_bytes(tool.avg_output_bytes),
        ));
    }
    md.push('\n');

    md.push_str("## Ground-Truth Correctness\n\n");
    md.push_str("| Check | Passed | Detail |\n");
    md.push_str("|-------|--------|--------|\n");
    for check in &report.correctness {
        md.push_str(&format!(
            "| {} | {} | {} |\n",
            check.check,
            if check.passed { "YES" } else { "NO" },
            check.detail,
        ));
    }
    md.push('\n');

    if !report.rss_samples.is_empty() {
        md.push_str("## Process RSS\n\n");
        md.push_str(
            "Single-process harness (generator + in-process MCP server + bench driver): \
             an upper bound on the serving footprint, tracked for regression trends.\n\n",
        );
        md.push_str("| Milestone | RSS |\n");
        md.push_str("|-----------|-----|\n");
        for sample in &report.rss_samples {
            md.push_str(&format!(
                "| {} | {} |\n",
                sample.label,
                format_bytes(sample.rss_bytes as usize)
            ));
        }
        md.push('\n');
    }

    let passed = report.correctness.iter().filter(|c| c.passed).count();
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "- Incremental batch touched files: {}\n",
        report.batch_touched_files
    ));
    md.push_str(&format!(
        "- Ground-truth checks passed: {}/{}\n",
        passed,
        report.correctness.len()
    ));

    md
}

// ── Markdown generation ────────────────────────────────────────────

/// Shared methodology note for cold/warm latency tables.
const LATENCY_METHODOLOGY_NOTE: &str = "Methodology: cold = first call of a fresh MCP session \
per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search \
LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 \
discarded warmup (cache-hit path).\n\n";

/// Format byte counts for human readability.
fn format_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Generate a Markdown benchmark report.
pub fn generate_benchmark_markdown(report: &BenchmarkReport) -> String {
    let mut md = String::new();

    md.push_str("# Benchmark Results\n\n");
    md.push_str(&format!("Generated: {}\n", report.generated_at));
    md.push_str(&format!("Dataset: {}\n", report.dataset_name));
    md.push_str(&format!("Files: {}\n\n", report.fixture_files));

    // Per-tool latency table
    md.push_str("## Per-Tool Latency\n\n");
    md.push_str(LATENCY_METHODOLOGY_NOTE);
    md.push_str(
        "Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles \
         aggregate across cases per tool.\n\n",
    );
    md.push_str(
        "| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |\n",
    );
    md.push_str(
        "|------|-------|----------|----------|----------|----------|----------|------------|\n",
    );

    for tb in &report.per_tool {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            tb.tool,
            tb.cases,
            format_us(tb.cold_p50_us),
            format_us(tb.cold_max_us),
            format_us(tb.warm_p50_us),
            format_us(tb.warm_p95_us),
            format_us(tb.warm_max_us),
            format_bytes(tb.avg_output_bytes),
        ));
    }
    md.push('\n');

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!("- Total cases: {}\n", report.total_cases));

    let all_under_500 = report.per_tool.iter().all(|tb| tb.warm_p95_us < 500_000);
    md.push_str(&format!(
        "- All tools under 500ms warm p95: {}\n",
        if all_under_500 { "YES" } else { "NO" }
    ));

    md
}
