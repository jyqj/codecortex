use crate::types::{
    CategorySummary, EvalCaseReport, EvalDelta, EvalMetrics, EvalReport, EvalSummary,
};
use std::collections::HashMap;

pub fn generate_markdown(report: &EvalReport) -> String {
    let mut md = String::new();
    md.push_str("# CodeCortex Evaluation Report\n\n");
    md.push_str(&format!("Generated: {}\n\n", report.generated_at));

    // Summary table
    md.push_str("## Summary\n\n");
    md.push_str("| Metric | Reduction |\n|--------|----------|\n");
    md.push_str(&format!(
        "| Wall-clock time | {:.1}% |\n",
        report.summary.avg_wall_clock_reduction_pct
    ));
    md.push_str(&format!(
        "| Tokens | {:.1}% |\n",
        report.summary.avg_token_reduction_pct
    ));
    md.push_str(&format!(
        "| Tool calls | {:.1}% |\n",
        report.summary.avg_tool_call_reduction_pct
    ));
    md.push_str(&format!(
        "| Cost | {:.1}% |\n",
        report.summary.avg_cost_reduction_pct
    ));

    // Per-category breakdown
    if !report.summary.per_category.is_empty() {
        md.push_str("\n## Per-Category Results\n\n");
        md.push_str(
            "| Category | Cases | Avg Wall Clock Δ | Avg Token Δ | Success Rate |\n",
        );
        md.push_str(
            "|----------|-------|-------------------|-------------|--------------|\n",
        );
        let mut cats: Vec<_> = report.summary.per_category.iter().collect();
        cats.sort_by(|(a, _), (b, _)| a.cmp(b));
        for (cat, summary) in cats {
            md.push_str(&format!(
                "| {} | {} | {:.1}% | {:.1}% | {:.0}% |\n",
                cat,
                summary.case_count,
                summary.avg_wall_clock_reduction_pct,
                summary.avg_token_reduction_pct,
                summary.success_rate * 100.0,
            ));
        }
    }

    // Per-case details
    md.push_str("\n## Cases\n\n");
    for case in &report.cases {
        md.push_str(&format!("### {}\n\n", case.case_name));
        md.push_str("| | With CC | Without CC | Delta |\n|---|---------|------------|-------|\n");
        md.push_str(&format!(
            "| Time | {}ms | {}ms | {:.1}% |\n",
            case.with_cc.metrics.wall_clock_ms,
            case.without_cc.metrics.wall_clock_ms,
            case.delta.wall_clock_reduction_pct
        ));
        md.push_str(&format!(
            "| Tokens | {} | {} | {:.1}% |\n",
            case.with_cc.metrics.token_count_input + case.with_cc.metrics.token_count_output,
            case.without_cc.metrics.token_count_input + case.without_cc.metrics.token_count_output,
            case.delta.token_reduction_pct
        ));
        md.push_str(&format!(
            "| Tool calls | {} | {} | {:.1}% |\n",
            case.with_cc.metrics.tool_call_count,
            case.without_cc.metrics.tool_call_count,
            case.delta.tool_call_reduction_pct
        ));
        md.push_str("\n");
    }

    md
}

pub fn compute_delta(with_cc: &EvalMetrics, without_cc: &EvalMetrics) -> EvalDelta {
    let pct = |a: f64, b: f64| -> f64 {
        if b == 0.0 {
            0.0
        } else {
            (1.0 - a / b) * 100.0
        }
    };
    EvalDelta {
        wall_clock_reduction_pct: pct(
            with_cc.wall_clock_ms as f64,
            without_cc.wall_clock_ms as f64,
        ),
        token_reduction_pct: pct(
            (with_cc.token_count_input + with_cc.token_count_output) as f64,
            (without_cc.token_count_input + without_cc.token_count_output) as f64,
        ),
        tool_call_reduction_pct: pct(
            with_cc.tool_call_count as f64,
            without_cc.tool_call_count as f64,
        ),
        cost_reduction_pct: pct(with_cc.estimated_cost_usd, without_cc.estimated_cost_usd),
        file_read_reduction_pct: pct(
            with_cc.file_read_count as f64,
            without_cc.file_read_count as f64,
        ),
    }
}

pub fn compute_summary(cases: &[EvalCaseReport]) -> EvalSummary {
    if cases.is_empty() {
        return EvalSummary::default();
    }
    let n = cases.len() as f64;

    // Build per-category breakdown
    let mut cat_groups: HashMap<String, Vec<&EvalCaseReport>> = HashMap::new();
    for case in cases {
        cat_groups
            .entry(case.category.as_str().to_string())
            .or_default()
            .push(case);
    }
    let per_category: HashMap<String, CategorySummary> = cat_groups
        .into_iter()
        .map(|(cat, group)| {
            let cnt = group.len() as f64;
            let avg_wc = group
                .iter()
                .map(|c| c.delta.wall_clock_reduction_pct)
                .sum::<f64>()
                / cnt;
            let avg_tok = group
                .iter()
                .map(|c| c.delta.token_reduction_pct)
                .sum::<f64>()
                / cnt;
            let successes = group.iter().filter(|c| c.with_cc.success).count() as f64;
            (
                cat,
                CategorySummary {
                    case_count: group.len(),
                    avg_wall_clock_reduction_pct: avg_wc,
                    avg_token_reduction_pct: avg_tok,
                    success_rate: successes / cnt,
                },
            )
        })
        .collect();

    EvalSummary {
        total_cases: cases.len(),
        avg_wall_clock_reduction_pct: cases
            .iter()
            .map(|c| c.delta.wall_clock_reduction_pct)
            .sum::<f64>()
            / n,
        avg_token_reduction_pct: cases
            .iter()
            .map(|c| c.delta.token_reduction_pct)
            .sum::<f64>()
            / n,
        avg_tool_call_reduction_pct: cases
            .iter()
            .map(|c| c.delta.tool_call_reduction_pct)
            .sum::<f64>()
            / n,
        avg_cost_reduction_pct: cases
            .iter()
            .map(|c| c.delta.cost_reduction_pct)
            .sum::<f64>()
            / n,
        per_category,
    }
}
