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
            let avg_ms = if count > 0 { total_ms / count as u64 } else { 0 };
            (
                tool,
                ToolSummary {
                    case_count: count,
                    passed: tool_passed,
                    failed: tool_failed,
                    avg_duration_ms: avg_ms,
                },
            )
        })
        .collect();

    EvalSummary {
        total_cases,
        passed,
        failed,
        total_duration_ms,
        per_tool,
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
        "| Total | Passed | Failed | Duration |\n|-------|--------|--------|----------|\n| {} | {} | {} | {}ms |\n\n",
        report.summary.total_cases,
        report.summary.passed,
        report.summary.failed,
        report.summary.total_duration_ms,
    ));

    // Per-tool breakdown
    if !report.summary.per_tool.is_empty() {
        md.push_str("## Per-Tool Results\n\n");
        md.push_str("| Tool | Cases | Passed | Failed | Avg Duration |\n");
        md.push_str("|------|-------|--------|--------|-------------|\n");
        let mut tools: Vec<_> = report.summary.per_tool.iter().collect();
        tools.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (tool, summary) in tools {
            md.push_str(&format!(
                "| {} | {} | {} | {} | {}ms |\n",
                tool, summary.case_count, summary.passed, summary.failed, summary.avg_duration_ms,
            ));
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
