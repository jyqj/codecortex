//! Independent literal/analytic oracles for PR61. Synthetic, offline, no gold.
#![cfg(feature = "semantic")]
use cc_model::{config::ProjectConfig, query::RetrievalStrategy, search::SearchRequest, Intent};
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc, time::Duration};

fn fixture() -> (tempfile::TempDir, cc_server::engine::CodeIndex) {
    let root = tempfile::tempdir().unwrap();
    for (name, number) in [("a", 111), ("b", 222), ("z", 999)] {
        std::fs::write(
            root.path().join(format!("{name}.py")),
            format!("def needle():\n    return {number}\n"),
        )
        .unwrap();
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
        top_k: 3,
        include_grep: true,
        retrieval_strategy: Some(RetrievalStrategy::Local),
        ..Default::default()
    }
}
fn save(name: &str, value: Value) {
    if let Some(dir) = std::env::var_os("V11_REVIEW_OUTPUT") {
        std::fs::write(
            std::path::PathBuf::from(dir).join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn literal_graph_scope_and_analytic_captured_ranking() {
    let (_root, index) = fixture();
    let db = index.index_db().unwrap().clone();
    let base = ProjectConfig::default();
    let original = cc_search::SearchEngine::new(db.clone(), &base, None);
    let req = request();
    let old = original.search(&req).unwrap();
    assert_eq!(
        old.iter()
            .map(|h| h.file_path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a.py", "b.py", "z.py"])
    );
    let mut config = base.clone();
    config.ranking.symbol_exact_bonus += 2.0;
    let changed = cc_search::SearchEngine::new(db.clone(), &config, None);
    config.ranking.symbol_exact_bonus += 9.0;
    let cold = changed.search(&req).unwrap();
    let warm = changed.search(&req).unwrap();
    assert!(Arc::ptr_eq(&cold, &warm));
    for hit in warm.iter() {
        let previous = old.iter().find(|h| h.chunk_id == hit.chunk_id).unwrap();
        assert_eq!(hit.symbol_name.as_deref(), Some("needle"));
        assert!((hit.rerank_score - previous.rerank_score - 2.0).abs() < 1e-12);
        assert_eq!(
            hit.text,
            std::fs::read_to_string(_root.path().join(&hit.file_path)).unwrap()
        );
    }
    assert!(Arc::ptr_eq(&old, &original.search(&req).unwrap()));
    let mut scoped = request();
    scoped.file_paths = Some(vec!["z.py".into(), "a.py".into()]);
    let limits = cc_model::config::RepoSizeTier::Tiny.graph_enrich_limits();
    let scoped_cold = changed
        .search_context_candidates(&scoped, &limits, 4000)
        .unwrap();
    assert_eq!(
        scoped_cold
            .0
            .iter()
            .map(|h| h.file_path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a.py", "z.py"])
    );
    scoped.file_paths.as_mut().unwrap().reverse();
    let scoped_warm = changed
        .search_context_candidates(&scoped, &limits, 4000)
        .unwrap();
    assert!(Arc::ptr_eq(&scoped_cold, &scoped_warm));
    scoped.top_k = 1;
    let one = changed
        .search_with_graph_context(&scoped, &limits, 4000)
        .unwrap();
    assert_eq!(one.0.len(), 1);
    assert!(!Arc::ptr_eq(&one, &scoped_warm));
    save(
        "analytic-graph",
        json!({"score_delta":2,"paths":["a.py","z.py"],"top_k_one":one.0,"warm":warm,"stats":format!("{:?}",changed.cache_stats())}),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_envelope_matches_reopened_cold_domain_and_obeys_real_budgets() {
    use cc_semantic::{
        ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
        types::VectorSpace,
    };
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
    let handle = index.query_handle().unwrap();
    let old = handle
        .search_async("needle".into(), 3, Some(Intent::Locate), request())
        .await
        .unwrap();
    let old_warm = handle
        .search_async("needle".into(), 3, Some(Intent::Locate), request())
        .await
        .unwrap();
    assert_eq!(old.machine_pack["hits"], old_warm.machine_pack["hits"]);
    let mut cfg = ProjectConfig::default();
    cfg.semantic.enabled = true;
    cfg.semantic.model_id = "fake/independent-envelope".into();
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
    let provider = Arc::new(Provider(subsystem.space.clone()));
    let worker = cc_server::semantic_runtime::SemanticRuntime::new(
        db.clone(),
        subsystem.clone(),
        Arc::new(cc_server::service_factory::QueryServices::default()),
        provider.clone(),
    )
    .unwrap();
    let before = db.reads().read_generation().unwrap();
    assert!(worker.schedule());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let coverage = db.reads().semantic_coverage().unwrap().coverage;
            if coverage.eligible > 0 && coverage.uncovered == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let after = db.reads().read_generation().unwrap();
    assert_eq!(
        (
            before.incarnation,
            before.index_epoch,
            before.evidence_epoch
        ),
        (after.incarnation, after.index_epoch, after.evidence_epoch)
    );
    assert_ne!(before.semantic_epoch, after.semantic_epoch);
    let current = handle
        .search_async("needle".into(), 3, Some(Intent::Locate), request())
        .await
        .unwrap();
    let fresh_index = cc_server::engine::CodeIndex::new(Some(root.path())).unwrap();
    let fresh = fresh_index
        .query_handle()
        .unwrap()
        .search_async("needle".into(), 3, Some(Intent::Locate), request())
        .await
        .unwrap();
    assert_eq!(current.rendered_prompt, fresh.rendered_prompt);
    assert_eq!(current.machine_pack["hits"], fresh.machine_pack["hits"]);
    assert_eq!(
        serde_json::to_value(&current.spans).unwrap(),
        serde_json::to_value(&fresh.spans).unwrap()
    );
    assert_eq!(
        current.evidence_summary["selection"],
        fresh.evidence_summary["selection"]
    );
    assert_eq!(
        current.evidence_summary["source_freshness"]["generation"],
        serde_json::to_value(after).unwrap()
    );
    assert_ne!(
        old.evidence_summary["source_freshness"]["generation"],
        current.evidence_summary["source_freshness"]["generation"]
    );
    let hits = current.machine_pack["hits"].as_array().unwrap();
    assert_eq!(
        hits.iter()
            .map(|h| h["file_path"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a.py", "b.py", "z.py"])
    );
    for h in hits {
        let path = h["file_path"].as_str().unwrap();
        assert_eq!(
            h["text"].as_str().unwrap(),
            std::fs::read_to_string(root.path().join(path)).unwrap()
        );
    }
    use cc_server::semantic_query_encoding::{
        NetworkLimits, QueryEncodingInputs, QueryEncodingService, QueryNetworkCapacity,
    };
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
            cc_model::query::QueryControl::new(Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap();
    index.set_semantic_recall(Some(subsystem.recall.clone()));
    fresh_index.set_semantic_recall(Some(subsystem.recall.clone()));
    let mut auto = request();
    auto.retrieval_strategy = Some(RetrievalStrategy::Auto);
    let dense = handle
        .search_async("needle".into(), 3, Some(Intent::Locate), auto.clone())
        .await
        .unwrap();
    let dense_fresh = fresh_index
        .query_handle()
        .unwrap()
        .search_async("needle".into(), 3, Some(Intent::Locate), auto.clone())
        .await
        .unwrap();
    assert_eq!(dense.rendered_prompt, dense_fresh.rendered_prompt);
    assert_eq!(dense.machine_pack["hits"], dense_fresh.machine_pack["hits"]);
    assert_eq!(
        dense.evidence_summary["selection"],
        dense_fresh.evidence_summary["selection"]
    );
    let retrieval = &dense.evidence_summary["retrieval"];
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
    assert_eq!(
        dense.machine_pack["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["file_path"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a.py", "b.py", "z.py"])
    );
    let mut pinned = auto;
    pinned.pinned_file_paths = Some(vec!["z.py".into()]);
    let selected = handle
        .search_async("needle".into(), 1, Some(Intent::Locate), pinned.clone())
        .await
        .unwrap();
    let selected_fresh = fresh_index
        .query_handle()
        .unwrap()
        .search_async("needle".into(), 1, Some(Intent::Locate), pinned)
        .await
        .unwrap();
    assert_eq!(
        selected.machine_pack["hits"],
        selected_fresh.machine_pack["hits"]
    );
    assert_eq!(selected.machine_pack["hits"][0]["file_path"], "z.py");
    let large = cc_search::selection::budget::pack(dense.clone(), 16_384).unwrap();
    let too_small = cc_search::selection::budget::pack(dense.clone(), 2048).unwrap_err();
    assert!(too_small
        .to_string()
        .contains("required context metadata exceeds output budget"));
    let small = cc_search::selection::budget::pack(dense.clone(), 8192).unwrap();
    let large_bytes = serde_json::to_vec(&large).unwrap().len();
    let small_bytes = serde_json::to_vec(&small).unwrap().len();
    assert!(large_bytes <= 16_384 && small_bytes <= 8192);
    assert!(small_bytes < large_bytes);
    assert_eq!(small.evidence_summary["packing"]["used_bytes"], small_bytes);
    assert_eq!(small.token_estimate as usize, small_bytes.div_ceil(4));
    assert_ne!(large.machine_pack, small.machine_pack);
    save(
        "final-envelope",
        json!({"generation_before":before,"generation_after":after,"warm":current,"fresh":fresh,"dense":dense,"dense_fresh":dense_fresh,"selected":selected,"budget_large_bytes":large_bytes,"budget_small_bytes":small_bytes,"budget_small":small}),
    );
    worker.close();
}
