pub mod corpus;
pub mod report;
pub mod runner;
pub mod types;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn eval_category_serde_roundtrip() {
        use types::EvalCategory;

        let categories = vec![
            EvalCategory::RouteHandler,
            EvalCategory::ImpactRadius,
            EvalCategory::DynamicCallback,
            EvalCategory::TypeHierarchy,
            EvalCategory::CircularDeps,
            EvalCategory::DeadCode,
        ];
        for cat in &categories {
            let json = serde_json::to_string(cat).expect("serialize");
            let back: EvalCategory = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*cat, back, "roundtrip failed for {json}");
        }
        // Also check as_str consistency
        assert_eq!(EvalCategory::RouteHandler.as_str(), "route_handler");
        assert_eq!(EvalCategory::DeadCode.as_str(), "dead_code");
    }

    #[test]
    fn category_summary_computation() {
        use types::{
            EvalCaseReport, EvalCategory, EvalDelta,
            EvalMetrics, EvalRun,
        };

        let make_case_report = |name: &str, cat: EvalCategory, wc_pct: f64, tok_pct: f64, success: bool| {
            EvalCaseReport {
                case_name: name.to_string(),
                category: cat,
                with_cc: EvalRun {
                    case_name: name.to_string(),
                    with_codecortex: true,
                    metrics: EvalMetrics::default(),
                    tool_calls: vec![],
                    success,
                    error: None,
                },
                without_cc: EvalRun {
                    case_name: name.to_string(),
                    with_codecortex: false,
                    metrics: EvalMetrics::default(),
                    tool_calls: vec![],
                    success: true,
                    error: None,
                },
                delta: EvalDelta {
                    wall_clock_reduction_pct: wc_pct,
                    token_reduction_pct: tok_pct,
                    tool_call_reduction_pct: 0.0,
                    cost_reduction_pct: 0.0,
                    file_read_reduction_pct: 0.0,
                },
            }
        };

        let cases = vec![
            make_case_report("rh1", EvalCategory::RouteHandler, 30.0, 40.0, true),
            make_case_report("rh2", EvalCategory::RouteHandler, 40.0, 50.0, true),
            make_case_report("dc1", EvalCategory::DeadCode, 20.0, 25.0, false),
        ];

        let summary = report::compute_summary(&cases);
        assert_eq!(summary.total_cases, 3);
        assert_eq!(summary.per_category.len(), 2);

        let rh = summary.per_category.get("route_handler").expect("route_handler category");
        assert_eq!(rh.case_count, 2);
        assert!((rh.avg_wall_clock_reduction_pct - 35.0).abs() < 0.01);
        assert!((rh.avg_token_reduction_pct - 45.0).abs() < 0.01);
        assert!((rh.success_rate - 1.0).abs() < 0.01);

        let dc = summary.per_category.get("dead_code").expect("dead_code category");
        assert_eq!(dc.case_count, 1);
        assert!((dc.success_rate - 0.0).abs() < 0.01);
    }

    #[test]
    fn corpus_load_from_toml() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let toml_content = r#"
name = "test_case"
category = "dead_code"
difficulty = "hard"
prompt = "find dead code"
repo_path = "."
expected_markers = ["unused_fn"]
mcp_tools = ["find_dead_code"]
timeout = 60
"#;
        std::fs::write(dir.path().join("test.toml"), toml_content).expect("write toml");

        let cases = corpus::load_corpus(dir.path()).expect("load corpus");
        assert_eq!(cases.len(), 1);
        let case = &cases[0];
        assert_eq!(case.name, "test_case");
        assert_eq!(case.category, types::EvalCategory::DeadCode);
        assert_eq!(case.mcp_tools, vec!["find_dead_code"]);
        assert_eq!(case.expected_markers, vec!["unused_fn"]);
        assert_eq!(case.timeout, Duration::from_secs(60));
    }

    #[test]
    fn corpus_load_backwards_compatible() {
        // Old-style TOML without category/difficulty/mcp_tools should still parse
        let dir = tempfile::tempdir().expect("create tempdir");
        let toml_content = r#"
name = "legacy_case"
prompt = "find stuff"
repo_path = "."
expected_markers = ["marker1"]
timeout = 30
"#;
        std::fs::write(dir.path().join("legacy.toml"), toml_content).expect("write toml");

        let cases = corpus::load_corpus(dir.path()).expect("load corpus");
        assert_eq!(cases.len(), 1);
        let case = &cases[0];
        assert_eq!(case.name, "legacy_case");
        // defaults
        assert_eq!(case.category, types::EvalCategory::RouteHandler);
        assert!(case.mcp_tools.is_empty());
    }
}
