//! Independent literal-gold variants for PR45; production modules remain untouched.
#![cfg(feature = "semantic-http")]
use cc_model::{
    config::ProjectConfig,
    query::{QueryControl, RetrievalStrategy},
    retrieval::{HardScope, LaneOutcome, LaneStatus},
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    Language,
};
use cc_server::{
    engine::CodeIndex,
    handlers::{core, SharedCodeIndex},
    query_handle::QueryHandle,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const EAST: &str = "review direction east";
const NORTH: &str = "review direction north";
// Integer vectors make analytic scores independent of production arithmetic.
const DOCS: [(&str, &str, [f32; 2]); 8] = [
    ("scope/best.rs", "review_best", [8.0, 6.0]),
    ("scope/scaled.rs", "review_scaled", [30.0, 40.0]),
    ("scope/tie.rs", "review_tie", [3.0, 4.0]),
    ("scope/zero.rs", "review_zero", [0.0, 2.0]),
    ("scope/negative.rs", "review_negative", [-3.0, 4.0]),
    ("scope/only.py", "review_python", [14.0, 48.0]),
    ("scope_extra/strong.rs", "review_sibling", [100.0, 0.0]),
    ("outside/strong.rs", "review_outside", [2.0, 0.0]),
];
struct Probe {
    endpoint: String,
    queries: Arc<AtomicUsize>,
    documents: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
impl Probe {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let queries = Arc::new(AtomicUsize::new(0));
        let documents = Arc::new(AtomicUsize::new(0));
        let q = queries.clone();
        let d = documents.clone();
        let task = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let q = q.clone();
                let d = d.clone();
                tokio::spawn(async move {
                    let mut socket = BufReader::new(socket);
                    let mut line = String::new();
                    let mut length = None;
                    loop {
                        line.clear();
                        assert!(socket.read_line(&mut line).await.unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                length = Some(value.trim().parse::<usize>().unwrap())
                            }
                        }
                    }
                    let length = length.unwrap();
                    assert!(length < 65536);
                    let mut bytes = vec![0; length];
                    socket.read_exact(&mut bytes).await.unwrap();
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    let data: Vec<_> = body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                        .map(|(i, v)| {
                            let text = v.as_str().unwrap();
                            let vector = match text {
                                EAST => {
                                    q.fetch_add(1, Ordering::SeqCst);
                                    [5.0, 0.0]
                                }
                                NORTH => {
                                    q.fetch_add(1, Ordering::SeqCst);
                                    [0.0, 7.0]
                                }
                                _ => {
                                    d.fetch_add(1, Ordering::SeqCst);
                                    DOCS.iter()
                                        .find(|(_, marker, _)| text.contains(marker))
                                        .unwrap()
                                        .2
                                }
                            };
                            json!({"index":i,"embedding":vector})
                        })
                        .collect();
                    let response = json!({"model":body["model"],"data":data}).to_string();
                    let wire=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len());
                    socket.get_mut().write_all(wire.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            queries,
            documents,
            task,
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Env(Option<std::ffi::OsString>);
impl Env {
    fn new(p: &std::path::Path) -> Self {
        let old = std::env::var_os(cc_semantic::cache::CACHE_ROOT_ENV);
        std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, p);
        Self(old)
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        if let Some(v) = self.0.take() {
            std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, v)
        } else {
            std::env::remove_var(cc_semantic::cache::CACHE_ROOT_ENV)
        }
    }
}
async fn build(index: &SharedCodeIndex) {
    let owned = index.clone();
    tokio::task::spawn_blocking(move || core::build_index(owned, false))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let done = {
                let i = index.read().unwrap();
                i.index_db()
                    .unwrap()
                    .reads()
                    .semantic_coverage()
                    .unwrap()
                    .coverage
                    .uncovered
                    == 0
                    && i.query_pins() == 0
            };
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    })
    .await
    .expect("original five-second bound");
}
fn catalog(index: &SharedCodeIndex) -> BTreeMap<String, String> {
    let db = index.read().unwrap().index_db().unwrap().clone();
    let conn = db.read_conn().unwrap();
    let mut stmt = conn
        .prepare("SELECT file_path,doc_key FROM document_manifest")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn insertion_order(index: &SharedCodeIndex) -> Vec<String> {
    let db = index.read().unwrap().index_db().unwrap().clone();
    let conn = db.read_conn().unwrap();
    let mut stmt = conn
        .prepare("SELECT file_path FROM document_manifest ORDER BY rowid")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
async fn recall(index: &SharedCodeIndex, query: &str, scope: HardScope, k: usize) -> LaneOutcome {
    let (subsystem, db) = {
        let i = index.read().unwrap();
        (
            i.semantic_subsystem().unwrap(),
            i.index_db().unwrap().clone(),
        )
    };
    subsystem
        .recall
        .recall(
            SemanticRequest {
                query: query.into(),
                scope,
                limit: k,
                policy_fingerprint: "independent-literal-variant-v1".into(),
                generation: db.reads().read_generation().unwrap(),
            },
            QueryControl::new(Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap()
}
// Admitted paths and scores are literal per-case expectations. No production
// HardScope predicate, path helper, vector scorer, ranking or gold is imported.
fn expect(index: &SharedCodeIndex, out: &LaneOutcome, expected: &[(&str, f64)], k: usize) {
    assert_eq!(out.status, LaneStatus::Complete);
    assert!(out.coverage.complete);
    let ids = catalog(index);
    let mut gold: Vec<_> = expected
        .iter()
        .map(|(p, s)| (ids[*p].as_str(), *p, *s))
        .collect();
    gold.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(b.0)));
    gold.truncate(k);
    assert_eq!(out.candidates.len(), gold.len());
    for (actual, (id, path, score)) in out.candidates.iter().zip(gold) {
        assert_eq!(actual.document.doc_key, id, "literal path {path}");
        assert!(
            (actual.raw_score - score).abs() < 1e-12,
            "{path} expected={score} actual={}",
            actual.raw_score
        );
        assert_eq!(actual.scoring_spec, "cosine-exact-v1");
    }
}
fn configured(root: &std::path::Path, endpoint: &str, model: &str) {
    let key = root.join("synthetic-key");
    std::fs::write(&key, "synthetic-loopback").unwrap();
    let mut c = ProjectConfig::default();
    c.auto_index.enabled = false;
    c.indexing.db_read_pool_size = Some(1);
    c.semantic.enabled = true;
    c.semantic.network_opt_in = true;
    c.semantic.allow_query_network = true;
    c.semantic.allow_http = true;
    c.semantic.endpoint = endpoint.into();
    c.semantic.api_key_ref = Some(format!("file:{}", key.display()));
    c.semantic.model_id = model.into();
    c.semantic.dimensions = Some(2);
    c.semantic.max_input_tokens = Some(8192);
    c.semantic.max_batch_items = Some(16);
    c.semantic.max_concurrent = 2;
    std::fs::write(
        root.join(".codecortex.json"),
        serde_json::to_vec(&c).unwrap(),
    )
    .unwrap();
}
async fn public(index: &SharedCodeIndex, query: &str) -> Value {
    let value = QueryHandle::capture(index)
        .unwrap()
        .search_async(
            query.into(),
            8,
            None,
            SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Semantic),
                path_prefix: Some("scope".into()),
                languages: Some(vec![Language::Rust]),
                boost_file_paths: Some(vec![
                    "scope_extra/strong.rs".into(),
                    "outside/strong.rs".into(),
                ]),
                file_preselect_limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let serialized = serde_json::to_value(&value).unwrap();
    let lane = serialized["evidence_summary"]["retrieval"]["lane_receipts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["lane_id"] == "semantic")
        .unwrap();
    assert_eq!(lane["status"], "complete");
    assert!(lane["candidate_count"].as_u64().unwrap() > 0);
    let hits = value.machine_pack["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(
        hits.iter().any(|hit| hit["score_trace"]
            .as_array()
            .unwrap()
            .iter()
            .any(|term| term[0] == "rrf:semantic")),
        "public hydration must actually retain semantic contribution"
    );
    for hit in hits {
        assert!([
            "scope/best.rs",
            "scope/scaled.rs",
            "scope/tie.rs",
            "scope/zero.rs",
            "scope/negative.rs"
        ]
        .contains(&hit["file_path"].as_str().unwrap()));
    }
    serde_json::to_value(value).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nonunit_literal_gold_real_insertion_orders_prefix_topk_delete_and_new_model() {
    let cache = tempfile::tempdir().unwrap();
    let _env = Env::new(cache.path());
    let probe = Probe::start().await;
    let orders = [
        [0, 1, 2, 3, 4, 5, 6, 7],
        [7, 6, 5, 4, 3, 2, 1, 0],
        [2, 5, 0, 7, 1, 6, 3, 4],
    ];
    for (round, order) in orders.iter().enumerate() {
        let root = tempfile::tempdir().unwrap();
        configured(
            root.path(),
            &probe.endpoint,
            &format!("independent/model-a-{round}"),
        );
        let index = Arc::new(RwLock::new(CodeIndex::new(Some(root.path())).unwrap()));
        let docs_before = probe.documents.load(Ordering::SeqCst);
        // Incremental one-file commits establish actual DB insertion order, rather
        // than relying on file creation order before the scanner's path sort.
        let mut committed = vec![];
        for &n in order {
            let (path, marker, _) = DOCS[n];
            let p = root.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            let text = if path.ends_with(".py") {
                format!("def {marker}():\n    return 99\n")
            } else {
                format!("pub fn {marker}() -> u32 {{ 99 }}\n")
            };
            std::fs::write(p, text).unwrap();
            build(&index).await;
            committed.push(path.to_string());
            assert_eq!(insertion_order(&index), committed);
        }
        assert_eq!(probe.documents.load(Ordering::SeqCst) - docs_before, 8);
        assert_eq!(
            catalog(&index)
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            DOCS.iter().map(|d| d.0).collect()
        );
        let queries_before = probe.queries.load(Ordering::SeqCst);
        public(&index, EAST).await;
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 1);
        let rust = HardScope {
            path_prefix: Some("scope/".into()),
            languages: Some(vec![Language::Rust]),
            file_paths: None,
        };
        let east = [
            ("scope/best.rs", 0.8),
            ("scope/scaled.rs", 0.6),
            ("scope/tie.rs", 0.6),
            ("scope/zero.rs", 0.0),
            ("scope/negative.rs", -0.6),
        ];
        for k in [0, 1, 2, 5, 99] {
            expect(
                &index,
                &recall(&index, EAST, rust.clone(), k).await,
                &east,
                k,
            );
        }
        let intersection = HardScope {
            file_paths: Some(vec![
                "scope/scaled.rs".into(),
                "scope/only.py".into(),
                "scope_extra/strong.rs".into(),
            ]),
            ..rust.clone()
        };
        expect(
            &index,
            &recall(&index, EAST, intersection, 2).await,
            &[("scope/scaled.rs", 0.6)],
            2,
        );
        let exact = HardScope {
            path_prefix: Some("scope/tie.rs".into()),
            ..rust.clone()
        };
        expect(
            &index,
            &recall(&index, EAST, exact, 3).await,
            &[("scope/tie.rs", 0.6)],
            3,
        );
        let disjoint = HardScope {
            path_prefix: Some("SCOPE".into()),
            ..rust.clone()
        };
        expect(&index, &recall(&index, EAST, disjoint, 3).await, &[], 3);
        for empty in [
            HardScope {
                languages: Some(vec![]),
                ..rust.clone()
            },
            HardScope {
                file_paths: Some(vec![]),
                ..rust.clone()
            },
        ] {
            expect(&index, &recall(&index, EAST, empty, 3).await, &[], 3);
        }
        let python = HardScope {
            path_prefix: Some("scope".into()),
            languages: Some(vec![Language::Python]),
            file_paths: None,
        };
        expect(
            &index,
            &recall(&index, EAST, python.clone(), 3).await,
            &[("scope/only.py", 0.28)],
            3,
        );
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 1);
        let north = [
            ("scope/zero.rs", 1.0),
            ("scope/scaled.rs", 0.8),
            ("scope/tie.rs", 0.8),
            ("scope/negative.rs", 0.8),
            ("scope/best.rs", 0.6),
        ];
        expect(
            &index,
            &recall(&index, NORTH, rust.clone(), 5).await,
            &north,
            5,
        );
        expect(
            &index,
            &recall(&index, NORTH, python, 3).await,
            &[("scope/only.py", 0.96)],
            3,
        );
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 2);
        std::fs::remove_file(root.path().join("scope/best.rs")).unwrap();
        build(&index).await;
        assert!(!catalog(&index).contains_key("scope/best.rs"));
        expect(
            &index,
            &recall(&index, EAST, rust.clone(), 2).await,
            &east[1..],
            2,
        );
        let hydrated = public(&index, EAST).await;
        assert!(hydrated["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["file_path"] != "scope/best.rs"));
        let old = index.read().unwrap().semantic_subsystem().unwrap();
        configured(
            root.path(),
            &probe.endpoint,
            &format!("independent/model-b-{round}"),
        );
        index
            .write()
            .unwrap()
            .set_project(root.path(), false)
            .unwrap();
        build(&index).await;
        let db = index.read().unwrap().index_db().unwrap().clone();
        let rejected = old
            .recall
            .recall(
                SemanticRequest {
                    query: EAST.into(),
                    scope: rust.clone(),
                    limit: 5,
                    policy_fingerprint: "independent-old-space".into(),
                    generation: db.reads().read_generation().unwrap(),
                },
                QueryControl::new(Duration::from_secs(5)).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status, LaneStatus::Unavailable);
        assert_eq!(
            rejected.truncation_reason.as_deref(),
            Some("semantic_space_not_active")
        );
        assert!(rejected.candidates.is_empty());
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 2);
        public(&index, EAST).await;
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 3);
        expect(
            &index,
            &recall(&index, EAST, rust.clone(), 2).await,
            &east[1..],
            2,
        );
        public(&index, EAST).await;
        assert_eq!(probe.queries.load(Ordering::SeqCst), queries_before + 3);
        eprintln!("independent variant round {round}: actual rowid order={committed:?}; cosine/scope/state gold passed; query_posts=3");
        index.write().unwrap().close();
    }
}

#[test]
fn original_unit_gold_cannot_distinguish_inner_product_but_nonunit_variant_can() {
    // Deliberately wrong scorer, independently written: raw dot, no norms.
    let original = [
        ([0.96_f64, 0.28], 0.96),
        ([0.6, 0.8], 0.6),
        ([0.0, 1.0], 0.0),
        ([0.8, 0.6], 0.8),
        ([1.0, 0.0], 1.0),
    ];
    for (v, gold) in original {
        let wrong_dot = v[0];
        assert!((wrong_dot - gold).abs() < 2e-6);
    }
    let wrong_dot = 5.0 * 30.0;
    assert!((wrong_dot - 0.6_f64).abs() > 100.0);
    // Analytic norms: query=5, doc=50, dot=150 -> exact score=3/5.
    assert_eq!(150.0 / (5.0 * 50.0), 0.6);
}
