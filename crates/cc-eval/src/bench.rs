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
        fixture_files,
        total_cases: cases.len(),
        per_tool,
    }
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
    md.push_str(&format!("Fixture: {} files\n\n", report.fixture_files));

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
