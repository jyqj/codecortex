//! Protocol controls for P7-019. Synthetic responses prove harness behavior,
//! never dense quality; the ignored test requires an explicitly pinned product.
use cc_eval::benchmark::{
    ablation::strategy::{
        compare_budgets, compare_population, read_observations, reporting, response_budget,
        status_is_ready, validate_configuration, Network, Plan, ProfileBackend, PublicRpc,
        ReadyPolicy,
    },
    adapters::{mcp_stdio::McpStdio, Backend},
    manifest, normalizer, report,
    schema::{Query, ScoreProfile, SearchInput, SourceLock, Suite},
    BenchError, Result,
};
use cc_model::query::{QueryConfig, RetrievalStrategy};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

fn ready_policy(dense: bool) -> ReadyPolicy {
    ReadyPolicy {
        expected_space: dense.then(|| "a".repeat(64)),
        timeout_ms: 1_000,
        poll_interval_ms: 10,
        max_polls: 2,
    }
}
fn input() -> SearchInput {
    SearchInput {
        query: "needle".into(),
        top_k: 1,
        path_prefix: None,
    }
}
fn suite(config: Value) -> Suite {
    Suite {
        schema_version: 1,
        name: "mechanism-only".into(),
        source: SourceLock {
            root: PathBuf::from("."),
            commit: None,
            digest: "unused".into(),
            files: vec!["a.rs".into()],
        },
        queries: PathBuf::from("queries.jsonl"),
        queries_digest: "unused".into(),
        scoring: ScoreProfile::Native,
        repetitions: 1,
        warmup: 0,
        seed: 5,
        timeout_ms: 1_000,
        top_k: 1,
        engine_config: config,
    }
}
fn plan(dense: bool) -> Plan {
    Plan {
        schema_version: 1,
        suite: PathBuf::from("suite.json"),
        source_snapshot: PathBuf::from("source"),
        binary: PathBuf::from("codecortex"),
        build_receipt: PathBuf::from("build.json"),
        strategies: if dense {
            vec![
                RetrievalStrategy::Local,
                RetrievalStrategy::Auto,
                RetrievalStrategy::Semantic,
            ]
        } else {
            vec![RetrievalStrategy::Local, RetrievalStrategy::Auto]
        },
        network: if dense {
            Network::LoopbackOnly
        } else {
            Network::Disabled
        },
        readiness: ready_policy(dense),
    }
}
fn status() -> Value {
    json!({"indexed_files":1,"retrieval":{"spec":"retrieval-capabilities-v2",
        "consistency":"point_in_time","generation_scope":"observed_database_snapshot",
        "identity_validation":"checked_at_observation_boundary","local_state":"available",
        "semantic_active_space":"a".repeat(64),"semantic_state":"ready","dense_state":"ready",
        "semantic_pending":0,"semantic_failed":0,"dense_desired":2,"dense_published":2}})
}
fn account(value: &mut Value) {
    for _ in 0..8 {
        let bytes = serde_json::to_vec(value).unwrap().len();
        value["token_estimate"] = json!(bytes.div_ceil(4));
        value["evidence_summary"]["packing"]["used_bytes"] = json!(bytes);
        if serde_json::to_vec(value).unwrap().len() == bytes {
            return;
        }
    }
    panic!("test accounting failed to converge")
}
fn response(requested: &str, effective: &str) -> Value {
    let mut raw = json!({"nodes":[],"machine_pack":{"hits":[]},"token_budget":4_000,
        "token_estimate":0,"evidence_summary":{
        "packing":{"spec":cc_model::context::CONTEXT_PACKING_SPEC,"configured_max_bytes":18_000,
            "limit_bytes":16_000,"used_bytes":0,"partial":false},
        "retrieval":{"policy":{"version":"synthetic-control-v1","requested":requested,
            "effective":effective,"deadline_ms":30_000,"lane_timeout_ms":20_000,
            "semantic_timeout_ms":5_000,"semantic_top_k":24},"lanes":[],
            "scope":{"schema_version":1,"policy":"synthetic-scope-v1",
                "budget":{"top_k":1,"exact_symbol_candidates":24,"path_candidates":24,
                    "lexical_candidates":24,"grep_candidates":12,"grep_scan_cap":20_000,
                    "rerank_window":40,"grep_enabled":true,"units":"synthetic control"},
                "hard":{"path_prefix":null,"path_prefix_truncated":false,"languages":null,
                    "explicit_file_count":null,"empty":false,"semantics":"synthetic control"}}}}});
    if effective != "local" {
        raw["evidence_summary"]["retrieval"]["lanes"] = json!([{
            "schema_version":1,"lane_id":"semantic","weight":1.0,"status":"complete",
            "elapsed_us":0,"candidate_count":0,"coverage":{"scope":"hard_scope",
                "complete":true,"examined":0,"total_lower_bound":0},
            "truncation_reason":null,"candidates":[]}]);
    }
    account(&mut raw);
    raw
}

#[test]
fn configuration_rejects_implicit_network_remote_endpoint_and_fake_dense_name() {
    let local = suite(json!({"auto_index":{"enabled":false}}));
    validate_configuration(&plan(false), &local).unwrap();
    let mut dense = suite(json!({"auto_index":{"enabled":false},"semantic":{
        "enabled":true,"network_opt_in":true,"allow_http":true,
        "endpoint":"http://127.0.0.1:12345/v1"}}));
    validate_configuration(&plan(true), &dense).unwrap();
    assert!(validate_configuration(&plan(false), &dense).is_err());
    for endpoint in [
        "https://example.test/v1",
        "http://localhost:12345/v1",
        "http://192.0.2.1:12345/v1",
        "http://user@127.0.0.1:12345/v1",
        "http://127.0.0.1:12345/v1?redirect=elsewhere",
    ] {
        dense.engine_config["semantic"]["endpoint"] = json!(endpoint);
        assert!(
            validate_configuration(&plan(true), &dense).is_err(),
            "{endpoint}"
        );
    }
    let mut bad = serde_json::to_value(plan(false)).unwrap();
    bad["strategies"] = json!(["local", "dense_only"]);
    assert!(serde_json::from_value::<Plan>(bad).is_err());
    let mut duplicate = plan(false);
    duplicate.strategies = vec![RetrievalStrategy::Local, RetrievalStrategy::Local];
    assert!(validate_configuration(&duplicate, &local).is_err());
}

#[test]
fn readiness_requires_checked_space_and_real_dense_coverage_not_file_count() {
    let ready = status();
    assert!(status_is_ready(&ready, 1, &ready_policy(true)).unwrap());
    for field in ["dense_published", "dense_desired"] {
        let mut incomplete = ready.clone();
        incomplete["retrieval"][field] = json!(0);
        assert_ne!(
            status_is_ready(&incomplete, 1, &ready_policy(true)).ok(),
            Some(true)
        );
    }
    let mut pending = ready.clone();
    pending["retrieval"]["semantic_pending"] = json!(1);
    pending["retrieval"]["semantic_state"] = json!("backfilling");
    assert!(!status_is_ready(&pending, 1, &ready_policy(true)).unwrap());
    assert!(status_is_ready(&pending, 1, &ready_policy(false)).unwrap());
    for (field, value) in [
        ("semantic_failed", json!(1)),
        ("semantic_active_space", json!("b".repeat(64))),
        ("identity_validation", json!("not_observed")),
    ] {
        let mut wrong = ready.clone();
        wrong["retrieval"][field] = value;
        assert!(status_is_ready(&wrong, 1, &ready_policy(true)).is_err());
    }
}

#[test]
fn public_budget_checks_policy_final_bytes_and_numeric_scope_without_rewriting_partial() {
    let raw = response("local", "local");
    let expected = response_budget(
        &raw,
        &input(),
        RetrievalStrategy::Local,
        &QueryConfig::default(),
        false,
    )
    .unwrap();
    for pointer in [
        "/evidence_summary/retrieval/policy/deadline_ms",
        "/evidence_summary/retrieval/scope/budget/top_k",
        "/evidence_summary/packing/used_bytes",
        "/token_estimate",
    ] {
        let mut wrong = raw.clone();
        *wrong.pointer_mut(pointer).unwrap() = json!(999);
        assert!(
            response_budget(
                &wrong,
                &input(),
                RetrievalStrategy::Local,
                &QueryConfig::default(),
                false
            )
            .is_err(),
            "{pointer}"
        );
    }
    let mut compact = raw.clone();
    compact["evidence_summary"]["retrieval"]["scope"]["budget"]["units"] = json!("compact prose");
    compact["evidence_summary"]["retrieval"]["scope"]["hard"]["semantics"] = json!("compact prose");
    account(&mut compact);
    assert_eq!(
        expected,
        response_budget(
            &compact,
            &input(),
            RetrievalStrategy::Local,
            &QueryConfig::default(),
            false
        )
        .unwrap()
    );
    let mut partial = response("semantic", "semantic");
    let lane = &mut partial["evidence_summary"]["retrieval"]["lanes"][0];
    lane["status"] = json!("partial");
    lane["coverage"]["complete"] = json!(false);
    lane["truncation_reason"] = json!("controlled incomplete coverage");
    account(&mut partial);
    let before = partial.clone();
    response_budget(
        &partial,
        &input(),
        RetrievalStrategy::Semantic,
        &QueryConfig::default(),
        true,
    )
    .unwrap();
    assert_eq!(partial, before);
    assert_eq!(
        normalizer::mcp(&partial).unwrap().1,
        cc_eval::benchmark::schema::ResultStatus::Partial
    );
    partial["evidence_summary"]["retrieval"]["lanes"] = json!([]);
    account(&mut partial);
    assert!(response_budget(
        &partial,
        &input(),
        RetrievalStrategy::Semantic,
        &QueryConfig::default(),
        true
    )
    .is_err());
}

#[derive(Default)]
struct Calls {
    requests: Vec<(String, Value)>,
    closed: usize,
}
struct Stub {
    calls: Arc<Mutex<Calls>>,
    status: VecDeque<Value>,
    search: VecDeque<Value>,
}
impl PublicRpc for Stub {
    async fn call(&mut self, tool: &str, args: Value) -> Result<Value> {
        self.calls
            .lock()
            .unwrap()
            .requests
            .push((tool.into(), args));
        match tool {
            "index" => Ok(json!({"indexed":1})),
            "status" => self
                .status
                .pop_front()
                .ok_or_else(|| BenchError::Protocol("unexpected status".into())),
            "search" => self
                .search
                .pop_front()
                .ok_or_else(|| BenchError::Protocol("unexpected search".into())),
            _ => Err(BenchError::Protocol("unexpected tool".into())),
        }
    }
    async fn close(&mut self) -> Result<()> {
        self.calls.lock().unwrap().closed += 1;
        Ok(())
    }
}
fn stub(raw: Value) -> (Stub, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    (
        Stub {
            calls: calls.clone(),
            status: VecDeque::new(),
            search: VecDeque::from([raw]),
        },
        calls,
    )
}

#[tokio::test]
async fn strategy_rejection_keeps_original_public_response_and_replay_cannot_self_authorize() {
    let out = tempfile::tempdir().unwrap();
    let raw = response("local", "local"); // Wrong policy for the requested auto cell.
    let (client, calls) = stub(raw.clone());
    let mut backend = ProfileBackend::new(
        client,
        out.path().to_path_buf(),
        out.path().to_path_buf(),
        RetrievalStrategy::Auto,
        QueryConfig::default(),
        ready_policy(false),
    )
    .unwrap();
    assert!(backend.search(&input()).await.is_err());
    let requests = &calls.lock().unwrap().requests;
    assert_eq!(requests[0].0, "search");
    assert_eq!(
        requests[0].1,
        json!({"query":"needle","top_k":1,"mode":"hybrid","retrieval_strategy":"auto"})
    );
    let saved: Value =
        serde_json::from_slice(&std::fs::read(out.path().join("profile-raw/000000.json")).unwrap())
            .unwrap();
    assert_eq!(saved, raw);
    let mut rows = read_observations(
        out.path(),
        RetrievalStrategy::Auto,
        &QueryConfig::default(),
        false,
    )
    .unwrap();
    assert!(rows[0].error.is_some());
    rows[0].error = None;
    report::jsonl(&out.path().join("profile-queries.jsonl"), &rows).unwrap();
    assert!(read_observations(
        out.path(),
        RetrievalStrategy::Auto,
        &QueryConfig::default(),
        false
    )
    .is_err());
}

#[tokio::test]
async fn equal_top_k_does_not_hide_different_output_cap_or_hard_scope() {
    let mut cells = BTreeMap::new();
    for (name, strategy) in [
        ("local", RetrievalStrategy::Local),
        ("auto", RetrievalStrategy::Auto),
    ] {
        let out = tempfile::tempdir().unwrap();
        let (client, _) = stub(response(name, "local"));
        let mut backend = ProfileBackend::new(
            client,
            out.path().to_path_buf(),
            out.path().to_path_buf(),
            strategy,
            QueryConfig::default(),
            ready_policy(false),
        )
        .unwrap();
        backend.search(&input()).await.unwrap();
        cells.insert(
            name.to_string(),
            read_observations(out.path(), strategy, &QueryConfig::default(), false).unwrap(),
        );
    }
    compare_budgets(&cells).unwrap();
    compare_population(&cells, &[input()], 1).unwrap();
    // Two equally truncated profiles must not pass against the locked suite.
    assert!(compare_population(&cells, &[input()], 2).is_err());
    let costs = reporting::cost_columns(&cells["local"], "fake");
    assert_eq!(costs["provider_cost_units"], Value::Null);
    assert_eq!(costs["provider_cost_unknown"], 1);
    assert_eq!(costs["cache_reuse"], Value::Null);
    assert_eq!(costs["cache_reuse_unknown"], 1);
    let original = cells.clone();
    cells.get_mut("auto").unwrap()[0]
        .budget
        .as_mut()
        .unwrap()
        .configured_max_bytes += 1;
    assert!(compare_budgets(&cells).is_err());
    cells = original.clone();
    cells.get_mut("auto").unwrap()[0]
        .budget
        .as_mut()
        .unwrap()
        .hard_scope["path_prefix"] = json!("elsewhere");
    assert!(compare_budgets(&cells).is_err());
    cells = original;
    cells.get_mut("auto").unwrap().clear();
    assert!(compare_budgets(&cells).is_err());
}

#[test]
fn paired_effects_use_family_units_and_do_not_label_fake_scores_as_quality() {
    let queries: Vec<Query> = [
        ("a", "same-family"),
        ("b", "same-family"),
        ("c", "other-family"),
    ]
    .into_iter()
    .map(|(id, family)| {
        serde_json::from_value(json!({
            "id":id,"category":"synthetic-control","difficulty":1,"language":"rust",
            "split":"dev","query_family":family,"query":"fixture","path_prefix":null,
            "no_answer":false,"expected_files":[],"answers":[],"annotations":{}
        }))
        .unwrap()
    })
    .collect();
    let make = |value| report::Summary {
        queries: 3,
        measured_rows: 6,
        mean_top1: value,
        mean_ndcg10: value,
        cases: queries
            .iter()
            .map(|query| report::CaseScore {
                id: query.id.clone(),
                category: query.category.clone(),
                family: query.query_family.clone(),
                repetitions: 2,
                top1: value,
                ndcg10: value,
            })
            .collect(),
        category_means: BTreeMap::new(),
        family_ndcg_ci: None,
        unverified_hits: 0,
        invalid_hits: 0,
    };
    let baseline = make(0.25);
    let mut candidate = make(0.5);
    candidate.cases.reverse(); // Pair by identity, not vector position.
    let paired = reporting::paired_summary(&baseline, &candidate, &queries, 2, 19, "fake").unwrap();
    assert_eq!(paired["profile"], "fake");
    let ci = &paired["family_ndcg_delta_ci"];
    assert_eq!(ci["independent_units"], 2); // Not 3 translations or 6 requests.
    for key in ["mean", "low", "high"] {
        assert_eq!(ci[key], 0.25);
    }
    assert_eq!(
        paired,
        reporting::paired_summary(&baseline, &candidate, &queries, 2, 19, "fake").unwrap()
    );
    assert!(paired["quality_gate"]
        .as_str()
        .unwrap()
        .starts_with("not_evaluated"));
    candidate.cases[0].repetitions = 1;
    assert!(reporting::paired_summary(&baseline, &candidate, &queries, 2, 19, "fake").is_err());
    candidate.cases.pop();
    assert!(reporting::paired_summary(&baseline, &candidate, &queries, 2, 19, "fake").is_err());
}

#[tokio::test]
async fn pending_dense_readiness_is_bounded_and_close_still_releases_client() {
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("a.rs"), "pub fn needle() {}\n").unwrap();
    let files = manifest::inventory(out.path(), &["a.rs".into()]).unwrap();
    let mut pending = status();
    pending["retrieval"]["dense_published"] = json!(1);
    pending["retrieval"]["dense_state"] = json!("partial");
    let (mut client, calls) = stub(response("local", "local"));
    client.status = VecDeque::from([pending.clone(), pending.clone(), pending]);
    let mut backend = ProfileBackend::new(
        client,
        out.path().to_path_buf(),
        out.path().to_path_buf(),
        RetrievalStrategy::Local,
        QueryConfig::default(),
        ready_policy(true),
    )
    .unwrap();
    assert!(backend.readiness(&files).await.is_err());
    assert_eq!(calls.lock().unwrap().requests.len(), 2);
    assert!(backend.close().await.is_err());
    assert_eq!(calls.lock().unwrap().closed, 1);
}

#[tokio::test]
#[ignore = "requires explicit current product path and SHA256; mechanism smoke, no quality certification"]
async fn actual_stdio_local_and_auto_have_equal_effective_budget_and_verified_source() {
    let binary = PathBuf::from(std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit product"));
    let expected = std::env::var("P7_STRATEGY_BINARY_SHA256").expect("explicit product digest");
    assert_eq!(
        format!("{:x}", Sha256::digest(std::fs::read(&binary).unwrap())),
        expected
    );
    let mut cells = BTreeMap::new();
    for (name, strategy) in [
        ("local", RetrievalStrategy::Local),
        ("auto", RetrievalStrategy::Auto),
    ] {
        let project = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("a.rs"),
            "pub fn needle() -> i32 { 7 }\n",
        )
        .unwrap();
        std::fs::write(
            project.path().join(".codecortex.json"),
            "{\"auto_index\":{\"enabled\":false}}",
        )
        .unwrap();
        let files = manifest::inventory(project.path(), &["a.rs".into()]).unwrap();
        let client = McpStdio::spawn(&binary, project.path(), Duration::from_secs(15))
            .await
            .unwrap();
        let mut backend = ProfileBackend::new(
            client,
            project.path().to_path_buf(),
            out.path().to_path_buf(),
            strategy,
            QueryConfig::default(),
            ready_policy(false),
        )
        .unwrap();
        backend.prepare(&files).await.unwrap();
        backend.readiness(&files).await.unwrap();
        let raw = backend.search(&input()).await.unwrap();
        let (mut hits, _) = normalizer::mcp(&raw).unwrap();
        assert!(!hits.is_empty());
        for hit in &mut hits {
            normalizer::verify_source(hit, project.path()).unwrap();
            assert_eq!(hit.evidence_valid, Some(true));
        }
        backend.close().await.unwrap();
        cells.insert(
            name.to_string(),
            read_observations(out.path(), strategy, &QueryConfig::default(), false).unwrap(),
        );
    }
    compare_budgets(&cells).unwrap();
}
