//! Benchmark runner: measures per-tool latency and output size across eval cases.
//!
//! Each case is run 3 times (1 warmup + 2 measured). The faster of the 2
//! measured runs is used as the "best" duration. Results are aggregated per-tool
//! with p50/p95/max latency and average output size.

use crate::runner::CodeIndexBackend;
use crate::types::EvalCase;
use serde::Serialize;
use std::collections::HashMap;
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
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
    pub avg_output_bytes: usize,
}

// ── Single-case measurement ────────────────────────────────────────

#[derive(Debug, Clone)]
struct CaseMeasurement {
    tool: String,
    best_ms: u64,
    output_bytes: usize,
}

/// Run a single eval case 3 times (1 warmup + 2 measured), return the best
/// measured duration and output size from the last successful run.
fn measure_case(backend: &CodeIndexBackend, case: &EvalCase) -> CaseMeasurement {
    let mut durations = Vec::with_capacity(2);
    let mut last_output_bytes: usize = 0;

    for iteration in 0..3 {
        let start = Instant::now();
        let result = backend.call_tool(&case.tool, &case.params);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        let output_bytes = match &result {
            Ok(output) => serde_json::to_string(output).unwrap_or_default().len(),
            Err(_) => 0,
        };

        if iteration >= 1 {
            // Measured runs (skip iteration 0 = warmup)
            durations.push(elapsed_ms);
            last_output_bytes = output_bytes;
        }
    }

    let best_ms = durations.iter().copied().min().unwrap_or(0);

    CaseMeasurement {
        tool: case.tool.clone(),
        best_ms,
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
/// `fixture_files` is the number of source files in the fixture project
/// (for display in the report header).
pub fn run_benchmark(
    backend: &CodeIndexBackend,
    cases: &[EvalCase],
    fixture_files: usize,
) -> BenchmarkReport {
    run_benchmark_named(backend, cases, fixture_files, "fixture")
}

/// Run benchmarks with a dataset label for the generated report.
pub fn run_benchmark_named(
    backend: &CodeIndexBackend,
    cases: &[EvalCase],
    fixture_files: usize,
    dataset_name: &str,
) -> BenchmarkReport {
    // Measure each case
    let measurements: Vec<CaseMeasurement> =
        cases.iter().map(|c| measure_case(backend, c)).collect();

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

            let mut durations: Vec<u64> = group.iter().map(|m| m.best_ms).collect();
            durations.sort_unstable();

            let total_output: usize = group.iter().map(|m| m.output_bytes).sum();
            let avg_output = if cases_count > 0 {
                total_output / cases_count
            } else {
                0
            };

            let max_ms = durations.last().copied().unwrap_or(0);

            ToolBenchmark {
                tool,
                cases: cases_count,
                p50_ms: percentile(&durations, 0.50),
                p95_ms: percentile(&durations, 0.95),
                max_ms,
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
    fn from_durations(durations: &[u64]) -> Self {
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

// ── Markdown generation ────────────────────────────────────────────

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
    md.push_str("| Tool | Cases | p50 | p95 | Max | Avg Output |\n");
    md.push_str("|------|-------|-----|-----|-----|------------|\n");

    for tb in &report.per_tool {
        md.push_str(&format!(
            "| {} | {} | {}ms | {}ms | {}ms | {} |\n",
            tb.tool,
            tb.cases,
            tb.p50_ms,
            tb.p95_ms,
            tb.max_ms,
            format_bytes(tb.avg_output_bytes),
        ));
    }
    md.push('\n');

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!("- Total cases: {}\n", report.total_cases));

    let all_under_500 = report.per_tool.iter().all(|tb| tb.p95_ms < 500);
    md.push_str(&format!(
        "- All tools under 500ms p95: {}\n",
        if all_under_500 { "YES" } else { "NO" }
    ));

    md
}
