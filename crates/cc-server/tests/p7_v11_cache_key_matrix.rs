//! V11 behavioral cache-axis matrix. Real product document artifacts plus
//! production query service and real SQLite SearchEngine; synthetic only.
//! Synthetic loopback only; helper derived from preserved PR44 deadline fixture.
#![cfg(feature = "semantic-http")]
use rmcp::{
    model::CallToolRequestParams,
    service::{RoleClient, RunningService},
    transport::{ConfigureCommandExt, TokioChildProcess},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const DOCS: [(&str, &str); 4] = [
    ("scope/needle.py", "def needle():\n    return 731\n"),
    ("scope/sub/needle.py", "def needle():\n    return 732\n"),
    ("scope/needle.rs", "pub fn needle() -> u32 { 733 }\n"),
    ("outside/needle.py", "def needle():\n    return 999\n"),
];
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Probe {
    endpoint: String,
    calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Probe {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let held = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::new()));
        let (h, c, e) = (held.clone(), calls.clone(), events.clone());
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, peer) = listener.accept().await.unwrap();
                assert!(peer.ip().is_loopback());
                let (h, c, e) = (h.clone(), c.clone(), e.clone());
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let body = loop {
                        let mut buf = [0_u8; 4096];
                        let n = socket.read(&mut buf).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buf[..n]);
                        assert!(bytes.len() < 200_000);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = std::str::from_utf8(&bytes[..end]).unwrap();
                            assert!(head.starts_with("POST /v1/embeddings HTTP/1.1"));
                            assert!(head
                                .to_ascii_lowercase()
                                .contains("authorization: bearer synthetic-public-deadline"));
                            let len: usize = head
                                .lines()
                                .find_map(|l| {
                                    let (key, v) = l.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse().unwrap())
                                })
                                .unwrap();
                            if bytes.len() >= end + 4 + len {
                                break serde_json::from_slice::<Value>(
                                    &bytes[end + 4..end + 4 + len],
                                )
                                .unwrap();
                            }
                        }
                    };
                    assert_eq!(body["model"], "synthetic-public-deadline");
                    let inputs = body["input"].as_array().unwrap();
                    let query = inputs
                        .iter()
                        .any(|v| v.as_str().unwrap().contains("v18_live_"));
                    e.lock()
                        .unwrap()
                        .push(json!({"kind":"request","query":query,"body":body}));
                    if query {
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                    while query && h.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    let data: Vec<_> = inputs
                        .iter()
                        .enumerate()
                        .map(|(i, _)| json!({"index":i,"embedding":[1.0,0.0]}))
                        .collect();
                    let response =
                        json!({"model":"synthetic-public-deadline","data":data}).to_string();
                    let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len());
                    socket.write_all(header.as_bytes()).await.unwrap();
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            calls,
            events,
            task,
        }
    }
}
struct Session {
    root: tempfile::TempDir,
    client: RunningService<RoleClient, ()>,
    records: Mutex<Vec<Value>>,
}
impl Session {
    async fn open(probe: &Probe, semantic_ms: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        for (path, content) in DOCS {
            let file = project.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        let config = json!({"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"strategy":"auto","deadline_ms":8000,"semantic_timeout_ms":semantic_ms},"semantic":{"enabled":true,"network_opt_in":true,"allow_query_network":true,"allow_http":true,"endpoint":probe.endpoint,"model_id":"synthetic-public-deadline","dimensions":2,"max_input_tokens":8192,"max_batch_items":4,"api_key_ref":"env:P7_PUBLIC_DEADLINE_TOKEN"}});
        std::fs::write(project.join(".codecortex.json"), config.to_string()).unwrap();
        let transport = TokioChildProcess::new(
            tokio::process::Command::new(env!("CARGO_BIN_EXE_codecortex")).configure(|cmd| {
                for (k, _) in std::env::vars_os() {
                    if k.to_string_lossy().starts_with("CODECORTEX_") {
                        cmd.env_remove(k);
                    }
                }
                cmd.arg("mcp")
                    .arg("--project-path")
                    .arg(&project)
                    .current_dir(&project)
                    .env("CODECORTEX_PPID_POLL_MS", "0")
                    .env("CODECORTEX_SEMANTIC_CACHE_ROOT", root.path().join("cache"))
                    .env("P7_PUBLIC_DEADLINE_TOKEN", "synthetic-public-deadline")
                    .env_remove("OPENAI_API_KEY")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
            }),
        )
        .unwrap();
        let client = tokio::time::timeout(Duration::from_secs(10), ().serve(transport))
            .await
            .unwrap()
            .unwrap();
        let session = Self {
            root,
            client,
            records: Mutex::new(Vec::new()),
        };
        session
            .call("index", json!({"path":project,"full":true}))
            .await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let s = session
                    .call("status", json!({"aspect":"capabilities"}))
                    .await;
                if s["retrieval"]["dense_state"] == "ready" {
                    assert_eq!(s["retrieval"]["query_encoding"]["network_authorized"], true);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        session
    }
    async fn call(&self, name: &str, args: Value) -> Value {
        let reply = tokio::time::timeout(
            Duration::from_secs(10),
            self.client.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_ne!(reply.is_error, Some(true), "{reply:?}");
        let value = reply.structured_content.unwrap()["result"].clone();
        self.records
            .lock()
            .unwrap()
            .push(json!({"kind":"call","tool":name,"args":args,"result":value}));
        value
    }
    fn trace(&self, probe: &Probe, label: &str) {
        let value = json!({"test":label,"calls":*self.records.lock().unwrap(),"http":*probe.events.lock().unwrap(),"query_calls":probe.calls.load(Ordering::SeqCst),"only_loopback":true,"real_credentials":false});
        if let Some(path) = std::env::var_os("P7_KEYS_EVIDENCE_DIR") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join(format!(
                    "{}-{}-{label}.json",
                    std::process::id(),
                    self.root.path().file_name().unwrap().to_string_lossy()
                )),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
        } else {
            println!("PUBLIC_JSON {value}");
        }
    }
}

#[derive(Default)]
struct KeyProbe {
    factory_calls: AtomicUsize,
    requests: Mutex<Vec<Value>>,
}
struct KeyTransport {
    probe: Arc<KeyProbe>,
    dimension: usize,
}
impl cc_semantic::providers::openai_compatible::EmbeddingHttpTransport for KeyTransport {
    fn post_json(
        &self,
        request: cc_semantic::providers::openai_compatible::HttpRequest,
    ) -> Result<
        cc_semantic::providers::openai_compatible::HttpResponse,
        cc_semantic::providers::openai_compatible::TransportError,
    > {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        self.probe.requests.lock().unwrap().push(body.clone());
        let mut vector = vec![0.0; self.dimension];
        vector[0] = 1.0;
        let data: Vec<_> = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, _)| json!({"index":i,"embedding":vector}))
            .collect();
        Ok(cc_semantic::providers::openai_compatible::HttpResponse {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: serde_json::to_vec(&json!({"model":body["model"],"data":data})).unwrap(),
        })
    }
}
fn query_service(
    cache: Arc<cc_semantic::cache::QueryVectorCache>,
    namespace: String,
    spec: cc_semantic::spec::QueryEncodingSpec,
    probe: Arc<KeyProbe>,
) -> Arc<cc_server::semantic_query_encoding::QueryEncodingService> {
    use cc_server::semantic_query_encoding::*;
    let config = cc_model::config::SemanticProviderConfig {
        enabled: true,
        network_opt_in: true,
        allow_http: true,
        endpoint: "http://127.0.0.1:1/v1".into(),
        api_key_ref: Some("env:SYNTHETIC_KEY_MATRIX".into()),
        model_id: spec.space().model_id().into(),
        dimensions: Some(spec.space().dimension()),
        max_input_tokens: Some(spec.max_tokens()),
        max_batch_items: Some(1),
        ..Default::default()
    };
    let dimensions = spec.space().dimension() as usize;
    let max = spec.max_tokens() as usize;
    QueryEncodingService::new(
        QueryEncodingInputs {
            cache,
            namespace,
            spec,
            budget: cc_semantic::admission::InputBudget::validated(1, 1024, max).unwrap(),
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
            probe.factory_calls.fetch_add(1, Ordering::SeqCst);
            let transport = context.wrap_transport(Arc::new(KeyTransport {
                probe: probe.clone(),
                dimension: dimensions,
            }));
            cc_server::semantic_provider_factory::build_embedding_provider_with(
                &config,
                |_| {
                    Ok(
                        cc_semantic::providers::openai_compatible::EmbeddingApiKey::new(
                            "synthetic-key-matrix",
                        ),
                    )
                },
                || Ok(transport),
            )?
            .ok_or_else(|| {
                cc_model::CcError::Config("synthetic provider unexpectedly disabled".into())
            })
        },
    )
    .unwrap()
}
fn artifact_snapshot(path: &std::path::Path) -> std::collections::BTreeMap<String, String> {
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        out: &mut std::collections::BTreeMap<String, String>,
    ) {
        for entry in std::fs::read_dir(path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().to_string_lossy().into(),
                    cc_model::identity::bytes_hash(&std::fs::read(p).unwrap()),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    visit(path, path, &mut out);
    out
}
fn save_keys(name: &str, value: &Value) {
    if let Some(path) = std::env::var_os("P7_KEYS_EVIDENCE_DIR") {
        let p = std::path::PathBuf::from(path);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join(format!("{name}.json")),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_query_cache_axes_change_transport_calls_without_reembedding_artifacts() {
    use cc_semantic::{
        cache::{namespace_key, QueryCacheKey, QueryVectorCache},
        ports::QueryInput,
        spec::{QueryEncodingSpec, VectorSpace},
    };
    use cc_server::semantic_query_encoding::QueryEncodingOutcome;
    let document_probe = Probe::start().await;
    let document_session = Session::open(&document_probe, 5000).await;
    let before = artifact_snapshot(&document_session.root.path().join("cache"));
    assert!(
        !before.is_empty(),
        "artifact evidence must come from actual product document worker"
    );
    let doc_requests = document_probe.events.lock().unwrap().len();
    assert!(doc_requests > 0);
    let cache = Arc::new(QueryVectorCache::new(64, 16384));
    let probe = Arc::new(KeyProbe::default());
    let ns = namespace_key("synthetic/project-a").unwrap();
    let other_ns = namespace_key("synthetic/project-b").unwrap();
    let base_space = VectorSpace::new("synthetic/key-model-a", 2).unwrap();
    let spec = |space: VectorSpace, instruction: Option<String>, max, tokenizer: &str| {
        QueryEncodingSpec::new(space, instruction, max, tokenizer).unwrap()
    };
    let tokenizer = cc_model::chunk_policy::TOKEN_ESTIMATOR;
    let base = spec(base_space.clone(), None, 8192, tokenizer);
    let query = b"needle v11_key_matrix".to_vec();
    let variants = vec![
        ("base-cold", ns.clone(), base.clone(), query.clone(), false),
        (
            "identical-warm",
            ns.clone(),
            base.clone(),
            query.clone(),
            true,
        ),
        ("namespace", other_ns, base.clone(), query.clone(), false),
        (
            "same-dimension-model",
            ns.clone(),
            spec(
                VectorSpace::new("synthetic/key-model-b", 2).unwrap(),
                None,
                8192,
                tokenizer,
            ),
            query.clone(),
            false,
        ),
        (
            "dimension",
            ns.clone(),
            spec(
                VectorSpace::new("synthetic/key-model-a", 3).unwrap(),
                None,
                8192,
                tokenizer,
            ),
            query.clone(),
            false,
        ),
        (
            "query-instruction-only",
            ns.clone(),
            spec(
                base_space.clone(),
                Some("synthetic query instruction".into()),
                8192,
                tokenizer,
            ),
            query.clone(),
            false,
        ),
        (
            "query-token-budget-only",
            ns.clone(),
            spec(base_space.clone(), None, 4096, tokenizer),
            query.clone(),
            false,
        ),
        (
            "query-byte-case",
            ns.clone(),
            base.clone(),
            b"needle V11_key_matrix".to_vec(),
            false,
        ),
        (
            "query-byte-leading-space",
            ns.clone(),
            base.clone(),
            b" needle v11_key_matrix".to_vec(),
            false,
        ),
        (
            "query-byte-trailing-newline",
            ns.clone(),
            base.clone(),
            b"needle v11_key_matrix\n".to_vec(),
            false,
        ),
        (
            "query-byte-unicode-composed",
            ns.clone(),
            base.clone(),
            "needle v11_key_matrix é".as_bytes().to_vec(),
            false,
        ),
        (
            "query-byte-unicode-decomposed",
            ns.clone(),
            base.clone(),
            "needle v11_key_matrix e\u{301}".as_bytes().to_vec(),
            false,
        ),
        (
            "return-original-key",
            ns.clone(),
            base.clone(),
            query.clone(),
            true,
        ),
    ];
    let mut receipts = Vec::new();
    for (axis, namespace, spec, bytes, hit) in variants {
        let service = query_service(
            cache.clone(),
            namespace.clone(),
            spec.clone(),
            probe.clone(),
        );
        let calls = probe.requests.lock().unwrap().len();
        let factories = probe.factory_calls.load(Ordering::SeqCst);
        let outcome = service
            .encode(
                bytes.clone(),
                cc_model::query::QueryControl::new(Duration::from_secs(3)).unwrap(),
            )
            .await
            .unwrap_or_else(|error| panic!("query axis {axis}: {error}"));
        assert_eq!(
            outcome,
            QueryEncodingOutcome::Ready { cache_hit: hit },
            "{axis}"
        );
        let delta = usize::from(!hit);
        assert_eq!(
            probe.requests.lock().unwrap().len(),
            calls + delta,
            "{axis}"
        );
        assert_eq!(
            probe.factory_calls.load(Ordering::SeqCst),
            factories + delta,
            "warm key must skip factory: {axis}"
        );
        let input = QueryInput::from_bytes(&bytes).unwrap();
        let key = QueryCacheKey::new(&namespace, &spec, &input).unwrap();
        let vector = cache.get(&key).unwrap();
        assert_eq!(vector.data.len(), spec.space().dimension() as usize);
        assert_eq!(vector.data[0], 1.0);
        if !hit {
            assert_eq!(
                probe.requests.lock().unwrap().last().unwrap()["input"],
                json!([String::from_utf8(bytes.clone()).unwrap()])
            );
        }
        let warm = service
            .encode(
                bytes,
                cc_model::query::QueryControl::new(Duration::from_secs(3)).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(warm, QueryEncodingOutcome::Ready { cache_hit: true });
        assert_eq!(probe.requests.lock().unwrap().len(), calls + delta);
        receipts.push(json!({"axis":axis,"expected_hit":hit,"transport_posts_before":calls,"transport_posts_after":calls+delta,"factories_before":factories,"factories_after":factories+delta,"vector_dimension":vector.dimension,"payload_bytes":cache.total_payload_bytes()}));
    }
    let calls = probe.requests.lock().unwrap().len();
    let factories = probe.factory_calls.load(Ordering::SeqCst);
    let entries = cache.len();
    let unknown = query_service(
        cache.clone(),
        ns,
        spec(base_space, None, 8192, "synthetic-unsupported-tokenizer"),
        probe.clone(),
    );
    assert!(unknown
        .encode(
            query,
            cc_model::query::QueryControl::new(Duration::from_secs(3)).unwrap()
        )
        .await
        .is_err());
    assert_eq!(probe.requests.lock().unwrap().len(), calls);
    assert_eq!(probe.factory_calls.load(Ordering::SeqCst), factories);
    assert_eq!(cache.len(), entries);
    assert_eq!(
        artifact_snapshot(&document_session.root.path().join("cache")),
        before,
        "query-only axes must not rewrite document artifacts"
    );
    assert_eq!(
        document_probe.events.lock().unwrap().len(),
        doc_requests,
        "query component must never rerun document embeddings"
    );
    save_keys(
        "query-axes",
        &json!({"cases":receipts,"requests":*probe.requests.lock().unwrap(),"unsupported_tokenizer":"rejected before factory/transport/cache","document_artifacts":before,"document_posts":doc_requests,"query_transport":"injected memory HTTP seam; not actual query network","level":"L2 production QueryEncodingService/shared cache/factory; actual document product stdio setup","mutable_metric_version":"not admitted by frozen constructors"}),
    );
    document_session.trace(&document_probe, "document-setup");
    document_session.client.cancel().await.unwrap();
}

#[test]
fn local_and_graph_cache_axes_force_real_misses_and_preserve_equivalent_sets() {
    use cc_model::{
        config::{ProjectConfig, RepoSizeTier},
        query::RetrievalStrategy,
        search::SearchRequest,
        Intent, Language,
    };
    let root = tempfile::tempdir().unwrap();
    for (path, content) in DOCS {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, content).unwrap();
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let db = index.index_db().unwrap().clone();
    let engine = cc_search::SearchEngine::new(db.clone(), &ProjectConfig::default(), None);
    let base = SearchRequest {
        query: "needle".into(),
        top_k: 10,
        include_grep: true,
        retrieval_strategy: Some(RetrievalStrategy::Local),
        ..Default::default()
    };
    let limits = RepoSizeTier::Tiny.graph_enrich_limits();
    let baseline = engine.search(&base).unwrap();
    assert!(!baseline.is_empty());
    let warm = engine.search(&base).unwrap();
    assert!(Arc::ptr_eq(&baseline, &warm));
    let mut variants = Vec::new();
    macro_rules! axis {
        ($label:literal,$field:ident,$value:expr) => {{
            let mut r = base.clone();
            r.$field = $value;
            variants.push(($label, r));
        }};
    }
    axis!("query", query, "needle return".into());
    axis!(
        "strategy",
        retrieval_strategy,
        Some(RetrievalStrategy::Auto)
    );
    axis!("intent", intent, Some(Intent::Fix));
    axis!("top-k", top_k, 1);
    axis!("path-prefix", path_prefix, Some("scope/".into()));
    axis!("grep", include_grep, false);
    axis!("preselect-limit", file_preselect_limit, Some(1));
    axis!("languages", languages, Some(vec![Language::Python]));
    axis!("files", file_paths, Some(vec!["scope/needle.py".into()]));
    axis!("empty-languages", languages, Some(vec![]));
    axis!("empty-files", file_paths, Some(vec![]));
    axis!(
        "boost-files",
        boost_file_paths,
        Some(vec!["outside/needle.py".into()])
    );
    axis!(
        "conversation",
        conversation_queries,
        Some(vec!["return 999".into()])
    );
    axis!(
        "recent",
        recent_file_paths,
        Some(vec!["outside/needle.py".into()])
    );
    axis!(
        "pinned",
        pinned_file_paths,
        Some(vec!["outside/needle.py".into()])
    );
    axis!(
        "overlay",
        overlay_file_paths,
        Some(vec!["outside/needle.py".into()])
    );
    axis!("empty-boost", boost_file_paths, Some(vec![]));
    axis!("empty-conversation", conversation_queries, Some(vec![]));
    axis!("empty-recent", recent_file_paths, Some(vec![]));
    axis!("empty-pinned", pinned_file_paths, Some(vec![]));
    axis!("empty-overlay", overlay_file_paths, Some(vec![]));
    let mut receipts = Vec::new();
    for (axis, request) in variants {
        let before = engine.cache_stats();
        let cold = engine.search(&request).unwrap();
        let after = engine.cache_stats();
        assert_eq!(after.result_misses, before.result_misses + 1, "{axis}");
        assert_eq!(after.result_hits, before.result_hits, "{axis}");
        let repeated = engine.search(&request).unwrap();
        assert!(
            Arc::ptr_eq(&cold, &repeated),
            "warm identical request must reuse cache: {axis}"
        );
        let recomputed = cc_search::SearchEngine::new(db.clone(), &ProjectConfig::default(), None)
            .search(&request)
            .unwrap();
        let projection = |hits: &[cc_model::search::SearchHit]| {
            hits.iter().map(|h|json!({"path":h.file_path,"text":h.text,"score":h.rerank_score,"trace":h.score_trace})).collect::<Vec<_>>()
        };
        assert_eq!(
            projection(&cold),
            projection(&recomputed),
            "cached request must match independent recomputation: {axis}"
        );
        if matches!(axis, "empty-files" | "empty-languages") {
            assert!(cold.is_empty());
        }
        if axis == "files" {
            assert!(cold.iter().all(|h| h.file_path == "scope/needle.py"));
        }
        if axis == "path-prefix" {
            assert!(cold.iter().all(|h| h.file_path.starts_with("scope/")));
        }
        let graph_stats = engine.cache_stats();
        let graph_cold = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert_eq!(
            engine.cache_stats().graph_misses,
            graph_stats.graph_misses + 1,
            "graph request axis {axis}"
        );
        let graph_warm = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(Arc::ptr_eq(&graph_cold, &graph_warm));
        receipts.push(json!({"axis":axis,"misses_before":before.result_misses,"misses_after":after.result_misses,"hits_before":before.result_hits,"hits_after_warm":engine.cache_stats().result_hits,"same_allocation_on_warm":true,"recompute_equal":true}));
    }
    for field in ["languages", "files"] {
        let mut one = base.clone();
        let mut two = base.clone();
        if field == "languages" {
            one.languages = Some(vec![Language::Python, Language::Rust]);
            two.languages = Some(vec![Language::Rust, Language::Python]);
        } else {
            one.file_paths = Some(vec!["scope/needle.py".into(), "scope/needle.rs".into()]);
            two.file_paths = Some(vec!["scope/needle.rs".into(), "scope/needle.py".into()]);
        }
        let first = engine.search(&one).unwrap();
        let stats = engine.cache_stats();
        let second = engine.search(&two).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(engine.cache_stats().result_hits, stats.result_hits + 1);
        receipts.push(
            json!({"axis":format!("{field}-set-order"),"expected":"hit","same_allocation":true}),
        );
    }
    for field in ["boost", "conversation", "recent", "pinned", "overlay"] {
        let mut one = base.clone();
        let mut two = base.clone();
        let a = Some(vec!["scope/needle.py".into(), "outside/needle.py".into()]);
        let b = Some(vec!["outside/needle.py".into(), "scope/needle.py".into()]);
        match field {
            "boost" => {
                one.boost_file_paths = a;
                two.boost_file_paths = b;
            }
            "conversation" => {
                one.conversation_queries = a;
                two.conversation_queries = b;
            }
            "recent" => {
                one.recent_file_paths = a;
                two.recent_file_paths = b;
            }
            "pinned" => {
                one.pinned_file_paths = a;
                two.pinned_file_paths = b;
            }
            "overlay" => {
                one.overlay_file_paths = a;
                two.overlay_file_paths = b;
            }
            _ => unreachable!(),
        }
        engine.search(&one).unwrap();
        let stats = engine.cache_stats();
        engine.search(&two).unwrap();
        assert_eq!(
            engine.cache_stats().result_misses,
            stats.result_misses + 1,
            "ordered hint axis {field}"
        );
        receipts.push(json!({"axis":format!("{field}-order"),"expected":"miss"}));
    }
    let graph = engine
        .search_with_graph_context(&base, &limits, 4000)
        .unwrap();
    let graph_warm = engine
        .search_with_graph_context(&base, &limits, 4000)
        .unwrap();
    assert!(Arc::ptr_eq(&graph, &graph_warm));
    let mut graph_variants = Vec::new();
    macro_rules! graph_axis {
        ($field:ident) => {{
            let mut v = limits.clone();
            v.$field += 1;
            graph_variants.push((stringify!($field), v, 4000));
        }};
    }
    graph_axis!(max_resolve);
    graph_axis!(callers_per_sym);
    graph_axis!(callees_per_sym);
    graph_axis!(max_tests);
    graph_axis!(max_routes);
    graph_axis!(graph_budget_pct);
    graph_variants.push(("token_budget", limits.clone(), 3000));
    for (axis, limit, budget) in graph_variants {
        let stats = engine.cache_stats();
        let cold = engine
            .search_with_graph_context(&base, &limit, budget)
            .unwrap();
        assert_eq!(
            engine.cache_stats().graph_misses,
            stats.graph_misses + 1,
            "{axis}"
        );
        let warm = engine
            .search_with_graph_context(&base, &limit, budget)
            .unwrap();
        assert!(Arc::ptr_eq(&cold, &warm));
        receipts.push(json!({"axis":format!("graph-{axis}"),"expected":"miss then hit","same_allocation_on_warm":true}));
    }
    // More than32 variants can legitimately evict the early baseline.
    // Rewarm explicitly before testing dependency-specific invalidation.
    engine.search(&base).unwrap();
    let before = db.reads().read_generation().unwrap();
    let stats = engine.cache_stats();
    db.writes()
        .upsert_runtime_evidence(
            "synthetic/key-evidence",
            "synthetic-service",
            Some("GET"),
            "/synthetic",
            Some("200"),
            "2026-10-03",
        )
        .unwrap();
    let after = db.reads().read_generation().unwrap();
    assert_eq!(before.index_epoch, after.index_epoch);
    assert_ne!(before.evidence_epoch, after.evidence_epoch);
    engine.search(&base).unwrap();
    assert_eq!(
        engine.cache_stats().result_hits,
        stats.result_hits + 1,
        "evidence-only write must retain local cache"
    );
    engine
        .search_with_graph_context(&base, &limits, 4000)
        .unwrap();
    assert_eq!(
        engine.cache_stats().graph_misses,
        stats.graph_misses + 1,
        "evidence-only write must miss graph cache"
    );
    let stats = engine.cache_stats();
    std::fs::write(
        root.path().join("scope/needle.py"),
        "def needle():\n    return 12345\n",
    )
    .unwrap();
    index.build_index(false).unwrap();
    assert_ne!(
        db.reads().read_generation().unwrap().index_epoch,
        after.index_epoch
    );
    let changed = engine.search(&base).unwrap();
    assert_eq!(engine.cache_stats().result_misses, stats.result_misses + 1);
    assert!(changed
        .iter()
        .any(|h| h.file_path == "scope/needle.py" && h.text.contains("12345")));
    assert!(changed
        .iter()
        .all(|h| h.file_path != "scope/needle.py" || !h.text.contains("731")));
    engine
        .search_with_graph_context(&base, &limits, 4000)
        .unwrap();
    assert_eq!(engine.cache_stats().graph_misses, stats.graph_misses + 1);
    receipts.push(json!({"axis":"generation/index-evidence-source","local_evidence_only":"hit","graph_evidence_only":"miss","index_change":"both miss; only new source bytes","before":before,"after_evidence":after,"after_rebuild":db.reads().read_generation().unwrap()}));
    save_keys(
        "local-graph-axes",
        &json!({"cases":receipts,"entry":"production SearchEngine public search/search_with_graph_context; actual typed rebuild/evidence write","immutable_config_spec_axes":"not mutable within one engine; pending separate proof boundaries","full_V11":false}),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_query_handles_observe_config_domains_and_real_rebuild_incarnations() {
    use cc_server::{engine::CodeIndex, query_handle::QueryHandle};
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("needle.py");
    std::fs::write(&file, "def needle():\n    return 731\n").unwrap();
    let config_file = root.path().join(".codecortex.json");
    std::fs::write(&config_file,json!({"auto_index":{"enabled":false},"query":{"strategy":"local","deadline_ms":10000},"indexing":{"db_read_pool_size":1}}).to_string()).unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    let runtime = Arc::new(std::sync::RwLock::new(index));
    async fn query(runtime: &Arc<std::sync::RwLock<CodeIndex>>) -> Value {
        serde_json::to_value(
            QueryHandle::capture(runtime)
                .unwrap()
                .search_async("needle".into(), 5, None, Default::default())
                .await
                .unwrap(),
        )
        .unwrap()
    }
    let before = query(&runtime).await;
    let warm = query(&runtime).await;
    assert_eq!(before["machine_pack"]["hits"], warm["machine_pack"]["hits"]);
    std::fs::write(&config_file,json!({"auto_index":{"enabled":false},"query":{"strategy":"local","deadline_ms":9000},"search":{"exact_symbol_weight":4.0},"indexing":{"db_read_pool_size":1}}).to_string()).unwrap();
    runtime
        .write()
        .unwrap()
        .set_project(root.path(), false)
        .unwrap();
    let configured = query(&runtime).await;
    fn weight(value: &Value) -> f64 {
        let r = &value["evidence_summary"]["retrieval"];
        let lanes = r
            .get("lanes")
            .or_else(|| r.get("lane_receipts"))
            .unwrap()
            .as_array()
            .unwrap();
        lanes
            .iter()
            .find(|l| l["lane_id"] == "exact_symbol")
            .unwrap()["weight"]
            .as_f64()
            .unwrap()
    }
    assert_ne!(weight(&before), weight(&configured));
    assert_eq!(weight(&configured), 4.0);
    assert!(configured["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["text"].as_str().unwrap().contains("731")));
    std::fs::write(&file, "def needle():\n    return 947\n").unwrap();
    runtime.write().unwrap().build_index(true).unwrap();
    let rebuilt = query(&runtime).await;
    let old_gen = &configured["evidence_summary"]["source_freshness"]["generation"];
    let new_gen = &rebuilt["evidence_summary"]["source_freshness"]["generation"];
    assert_ne!(
        old_gen["incarnation"], new_gen["incarnation"],
        "actual full rebuild must create a fresh cache identity"
    );
    let hits = rebuilt["machine_pack"]["hits"].as_array().unwrap();
    assert!(hits
        .iter()
        .any(|h| h["text"].as_str().unwrap().contains("947")));
    assert!(hits
        .iter()
        .all(|h| !h["text"].as_str().unwrap().contains("731")));
    let again = query(&runtime).await;
    assert_eq!(
        rebuilt["machine_pack"]["hits"],
        again["machine_pack"]["hits"]
    );
    save_keys(
        "public-config-incarnation",
        &json!({"before":before,"configured":configured,"rebuilt":rebuilt,"warm_rebuilt":again,"level":"L2 public QueryHandle and actual set_project/full rebuild","config_behavior":"new engine domain honors changed lane weight; not a mutable-config key collision test","incarnation":"actual DB replacement, no manual epoch/identity seeding"}),
    );
}
