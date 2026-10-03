//! Independent observable cache review; no key/hash reconstruction in oracle.
#![cfg(feature = "semantic-http")]
use cc_model::query::QueryControl;
use cc_semantic::{
    admission::InputBudget,
    cache::{encode_queries, QueryEncodeOutcome, QueryVectorCache},
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::openai_compatible::{
        EmbeddingHttpTransport, HttpRequest, HttpResponse, TransportError,
    },
    spec::{QueryEncodingSpec, VectorSpace},
};
use cc_server::semantic_query_encoding::*;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

#[derive(Default)]
struct Probe {
    factories: AtomicUsize,
    requests: Mutex<Vec<Value>>,
}
struct DistinctTransport {
    probe: Arc<Probe>,
    value: f32,
    dimension: usize,
}
impl EmbeddingHttpTransport for DistinctTransport {
    fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        self.probe.requests.lock().unwrap().push(body.clone());
        let mut vector = vec![0.0; self.dimension];
        vector[0] = self.value;
        vector[1] = 1.0;
        let data: Vec<_> = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, _)| json!({"index":i,"embedding":vector}))
            .collect();
        Ok(HttpResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: serde_json::to_vec(&json!({"model":body["model"],"data":data})).unwrap(),
        })
    }
}
struct NoReencode(VectorSpace);
impl EmbeddingProvider for NoReencode {
    fn space(&self) -> &VectorSpace {
        &self.0
    }
    fn embed_documents(&self, _: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        panic!("query consumer called document provider")
    }
    fn embed_queries(&self, _: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        panic!("production cache consumer missed a populated key")
    }
}
fn service(
    cache: Arc<QueryVectorCache>,
    namespace: &str,
    spec: QueryEncodingSpec,
    value: f32,
    probe: Arc<Probe>,
) -> Arc<QueryEncodingService> {
    let config = cc_model::config::SemanticProviderConfig {
        enabled: true,
        network_opt_in: true,
        allow_http: true,
        endpoint: "http://127.0.0.1:1/v1".into(),
        api_key_ref: Some("env:SYNTHETIC_REVIEW".into()),
        model_id: spec.space().model_id().into(),
        dimensions: Some(spec.space().dimension()),
        max_input_tokens: Some(spec.max_tokens()),
        max_batch_items: Some(1),
        ..Default::default()
    };
    let dimension = spec.space().dimension() as usize;
    let budget = InputBudget::validated(1, 1024, spec.max_tokens() as usize).unwrap();
    QueryEncodingService::new(
        QueryEncodingInputs {
            cache,
            namespace: namespace.into(),
            spec,
            budget,
            lifecycle: Arc::new(cc_db::semantic_publish::LifecycleFence::default()),
        },
        QueryNetworkCapacity::new(NetworkLimits {
            query_running: 1,
            query_queued: 0,
            background_running: 1,
            background_queued: 0,
        })
        .unwrap(),
        move |context| {
            probe.factories.fetch_add(1, Ordering::SeqCst);
            let transport = context.wrap_transport(Arc::new(DistinctTransport {
                probe: probe.clone(),
                value,
                dimension,
            }));
            cc_server::semantic_provider_factory::build_embedding_provider_with(
                &config,
                |_| {
                    Ok(
                        cc_semantic::providers::openai_compatible::EmbeddingApiKey::new(
                            "synthetic-review",
                        ),
                    )
                },
                || Ok(transport),
            )?
            .ok_or_else(|| cc_model::CcError::Config("fixture disabled".into()))
        },
    )
    .unwrap()
}
fn save(name: &str, value: Value) {
    if let Some(root) = std::env::var_os("CACHE_KEY_REVIEW_OUTPUT") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_service_adapter_and_consumer_preserve_distinct_cached_values() {
    let tokenizer = cc_model::chunk_policy::TOKEN_ESTIMATOR;
    let spec = |model: &str, dim, instruction, max| {
        QueryEncodingSpec::new(
            VectorSpace::new(model, dim).unwrap(),
            instruction,
            max,
            tokenizer,
        )
        .unwrap()
    };
    let base = spec("synthetic/review-a", 2, None, 8192);
    let cache = Arc::new(QueryVectorCache::new(64, 16384));
    let probe = Arc::new(Probe::default());
    // Expected markers are fixture outputs, independent of any cache digest.
    let cases = vec![
        (
            "base",
            "review-a",
            base.clone(),
            "REVIEW_QUERY",
            10.0,
            10.0,
            false,
        ),
        (
            "warm-changed-supplier",
            "review-a",
            base.clone(),
            "REVIEW_QUERY",
            999.0,
            10.0,
            true,
        ),
        (
            "namespace",
            "review-b",
            base.clone(),
            "REVIEW_QUERY",
            20.0,
            20.0,
            false,
        ),
        (
            "same-dimension-model",
            "review-a",
            spec("synthetic/review-b", 2, None, 8192),
            "REVIEW_QUERY",
            30.0,
            30.0,
            false,
        ),
        (
            "dimension",
            "review-a",
            spec("synthetic/review-a", 3, None, 8192),
            "REVIEW_QUERY",
            40.0,
            40.0,
            false,
        ),
        (
            "instruction",
            "review-a",
            spec(
                "synthetic/review-a",
                2,
                Some("review instruction".into()),
                8192,
            ),
            "REVIEW_QUERY",
            50.0,
            50.0,
            false,
        ),
        (
            "token-budget",
            "review-a",
            spec("synthetic/review-a", 2, None, 4096),
            "REVIEW_QUERY",
            60.0,
            60.0,
            false,
        ),
        (
            "raw-leading-byte",
            "review-a",
            base.clone(),
            " REVIEW_QUERY",
            70.0,
            70.0,
            false,
        ),
        (
            "unicode-composed",
            "review-a",
            base.clone(),
            "REVIEW_QUERY é",
            80.0,
            80.0,
            false,
        ),
        (
            "unicode-decomposed",
            "review-a",
            base.clone(),
            "REVIEW_QUERY e\u{301}",
            90.0,
            90.0,
            false,
        ),
        (
            "return-original",
            "review-a",
            base.clone(),
            "REVIEW_QUERY",
            999.0,
            10.0,
            true,
        ),
    ];
    let mut unique = 0;
    let mut records = vec![];
    for (axis, ns, spec, text, supplier, expected, hit) in cases {
        let encoder = service(cache.clone(), ns, spec.clone(), supplier, probe.clone());
        let calls = probe.requests.lock().unwrap().len();
        let factories = probe.factories.load(Ordering::SeqCst);
        let encoded = encoder
            .encode(
                text.as_bytes().to_vec(),
                QueryControl::new(Duration::from_secs(3)).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            encoded,
            QueryEncodingOutcome::Ready { cache_hit: hit },
            "{axis}"
        );
        unique += usize::from(!hit);
        assert_eq!(
            probe.requests.lock().unwrap().len(),
            calls + usize::from(!hit),
            "{axis}"
        );
        assert_eq!(
            probe.factories.load(Ordering::SeqCst),
            factories + usize::from(!hit),
            "{axis}"
        );
        assert_eq!(
            cache.len(),
            unique,
            "no capacity eviction ambiguity: {axis}"
        );
        // Read via a second production consumer, with provider calls forbidden.
        // The test does not call QueryCacheKey::new or construct/hash a key.
        let results = encode_queries(
            &NoReencode(spec.space().clone()),
            &cache,
            ns,
            &spec,
            &InputBudget::validated(1, 1024, spec.max_tokens() as usize).unwrap(),
            &[((), text.as_bytes().to_vec())],
        )
        .unwrap();
        let QueryEncodeOutcome::Encoded { vector, .. } = &results[0] else {
            panic!("skipped")
        };
        assert_eq!(vector.data[0], expected, "distinct cached output: {axis}");
        assert_eq!(vector.data[1], 1.0);
        assert_eq!(vector.data.len(), spec.space().dimension() as usize);
        assert_eq!(
            probe.requests.lock().unwrap().len(),
            calls + usize::from(!hit)
        );
        records.push(json!({"axis":axis,"expected_hit":hit,"posts_after":probe.requests.lock().unwrap().len(),"factories_after":probe.factories.load(Ordering::SeqCst),"consumer_value":vector.data,"cache_entries":cache.len()}));
    }
    save(
        "distinct-query-consumption",
        json!({"cases":records,"requests":*probe.requests.lock().unwrap(),"test_oracle_constructs_keys":false,"entry":"QueryEncodingService -> real OpenAI-compatible adapter with injected HTTP transport -> encode_queries cache consumer; provider forbidden on consumer miss","limitations":"not actual TCP query transport or public config lifecycle; no instruction config key exists"}),
    );
}

#[test]
fn public_schema_mismatch_reopen_serves_new_source_without_old_result_cache() {
    use cc_model::{config::ProjectConfig, query::RetrievalStrategy, search::SearchRequest};
    use cc_server::engine::CodeIndex;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("needle.py");
    std::fs::write(&source, "def needle():\n    return 111\n").unwrap();
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let request = SearchRequest {
        query: "needle".into(),
        retrieval_strategy: Some(RetrievalStrategy::Local),
        ..Default::default()
    };
    let (old_generation, db_path, old_hits) = {
        let mut index = CodeIndex::new(Some(root.path())).unwrap();
        index.build_index(true).unwrap();
        let db = index.index_db().unwrap().clone();
        let engine = cc_search::SearchEngine::new(db.clone(), &ProjectConfig::default(), None);
        let first = engine.search(&request).unwrap();
        let second = engine.search(&request).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(first.iter().any(|h| h.text.contains("111")));
        (
            db.reads().read_generation().unwrap(),
            db.admin().db_path().to_path_buf(),
            first,
        )
    };
    // Only synthetic on-disk schema header is changed, not production constants.
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .pragma_update(None, "user_version", 1_u32)
        .unwrap();
    std::fs::write(&source, "def needle():\n    return 222\n").unwrap();
    let mut reopened = CodeIndex::new(Some(root.path())).unwrap();
    let db = reopened.index_db().unwrap().clone();
    let reopened_generation = db.reads().read_generation().unwrap();
    assert_ne!(old_generation.incarnation, reopened_generation.incarnation);
    assert_eq!(
        db.reads().schema_version().unwrap(),
        cc_db::index_migrate::CURRENT_SCHEMA_VERSION
    );
    reopened.build_index(false).unwrap();
    let engine = cc_search::SearchEngine::new(db.clone(), &ProjectConfig::default(), None);
    let hits = engine.search(&request).unwrap();
    assert!(hits.iter().any(|h| h.text.contains("222")));
    assert!(hits.iter().all(|h| !h.text.contains("111")));
    assert!(!Arc::ptr_eq(&old_hits, &hits));
    assert!(Arc::ptr_eq(&hits, &engine.search(&request).unwrap()));
    save(
        "schema-reopen",
        json!({"old_generation":old_generation,"after_mismatch":reopened_generation,"after_index":db.reads().read_generation().unwrap(),"new_hits":hits,"misses":engine.cache_stats().result_misses,"hits":engine.cache_stats().result_hits,"oracle":"real incompatible user_version=1 reopen/reset, source111->222; no manual key/generation seeding","limits":"new engine domain; not adjacent21->22 additive migration nor cross-build parser/schema-spec mutation"}),
    );
}
