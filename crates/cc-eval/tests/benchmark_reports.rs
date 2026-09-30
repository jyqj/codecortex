use cc_eval::benchmark::{
    self as b,
    adapters::Backend,
    manifest::{self, FileRecord},
    readiness::Readiness,
    report,
    schema::SearchInput,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
fn setup() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("source")).unwrap();
    std::fs::write(
        d.path().join("source/a.py"),
        "def target():\n    return 1\n",
    )
    .unwrap();
    let q = json!({"id":"q","category":"symbol_location","difficulty":1,"language":"python","split":"dev","query_family":"target","query":"target","path_prefix":null,"no_answer":false,"expected_files":[],"answers":[{"id":"primary","primary":true,"grade":2,"alternatives":[{"path":"a.py","symbol":null,"span":null}]}]});
    std::fs::write(d.path().join("questions.jsonl"), format!("{q}\n")).unwrap();
    let p = d.path().join("suite.json");
    report::json(&p,&json!({"schema_version":1,"name":"report-test","source":{"root":"source","commit":null,"digest":"","files":["a.py"]},"queries":"questions.jsonl","queries_digest":"","scoring":"codecortex-native-v1","repetitions":2,"warmup":0,"seed":7,"timeout_ms":1000,"top_k":10,"engine_config":{"auto_index":{"enabled":false}}})).unwrap();
    report::json(&p, &manifest::freeze(&p).unwrap()).unwrap();
    (d, p)
}
struct Stub {
    response: Value,
    prepare_fails: bool,
}
impl Backend for Stub {
    fn name(&self) -> &'static str {
        "fixture-stub-not-product"
    }
    async fn prepare(&mut self, _: &[FileRecord]) -> b::Result<Value> {
        if self.prepare_fails {
            Err(b::BenchError::Protocol("fixture unavailable".into()))
        } else {
            Ok(json!({"fixture":true}))
        }
    }
    async fn readiness(&mut self, f: &[FileRecord]) -> b::Result<Readiness> {
        Readiness::new(f.len(), f.len(), 0, 0, 0)
    }
    async fn search(&mut self, _: &SearchInput) -> b::Result<Value> {
        Ok(self.response.clone())
    }
    async fn close(&mut self) -> b::Result<()> {
        Ok(())
    }
}
async fn run(path: &Path, out: &Path, response: Value, fail: bool) -> b::gate::Gate {
    let l = manifest::load(path).unwrap();
    let isolated = manifest::materialize(&l).unwrap();
    std::fs::create_dir(out).unwrap();
    b::runner::run(
        &l,
        Stub {
            response,
            prepare_fails: fail,
        },
        isolated.path(),
        out,
        json!({"test_engine":true}),
        "unit-stub",
    )
    .await
    .unwrap()
}
#[test]
fn slices_use_question_means_not_repetition_counts() {
    let (_d, p) = setup();
    let mut q = manifest::load(&p).unwrap().queries.remove(0);
    q.annotations.insert("query_language".into(), json!("zh"));
    let mut other = q.clone();
    other.id = "other".into();
    let cases = vec![
        report::CaseScore {
            id: q.id.clone(),
            category: q.category.clone(),
            family: q.query_family.clone(),
            repetitions: 100,
            top1: 1.0,
            ndcg10: 1.0,
        },
        report::CaseScore {
            id: other.id.clone(),
            category: other.category.clone(),
            family: other.query_family.clone(),
            repetitions: 1,
            top1: 0.0,
            ndcg10: 0.0,
        },
    ];
    let slices = report::query_slices(&cases, &[q, other]);
    assert_eq!(slices["slices"][0]["questions"], 2);
    assert_eq!(slices["slices"][0]["mean_top1"], 0.5);
}
#[tokio::test]
async fn cancelled_factorial_child_is_not_left_green() {
    let (d, p) = setup();
    let parent = d.path().join("matrix");
    std::fs::create_dir(&parent).unwrap();
    let out = parent.join("dataset--variant");
    run(&p, &out, json!({"nodes":[]}), false).await;
    report::record_cancelled(&parent).unwrap();
    assert_eq!(report::replay(&out).unwrap().exit_code, 3);
}

#[tokio::test]
async fn raw_replay_reconstructs_costs_without_inventing_missing_measurements() {
    let (d, p) = setup();
    let out = d.path().join("cost-run");
    let mut cost = cc_model::retrieval_cost::RetrievalCost::default();
    cost.lexical_sql.vm_steps = Some(17);
    run(
        &p,
        &out,
        json!({"nodes":[],"evidence_summary":{"retrieval":{"cost":cost}}}),
        false,
    )
    .await;
    let before = std::fs::read(out.join("costs.jsonl")).unwrap();
    let rows: Vec<Value> = report::read_jsonl(&out.join("costs.jsonl")).unwrap();
    assert_eq!(rows[0]["work"]["cost"]["lexical_sql"]["vm_steps"], 17);
    report::replay(&out).unwrap();
    assert_eq!(before, std::fs::read(out.join("costs.jsonl")).unwrap());
    let old = d.path().join("old");
    run(&p, &old, json!({"nodes":[]}), false).await;
    let rows: Vec<Value> = report::read_jsonl(&old.join("costs.jsonl")).unwrap();
    assert!(rows.iter().all(|r| r["work"].is_null()));
}

#[tokio::test]
async fn raw_replay_reconstructs_identical_metrics() {
    let (d, p) = setup();
    let out = d.path().join("run");
    let gate=run(&p,&out,json!({"nodes":[{"node_type":"SEARCH_HIT","file_path":"a.py","start_line":1,"end_line":2,"text":"def target():\n    return 1","metadata":{}}]}),false).await;
    assert_eq!(gate.exit_code, 0);
    let before = std::fs::read(out.join("metrics.json")).unwrap();
    assert_eq!(report::replay(&out).unwrap().exit_code, 0);
    assert_eq!(before, std::fs::read(out.join("metrics.json")).unwrap());
}
#[tokio::test]
async fn malformed_response_is_invalid_not_zero_hit_success() {
    let (d, p) = setup();
    let gate = run(
        &p,
        &d.path().join("run"),
        json!({"new_unrecognized_shape":true}),
        false,
    )
    .await;
    assert_eq!(gate.exit_code, 2);
}
#[tokio::test]
async fn readiness_failure_keeps_missing_measurements_in_gate() {
    let (d, p) = setup();
    let out = d.path().join("run");
    let gate = run(&p, &out, json!({}), true).await;
    assert_eq!(gate.exit_code, 2);
    let m: Value = manifest::json_file(&out.join("metrics.json")).unwrap();
    assert_eq!(m["queries"], 1);
    assert_eq!(m["measured_rows"], 0);
    assert_eq!(report::replay(&out).unwrap().exit_code, 2);
}
#[tokio::test]
async fn wrong_source_text_fails_hard_gate() {
    let (d, p) = setup();
    let gate=run(&p,&d.path().join("run"),json!({"nodes":[{"node_type":"SEARCH_HIT","file_path":"a.py","start_line":1,"end_line":2,"text":"invented source","metadata":{}}]}),false).await;
    assert_eq!(gate.exit_code, 1);
}
#[tokio::test]
async fn raw_and_normalized_drift_are_rejected() {
    let (d, p) = setup();
    let out = d.path().join("run");
    run(&p, &out, json!({"nodes":[]}), false).await;
    std::fs::write(out.join("raw/000000.json"), "{}").unwrap();
    assert!(report::replay(&out).is_err());
    let out2 = d.path().join("run2");
    run(&p, &out2, json!({"nodes":[]}), false).await;
    std::fs::write(out2.join("normalized.jsonl"), "").unwrap();
    assert!(report::replay(&out2).is_err());
}
#[tokio::test]
async fn cancelled_run_is_never_green() {
    let (d, p) = setup();
    let out = d.path().join("run");
    run(&p, &out, json!({"nodes":[]}), false).await;
    report::record_cancelled(&out).unwrap();
    assert_eq!(report::replay(&out).unwrap().exit_code, 3);
}
#[tokio::test]
async fn small_sample_comparison_is_inconclusive() {
    let (d, p) = setup();
    let a = d.path().join("a");
    let b = d.path().join("b");
    run(&p, &a, json!({"nodes":[]}), false).await;
    run(&p, &b, json!({"nodes":[]}), false).await;
    let policy = b::comparison::Policy {
        schema_version: 1,
        max_ndcg_regression: 0.01,
        max_top1_regression: 0.01,
        max_latency_ratio: 1.2,
        minimum_latency_samples: 200,
    };
    let result = b::comparison::compare(&a, &b, &policy).unwrap();
    assert_eq!(result["status"], "inconclusive");
    assert_eq!(result["exit_code"], 1);
}
#[test]
fn cli_single_and_suite_validate_lock_before_run() {
    let (d, p) = setup();
    let bin = env!("CARGO_BIN_EXE_cc-eval");
    let status = std::process::Command::new(bin)
        .args(["validate", "--suite"])
        .arg(&p)
        .status()
        .unwrap();
    assert!(status.success());
    let bad = d.path().join("missing.json");
    let status = std::process::Command::new(bin)
        .args(["validate", "--suite"])
        .arg(&p)
        .arg("--suite")
        .arg(bad)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    std::fs::write(d.path().join("source/a.py"), "changed").unwrap();
    let out = d.path().join("invalid-run");
    let status = std::process::Command::new(bin)
        .args(["run", "--suite"])
        .arg(&p)
        .args(["--backend", "mcp-stdio", "--binary", "missing", "--output"])
        .arg(&out)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(!out.exists());
}
