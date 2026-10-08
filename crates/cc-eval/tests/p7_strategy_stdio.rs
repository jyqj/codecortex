//! Explicit actual-product control for the P7-019 fake engineering leg.
//! The loopback endpoint returns the same unit vector for every input. It has
//! no access to gold and cannot support a semantic-quality interpretation.
#![cfg(feature = "semantic")]

use cc_eval::benchmark::{
    ablation::strategy::{self, reporting, Network, Plan, ReadyPolicy},
    manifest, report,
    schema::{Query, ScoreProfile, SourceLock, Suite},
};
use cc_model::query::RetrievalStrategy;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

#[path = "support/p7_fake_endpoint.rs"]
mod fake;
use fake::{FakeEndpoint, MODEL};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit semantic-http product/build receipt/source snapshot and a new evidence directory; fake mechanism only"]
async fn actual_loopback_three_public_policies_keep_equal_budgets_and_replayable_fake_effects() {
    let binary = PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit semantic-http product"),
    );
    let expected = std::env::var("P7_STRATEGY_BINARY_SHA256").expect("explicit product digest");
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&binary).unwrap())),
        expected
    );
    let evidence = PathBuf::from(
        std::env::var("P7_STRATEGY_EVIDENCE_DIR").expect("new persistent evidence directory"),
    );
    std::fs::create_dir(&evidence).unwrap();
    let source = evidence.join("input");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.rs"), "pub fn needle() -> i32 { 7 }\n").unwrap();
    std::fs::write(source.join("b.rs"), "pub fn copper() -> i32 { 11 }\n").unwrap();
    let endpoint = FakeEndpoint::start().await;
    let credential = tempfile::tempdir().unwrap();
    let key = credential.path().join("synthetic-key");
    std::fs::write(&key, "p7-019-synthetic-loopback-only").unwrap();
    let queries: Vec<Query> = [
        ("exact-a", "needle", "a.rs", "exact"),
        ("exact-b", "copper", "b.rs", "exact"),
        (
            "authored-intent",
            "find a synthetic implementation",
            "a.rs",
            "intent",
        ),
    ]
    .into_iter()
    .map(|(id, query, path, category)| {
        serde_json::from_value(json!({
            "id":id,"category":category,"difficulty":1,"language":"rust","split":"dev",
            "query_family":id,"query":query,"path_prefix":null,"no_answer":false,
            "expected_files":[],"answers":[{"id":"primary","primary":true,"grade":3,
                "alternatives":[{"path":path,"symbol":null,"span":null}]}],
            "annotations":{"profile":"fake","quality_interpretation":"none"}
        }))
        .unwrap()
    })
    .collect();
    report::jsonl(&evidence.join("queries.jsonl"), &queries).unwrap();
    let suite = Suite {
        schema_version: 1,
        name: "p7-019-fake-public-three-policy-mechanism".into(),
        source: SourceLock {
            root: PathBuf::from("input"),
            commit: None,
            digest: String::new(),
            files: vec!["a.rs".into(), "b.rs".into()],
        },
        queries: PathBuf::from("queries.jsonl"),
        queries_digest: String::new(),
        scoring: ScoreProfile::Native,
        repetitions: 2,
        warmup: 1,
        seed: 19,
        timeout_ms: 30_000,
        top_k: 1,
        engine_config: json!({"auto_index":{"enabled":false},"semantic":{
            "enabled":true,"network_opt_in":true,"allow_query_network":true,"allow_http":true,
            "endpoint":endpoint.endpoint,"api_key_ref":format!("file:{}",key.display()),
            "model_id":MODEL,"dimensions":4,"max_input_tokens":8192,"max_batch_items":8,"max_concurrent":1}}),
    };
    let suite_path = evidence.join("suite.json");
    report::json(&suite_path, &suite).unwrap();
    report::json(&suite_path, &manifest::freeze(&suite_path).unwrap()).unwrap();
    let plan = Plan {
        schema_version: 1,
        suite: PathBuf::from("suite.json"),
        source_snapshot: PathBuf::from(
            std::env::var("P7_STRATEGY_SOURCE_SNAPSHOT").expect("exact product source snapshot"),
        ),
        binary,
        build_receipt: PathBuf::from(
            std::env::var("P7_STRATEGY_BUILD_RECEIPT").expect("actual recorded product build"),
        ),
        strategies: vec![
            RetrievalStrategy::Local,
            RetrievalStrategy::Auto,
            RetrievalStrategy::Semantic,
        ],
        network: Network::LoopbackOnly,
        readiness: ReadyPolicy {
            expected_space: Some(
                cc_semantic::types::VectorSpace::new(MODEL, 4)
                    .unwrap()
                    .digest()
                    .unwrap()
                    .to_string(),
            ),
            timeout_ms: 30_000,
            poll_interval_ms: 50,
            max_polls: 600,
        },
    };
    let plan_path = evidence.join("plan.json");
    report::json(&plan_path, &plan).unwrap();
    let out = evidence.join("run");
    let result = strategy::run(&plan_path, &out).await;
    // Retain the fixture-side observations even if the experiment is rejected.
    report::json(&evidence.join("fake-provider-observations.json"), &json!({
        "profile":"fake","vector_rule":"constant [1,0,0,0] for every input; no gold available",
        "requests":*endpoint.requests.lock().unwrap(),"provider_billing":"unreported; not converted to zero"
    })).unwrap();
    assert_eq!(result.unwrap(), 0, "{}", out.display());
    let comparison: Value = manifest::json_file(&out.join("comparison.json")).unwrap();
    assert_eq!(comparison["profile"], "fake");
    assert_eq!(comparison["equal_effective_budgets"], true);
    assert_eq!(comparison["gates"].as_object().unwrap().len(), 3);
    assert_eq!(
        comparison["paired_against_local"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        comparison["capability_scope"]["dense_only"],
        "unsupported_by_current_public_api"
    );
    let mut summaries = BTreeMap::new();
    for name in ["local", "auto", "semantic"] {
        let cell = out.join(name);
        let before: report::Summary = manifest::json_file(&cell.join("metrics.json")).unwrap();
        assert_eq!(report::replay(&cell).unwrap().exit_code, 0);
        let after: report::Summary = manifest::json_file(&cell.join("metrics.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
        summaries.insert(name, after);
        let manifest: report::RunManifest =
            manifest::json_file(&cell.join("manifest.json")).unwrap();
        assert_eq!(manifest.measurement_profile, "fake");
        let costs: Vec<Value> = report::read_jsonl(&cell.join("strategy-costs.jsonl")).unwrap();
        assert_eq!(costs.len(), 9);
        assert!(costs.iter().all(|cost| cost["profile"] == "fake"
            && cost["provider_cost_units"].is_null()
            && cost["provider_cost_unknown"] == true
            && cost["cache_reuse"].is_null()));
    }
    for pair in comparison["paired_against_local"].as_array().unwrap() {
        let name = pair["candidate"].as_str().unwrap();
        assert_eq!(
            pair["observations"],
            reporting::paired_summary(
                &summaries["local"],
                &summaries[name],
                &queries,
                2,
                19,
                "fake"
            )
            .unwrap()
        );
    }
    let requested_query_hash = format!("{:x}", Sha256::digest(b"find a synthetic implementation"));
    assert!(endpoint
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request["input_sha256"]
            .as_array()
            .unwrap()
            .iter()
            .any(|digest| digest == &requested_query_hash)));
}
