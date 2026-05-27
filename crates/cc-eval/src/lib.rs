pub mod corpus;
pub mod report;
pub mod runner;
pub mod types;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_load_new_format() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let toml_content = r#"
name = "test_search"
tool = "search"
description = "search for a function"

[params]
query = "formatName"
mode = "symbol"
top_k = 5

[[assertions]]
kind = "is_success"

[[assertions]]
kind = "output_contains"
value = "formatName"
"#;
        std::fs::write(dir.path().join("test.toml"), toml_content).expect("write toml");

        let cases = corpus::load_corpus(dir.path()).expect("load corpus");
        assert_eq!(cases.len(), 1);
        let case = &cases[0];
        assert_eq!(case.name, "test_search");
        assert_eq!(case.tool, "search");
        assert_eq!(
            case.params.get("query").and_then(|v| v.as_str()),
            Some("formatName")
        );
        assert_eq!(case.assertions.len(), 2);
        assert_eq!(case.assertions[0].kind, "is_success");
        assert_eq!(case.assertions[1].kind, "output_contains");
    }

    #[test]
    fn assertion_check_output_contains() {
        let output = serde_json::json!({
            "hits": [{"name": "formatName", "file": "utils.js"}],
            "count": 1,
        });
        let assertion = types::Assertion {
            kind: "output_contains".to_string(),
            value: Some("formatName".to_string()),
            field: None,
        };
        assert!(runner::check_assertion(&output, &assertion));

        let assertion_miss = types::Assertion {
            kind: "output_contains".to_string(),
            value: Some("nonexistent_symbol".to_string()),
            field: None,
        };
        assert!(!runner::check_assertion(&output, &assertion_miss));
    }

    #[test]
    fn assertion_check_field_exists() {
        let output = serde_json::json!({
            "hits": [{"name": "foo"}],
            "meta": {"count": 1},
        });
        let assertion = types::Assertion {
            kind: "field_exists".to_string(),
            value: None,
            field: Some("meta.count".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion));

        let assertion_miss = types::Assertion {
            kind: "field_exists".to_string(),
            value: None,
            field: Some("meta.missing".to_string()),
        };
        assert!(!runner::check_assertion(&output, &assertion_miss));
    }

    #[test]
    fn assertion_check_min_results() {
        let output = serde_json::json!({
            "hits": [{"name": "a"}, {"name": "b"}, {"name": "c"}],
        });
        let assertion_pass = types::Assertion {
            kind: "min_results".to_string(),
            value: Some("2".to_string()),
            field: Some("hits".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion_pass));

        let assertion_fail = types::Assertion {
            kind: "min_results".to_string(),
            value: Some("5".to_string()),
            field: Some("hits".to_string()),
        };
        assert!(!runner::check_assertion(&output, &assertion_fail));
    }

    #[test]
    fn report_summary_computation() {
        let results = vec![
            types::EvalCaseResult {
                case_name: "case_a".to_string(),
                tool: "search".to_string(),
                passed: true,
                duration_ms: 100,
                assertions_passed: 2,
                assertions_failed: vec![],
                error: None,
            },
            types::EvalCaseResult {
                case_name: "case_b".to_string(),
                tool: "search".to_string(),
                passed: false,
                duration_ms: 200,
                assertions_passed: 1,
                assertions_failed: vec!["output_contains: missing".to_string()],
                error: None,
            },
            types::EvalCaseResult {
                case_name: "case_c".to_string(),
                tool: "status".to_string(),
                passed: true,
                duration_ms: 50,
                assertions_passed: 1,
                assertions_failed: vec![],
                error: None,
            },
        ];

        let summary = report::compute_summary(&results);
        assert_eq!(summary.total_cases, 3);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.total_duration_ms, 350);
        assert_eq!(summary.per_tool.len(), 2);

        let search = summary.per_tool.get("search").unwrap();
        assert_eq!(search.case_count, 2);
        assert_eq!(search.passed, 1);
        assert_eq!(search.failed, 1);

        let status = summary.per_tool.get("status").unwrap();
        assert_eq!(status.case_count, 1);
        assert_eq!(status.passed, 1);
    }

    /// Integration test: load fixtures, build index, run all corpus cases.
    /// This test requires the fixture files to exist.
    #[test]
    fn integration_fixtures_and_corpus() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures_dir = crate_dir.join("fixtures").join("sample-project");
        let corpus_dir = crate_dir.join("corpus");

        if !fixtures_dir.exists() || !corpus_dir.exists() {
            eprintln!(
                "Skipping integration test: fixtures or corpus not found at {}",
                crate_dir.display()
            );
            return;
        }

        // Build the backend — must succeed; do not silently skip.
        let backend = runner::CodeIndexBackend::new(&fixtures_dir)
            .expect("backend should build fixture index");

        // Load corpus
        let cases = corpus::load_corpus(&corpus_dir).expect("load corpus");
        assert!(!cases.is_empty(), "corpus should have at least one case");

        // Run all cases
        let results = runner::run_all(&backend, &cases);
        let eval_report = report::build_report(results);

        // Print the markdown report for CI visibility
        let md = report::generate_markdown(&eval_report);
        eprintln!("{}", md);

        // At least some cases should pass (we don't require 100% since some
        // tools like trace/graph_query may not find results in a tiny fixture)
        assert!(
            eval_report.summary.passed > 0,
            "at least one eval case should pass"
        );

        // Status and files should always pass on any indexed project
        for result in &eval_report.results {
            if result.tool == "status" || (result.tool == "files" && result.case_name.contains("list")) {
                assert!(
                    result.passed,
                    "tool={} case={} should pass but failed: {:?}",
                    result.tool,
                    result.case_name,
                    result.assertions_failed
                );
            }
        }
    }
}
