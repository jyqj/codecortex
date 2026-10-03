//! Main-owned V05 gap: actual worker/recall, raw six-lane identities and final source.
//! Local synthetic provider only; no manual vector-cache put or SQL publication seed.
#![cfg(feature = "semantic")]
use cc_model::{
    config::ProjectConfig,
    query::QueryControl,
    retrieval::HardScope,
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest, SemanticResponse},
    Language,
};
use cc_semantic::{
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    types::VectorSpace,
};
use cc_server::{
    engine::CodeIndex,
    semantic_query_encoding::{
        NetworkLimits, QueryEncodingInputs, QueryEncodingService, QueryNetworkCapacity,
    },
    service_factory::QueryServices,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
const DOCS: [(&str, &str); 7] = [
    ("scope/needle.rs", "pub fn needle() -> u32 { 731 }\n"),
    ("scope/sub/needle.rs", "pub fn needle() -> u32 { 732 }\n"),
    (
        "scope/needle.py",
        "from outside.callee import foreign\n\ndef needle():\n    return foreign() + 733\n",
    ),
    ("scope/needle_class.py", "class Needle:\n    pass\n"),
    ("scope_extra/needle.py", "def needle():\n    return 998\n"),
    ("outside/needle.py", "def needle():\n    return 999\n"),
    ("outside/callee.py", "def foreign():\n    return 777\n"),
];
struct UnitProvider(VectorSpace);
impl EmbeddingProvider for UnitProvider {
    fn space(&self) -> &VectorSpace {
        &self.0
    }
    fn embed_documents(&self, input: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(input.iter().map(|_| vec![1.0, 0.0]).collect())
    }
    fn embed_queries(&self, input: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(input.iter().map(|_| vec![1.0, 0.0]).collect())
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_graph_dense_and_hydrate_share_literal_hard_domain_and_symbol_filters() {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in DOCS {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(false).unwrap();
    let db = index.index_db().unwrap().clone();
    let mut config = ProjectConfig::default();
    config.indexing.db_read_pool_size = Some(1);
    config.semantic.enabled = true;
    config.semantic.model_id = "fake/all-lane-scope".into();
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(16);
    config.semantic.endpoint = "https://semantic.invalid/v1".into();
    let subsystem = Arc::new(
        cc_server::semantic_wiring::assemble_with(
            &root.path().to_string_lossy(),
            &config,
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
    let provider = Arc::new(UnitProvider(subsystem.space.clone()));
    let worker = cc_server::semantic_runtime::SemanticRuntime::new(
        db.clone(),
        subsystem.clone(),
        services,
        provider.clone(),
    )
    .unwrap();
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
    let engine = cc_search::SearchEngine::new(db.clone(), &config, None);
    let mut catalog = BTreeMap::new();
    for (path, _) in DOCS {
        for reference in cc_db::document_store::references(&db, path).unwrap() {
            catalog.insert(reference.doc_key, (path, reference.doc_version));
        }
    }
    assert_eq!(
        catalog.values().map(|x| x.0).collect::<BTreeSet<_>>(),
        DOCS.iter().map(|x| x.0).collect()
    );
    let all: Vec<_> = DOCS.iter().map(|x| x.0).collect();
    let scoped = vec![
        "scope/needle.rs",
        "scope/sub/needle.rs",
        "scope/needle.py",
        "scope/needle_class.py",
    ];
    let rust = vec!["scope/needle.rs", "scope/sub/needle.rs"];
    let base = SearchRequest {
        query: "needle".into(),
        retrieval_strategy: Some(cc_model::query::RetrievalStrategy::Auto),
        top_k: 50,
        include_grep: true,
        boost_file_paths: Some(vec!["outside/needle.py".into()]),
        recent_file_paths: Some(vec!["outside/needle.py".into()]),
        pinned_file_paths: Some(vec!["outside/needle.py".into()]),
        overlay_file_paths: Some(vec!["outside/needle.py".into()]),
        file_preselect_limit: Some(1),
        ..Default::default()
    };
    // The authored SemanticRequest hard scopes exercise the actual recall port;
    // existing public DSL tests separately prove dispatch-to-port compilation.
    let cases = vec![
        ("all", base.clone(), HardScope::default(), all.clone(), all),
        (
            "prefix",
            SearchRequest {
                path_prefix: Some("scope/".into()),
                ..base.clone()
            },
            HardScope {
                path_prefix: Some("scope/".into()),
                ..Default::default()
            },
            scoped.clone(),
            scoped.clone(),
        ),
        (
            "prefix-rust",
            SearchRequest {
                query: "needle path:scope/ lang:rust".into(),
                ..base.clone()
            },
            HardScope {
                path_prefix: Some("scope/".into()),
                languages: Some(vec![Language::Rust]),
                ..Default::default()
            },
            rust.clone(),
            rust,
        ),
        (
            "explicit-empty-files",
            SearchRequest {
                file_paths: Some(vec![]),
                ..base.clone()
            },
            HardScope {
                file_paths: Some(vec![]),
                ..Default::default()
            },
            vec![],
            vec![],
        ),
        (
            "explicit-empty-languages",
            SearchRequest {
                languages: Some(vec![]),
                ..base.clone()
            },
            HardScope {
                languages: Some(vec![]),
                ..Default::default()
            },
            vec![],
            vec![],
        ),
        (
            "dsl-conflict",
            SearchRequest {
                query: "needle path:scope/ path:outside/".into(),
                ..base.clone()
            },
            HardScope {
                file_paths: Some(vec![]),
                ..Default::default()
            },
            vec![],
            vec![],
        ),
        (
            "caller-dsl-language-conflict",
            SearchRequest {
                query: "needle lang:python".into(),
                languages: Some(vec![Language::Rust]),
                ..base.clone()
            },
            HardScope {
                languages: Some(vec![]),
                ..Default::default()
            },
            vec![],
            vec![],
        ),
        (
            "files-intersection",
            SearchRequest {
                file_paths: Some(vec!["scope/needle.py".into(), "outside/needle.py".into()]),
                path_prefix: Some("scope/".into()),
                ..base.clone()
            },
            HardScope {
                file_paths: Some(vec!["scope/needle.py".into(), "outside/needle.py".into()]),
                path_prefix: Some("scope/".into()),
                ..Default::default()
            },
            vec!["scope/needle.py"],
            vec!["scope/needle.py"],
        ),
        (
            "kind-function",
            SearchRequest {
                query: "needle path:scope/ kind:function name:needle".into(),
                ..base.clone()
            },
            HardScope {
                path_prefix: Some("scope/".into()),
                ..Default::default()
            },
            scoped.clone(),
            vec!["scope/needle.rs", "scope/sub/needle.rs", "scope/needle.py"],
        ),
        (
            "kind-class",
            SearchRequest {
                query: "needle path:scope/ kind:class name:Needle".into(),
                ..base.clone()
            },
            HardScope {
                path_prefix: Some("scope/".into()),
                ..Default::default()
            },
            scoped.clone(),
            vec!["scope/needle_class.py"],
        ),
        (
            "absent-name",
            SearchRequest {
                query: "needle path:scope/ name:no_such_symbol".into(),
                ..base
            },
            HardScope {
                path_prefix: Some("scope/".into()),
                ..Default::default()
            },
            scoped,
            vec![],
        ),
    ];
    let mut rows = vec![];
    for (case, mut request, scope, allowed, expected_final) in cases {
        let generation = db.reads().read_generation().unwrap();
        let outcome = subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: "needle".into(),
                    scope: scope.clone(),
                    limit: 50,
                    policy_fingerprint: "all-lane-scope-v1".into(),
                    generation,
                },
                QueryControl::new(Duration::from_secs(5)).unwrap(),
            )
            .await
            .unwrap();
        request.semantic = Some(Arc::new(SemanticResponse {
            generation,
            outcome,
        }));
        let result = engine.search_with_diagnostics(&request).unwrap();
        assert_eq!(
            result
                .lanes
                .iter()
                .map(|l| l.lane_id.as_str())
                .collect::<BTreeSet<_>>(),
            [
                "exact_symbol",
                "path",
                "lexical",
                "grep",
                "graph",
                "semantic"
            ]
            .into_iter()
            .collect(),
            "{case}: every lane receipt required"
        );
        for lane in &result.lanes {
            for candidate in &lane.candidates {
                let (path, version) = catalog
                    .get(&candidate.document.doc_key)
                    .expect("actual fixture identity required");
                assert!(
                    allowed.contains(path),
                    "{case}: {} escaped hard domain: {path}",
                    lane.lane_id
                );
                assert_eq!(&candidate.document.doc_version, version);
                let source = DOCS.iter().find(|x| x.0 == *path).unwrap().1.as_bytes();
                let span = candidate.source_span;
                assert!(
                    span.start < span.end && span.end as usize <= source.len(),
                    "{case}: independent source bounds"
                );
            }
        }
        let paths = result
            .hits
            .iter()
            .map(|h| h.file_path.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            paths,
            expected_final.iter().copied().collect(),
            "{case}: full literal final set required; subset/vacuous passes forbidden"
        );
        for hit in &result.hits {
            let source = DOCS.iter().find(|d| d.0 == hit.file_path).unwrap().1;
            assert!(
                !hit.text.is_empty() && source.contains(&hit.text),
                "{case}: exact nonempty current source"
            );
            assert!(hit.start_line > 0 && hit.end_line >= hit.start_line);
        }
        if allowed.is_empty() {
            assert!(result.lanes.iter().all(|l| l.candidates.is_empty()));
        }
        if case == "all" {
            let graph = result.lanes.iter().find(|l| l.lane_id == "graph").unwrap();
            assert!(
                graph
                    .candidates
                    .iter()
                    .any(|c| catalog[&c.document.doc_key].0 == "outside/callee.py"),
                "real parsed cross-file graph neighbor must be exercised, not only seed candidates"
            );
            for id in [
                "exact_symbol",
                "path",
                "lexical",
                "grep",
                "graph",
                "semantic",
            ] {
                assert!(
                    !result
                        .lanes
                        .iter()
                        .find(|l| l.lane_id == id)
                        .unwrap()
                        .candidates
                        .is_empty(),
                    "positive {id} must not be vacuous"
                );
            }
        }
        rows.push(json!({"case":case,"hard_scope":scope,"allowed_literal":allowed,"expected_final_literal":expected_final,"lanes":result.lanes,"hits":result.hits,"level":"L2","semantic_scope_authored_port_input":true,"public_dispatch_scope_claim":false}));
    }
    worker.close();
    index.close();
    if let Some(dir) = std::env::var_os("P7_ALL_LANE_SCOPE_OUTPUT") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("all-lane-scope.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
}
