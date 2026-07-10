use crate::types::{EvalCaseResult, EvalReport, EvalSummary, ToolSummary};
use std::collections::HashMap;

/// Build an EvalReport from a set of case results.
pub fn build_report(results: Vec<EvalCaseResult>) -> EvalReport {
    let summary = compute_summary(&results);
    let generated_at = chrono::Utc::now().to_rfc3339();
    EvalReport {
        results,
        summary,
        generated_at,
    }
}

/// Compute summary statistics from case results.
pub fn compute_summary(results: &[EvalCaseResult]) -> EvalSummary {
    if results.is_empty() {
        return EvalSummary::default();
    }

    let total_cases = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total_cases - passed;
    let total_duration_ms: u64 = results.iter().map(|r| r.duration_ms).sum();
    let total_output_bytes: usize = results.iter().map(|r| r.output_size_bytes).sum();

    // Compute p95 and max duration across all results
    let mut all_durations: Vec<u64> = results.iter().map(|r| r.duration_ms).collect();
    all_durations.sort_unstable();
    let max_duration_ms = all_durations.last().copied().unwrap_or(0);
    let p95_duration_ms = if all_durations.is_empty() {
        0
    } else {
        let idx = ((all_durations.len() as f64) * 0.95).ceil() as usize;
        let idx = idx.min(all_durations.len()).saturating_sub(1);
        all_durations[idx]
    };

    let mut per_tool: HashMap<String, Vec<&EvalCaseResult>> = HashMap::new();
    for result in results {
        per_tool
            .entry(result.tool.clone())
            .or_default()
            .push(result);
    }

    let per_tool: HashMap<String, ToolSummary> = per_tool
        .into_iter()
        .map(|(tool, group)| {
            let count = group.len();
            let tool_passed = group.iter().filter(|r| r.passed).count();
            let tool_failed = count - tool_passed;
            let total_ms: u64 = group.iter().map(|r| r.duration_ms).sum();
            let avg_ms = if count > 0 {
                total_ms / count as u64
            } else {
                0
            };

            let mut tool_durations: Vec<u64> = group.iter().map(|r| r.duration_ms).collect();
            tool_durations.sort_unstable();
            let tool_max = tool_durations.last().copied().unwrap_or(0);
            let tool_p95 = if tool_durations.is_empty() {
                0
            } else {
                let idx = ((tool_durations.len() as f64) * 0.95).ceil() as usize;
                let idx = idx.min(tool_durations.len()).saturating_sub(1);
                tool_durations[idx]
            };

            (
                tool,
                ToolSummary {
                    case_count: count,
                    passed: tool_passed,
                    failed: tool_failed,
                    avg_duration_ms: avg_ms,
                    p95_duration_ms: tool_p95,
                    max_duration_ms: tool_max,
                },
            )
        })
        .collect();

    // Compute average Recall@5 and MRR across cases that have values
    let recall_values: Vec<f64> = results.iter().filter_map(|r| r.recall_at_5).collect();
    let mrr_values: Vec<f64> = results.iter().filter_map(|r| r.mrr).collect();

    let avg_recall_at_5 = if recall_values.is_empty() {
        None
    } else {
        Some(recall_values.iter().sum::<f64>() / recall_values.len() as f64)
    };

    let avg_mrr = if mrr_values.is_empty() {
        None
    } else {
        Some(mrr_values.iter().sum::<f64>() / mrr_values.len() as f64)
    };

    EvalSummary {
        total_cases,
        passed,
        failed,
        total_duration_ms,
        p95_duration_ms,
        max_duration_ms,
        total_output_bytes,
        per_tool,
        avg_recall_at_5,
        avg_mrr,
    }
}

/// Generate a Markdown report from an EvalReport.
pub fn generate_markdown(report: &EvalReport) -> String {
    let mut md = String::new();
    md.push_str("# CodeCortex Eval Report\n\n");
    md.push_str(&format!("Generated: {}\n\n", report.generated_at));

    // Summary
    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "| Total | Passed | Failed | Duration | p95 | Max | Output |\n|-------|--------|--------|----------|-----|-----|--------|\n| {} | {} | {} | {}ms | {}ms | {}ms | {} bytes |\n\n",
        report.summary.total_cases,
        report.summary.passed,
        report.summary.failed,
        report.summary.total_duration_ms,
        report.summary.p95_duration_ms,
        report.summary.max_duration_ms,
        report.summary.total_output_bytes,
    ));

    // Per-tool breakdown
    if !report.summary.per_tool.is_empty() {
        md.push_str("## Per-Tool Results\n\n");
        md.push_str("| Tool | Cases | Passed | Failed | Avg Duration | p95 | Max |\n");
        md.push_str("|------|-------|--------|--------|-------------|-----|-----|\n");
        let mut tools: Vec<_> = report.summary.per_tool.iter().collect();
        tools.sort_by_key(|(a, _)| *a);
        for (tool, summary) in tools {
            md.push_str(&format!(
                "| {} | {} | {} | {} | {}ms | {}ms | {}ms |\n",
                tool,
                summary.case_count,
                summary.passed,
                summary.failed,
                summary.avg_duration_ms,
                summary.p95_duration_ms,
                summary.max_duration_ms,
            ));
        }
        md.push('\n');
    }

    // Retrieval Quality section (only if we have metrics)
    if report.summary.avg_recall_at_5.is_some() || report.summary.avg_mrr.is_some() {
        md.push_str("## Retrieval Quality\n\n");
        md.push_str("| Metric | Value |\n");
        md.push_str("|--------|-------|\n");
        if let Some(recall) = report.summary.avg_recall_at_5 {
            md.push_str(&format!("| Avg Recall@5 | {:.2} |\n", recall));
        }
        if let Some(mrr) = report.summary.avg_mrr {
            md.push_str(&format!("| Avg MRR | {:.2} |\n", mrr));
        }
        md.push('\n');
    }

    // Per-case details
    md.push_str("## Cases\n\n");
    for result in &report.results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        md.push_str(&format!(
            "### {} [{}] ({}ms)\n\n",
            result.case_name, status, result.duration_ms,
        ));
        md.push_str(&format!("- Tool: `{}`\n", result.tool));
        md.push_str(&format!(
            "- Assertions passed: {}\n",
            result.assertions_passed,
        ));
        if !result.assertions_failed.is_empty() {
            md.push_str("- Failed assertions:\n");
            for fail in &result.assertions_failed {
                md.push_str(&format!("  - {}\n", fail));
            }
        }
        if let Some(ref err) = result.error {
            md.push_str(&format!("- Error: {}\n", err));
        }
        md.push('\n');
    }

    md
}
