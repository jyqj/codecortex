//! Deterministic artifact/CLI fault injection, not retrieval performance evidence.
use cc_eval::benchmark::{
    self as b, manifest,
    report::{self, RunManifest},
    schema::{Hit, ResultStatus, Row, ADAPTER_VERSION},
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// An explicit harness-owned directory retains actual CLI fault artifacts after
// the test exits, including on a panic. Ordinary tests remain temporary.
struct FixtureRoot {
    temporary: Option<tempfile::TempDir>,
    retained: Option<PathBuf>,
}

impl FixtureRoot {
    fn new(case: &str) -> Self {
        if let Some(parent) = std::env::var_os("CODECORTEX_GATE_EVIDENCE_DIR") {
            let parent = PathBuf::from(parent);
            std::fs::create_dir_all(&parent).unwrap();
            let directory = tempfile::Builder::new()
                .prefix(case)
                .tempdir_in(parent)
                .unwrap();
            Self {
                temporary: None,
                retained: Some(directory.keep()),
            }
        } else {
            Self {
                temporary: Some(tempfile::tempdir().unwrap()),
                retained: None,
            }
        }
    }

    fn path(&self) -> &Path {
        match &self.retained {
            Some(path) => path,
            None => self.temporary.as_ref().unwrap().path(),
        }
    }
}

fn run_fixture(root: &Path, name: &str, samples: usize, elapsed_us: u64, found: bool) -> PathBuf {
    let out = root.join(name);
    std::fs::create_dir_all(out.join("raw")).unwrap();
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("a.py"), "def target():\n    return 1\n").unwrap();
    let query = json!({"id":"q","category":"symbol_location","difficulty":1,"language":"python","split":"dev","query_family":"target","query":"target","path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"primary","primary":true,"grade":2,"alternatives":[{"path":"a.py","symbol":null,"span":null}]}]});
    std::fs::write(root.join("questions.jsonl"), format!("{query}\n")).unwrap();
    let suite = root.join("suite.json");
    report::json(&suite, &json!({"schema_version":1,"name":"fault-injection","source":{"root":"source","commit":null,"digest":"","files":["a.py"]},"queries":"questions.jsonl","queries_digest":"","scoring":"codecortex-native-v1","repetitions":samples,"warmup":0,"seed":7,"timeout_ms":1000,"top_k":10,"engine_config":{"auto_index":{"enabled":false}}})).unwrap();
    report::json(&suite, &manifest::freeze(&suite).unwrap()).unwrap();
    let loaded = manifest::load(&suite).unwrap();
    report::jsonl(&out.join("queries.jsonl"), &loaded.queries).unwrap();
    let mut rows = Vec::new();
    for repetition in 0..samples {
        let raw_path = format!("raw/{repetition:06}.json");
        report::json(&out.join(&raw_path), &json!({"fixture":true,"nodes":[]})).unwrap();
        let mut hits = Vec::new();
        if found {
            let mut hit = Hit::path_only("a.py".into());
            hit.evidence_valid = Some(true);
            hits.push(hit);
        }
        rows.push(Row {
            case_id: "q".into(),
            repetition,
            status: if found {
                ResultStatus::Success
            } else {
                ResultStatus::NoMatch
            },
            elapsed_us,
            hits,
            raw_digest: manifest::file_digest(&out.join(&raw_path)).unwrap(),
            raw_path,
            diagnostic: Some("synthetic CLI gate test; not measured product behavior".into()),
        });
    }
    report::jsonl(&out.join("normalized.jsonl"), &rows).unwrap();
    let m = RunManifest {
        schema_version: 1,
        suite: loaded.suite,
        input: loaded.input,
        adapter: "synthetic-gate-fixture".into(),
        adapter_version: ADAPTER_VERSION.into(),
        engine: json!({"os":"fixture","arch":"fixture","hardware":"synthetic test metadata, not an observed machine","cpu_parallelism":1}),
        query_snapshot_digest: manifest::file_digest(&out.join("queries.jsonl")).unwrap(),
        normalized_digest: manifest::file_digest(&out.join("normalized.jsonl")).unwrap(),
        measurement_profile: "synthetic-gate-fixture".into(),
        source_verification: "test-controlled artifact fixture".into(),
        infrastructure_failure: None,
    };
    report::json(&out.join("manifest.json"), &m).unwrap();
    report::finish(&out, &m, &loaded.queries, &rows).unwrap();
    out
}

fn policy() -> b::comparison::Policy {
    b::comparison::Policy {
        schema_version: 1,
        max_ndcg_regression: 0.01,
        max_top1_regression: 0.01,
        max_latency_ratio: 1.2,
        minimum_latency_samples: 30,
    }
}

fn compare_cli(
    base: &Path,
    candidate: &Path,
    policy: &Path,
    output: &Path,
) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_cc-eval"))
        .arg("compare")
        .arg("--baseline")
        .arg(base)
        .arg("--candidate")
        .arg(candidate)
        .arg("--gate")
        .arg(policy)
        .arg("--output")
        .arg(output)
        .output()
        .unwrap()
}

#[test]
fn unmeasurable_latency_cannot_pass_comparison() {
    let d = FixtureRoot::new("unmeasurable_latency_cannot_pass_comparison-");
    let base = run_fixture(d.path(), "base", 30, 0, true);
    let candidate = run_fixture(d.path(), "candidate", 30, 10, true);
    let result = b::comparison::compare(&base, &candidate, &policy()).unwrap();
    assert_eq!(result["p95_ratio"], Value::Null);
    assert_eq!(result["status"], "inconclusive");
    assert_eq!(result["exit_code"], 1);
}

#[test]
fn zero_measurement_gate_is_invalid() {
    let gate = b::gate::evaluate(&[], &[], 0, None);
    assert_eq!(gate.exit_code, 2);
    assert_eq!(gate.status, "invalid_measurement");
}

#[test]
fn cli_quality_and_latency_failures_are_nonzero_and_keep_raw() {
    let d = FixtureRoot::new("cli_quality_and_latency_failures_are_nonzero_and_keep_raw-");
    let base = run_fixture(d.path(), "base", 30, 10, true);
    let policy_file = d.path().join("policy.json");
    report::json(&policy_file, &policy()).unwrap();
    for (name, duration, found, reason) in [
        ("quality", 10, false, "Top-1 regression"),
        ("latency", 100, true, "p95 regression"),
    ] {
        let candidate = run_fixture(d.path(), name, 30, duration, found);
        let raw = candidate.join("raw/000000.json");
        let before = std::fs::read(&raw).unwrap();
        let output = d.path().join(format!("{name}-comparison.json"));
        assert_eq!(
            compare_cli(&base, &candidate, &policy_file, &output)
                .status
                .code(),
            Some(1)
        );
        let result: Value = manifest::json_file(&output).unwrap();
        assert_eq!(result["status"], "failed");
        assert!(result["reasons"]
            .as_array()
            .unwrap()
            .contains(&json!(reason)));
        assert_eq!(std::fs::read(raw).unwrap(), before);
    }
}

#[test]
fn cli_inconclusive_is_nonzero() {
    let d = FixtureRoot::new("cli_inconclusive_is_nonzero-");
    let base = run_fixture(d.path(), "base", 2, 10, true);
    let candidate = run_fixture(d.path(), "candidate", 2, 10, true);
    let policy_file = d.path().join("policy.json");
    report::json(&policy_file, &policy()).unwrap();
    let output = d.path().join("comparison.json");
    assert_eq!(
        compare_cli(&base, &candidate, &policy_file, &output)
            .status
            .code(),
        Some(1)
    );
    let result: Value = manifest::json_file(&output).unwrap();
    assert_eq!(result["status"], "inconclusive");
}

#[test]
fn cli_lock_failure_preserves_machine_readable_failure_and_raw() {
    let d = FixtureRoot::new("cli_lock_failure_preserves_machine_readable_failure_and_raw-");
    let base = run_fixture(d.path(), "base", 2, 10, true);
    let candidate = run_fixture(d.path(), "candidate", 2, 10, true);
    let raw = candidate.join("raw/000000.json");
    std::fs::write(&raw, "changed raw evidence\n").unwrap();
    let policy_file = d.path().join("policy.json");
    report::json(&policy_file, &policy()).unwrap();
    let output = d.path().join("comparison.json");
    assert_eq!(
        compare_cli(&base, &candidate, &policy_file, &output)
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        std::fs::read_to_string(&raw).unwrap(),
        "changed raw evidence\n"
    );
    let result: Value =
        manifest::json_file(&output).expect("invalid comparison must retain a failure receipt");
    assert_eq!(result["status"], "invalid_measurement");
    assert_eq!(result["exit_code"], 2);
    assert!(result["reasons"][0]
        .as_str()
        .unwrap()
        .contains("raw response drift"));
}

#[test]
fn cli_bad_policy_is_recorded_without_overwriting_existing_report() {
    let d = FixtureRoot::new("cli_bad_policy_is_recorded_without_overwriting_existing_report-");
    let policy_file = d.path().join("policy.json");
    std::fs::write(&policy_file, "broken JSON").unwrap();
    let output = d.path().join("comparison.json");
    let missing = d.path().join("missing-run");
    assert_eq!(
        compare_cli(&missing, &missing, &policy_file, &output)
            .status
            .code(),
        Some(2)
    );
    let result: Value =
        manifest::json_file(&output).expect("invalid policy must retain a failure receipt");
    assert_eq!(result["status"], "invalid_measurement");
    let before = std::fs::read(&output).unwrap();
    assert_eq!(
        compare_cli(&missing, &missing, &policy_file, &output)
            .status
            .code(),
        Some(2)
    );
    assert_eq!(std::fs::read(&output).unwrap(), before);
}
