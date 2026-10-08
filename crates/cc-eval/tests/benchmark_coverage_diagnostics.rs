//! Coverage diagnostics collect real-shaped protocol observations without
//! promoting incomplete coverage or replacing the strict runner's gates.
use cc_eval::benchmark::{
    self as b,
    adapters::Backend,
    manifest::{self, FileRecord},
    readiness::Readiness,
    report,
    schema::{ResultStatus, Row, SearchInput},
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn setup() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.py"), "def target():\n    return 1\n").unwrap();
    std::fs::write(source.join("guide.rst"), "A guide.\n").unwrap();
    let queries: Vec<_> = ["q1", "q2"]
        .into_iter()
        .map(|id| {
            json!({
                "id":id,"category":"symbol_location","difficulty":1,"language":"python",
                "split":"dev","query_family":"target","query":"target","path_prefix":null,
                "no_answer":false,"expected_files":[],"answers":[{"id":"primary","primary":true,
                "grade":2,"alternatives":[{"path":"a.py","symbol":null,"span":null}]}]
            })
        })
        .collect();
    report::jsonl(&directory.path().join("queries.jsonl"), &queries).unwrap();
    let suite = directory.path().join("suite.json");
    report::json(
        &suite,
        &json!({
            "schema_version":1,"name":"diagnostic-fixture","source":{"root":"source",
            "commit":null,"digest":"","files":["a.py","guide.rst"]},"queries":"queries.jsonl",
            "queries_digest":"","scoring":"codecortex-native-v1","repetitions":2,
            "warmup":1,"seed":7,"timeout_ms":1000,"top_k":10,
            "engine_config":{"auto_index":{"enabled":false}}
        }),
    )
    .unwrap();
    report::json(&suite, &manifest::freeze(&suite).unwrap()).unwrap();
    (directory, suite)
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    Prepare,
    Readiness,
    Warmup,
    Measurement,
}
struct Stub {
    ready: bool,
    fault: Fault,
    searches: Arc<AtomicUsize>,
}
impl Backend for Stub {
    fn name(&self) -> &'static str {
        "coverage-fixture-not-product"
    }
    async fn prepare(&mut self, _: &[FileRecord]) -> b::Result<Value> {
        if matches!(self.fault, Fault::Prepare) {
            Err(b::BenchError::Protocol("fixture prepare failure".into()))
        } else {
            Ok(json!({"fixture":true}))
        }
    }
    async fn readiness(&mut self, files: &[FileRecord]) -> b::Result<Readiness> {
        if matches!(self.fault, Fault::Readiness) {
            return Err(b::BenchError::Protocol("fixture malformed status".into()));
        }
        let ready = if self.ready { files.len() } else { 1 };
        Readiness::new(files.len(), ready, 0, 0, files.len() - ready)
    }
    async fn search(&mut self, _: &SearchInput) -> b::Result<Value> {
        let index = self.searches.fetch_add(1, Ordering::SeqCst);
        if (matches!(self.fault, Fault::Warmup) && index == 0)
            || (matches!(self.fault, Fault::Measurement) && index == 2)
        {
            Err(b::BenchError::Timeout("fixture deadline".into()))
        } else {
            Ok(json!({"nodes":[]}))
        }
    }
    async fn close(&mut self) -> b::Result<()> {
        Ok(())
    }
}

async fn execute(
    suite: &Path,
    out: &Path,
    diagnostic: bool,
    ready: bool,
    fault: Fault,
) -> (b::gate::Gate, usize) {
    let loaded = manifest::load(suite).unwrap();
    let isolated = manifest::materialize(&loaded).unwrap();
    std::fs::create_dir(out).unwrap();
    let searches = Arc::new(AtomicUsize::new(0));
    let backend = Stub {
        ready,
        fault,
        searches: searches.clone(),
    };
    let gate = if diagnostic {
        b::runner::collect_coverage_diagnostics(
            &loaded,
            backend,
            isolated.path(),
            out,
            json!({"fixture":true}),
        )
        .await
        .unwrap()
    } else {
        b::runner::run(
            &loaded,
            backend,
            isolated.path(),
            out,
            json!({"fixture":true}),
            "fixture-strict",
        )
        .await
        .unwrap()
    };
    (gate, searches.load(Ordering::SeqCst))
}

#[tokio::test]
async fn strict_incomplete_coverage_still_issues_zero_queries() {
    let (directory, suite) = setup();
    let out = directory.path().join("strict");
    let (gate, searches) = execute(&suite, &out, false, false, Fault::None).await;
    assert_eq!(gate.exit_code, 2);
    assert_eq!(searches, 0);
    assert!(gate
        .reasons
        .iter()
        .any(|reason| reason == "missing measurements"));
    assert!(!out.join("collection-policy.json").exists());
}

#[tokio::test]
async fn diagnostic_collects_every_query_without_promoting_unknown_readiness() {
    let (directory, suite) = setup();
    let out = directory.path().join("diagnostic");
    let (gate, searches) = execute(&suite, &out, true, false, Fault::None).await;
    assert_eq!(searches, 6); // Two warmups, four measured requests.
    assert_eq!(gate.exit_code, 2);
    assert_eq!(gate.status, "invalid_measurement");
    assert!(!gate
        .reasons
        .iter()
        .any(|reason| reason == "missing measurements"));
    let readiness: Value = manifest::json_file(&out.join("readiness.json")).unwrap();
    assert_eq!(readiness["state"], "unknown");
    assert_eq!(readiness["ready"], 1);
    assert_eq!(readiness["unknown"], 1);
    let recorded: report::RunManifest = manifest::json_file(&out.join("manifest.json")).unwrap();
    assert_eq!(recorded.measurement_profile, "coverage-diagnostic");
    assert!(recorded
        .infrastructure_failure
        .unwrap()
        .contains("input coverage not ready"));
    let rows: Vec<Row> = report::read_jsonl(&out.join("normalized.jsonl")).unwrap();
    let identities: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|row| (row.case_id.as_str(), row.repetition))
        .collect();
    assert_eq!(
        identities,
        [("q1", 0), ("q1", 1), ("q2", 0), ("q2", 1)]
            .into_iter()
            .collect()
    );
    assert_eq!(report::replay(&out).unwrap().exit_code, 2);
    std::fs::write(out.join(&rows[0].raw_path), "{}").unwrap();
    assert!(report::replay(&out)
        .unwrap_err()
        .to_string()
        .contains("raw response drift"));
}

#[tokio::test]
async fn even_ready_diagnostics_are_ineligible_for_a_valid_gate() {
    let (directory, suite) = setup();
    let out = directory.path().join("ready-diagnostic");
    let (gate, searches) = execute(&suite, &out, true, true, Fault::None).await;
    assert_eq!(searches, 6);
    assert_eq!(gate.exit_code, 2);
    let readiness: Value = manifest::json_file(&out.join("readiness.json")).unwrap();
    assert_eq!(readiness["state"], "ready");
    assert_eq!(report::replay(&out).unwrap().exit_code, 2);
}

#[tokio::test]
async fn diagnostics_do_not_bypass_prepare_status_or_warmup_errors() {
    for (index, fault) in [Fault::Prepare, Fault::Readiness, Fault::Warmup]
        .into_iter()
        .enumerate()
    {
        let (directory, suite) = setup();
        let out = directory.path().join(format!("failure-{index}"));
        let (gate, searches) = execute(&suite, &out, true, false, fault).await;
        assert_eq!(gate.exit_code, 2);
        assert_eq!(searches, usize::from(matches!(fault, Fault::Warmup)));
        assert!(gate
            .reasons
            .iter()
            .any(|reason| reason == "missing measurements"));
        assert_eq!(report::replay(&out).unwrap().exit_code, 2);
    }
}

#[tokio::test]
async fn measured_failures_remain_in_the_complete_diagnostic_row_set() {
    let (directory, suite) = setup();
    let out = directory.path().join("timeout");
    let (gate, searches) = execute(&suite, &out, true, false, Fault::Measurement).await;
    let rows: Vec<Row> = report::read_jsonl(&out.join("normalized.jsonl")).unwrap();
    assert_eq!(searches, 6);
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .filter(|row| row.status == ResultStatus::Timeout)
            .count(),
        1
    );
    assert_eq!(gate.exit_code, 2);
    assert_eq!(report::replay(&out).unwrap().exit_code, 2);
}

#[test]
fn path_snapshot_distinguishes_observed_indexing_and_specific_exclusions() {
    let (directory, suite) = setup();
    let loaded = manifest::load(&suite).unwrap();
    let work = directory.path().join("source");
    std::fs::write(work.join(".hidden"), "hidden\n").unwrap();
    std::fs::create_dir(work.join(".codecortex")).unwrap();
    let db_path = work.join(".codecortex/index.sqlite3");
    let db = rusqlite::Connection::open(&db_path).unwrap();
    db.execute_batch(
        "CREATE TABLE files(file_path TEXT,language TEXT,parser_tier TEXT);
        INSERT INTO files VALUES('a.py','python','tree_sitter');",
    )
    .unwrap();
    drop(db);
    let before = std::fs::read(&db_path).unwrap();
    let files = manifest::inventory(
        &work,
        &[".hidden".into(), "a.py".into(), "guide.rst".into()],
    )
    .unwrap();
    let snapshot =
        b::coverage_diagnostics::snapshot(&work, &files, &loaded.suite.engine_config).unwrap();
    assert_eq!(snapshot["indexed_files"], 1);
    assert_eq!(snapshot["unexpected_indexed_paths"], json!([]));
    assert_eq!(snapshot["reason_counts"]["indexed"], 1);
    assert_eq!(
        snapshot["reason_counts"]["hidden_path_excluded_by_scanner"],
        1
    );
    assert_eq!(
        snapshot["reason_counts"]["format_not_admitted_by_scanner"],
        1
    );
    assert_eq!(std::fs::read(&db_path).unwrap(), before);
}

#[test]
fn cli_diagnostics_reject_quality_performance_and_non_mcp_backends() {
    for extra in [
        vec!["--backend", "rg"],
        vec!["--profile", "quality"],
        vec!["--profile", "performance"],
    ] {
        let (directory, suite) = setup();
        let out = directory.path().join("must-not-create");
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_cc-eval"))
            .args(["run", "--collect-incomplete-coverage", "--suite"])
            .arg(&suite)
            .arg("--output")
            .arg(&out)
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(!out.exists());
        assert!(String::from_utf8_lossy(&result.stderr).contains("coverage diagnostics require"));
    }
}

fn unindexed_hidden_source_snapshot(include_hidden_files: bool) -> Value {
    let directory = tempfile::tempdir().unwrap();
    let work = directory.path();
    std::fs::create_dir(work.join(".demo")).unwrap();
    std::fs::write(
        work.join(".demo/example.py"),
        "def target():\n    return 1\n",
    )
    .unwrap();
    std::fs::write(work.join(".source.py"), "def source():\n    return 2\n").unwrap();
    std::fs::create_dir(work.join(".codecortex")).unwrap();
    let database = work.join(".codecortex/index.sqlite3");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("CREATE TABLE files(file_path TEXT,language TEXT,parser_tier TEXT);")
        .unwrap();
    drop(connection);
    let before = std::fs::read(&database).unwrap();
    let files =
        manifest::inventory(work, &[".demo/example.py".into(), ".source.py".into()]).unwrap();
    let config = json!({"indexing":{"include_hidden_files":include_hidden_files}});
    let snapshot = b::coverage_diagnostics::snapshot(work, &files, &config).unwrap();
    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(snapshot["indexed_files"], 0);
    assert_eq!(snapshot["valid_measurement_eligible"], false);
    snapshot
}

#[test]
fn default_hidden_sources_keep_the_scanner_exclusion_reason() {
    let snapshot = unindexed_hidden_source_snapshot(false);
    assert_eq!(
        snapshot["reason_counts"]["hidden_path_excluded_by_scanner"],
        2
    );
    for record in snapshot["records"].as_array().unwrap() {
        assert_eq!(record["scanner_admitted"], false);
        assert_eq!(record["indexed"], false);
    }
}

#[test]
fn enabled_hidden_sources_admitted_but_not_indexed_have_an_unknown_reason() {
    let snapshot = unindexed_hidden_source_snapshot(true);
    assert_eq!(
        snapshot["reason_counts"]["scanner_admitted_but_not_indexed_reason_unknown"],
        2
    );
    assert!(snapshot["reason_counts"]
        .get("hidden_path_excluded_by_scanner")
        .is_none());
    for record in snapshot["records"].as_array().unwrap() {
        assert_eq!(record["scanner_admitted"], true);
        assert_eq!(record["indexed"], false);
    }
}
