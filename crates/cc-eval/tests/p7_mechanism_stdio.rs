//! Actual independent binaries for the P7-019 fake engineering leg. Source
//! controls never become production flags; authored fake effects are not V19.
#![cfg(feature = "semantic")]
use cc_eval::benchmark::{
    ablation::{
        mechanism::{self, Build, Plan},
        strategy::{reporting, QueryObservation, ReadyPolicy},
    },
    manifest, normalizer, report,
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
#[ignore = "requires four independently compiled snapshot binaries and a new evidence directory; fake engineering only"]
async fn actual_source_isolated_local_dense_hybrid_and_no_lanes_controls() {
    let build_path = PathBuf::from(
        std::env::var("P7_MECHANISM_BUILD_MANIFEST")
            .expect("explicit independently built mechanism manifest"),
    )
    .canonicalize()
    .unwrap();
    let build: Build = manifest::json_file(&build_path).unwrap();
    let evidence = PathBuf::from(
        std::env::var("P7_MECHANISM_EVIDENCE_DIR").expect("new persistent evidence directory"),
    );
    std::fs::create_dir(&evidence).unwrap();
    report::json(
        &evidence.join("acceptance.json"),
        &json!({
            "profile":"fake","source_commit":build.source_commit,
            "passed":false,"status":"running; raw artifacts retained if any control fails"
        }),
    )
    .unwrap();
    let source = evidence.join("input");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.rs"), "pub fn needle() -> i32 { 7 }\n").unwrap();
    std::fs::write(source.join("b.rs"), "pub fn copper() -> i32 { 11 }\n").unwrap();
    let endpoint = FakeEndpoint::start().await;
    let credential = tempfile::tempdir().unwrap();
    let key = credential.path().join("synthetic-key");
    std::fs::write(&key, "p7-019-synthetic-loopback-only").unwrap();
    let queries: Vec<Query> = [
        ("exact-a", "needle", "a.rs", "exact", "dev"),
        ("exact-b", "copper", "b.rs", "exact", "holdout"),
        ("dense-control", "zqxvplmnb", "a.rs", "synthetic_dense_control", "holdout"),
    ].into_iter().map(|(id, query, path, category, split)| {
        serde_json::from_value(json!({"id":id,"category":category,"difficulty":1,
            "language":"rust","split":split,"query_family":id,"query":query,
            "path_prefix":null,"no_answer":false,"expected_files":[],
            "answers":[{"id":"primary","primary":true,"grade":3,
                "alternatives":[{"path":path,"symbol":null,"span":null}]}],
            "annotations":{"profile":"fake","quality_interpretation":"none",
                "split_meaning":"fixed authored engineering partition; not independent public holdout"}})).unwrap()
    }).collect();
    report::jsonl(&evidence.join("queries.jsonl"), &queries).unwrap();
    let suite = Suite {
        schema_version: 1,
        name: "p7-019-isolated-fake-mechanism-four-cells".into(),
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
        build: build_path.clone(),
        suite: PathBuf::from("suite.json"),
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
    let result = mechanism::run(&plan_path, &out).await;
    report::json(&evidence.join("fake-provider-observations.json"), &json!({
        "profile":"fake","vector_rule":"constant [1,0,0,0] for every input; no gold access",
        "requests":*endpoint.requests.lock().unwrap(),"provider_billing":"unreported; never zero-filled"
    })).unwrap();
    assert_eq!(result.unwrap(), 0, "{}", out.display());
    let comparison: Value = manifest::json_file(&out.join("comparison.json")).unwrap();
    assert_eq!(comparison["profile"], "fake");
    assert_eq!(comparison["equal_effective_budgets"], true);
    assert_eq!(comparison["gates"].as_object().unwrap().len(), 4);
    assert_eq!(
        comparison["paired_against_local"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        comparison["held_out_semantic_quality_gate"],
        "not_evaluated"
    );
    let mut controls = Vec::new();
    let mut summaries = BTreeMap::new();
    for name in ["none", "local", "dense_only", "hybrid"] {
        let cell = out.join(name);
        let before: report::Summary = manifest::json_file(&cell.join("metrics.json")).unwrap();
        assert_eq!(report::replay(&cell).unwrap().exit_code, 0);
        let after: report::Summary = manifest::json_file(&cell.join("metrics.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(&after).unwrap()
        );
        summaries.insert(name, after);
        let run_manifest: report::RunManifest =
            manifest::json_file(&cell.join("manifest.json")).unwrap();
        assert_eq!(run_manifest.measurement_profile, "fake");
        let observations: Vec<QueryObservation> =
            report::read_jsonl(&cell.join("profile-queries.jsonl")).unwrap();
        let witnesses: Vec<Value> = report::read_jsonl(&cell.join("lane-execution.jsonl")).unwrap();
        let costs: Vec<Value> = report::read_jsonl(&cell.join("strategy-costs.jsonl")).unwrap();
        assert_eq!(observations.len(), 9);
        assert_eq!(witnesses.len(), 9);
        assert_eq!(costs.len(), 9);
        assert!(costs.iter().all(|cost| cost["profile"] == "fake"
            && cost["provider_cost_units"].is_null()
            && cost["provider_cost_unknown"] == true
            && cost["cache_reuse"].is_null()
            && cost["cache_reuse_unknown"] == true));
        for (observation, saved_witness) in observations.iter().zip(&witnesses) {
            assert_eq!(observation.requested, RetrievalStrategy::Semantic);
            let raw: Value = manifest::json_file(&cell.join(&observation.raw_path)).unwrap();
            let witness = mechanism::lane_witness(&raw, name).unwrap();
            assert_eq!(saved_witness["witness"], witness);
            let lanes = witness["lane_receipts"].as_array().unwrap();
            let (hits, _) = normalizer::mcp(&raw).unwrap();
            let dense = matches!(name, "dense_only" | "hybrid");
            if dense {
                let semantic = lanes
                    .iter()
                    .find(|lane| lane["lane_id"] == "semantic")
                    .unwrap();
                assert_eq!(semantic["status"], "complete");
                assert!(semantic["candidate_count"].as_u64().unwrap() > 0);
                assert!(!hits.is_empty());
            }
            if name == "none" {
                assert!(lanes.is_empty());
                assert!(hits.is_empty(), "negative control returned a candidate");
            }
            if matches!(name, "local" | "hybrid") && observation.input.query != "zqxvplmnb" {
                assert!(lanes.iter().any(|lane| lane["lane_id"] == "exact_symbol"
                    && lane["status"] == "complete"
                    && lane["candidate_count"].as_u64().unwrap() > 0));
                let expected = if observation.input.query == "needle" {
                    "a.rs"
                } else {
                    "b.rs"
                };
                assert_eq!(hits[0].path, expected, "exact local control drifted");
            }
            if name == "local" && observation.input.query == "zqxvplmnb" {
                assert!(
                    hits.is_empty(),
                    "authored absent local token unexpectedly matched"
                );
            }
            if name == "hybrid" {
                assert!(
                    mechanism::lane_witness(&raw, "dense_only").is_err(),
                    "actual hybrid output was accepted under a false dense-only label"
                );
            }
        }
        controls.push(json!({"cell":name,"requests_including_warmup":observations.len(),
            "actual_lane_witnesses":witnesses.len(),"metrics_replayed":true,"positive_negative_controls_passed":true}));
    }
    for pair in comparison["paired_against_local"].as_array().unwrap() {
        let candidate = pair["candidate"].as_str().unwrap();
        assert_eq!(
            pair["observations"],
            reporting::paired_summary(
                &summaries["local"],
                &summaries[candidate],
                &queries,
                2,
                19,
                "fake"
            )
            .unwrap()
        );
        assert!(pair["observations"]["strata"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["split"] == "holdout"));
    }
    // Real built artifacts are also used for negative source/binary controls.
    let build_base = build_path.parent().unwrap();
    let mut aliased_source = build.clone();
    let hybrid = aliased_source
        .variants
        .iter()
        .find(|v| v.id == "hybrid")
        .unwrap()
        .source_root
        .clone();
    aliased_source
        .variants
        .iter_mut()
        .find(|v| v.id == "dense_only")
        .unwrap()
        .source_root = hybrid;
    assert!(mechanism::validate_build(&aliased_source, build_base, &suite_path).is_err());
    let mut aliased_binary = build.clone();
    let hybrid = aliased_binary
        .variants
        .iter()
        .find(|v| v.id == "hybrid")
        .unwrap()
        .binary
        .clone();
    aliased_binary
        .variants
        .iter_mut()
        .find(|v| v.id == "dense_only")
        .unwrap()
        .binary = hybrid;
    assert!(mechanism::validate_build(&aliased_binary, build_base, &suite_path).is_err());
    for query in &queries {
        let expected = format!("{:x}", Sha256::digest(query.query.as_bytes()));
        assert!(
            endpoint
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request["input_sha256"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|hash| hash == &expected)),
            "positive dense query control never reached the actual loopback provider"
        );
    }
    let result = json!({"spec":"p7-019-fake-mechanism-acceptance-v1","profile":"fake",
        "source_commit":build.source_commit,"passed":true,"cells":controls,
        "same_locked_inputs_and_effective_budgets":true,"all_36_requests_preserved":true,
        "actual_hybrid_mislabel_rejected":true,"aliased_source_rejected":true,"aliased_binary_rejected":true,
        "paired_bootstrap_replayed":true,"provider_query_positive_controls_observed":true,
        "cost_billing_and_cache_reuse":"unknown; never imputed as zero",
        "original_p7_019_fake_engineering_leg_complete":true,
        "held_out_semantic_quality_gate":"not_evaluated",
        "quality_interpretation":"none; fixed fake/authored holdout partition only, no real semantic benefit claim"});
    report::json(&evidence.join("acceptance.json"), &result).unwrap();
    println!("P7_019_MECHANISM {}", result);
}
