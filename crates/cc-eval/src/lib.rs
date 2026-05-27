pub mod bench;
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
    fn assertion_check_field_equals() {
        let output = serde_json::json!({
            "status": "ok",
            "count": 42,
            "active": true,
            "nothing": null,
        });

        // String match
        let assertion_str = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("ok".to_string()),
            field: Some("status".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion_str));

        // Number match
        let assertion_num = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("42".to_string()),
            field: Some("count".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion_num));

        // Bool match
        let assertion_bool = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("true".to_string()),
            field: Some("active".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion_bool));

        // Null match
        let assertion_null = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("null".to_string()),
            field: Some("nothing".to_string()),
        };
        assert!(runner::check_assertion(&output, &assertion_null));

        // Mismatch
        let assertion_miss = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("error".to_string()),
            field: Some("status".to_string()),
        };
        assert!(!runner::check_assertion(&output, &assertion_miss));

        // Missing field
        let assertion_missing = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("anything".to_string()),
            field: Some("nonexistent".to_string()),
        };
        assert!(!runner::check_assertion(&output, &assertion_missing));
    }

    #[test]
    fn report_summary_computation() {
        let results = vec![
            types::EvalCaseResult {
                case_name: "case_a".to_string(),
                tool: "search".to_string(),
                passed: true,
                duration_ms: 100,
                output_size_bytes: 512,
                assertions_passed: 2,
                assertions_failed: vec![],
                error: None,
                recall_at_5: None,
                mrr: None,
            },
            types::EvalCaseResult {
                case_name: "case_b".to_string(),
                tool: "search".to_string(),
                passed: false,
                duration_ms: 200,
                output_size_bytes: 1024,
                assertions_passed: 1,
                assertions_failed: vec!["output_contains: missing".to_string()],
                error: None,
                recall_at_5: None,
                mrr: None,
            },
            types::EvalCaseResult {
                case_name: "case_c".to_string(),
                tool: "status".to_string(),
                passed: true,
                duration_ms: 50,
                output_size_bytes: 256,
                assertions_passed: 1,
                assertions_failed: vec![],
                error: None,
                recall_at_5: None,
                mrr: None,
            },
        ];

        let summary = report::compute_summary(&results);
        assert_eq!(summary.total_cases, 3);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.total_duration_ms, 350);
        assert_eq!(summary.total_output_bytes, 1792);
        assert_eq!(summary.max_duration_ms, 200);
        assert_eq!(summary.p95_duration_ms, 200);
        assert_eq!(summary.per_tool.len(), 2);

        let search = summary.per_tool.get("search").unwrap();
        assert_eq!(search.case_count, 2);
        assert_eq!(search.passed, 1);
        assert_eq!(search.failed, 1);
        assert_eq!(search.max_duration_ms, 200);
        assert_eq!(search.p95_duration_ms, 200);

        let status = summary.per_tool.get("status").unwrap();
        assert_eq!(status.case_count, 1);
        assert_eq!(status.passed, 1);
        assert_eq!(status.max_duration_ms, 50);
        assert_eq!(status.p95_duration_ms, 50);
    }

    /// Benchmark test: load fixtures, run benchmark, optionally write report.
    /// This test requires the fixture files to exist.
    ///
    /// Uses a temporary copy of the fixture directory to avoid SQLite lock
    /// conflicts with `integration_fixtures_and_corpus` when tests run in
    /// parallel.
    #[test]
    fn benchmark_fixture() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures_dir = crate_dir.join("fixtures").join("sample-project");
        let corpus_dir = crate_dir.join("corpus");

        if !fixtures_dir.exists() || !corpus_dir.exists() {
            eprintln!(
                "Skipping benchmark test: fixtures or corpus not found at {}",
                crate_dir.display()
            );
            return;
        }

        // Copy fixture source files (not directories like .codecortex) to a
        // temp directory so we don't conflict with other tests that also build
        // an index on the same fixture path.
        let tmp = tempfile::tempdir().expect("create tempdir for benchmark");
        for entry in std::fs::read_dir(&fixtures_dir).expect("read fixtures") {
            let entry = entry.expect("read entry");
            let path = entry.path();
            if path.is_file() {
                let dest = tmp.path().join(entry.file_name());
                std::fs::copy(&path, &dest).expect("copy fixture file");
            }
        }
        let bench_fixtures = tmp.path().to_path_buf();

        // Count fixture files
        let fixture_files = std::fs::read_dir(&bench_fixtures)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count()
            })
            .unwrap_or(0);

        // Build the backend on the temp copy
        let backend = runner::CodeIndexBackend::new(&bench_fixtures)
            .expect("backend should build fixture index");

        // Load corpus
        let cases = corpus::load_corpus(&corpus_dir).expect("load corpus");
        assert!(!cases.is_empty(), "corpus should have at least one case");

        // Run benchmark
        let report = bench::run_benchmark(&backend, &cases, fixture_files);

        // Generate markdown
        let md = bench::generate_benchmark_markdown(&report);
        eprintln!("{}", md);

        // Basic assertions: benchmark should complete and produce results
        assert_eq!(report.total_cases, cases.len());
        assert!(!report.per_tool.is_empty(), "should have per-tool results");

        // Write to docs/benchmarks/latest.md only when explicitly requested
        // via CODECORTEX_WRITE_BENCHMARK=1 to avoid side effects in CI.
        if std::env::var("CODECORTEX_WRITE_BENCHMARK").is_ok() {
            let bench_dir = crate_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|root| root.join("docs").join("benchmarks"));
            if let Some(dir) = bench_dir {
                if dir.exists() || std::fs::create_dir_all(&dir).is_ok() {
                    let path = dir.join("latest.md");
                    if std::fs::write(&path, &md).is_ok() {
                        eprintln!("Benchmark report written to {}", path.display());
                    }
                }
            }
        }
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

        // All corpus cases must pass — zero tolerance for regressions.
        if eval_report.summary.failed > 0 {
            let failures: Vec<String> = eval_report
                .results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| {
                    format!(
                        "  - {} (tool={}): {:?}{}",
                        r.case_name,
                        r.tool,
                        r.assertions_failed,
                        r.error
                            .as_ref()
                            .map(|e| format!(" [error: {}]", e))
                            .unwrap_or_default()
                    )
                })
                .collect();
            panic!(
                "eval: {}/{} cases failed:\n{}",
                eval_report.summary.failed,
                eval_report.summary.total_cases,
                failures.join("\n")
            );
        }
    }
}
