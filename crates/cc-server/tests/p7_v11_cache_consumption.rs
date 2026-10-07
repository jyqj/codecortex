//! V11 actual consumers: immutable per-engine config domains, graph windows,
//! and real worker/recall dense bypass. Synthetic local provider only.
#![cfg(feature = "semantic")]
use cc_model::{
    config::{ProjectConfig, RepoSizeTier},
    query::RetrievalStrategy,
    search::{SearchHit, SearchRequest},
    Language,
};
use serde_json::{json, Value};
use std::sync::Arc;
fn fixture() -> (tempfile::TempDir, cc_server::engine::CodeIndex) {
    fixture_with(30)
}
fn fixture_with(count: usize) -> (tempfile::TempDir, cc_server::engine::CodeIndex) {
    let root = tempfile::tempdir().unwrap();
    for n in 0..count {
        let file = root.path().join(format!("scope/needle_{n:02}.py"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, format!("def needle():\n    return {}\n", 731 + n)).unwrap();
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(false).unwrap();
    (root, index)
}
fn request() -> SearchRequest {
    SearchRequest {
        query: "needle".into(),
        top_k: 1,
        include_grep: true,
        retrieval_strategy: Some(RetrievalStrategy::Local),
        ..Default::default()
    }
}
fn projection(hits: &[SearchHit]) -> Value {
    json!(hits.iter().map(|h|json!({"path":h.file_path,"text":h.text,"score":h.rerank_score,"trace":h.score_trace})).collect::<Vec<_>>())
}
fn save(label: &str, value: Value) {
    if let Some(dir) = std::env::var_os("P7_CONSUMPTION_EVIDENCE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{}-{label}.json", std::process::id())),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    } else {
        println!("CONSUMPTION_JSON {value}");
    }
}
#[test]
fn immutable_config_domains_consume_cold_warm_and_fresh_per_field() {
    let (_root, index) = fixture();
    let db = index.index_db().unwrap().clone();
    let base = ProjectConfig::default();
    let engine = cc_search::SearchEngine::new(db.clone(), &base, None);
    let req = request();
    let limits = RepoSizeTier::Tiny.graph_enrich_limits();
    let original_graph = engine
        .search_context_candidates(&req, &limits, 4000)
        .unwrap();
    let original = engine.search(&req).unwrap();
    assert!(!original.is_empty());
    let mut rows = vec![];
    let inventory: &[(&str, &[&str])] = &[
        (
            "search",
            &[
                "lexical_top_k",
                "exact_symbol_top_k",
                "path_top_k",
                "grep_top_k",
                "grep_scan_cap",
                "rrf_k",
                "lexical_weight",
                "exact_symbol_weight",
                "path_weight",
                "grep_weight",
                "rerank_window",
                "graph_weight",
                "graph_top_k",
            ],
        ),
        (
            "ranking",
            &[
                "graph_rerank_weight",
                "overlap_weight",
                "symbol_exact_bonus",
                "path_prefix_bonus",
                "doc_file_bonus",
                "working_set_boost",
                "recent_file_boost",
                "pinned_context_boost",
                "overlay_neighbor_boost",
                "stage_a_weight",
                "stage_a_cap",
                "dsl_name_bonus",
                "preselect_working_set_floor",
                "preselect_working_set_scale",
                "preselect_recent_floor",
                "preselect_recent_scale",
                "preselect_pinned_floor",
                "preselect_pinned_scale",
                "preselect_overlay_floor",
                "preselect_overlay_scale",
                "preselect_fts_base",
                "preselect_symbol_exact_bonus",
                "preselect_symbol_fuzzy_bonus",
                "preselect_path_token_bonus",
                "preselect_graph_neighbor_base",
                "preselect_graph_edge_increment",
                "preselect_graph_accum_cap",
                "preselect_fallback_score",
                "preselect_explicit_scope_score",
                "graph_neighbor_decay",
                "graph_seed_exact_score",
                "graph_seed_fuzzy_score",
            ],
        ),
        (
            "query",
            &[
                "strategy",
                "deadline_ms",
                "lane_timeout_ms",
                "semantic_timeout_ms",
                "semantic_top_k",
            ],
        ),
    ];
    for (section, names) in inventory {
        let serialized = serde_json::to_value(&base).unwrap();
        let actual = serialized[*section]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual,
            names.iter().copied().collect(),
            "field inventory must be reviewed when config grows: {section}"
        );
        for field in *names {
            let mut changed = serialized.clone();
            let value = &mut changed[*section][*field];
            *value = if *field == "strategy" {
                json!("auto")
            } else if let Some(n) = value.as_u64() {
                json!(n + 7)
            } else {
                json!(value.as_f64().unwrap() + 0.17)
            };
            let mut input: ProjectConfig = serde_json::from_value(changed.clone()).unwrap();
            input.query.validate().unwrap();
            input.search.validate_retrieval().unwrap();
            let new_engine = cc_search::SearchEngine::new(db.clone(), &input, None);
            // Mutation of the caller-owned object after construction cannot change
            // captured config or either cache. A new engine has a fresh LRU domain.
            input = base.clone();
            assert_eq!(serde_json::to_value(input).unwrap(), serialized);
            let cold = new_engine.search(&req).unwrap();
            let warm = new_engine.search(&req).unwrap();
            assert!(!cold.is_empty(), "{section}.{field}");
            assert!(Arc::ptr_eq(&cold, &warm));
            assert!(!Arc::ptr_eq(&original, &cold));
            let stats = new_engine.cache_stats();
            assert_eq!((stats.result_misses, stats.result_hits), (1, 1));
            let fresh_cfg: ProjectConfig = serde_json::from_value(changed.clone()).unwrap();
            let fresh = cc_search::SearchEngine::new(db.clone(), &fresh_cfg, None)
                .search(&req)
                .unwrap();
            assert_eq!(projection(&warm), projection(&fresh), "{section}.{field}");
            assert!(
                Arc::ptr_eq(&original, &engine.search(&req).unwrap()),
                "old captured domain {section}.{field}"
            );
            let graph_cold = new_engine
                .search_context_candidates(&req, &limits, 4000)
                .unwrap();
            let graph_warm = new_engine
                .search_context_candidates(&req, &limits, 4000)
                .unwrap();
            assert!(Arc::ptr_eq(&graph_cold, &graph_warm));
            assert!(!Arc::ptr_eq(&original_graph, &graph_cold));
            let graph_fresh = cc_search::SearchEngine::new(db.clone(), &fresh_cfg, None)
                .search_context_candidates(&req, &limits, 4000)
                .unwrap();
            assert_eq!(
                projection(&graph_warm.0),
                projection(&graph_fresh.0),
                "graph {section}.{field}"
            );
            assert!(Arc::ptr_eq(
                &original_graph,
                &engine
                    .search_context_candidates(&req, &limits, 4000)
                    .unwrap()
            ));
            assert_eq!(
                (
                    new_engine.cache_stats().graph_misses,
                    new_engine.cache_stats().graph_hits
                ),
                (1, 1)
            );
            let mut policy_request = req.clone();
            policy_request.retrieval_strategy = None;
            let policy = cc_search::query_policy::QueryPolicy::resolve(
                &fresh_cfg.query,
                &policy_request,
                true,
            )
            .unwrap();
            assert_eq!(policy.requested, fresh_cfg.query.strategy);
            assert_eq!(policy.deadline_ms, fresh_cfg.query.deadline_ms);
            assert_eq!(
                policy.lane_timeout_ms,
                fresh_cfg
                    .query
                    .lane_timeout_ms
                    .min(fresh_cfg.query.deadline_ms)
            );
            assert_eq!(
                policy.semantic_timeout_ms,
                fresh_cfg
                    .query
                    .semantic_timeout_ms
                    .min(fresh_cfg.query.deadline_ms)
            );
            assert_eq!(policy.semantic_top_k, fresh_cfg.query.semantic_top_k);
            rows.push(json!({"field":format!("{section}.{field}"),"input":changed[*section][*field],"cold_misses":stats.result_misses,"warm_hits":stats.result_hits,"graph_cold_misses":new_engine.cache_stats().graph_misses,"graph_warm_hits":new_engine.cache_stats().graph_hits,"query_policy":policy,"fresh_equal":true,"old_domain_reused":true,"score_or_output_changed":projection(&original)!=projection(&cold),"coverage":"immutable domain and actual search consumer; individual score activation is not assumed"}));
        }
    }
    let (_tier_root, tier_index) = fixture_with(220);
    let tier_db = tier_index.index_db().unwrap().clone();
    for tier in [
        None,
        Some(RepoSizeTier::Tiny),
        Some(RepoSizeTier::Small),
        Some(RepoSizeTier::Medium),
        Some(RepoSizeTier::Large),
    ] {
        let e = cc_search::SearchEngine::new(tier_db.clone(), &base, tier);
        let mut r = req.clone();
        r.top_k = 0;
        r.boost_file_paths = Some(
            (0..220)
                .map(|n| format!("scope/needle_{n:02}.py"))
                .collect(),
        );
        let cold = e.search(&r).unwrap();
        let warm = e.search(&r).unwrap();
        assert!(Arc::ptr_eq(&cold, &warm));
        assert!(!cold.is_empty());
        let fresh = cc_search::SearchEngine::new(tier_db.clone(), &base, tier)
            .search(&r)
            .unwrap();
        assert_eq!(projection(&warm), projection(&fresh));
        let diagnostics = e.search_with_diagnostics(&r).unwrap();
        let preselected = diagnostics.scope.unwrap().soft.preselected_count;
        let expected = match tier {
            Some(RepoSizeTier::Medium) => 150,
            Some(RepoSizeTier::Large) => 200,
            _ => 120,
        };
        assert_eq!(
            preselected, expected,
            "actual preselection must consume {tier:?}"
        );
        assert_eq!(
            cold.len(),
            10,
            "raw SearchEngine zero-topk fallback is fixed ten, not public context tier default"
        );
        rows.push(json!({"tier":format!("{tier:?}"),"returned":cold.len(),"actual_preselected_count":preselected,"cold_misses":e.cache_stats().result_misses,"warm_hits":e.cache_stats().result_hits,"fresh_equal":true}));
    }
    save(
        "immutable-domains",
        json!({"level":"L2 actual parsed SQLite SearchEngine; no shared mutable config admitted","rows":rows}),
    );
}
#[test]
fn graph_window_and_hard_set_permutations_are_consumed_separately() {
    let (_root, index) = fixture();
    let db = index.index_db().unwrap().clone();
    let cfg = ProjectConfig::default();
    let engine = cc_search::SearchEngine::new(db.clone(), &cfg, None);
    let req = request();
    let limits = RepoSizeTier::Tiny.graph_enrich_limits();
    let top = engine
        .search_with_graph_context(&req, &limits, 4000)
        .unwrap();
    assert_eq!(top.0.len(), 1);
    let context = engine
        .search_context_candidates(&req, &limits, 4000)
        .unwrap();
    assert!(context.0.len() > top.0.len());
    assert!(!Arc::ptr_eq(&top, &context));
    assert!(Arc::ptr_eq(
        &top,
        &engine
            .search_with_graph_context(&req, &limits, 4000)
            .unwrap()
    ));
    assert!(Arc::ptr_eq(
        &context,
        &engine
            .search_context_candidates(&req, &limits, 4000)
            .unwrap()
    ));
    assert_eq!(
        (
            engine.cache_stats().graph_misses,
            engine.cache_stats().graph_hits
        ),
        (2, 2)
    );
    let mut two = req.clone();
    two.top_k = 2;
    let two_result = engine
        .search_with_graph_context(&two, &limits, 4000)
        .unwrap();
    assert_eq!(two_result.0.len(), 2);
    assert_eq!(engine.cache_stats().graph_misses, 3);
    let mut scoped = req.clone();
    scoped.top_k = 10;
    scoped.file_paths = Some(vec![
        "scope/needle_01.py".into(),
        "scope/needle_02.py".into(),
    ]);
    scoped.languages = Some(vec![Language::Python, Language::Rust]);
    let set = engine
        .search_context_candidates(&scoped, &limits, 4000)
        .unwrap();
    assert_eq!(set.0.len(), 2);
    scoped.file_paths.as_mut().unwrap().reverse();
    scoped.languages.as_mut().unwrap().reverse();
    let before = engine.cache_stats();
    let permuted = engine
        .search_context_candidates(&scoped, &limits, 4000)
        .unwrap();
    assert!(Arc::ptr_eq(&set, &permuted));
    assert_eq!(engine.cache_stats().graph_hits, before.graph_hits + 1);
    assert_eq!(engine.cache_stats().graph_misses, before.graph_misses);
    let fresh = cc_search::SearchEngine::new(db, &cfg, None)
        .search_context_candidates(&scoped, &limits, 4000)
        .unwrap();
    assert_eq!(projection(&permuted.0), projection(&fresh.0));
    save(
        "graph-extra",
        json!({"top_k_window":top.0.len(),"context_window":context.0.len(),"top_k_two":two_result.0.len(),"scoped_paths":projection(&set.0),"permutations_hit":true,"fresh_equal":true,"graph_hits":engine.cache_stats().graph_hits,"graph_misses":engine.cache_stats().graph_misses,"level":"L2 real SearchEngine public methods; actual output and cache identity"}),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_dense_publication_bypasses_result_caches_and_context_is_reassembled() {
    use cc_model::{
        query::QueryControl,
        retrieval::HardScope,
        semantic::{SemanticRecall, SemanticRequest, SemanticResponse},
        Intent,
    };
    use cc_semantic::{
        ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
        types::VectorSpace,
    };
    use cc_server::{
        semantic_query_encoding::{
            NetworkLimits, QueryEncodingInputs, QueryEncodingService, QueryNetworkCapacity,
        },
        service_factory::QueryServices,
    };
    use std::time::Duration;
    struct Provider(VectorSpace);
    impl EmbeddingProvider for Provider {
        fn space(&self) -> &VectorSpace {
            &self.0
        }
        fn embed_documents(
            &self,
            inputs: &[DocumentInput],
        ) -> Result<Vec<Vec<f32>>, ProviderError> {
            Ok(inputs.iter().map(|_| vec![1.0, 0.0]).collect())
        }
        fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            Ok(inputs.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }
    let (root, index) = fixture();
    let db = index.index_db().unwrap().clone();
    let mut cfg = ProjectConfig::default();
    cfg.indexing.db_read_pool_size = Some(1);
    cfg.semantic.enabled = true;
    cfg.semantic.model_id = "fake/cache-consumption".into();
    cfg.semantic.dimensions = Some(2);
    cfg.semantic.max_input_tokens = Some(8192);
    cfg.semantic.max_batch_items = Some(16);
    cfg.semantic.endpoint = "https://semantic.invalid/v1".into();
    let subsystem = Arc::new(
        cc_server::semantic_wiring::assemble_with(
            &root.path().to_string_lossy(),
            &cfg,
            db.clone(),
            |key| {
                (key == cc_semantic::cache::CACHE_ROOT_ENV)
                    .then(|| root.path().join("cache").to_string_lossy().into_owned())
            },
            false,
        )
        .unwrap()
        .unwrap(),
    );
    let services = Arc::new(QueryServices::default());
    services.set_semantic(Some(subsystem.recall.clone()));
    let provider = Arc::new(Provider(subsystem.space.clone()));
    let worker = cc_server::semantic_runtime::SemanticRuntime::new(
        db.clone(),
        subsystem.clone(),
        services,
        provider.clone(),
    )
    .unwrap();
    let engine = cc_search::SearchEngine::new(db.clone(), &cfg, None);
    let req = request();
    let limits = RepoSizeTier::Tiny.graph_enrich_limits();
    let local = engine.search(&req).unwrap();
    let graph = engine
        .search_context_candidates(&req, &limits, 4000)
        .unwrap();
    let handle = index.query_handle().unwrap();
    let old_context = handle
        .search_in_context_with("needle", 1, Some(Intent::Locate), req.clone())
        .unwrap();
    let old_warm = handle
        .search_in_context_with("needle", 1, Some(Intent::Locate), req.clone())
        .unwrap();
    assert_eq!(old_context.rendered_prompt, old_warm.rendered_prompt);
    let before = db.reads().read_generation().unwrap();
    assert!(worker.schedule());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let c = db.reads().semantic_coverage().unwrap().coverage;
            if c.eligible > 0 && c.uncovered == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let after = db.reads().read_generation().unwrap();
    assert_eq!(before.incarnation, after.incarnation);
    assert_eq!(before.index_epoch, after.index_epoch);
    assert_eq!(before.evidence_epoch, after.evidence_epoch);
    assert_ne!(before.semantic_epoch, after.semantic_epoch);
    assert!(Arc::ptr_eq(&local, &engine.search(&req).unwrap()));
    assert!(Arc::ptr_eq(
        &graph,
        &engine
            .search_context_candidates(&req, &limits, 4000)
            .unwrap()
    ));
    let public_before = index.diagnostics_info()["search_cache"].clone();
    let current = handle
        .search_in_context_with("needle", 1, Some(Intent::Locate), req.clone())
        .unwrap();
    let public_after = index.diagnostics_info()["search_cache"].clone();
    assert_eq!(
        public_after["graph_hits"].as_u64().unwrap(),
        public_before["graph_hits"].as_u64().unwrap() + 1
    );
    assert_eq!(
        current.evidence_summary["source_freshness"]["generation"],
        serde_json::to_value(after).unwrap()
    );
    assert_ne!(
        old_context.evidence_summary["source_freshness"]["generation"],
        current.evidence_summary["source_freshness"]["generation"]
    );
    assert!(!current.spans.is_empty());
    assert!(current.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["text"].as_str().is_some_and(|s| s.contains("return"))));
    let encoder = QueryEncodingService::new(
        QueryEncodingInputs {
            cache: subsystem.query_cache.clone(),
            namespace: subsystem.namespace.clone(),
            spec: subsystem.query_spec.clone(),
            budget: cc_semantic::admission::InputBudget::validated(1, 1024, 8192).unwrap(),
            lifecycle: Arc::new(cc_db::semantic_publish::LifecycleFence::default()),
        },
        QueryNetworkCapacity::new(NetworkLimits {
            query_running: 1,
            query_queued: 0,
            background_running: 1,
            background_queued: 0,
        })
        .unwrap(),
        move |_| Ok(provider.clone()),
    )
    .unwrap();
    encoder
        .encode(
            b"needle".to_vec(),
            QueryControl::new(Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap();
    let outcome = subsystem
        .recall
        .recall(
            SemanticRequest {
                query: "needle".into(),
                scope: HardScope::default(),
                limit: 24,
                policy_fingerprint: "cache-consumption-v1".into(),
                generation: after,
            },
            QueryControl::new(Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap();
    assert!(!outcome.candidates.is_empty());
    let mut dense = req.clone();
    dense.retrieval_strategy = Some(RetrievalStrategy::Auto);
    dense.semantic = Some(Arc::new(SemanticResponse {
        generation: after,
        outcome,
    }));
    let stats = engine.cache_stats();
    for _ in 0..2 {
        let hits = engine.search(&dense).unwrap();
        assert!(!hits.is_empty());
        let context = engine
            .search_context_candidates(&dense, &limits, 4000)
            .unwrap();
        assert!(!context.0.is_empty());
        let fresh = cc_search::SearchEngine::new(db.clone(), &cfg, None)
            .search(&dense)
            .unwrap();
        assert_eq!(projection(&hits), projection(&fresh));
    }
    let dense_stats = engine.cache_stats();
    assert_eq!(dense_stats.result_hits, stats.result_hits);
    assert_eq!(dense_stats.graph_hits, stats.graph_hits);
    assert_eq!(dense_stats.result_misses, stats.result_misses + 2);
    assert_eq!(dense_stats.graph_misses, stats.graph_misses + 2);
    index.set_semantic_recall(Some(subsystem.recall.clone()));
    let mut auto = req.clone();
    auto.retrieval_strategy = Some(RetrievalStrategy::Auto);
    let start = index.diagnostics_info()["search_cache"].clone();
    let first = handle
        .search_async("needle".into(), 1, Some(Intent::Locate), auto.clone())
        .await
        .unwrap();
    let repeat = handle
        .search_async("needle".into(), 1, Some(Intent::Locate), auto.clone())
        .await
        .unwrap();
    assert_eq!(first.rendered_prompt, repeat.rendered_prompt);
    assert_eq!(first.machine_pack["hits"], repeat.machine_pack["hits"]);
    // The bounded packer may project large lane details into explicit receipts.
    // Require real nonzero semantic candidates in either admitted wire shape.
    let retrieval = &first.evidence_summary["retrieval"];
    let lanes = retrieval["lanes"]
        .as_array()
        .or_else(|| retrieval["lane_receipts"].as_array())
        .unwrap();
    assert!(
        lanes
            .iter()
            .any(|lane| lane["lane_id"] == "semantic"
                && lane["candidate_count"].as_u64().unwrap() > 0)
    );
    let finish = index.diagnostics_info()["search_cache"].clone();
    assert_eq!(finish["graph_hits"], start["graph_hits"]);
    assert_eq!(
        finish["graph_misses"].as_u64().unwrap(),
        start["graph_misses"].as_u64().unwrap() + 2
    );
    let changed = handle
        .search_async("needle".into(), 2, Some(Intent::Fix), auto)
        .await
        .unwrap();
    assert_eq!(changed.intent, Intent::Fix);
    assert_eq!(changed.machine_pack["top_k"], 2);
    assert!(
        changed.machine_pack["hits"].as_array().unwrap().len()
            > first.machine_pack["hits"].as_array().unwrap().len()
    );
    save(
        "dense-context",
        json!({"level":"L2 actual worker/query encoder/recall and public QueryHandle; synthetic provider; not stdio model-factory validation","generation_before":before,"generation_after":after,"public_local_cache_before":public_before,"public_local_cache_after":public_after,"dense_result_miss_delta":dense_stats.result_misses-stats.result_misses,"dense_graph_miss_delta":dense_stats.graph_misses-stats.graph_misses,"public_dense_before":start,"public_dense_after":finish,"context_before":old_context,"context_after":current,"dense_context":first,"changed_intent_top_k":changed}),
    );
    worker.close();
}
