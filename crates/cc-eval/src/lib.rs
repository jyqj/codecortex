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
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion));

        let assertion_miss = types::Assertion {
            kind: "output_contains".to_string(),
            value: Some("nonexistent_symbol".to_string()),
            field: None,
            negate: false,
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
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion));

        let assertion_miss = types::Assertion {
            kind: "field_exists".to_string(),
            value: None,
            field: Some("meta.missing".to_string()),
            negate: false,
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
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_pass));

        let assertion_fail = types::Assertion {
            kind: "min_results".to_string(),
            value: Some("5".to_string()),
            field: Some("hits".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_fail));
    }

    #[test]
    fn assertion_expected_symbols_enforces_recall_threshold() {
        let output = serde_json::json!({
            "hits": [{"name": "a"}, {"name": "b"}],
        });

        let pass = types::Assertion {
            kind: "expected_symbols".to_string(),
            value: Some("a,b".to_string()),
            field: Some("hits".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &pass));

        let fail = types::Assertion {
            kind: "expected_symbols".to_string(),
            value: Some("a,b,c".to_string()),
            field: Some("hits".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &fail));
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
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_str));

        // Number match
        let assertion_num = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("42".to_string()),
            field: Some("count".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_num));

        // Bool match
        let assertion_bool = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("true".to_string()),
            field: Some("active".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_bool));

        // Null match
        let assertion_null = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("null".to_string()),
            field: Some("nothing".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_null));

        // Mismatch
        let assertion_miss = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("error".to_string()),
            field: Some("status".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_miss));

        // Missing field
        let assertion_missing = types::Assertion {
            kind: "field_equals".to_string(),
            value: Some("anything".to_string()),
            field: Some("nonexistent".to_string()),
            negate: false,
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

    fn copy_real_workspace_fixture(src: &std::path::Path, dst: &std::path::Path) -> usize {
        const EXCLUDED_DIRS: &[&str] = &[
            ".git",
            ".codecortex",
            "target",
            "node_modules",
            ".next",
            "dist",
            "build",
        ];
        const INCLUDED_EXTS: &[&str] = &["rs", "toml", "md", "json", "yaml", "yml", "sql", "lock"];
        const INCLUDED_NAMES: &[&str] = &["Dockerfile", "LICENSE", "README", "Makefile"];

        fn should_copy(path: &std::path::Path) -> bool {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if INCLUDED_NAMES
                .iter()
                .any(|n| name == *n || name.starts_with(&format!("{}.", n)))
            {
                return true;
            }
            path.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| INCLUDED_EXTS.contains(&ext))
        }

        fn walk(src: &std::path::Path, dst: &std::path::Path, count: &mut usize) {
            let Ok(entries) = std::fs::read_dir(src) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let target = dst.join(entry.file_name());
                let Ok(ft) = entry.file_type() else {
                    continue;
                };
                if ft.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if EXCLUDED_DIRS.contains(&name.as_ref()) {
                        continue;
                    }
                    let _ = std::fs::create_dir_all(&target);
                    walk(&path, &target, count);
                } else if ft.is_file() && should_copy(&path) {
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::copy(&path, &target).expect("copy real workspace file");
                    *count += 1;
                }
            }
        }

        let mut count = 0;
        walk(src, dst, &mut count);
        count
    }

    fn real_workspace_benchmark_cases() -> Vec<types::EvalCase> {
        use serde_json::json;
        vec![
            types::EvalCase {
                name: "real_status".into(),
                tool: "status".into(),
                description: "Status on the real CodeCortex workspace".into(),
                params: json!({ "aspect": "all" }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_files_list".into(),
                tool: "files".into(),
                description: "List indexed files in real workspace".into(),
                params: json!({ "action": "list", "limit": 50 }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_search_codeindex".into(),
                tool: "search".into(),
                description: "Symbol search for CodeIndex".into(),
                params: json!({ "query": "CodeIndex", "mode": "symbol", "top_k": 10 }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_search_ranked_fusion".into(),
                tool: "search".into(),
                description: "Hybrid search over ranked local retrieval implementation".into(),
                params: json!({ "query": "ranked fusion SearchEngine grep lexical", "mode": "hybrid", "top_k": 10 }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_context_index_flow".into(),
                tool: "context".into(),
                description: "Context for index build and search flow".into(),
                params: json!({ "task": "understand CodeIndex build_index and SearchEngine ranked search flow", "max_symbols": 8, "include_source": false }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_node_searchengine".into(),
                tool: "node".into(),
                description: "Inspect SearchEngine outline".into(),
                params: json!({ "symbol": "SearchEngine", "include": "outline" }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_relations_build_index".into(),
                tool: "relations".into(),
                description: "Relations for build_index".into(),
                params: json!({ "symbol": "build_index", "kind": "both", "limit": 20 }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_graph_query_functions".into(),
                tool: "graph_query".into(),
                description: "Cypher function query on real workspace".into(),
                params: json!({ "query": "MATCH (f:Function) RETURN f.name, f.file_path LIMIT 20" }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_architecture_overview".into(),
                tool: "architecture".into(),
                description: "Architecture overview on real workspace".into(),
                params: json!({ "aspect": "overview" }),
                assertions: vec![],
                expect_error: false,
            },
            types::EvalCase {
                name: "real_impact_dead_code".into(),
                tool: "impact".into(),
                description: "Dead code scan on real workspace".into(),
                params: json!({ "scope": "dead_code", "limit": 50 }),
                assertions: vec![],
                expect_error: false,
            },
        ]
    }

    /// Ignored by default: builds and benchmarks a sanitized copy of the real
    /// CodeCortex workspace instead of the 15-file fixture.
    #[test]
    #[ignore = "benchmarks the real workspace copy; run explicitly"]
    fn benchmark_real_workspace() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = crate_dir
            .parent()
            .and_then(|p| p.parent())
            .expect("cc-eval should live under crates/cc-eval");

        let tmp = tempfile::tempdir().expect("create tempdir for real workspace benchmark");
        let bench_root = tmp.path().join("codecortex-rust");
        std::fs::create_dir_all(&bench_root).expect("create benchmark workspace copy");
        let file_count = copy_real_workspace_fixture(workspace_root, &bench_root);
        assert!(
            file_count >= 100,
            "real workspace benchmark should copy substantially more than fixture files, got {}",
            file_count
        );

        let backend = runner::CodeIndexBackend::new(&bench_root)
            .expect("backend should build real workspace index copy");
        let cases = real_workspace_benchmark_cases();
        let report = bench::run_benchmark_named(
            &backend,
            &cases,
            file_count,
            "codecortex-rust workspace copy",
        );
        let md = bench::generate_benchmark_markdown(&report);
        eprintln!("{}", md);

        assert_eq!(report.total_cases, cases.len());
        assert!(!report.per_tool.is_empty(), "should have per-tool results");

        if std::env::var("CODECORTEX_WRITE_REAL_BENCHMARK").is_ok()
            || std::env::var("CODECORTEX_WRITE_BENCHMARK").is_ok()
        {
            let bench_dir = workspace_root.join("docs").join("benchmarks");
            std::fs::create_dir_all(&bench_dir).expect("create docs/benchmarks");
            let path = bench_dir.join("real_workspace_latest.md");
            std::fs::write(&path, &md).expect("write real workspace benchmark report");
            eprintln!(
                "Real workspace benchmark report written to {}",
                path.display()
            );
        }
    }

    fn copy_sample_fixture_sources(src: &std::path::Path, dst: &std::path::Path) -> usize {
        let mut count = 0;
        for entry in std::fs::read_dir(src).expect("read fixture sources") {
            let entry = entry.expect("read fixture entry");
            let path = entry.path();
            if path.is_file() {
                let dest = dst.join(entry.file_name());
                std::fs::copy(&path, &dest).expect("copy fixture source file");
                count += 1;
            }
        }
        count
    }

    fn report_usize(report: &serde_json::Value, field: &str) -> usize {
        report
            .get(field)
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| panic!("IndexReport field `{}` should be a u64", field))
            as usize
    }

    fn assert_no_parse_errors(report: &serde_json::Value, phase: &str) {
        let parse_errors = report
            .get("parse_errors")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("IndexReport parse_errors missing for {}", phase));
        assert!(
            parse_errors.is_empty(),
            "{} build should not produce parse errors: {:?}",
            phase,
            parse_errors
        );
    }

    /// Ignored by default: exercises IndexReport counters across a full build,
    /// a no-op incremental build, and a one-file incremental update.
    #[test]
    #[ignore = "benchmarks incremental indexing report counters; run explicitly"]
    fn benchmark_incremental_index_report_correctness() {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures_dir = crate_dir.join("fixtures").join("sample-project");
        assert!(
            fixtures_dir.exists(),
            "sample project fixtures should exist at {}",
            fixtures_dir.display()
        );

        let tmp = tempfile::tempdir().expect("create tempdir for incremental benchmark");
        let fixture_files = copy_sample_fixture_sources(&fixtures_dir, tmp.path());
        assert!(
            fixture_files >= 10,
            "incremental benchmark should copy a representative fixture set, got {}",
            fixture_files
        );

        let backend = runner::CodeIndexBackend::new_unindexed(tmp.path())
            .expect("backend should initialize without building");

        let full = backend
            .build_index_report(true)
            .expect("full index build should succeed");
        assert_eq!(report_usize(&full, "files_scanned"), fixture_files);
        assert_eq!(report_usize(&full, "files_added"), fixture_files);
        assert_eq!(report_usize(&full, "files_updated"), 0);
        assert_eq!(report_usize(&full, "files_removed"), 0);
        assert_eq!(report_usize(&full, "files_skipped"), 0);
        assert_eq!(report_usize(&full, "files_parsed"), fixture_files);
        assert!(
            report_usize(&full, "symbols_total") > 0,
            "full build should parse fixture symbols"
        );
        assert_no_parse_errors(&full, "full");

        let noop = backend
            .build_index_report(false)
            .expect("no-op incremental index build should succeed");
        assert_eq!(report_usize(&noop, "files_scanned"), fixture_files);
        assert_eq!(report_usize(&noop, "files_added"), 0);
        assert_eq!(report_usize(&noop, "files_updated"), 0);
        assert_eq!(report_usize(&noop, "files_removed"), 0);
        assert_eq!(report_usize(&noop, "files_skipped"), fixture_files);
        assert_eq!(report_usize(&noop, "files_parsed"), 0);
        assert_no_parse_errors(&noop, "no-op incremental");

        let modified = tmp.path().join("main.go");
        let mut source = std::fs::read_to_string(&modified).expect("read file to modify");
        source.push_str("\n// incremental index report marker\n");
        std::fs::write(&modified, source).expect("write incremental file modification");

        let incremental = backend
            .build_index_report(false)
            .expect("single-file incremental index build should succeed");
        assert_eq!(report_usize(&incremental, "files_scanned"), fixture_files);
        assert_eq!(report_usize(&incremental, "files_added"), 0);
        assert_eq!(report_usize(&incremental, "files_updated"), 1);
        assert_eq!(report_usize(&incremental, "files_removed"), 0);
        assert_eq!(
            report_usize(&incremental, "files_skipped"),
            fixture_files - 1
        );
        assert_eq!(report_usize(&incremental, "files_parsed"), 1);
        assert_no_parse_errors(&incremental, "single-file incremental");
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

        // Performance regression gate: all tools must stay under 500ms p95
        for tb in &report.per_tool {
            assert!(
                tb.p95_ms < 500,
                "perf regression: tool '{}' p95 = {}ms (limit 500ms)",
                tb.tool,
                tb.p95_ms,
            );
        }

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
    /// Uses a temp copy to avoid polluting the fixture tree with `.codecortex/`.
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

        // Copy fixtures to a temp directory so the index DB doesn't pollute
        // the source tree.
        let tmp = tempfile::tempdir().expect("create tempdir for integration test");
        copy_sample_fixture_sources(&fixtures_dir, tmp.path());

        // Build the backend — must succeed; do not silently skip.
        let backend =
            runner::CodeIndexBackend::new(tmp.path()).expect("backend should build fixture index");

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

    // ── Negative testing / new assertion tests ─────────────────────

    #[test]
    fn assertion_expected_error_pass() {
        // expect_error=true case should pass when tool returns Err.
        let case = types::EvalCase {
            name: "error_case".to_string(),
            tool: "nonexistent_tool".to_string(),
            description: "should error".to_string(),
            params: serde_json::json!({}),
            assertions: vec![types::Assertion {
                kind: "is_success".to_string(),
                value: None,
                field: None,
                negate: false,
            }],
            expect_error: true,
        };

        // Simulate what run_case does for expect_error + Err result.
        let err_msg = "unknown tool: nonexistent_tool";
        let result: Result<serde_json::Value, String> = Err(err_msg.to_string());

        let mut assertions_passed = 0;
        let mut assertions_failed: Vec<String> = Vec::new();

        assert!(case.expect_error);
        match &result {
            Err(err) => {
                let error_output = serde_json::Value::String(err.clone());
                for assertion in &case.assertions {
                    if assertion.kind == "is_success" {
                        assertions_passed += 1;
                        continue;
                    }
                    if runner::check_assertion(&error_output, assertion) {
                        assertions_passed += 1;
                    } else {
                        assertions_failed.push(assertion.describe());
                    }
                }
            }
            Ok(_) => {
                assertions_failed.push("expected tool error but got success".to_string());
            }
        }

        assert_eq!(assertions_passed, 1);
        assert!(assertions_failed.is_empty(), "should pass on Err");
    }

    #[test]
    fn assertion_expected_error_fail() {
        // expect_error=true case should fail when tool returns Ok.
        let case = types::EvalCase {
            name: "error_case_fail".to_string(),
            tool: "status".to_string(),
            description: "expected error but got ok".to_string(),
            params: serde_json::json!({}),
            assertions: vec![],
            expect_error: true,
        };

        let result: Result<serde_json::Value, String> = Ok(serde_json::json!({"status": "ok"}));

        let mut assertions_failed: Vec<String> = Vec::new();

        assert!(case.expect_error);
        match &result {
            Err(_) => {}
            Ok(_) => {
                assertions_failed.push("expected tool error but got success".to_string());
            }
        }

        assert_eq!(assertions_failed.len(), 1);
        assert_eq!(assertions_failed[0], "expected tool error but got success");
    }

    #[test]
    fn assertion_negate() {
        // negate=true should invert output_contains.
        let output = serde_json::json!({"name": "hello"});

        // "hello" IS in the output, so output_contains would pass.
        // With negate=true, it should FAIL.
        let assertion_negated_fail = types::Assertion {
            kind: "output_contains".to_string(),
            value: Some("hello".to_string()),
            field: None,
            negate: true,
        };
        assert!(
            !runner::check_assertion(&output, &assertion_negated_fail),
            "negated output_contains should fail when string is present"
        );

        // "missing" is NOT in the output, so output_contains would fail.
        // With negate=true, it should PASS.
        let assertion_negated_pass = types::Assertion {
            kind: "output_contains".to_string(),
            value: Some("missing".to_string()),
            field: None,
            negate: true,
        };
        assert!(
            runner::check_assertion(&output, &assertion_negated_pass),
            "negated output_contains should pass when string is absent"
        );
    }

    #[test]
    fn assertion_output_not_contains() {
        let output = serde_json::json!({"status": "ok", "data": [1, 2, 3]});

        // "error" is NOT in the output, so output_not_contains should pass.
        let assertion_pass = types::Assertion {
            kind: "output_not_contains".to_string(),
            value: Some("error".to_string()),
            field: None,
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_pass));

        // "ok" IS in the output, so output_not_contains should fail.
        let assertion_fail = types::Assertion {
            kind: "output_not_contains".to_string(),
            value: Some("ok".to_string()),
            field: None,
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_fail));
    }

    #[test]
    fn assertion_field_matches_regex() {
        let output = serde_json::json!({
            "version": "1.2.3",
            "count": 42,
            "active": true,
        });

        // Regex matches version string.
        let assertion_pass = types::Assertion {
            kind: "field_matches_regex".to_string(),
            value: Some(r"^\d+\.\d+\.\d+$".to_string()),
            field: Some("version".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_pass));

        // Regex does NOT match.
        let assertion_fail = types::Assertion {
            kind: "field_matches_regex".to_string(),
            value: Some(r"^v\d+".to_string()),
            field: Some("version".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_fail));

        // Regex on a number field.
        let assertion_num = types::Assertion {
            kind: "field_matches_regex".to_string(),
            value: Some(r"^\d{2}$".to_string()),
            field: Some("count".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_num));

        // Invalid regex should return false (not panic).
        let assertion_bad_regex = types::Assertion {
            kind: "field_matches_regex".to_string(),
            value: Some(r"[invalid".to_string()),
            field: Some("version".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_bad_regex));
    }

    #[test]
    fn assertion_array_contains_item() {
        let output = serde_json::json!({
            "users": [
                {"name": "processUser", "role": "admin"},
                {"name": "guestUser", "role": "viewer"},
            ]
        });

        // Should find processUser in the array.
        let assertion_pass = types::Assertion {
            kind: "array_contains_item".to_string(),
            value: Some("name=processUser".to_string()),
            field: Some("users".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_pass));

        // Should find by role.
        let assertion_role = types::Assertion {
            kind: "array_contains_item".to_string(),
            value: Some("role=viewer".to_string()),
            field: Some("users".to_string()),
            negate: false,
        };
        assert!(runner::check_assertion(&output, &assertion_role));

        // Should NOT find nonexistent user.
        let assertion_fail = types::Assertion {
            kind: "array_contains_item".to_string(),
            value: Some("name=unknownUser".to_string()),
            field: Some("users".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_fail));

        // Missing field path should fail.
        let assertion_missing = types::Assertion {
            kind: "array_contains_item".to_string(),
            value: Some("name=processUser".to_string()),
            field: Some("nonexistent".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_missing));

        // Invalid value format (no =) should fail.
        let assertion_bad_fmt = types::Assertion {
            kind: "array_contains_item".to_string(),
            value: Some("no_equals_sign".to_string()),
            field: Some("users".to_string()),
            negate: false,
        };
        assert!(!runner::check_assertion(&output, &assertion_bad_fmt));
    }
}
