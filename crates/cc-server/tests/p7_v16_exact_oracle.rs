//! V16 L2 hand-gold through real SQLite, document worker and cold query HTTP.
//! Synthetic loopback only; no manual cache priming, live-model or L3 claim.
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const QUERY: &str = "independent synthetic vector direction";
// Gold scores are hand values, independent of the production cosine helper.
const DOCS: [(&str, &str, [f32; 2], f64); 6] = [
    ("scope/top.rs", "oracle_top", [0.96, 0.28], 0.96),
    ("scope/tie_a.rs", "oracle_tie_a", [0.6, 0.8], 0.6),
    ("scope/tie_b.rs", "oracle_tie_b", [0.6, 0.8], 0.6),
    ("scope/zero.rs", "oracle_zero", [0.0, 1.0], 0.0),
    ("scope/near.py", "oracle_python", [0.8, 0.6], 0.8),
    ("outside/strong.rs", "oracle_outside", [1.0, 0.0], 1.0),
];
struct Probe {
    endpoint: String,
    queries: Arc<AtomicUsize>,
    job: tokio::task::JoinHandle<()>,
}
impl Probe {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let queries = Arc::new(AtomicUsize::new(0));
        let calls = queries.clone();
        let job = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let calls = calls.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    let body = loop {
                        let n = socket.read(&mut buffer).await.unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 1_048_576);
                        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&bytes[..end]);
                            let length: usize = headers
                                .lines()
                                .find_map(|line| {
                                    let (k, v) = line.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse().unwrap())
                                })
                                .unwrap();
                            if bytes.len() >= end + 4 + length {
                                break serde_json::from_slice::<Value>(
                                    &bytes[end + 4..end + 4 + length],
                                )
                                .unwrap();
                            }
                        }
                    };
                    let data: Vec<_> = body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .enumerate()
                        .map(|(index, input)| {
                            let text = input.as_str().unwrap();
                            let vector = if text == QUERY {
                                calls.fetch_add(1, Ordering::SeqCst);
                                [1.0, 0.0]
                            } else {
                                DOCS.iter()
                                    .find(|doc| text.contains(doc.1))
                                    .expect("synthetic document marker")
                                    .2
                            };
                            json!({"index":index,"embedding":vector})
                        })
                        .collect();
                    let response = json!({"model":body["model"],"data":data}).to_string();
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",response.len()).as_bytes()).await.unwrap();
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        Self {
            endpoint,
            queries,
            job,
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.job.abort();
    }
}
struct CacheEnv(Option<std::ffi::OsString>);
impl CacheEnv {
    fn install(path: &std::path::Path) -> Self {
        let old = std::env::var_os(cc_semantic::cache::CACHE_ROOT_ENV);
        std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, path);
        Self(old)
    }
}
impl Drop for CacheEnv {
    fn drop(&mut self) {
        match self.0.take() {
            Some(old) => std::env::set_var(cc_semantic::cache::CACHE_ROOT_ENV, old),
            None => std::env::remove_var(cc_semantic::cache::CACHE_ROOT_ENV),
        }
    }
}
async fn build(index: &SharedCodeIndex, full: bool) {
    let index = index.clone();
    tokio::task::spawn_blocking(move || core::build_index(index, full))
        .await
        .unwrap()
        .unwrap();
}
async fn ready(index: &SharedCodeIndex) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let done = {
                let index = index.read().unwrap();
                let coverage = index
                    .index_db()
                    .unwrap()
                    .reads()
                    .semantic_coverage()
                    .unwrap()
                    .coverage;
                coverage.uncovered == 0 && coverage.published > 0 && index.query_pins() == 0
            };
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("synthetic worker must finish within original5s bound");
}
async fn recall(index: &SharedCodeIndex, scope: HardScope, k: usize) -> LaneOutcome {
    let (subsystem, db) = {
        let index = index.read().unwrap();
        (
            index.semantic_subsystem().unwrap(),
            index.index_db().unwrap().clone(),
        )
    };
    subsystem
        .recall
        .recall(
            SemanticRequest {
                query: QUERY.into(),
                scope,
                limit: k,
                policy_fingerprint: "v16-hand-gold-v1".into(),
                generation: db.reads().read_generation().unwrap(),
            },
            QueryControl::new(Duration::from_secs(5)).unwrap(),
        )
        .await
        .unwrap()
}
fn paths(index: &SharedCodeIndex) -> BTreeMap<String, String> {
    let db = index.read().unwrap().index_db().unwrap().clone();
    let conn = db.read_conn().unwrap();
    let mut stmt = conn
        .prepare("SELECT doc_key,file_path FROM document_manifest ORDER BY doc_key")
        .unwrap();
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn check_gold(
    index: &SharedCodeIndex,
    outcome: &LaneOutcome,
    scope: &HardScope,
    k: usize,
) -> Value {
    assert_eq!(outcome.status, LaneStatus::Complete);
    assert!(outcome.coverage.complete);
    let paths = paths(index);
    let mut expected: Vec<_> = DOCS
        .iter()
        .filter(|doc| {
            paths.values().any(|p| p == doc.0)
                && scope.passes(
                    doc.0,
                    if doc.0.ends_with(".py") {
                        Language::Python
                    } else {
                        Language::Rust
                    },
                )
        })
        .map(|doc| {
            let key = paths
                .iter()
                .find(|(_, path)| path.as_str() == doc.0)
                .unwrap()
                .0
                .clone();
            (key, doc.0, doc.3)
        })
        .collect();
    expected.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    expected.truncate(k);
    assert_eq!(outcome.candidates.len(), expected.len());
    for (actual, (key, path, score)) in outcome.candidates.iter().zip(&expected) {
        assert_eq!(
            &actual.document.doc_key, key,
            "hand-gold ordering for {path}"
        );
        assert!(
            (actual.raw_score - score).abs() < 2e-6,
            "{path}: actual={} hand={score}",
            actual.raw_score
        );
        assert_eq!(actual.scoring_spec, "cosine-exact-v1");
    }
    json!({"gold":expected,"actual":outcome.candidates,"coverage":outcome.coverage})
}
async fn public(index: &SharedCodeIndex, scope: &HardScope) -> Value {
    let query = QueryHandle::capture(index).unwrap();
    let result = query
        .search_async(
            QUERY.into(),
            8,
            None,
            SearchRequest {
                retrieval_strategy: Some(RetrievalStrategy::Semantic),
                path_prefix: scope.path_prefix.clone(),
                languages: scope.languages.clone(),
                file_paths: scope.file_paths.clone(),
                boost_file_paths: Some(vec!["outside/strong.rs".into()]),
                file_preselect_limit: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    for hit in result.machine_pack["hits"].as_array().unwrap() {
        let path = hit["file_path"].as_str().unwrap();
        assert!(
            scope.passes(
                path,
                if path.ends_with(".py") {
                    Language::Python
                } else {
                    Language::Rust
                }
            ),
            "hydrate escaped hard scope: {path}"
        );
    }
    assert!(
        !result.machine_pack["hits"].as_array().unwrap().is_empty(),
        "soft hint/preselect erased dense candidates"
    );
    serde_json::to_value(result).unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_hand_cosine_topk_ties_scope_delete_and_space_rejection() {
    let cache = tempfile::tempdir().unwrap();
    let _env = CacheEnv::install(cache.path());
    let probe = Probe::start().await;
    let mut observations = Vec::new();
    for seed in [0, 1, 3] {
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join("synthetic-key");
        std::fs::write(&key, "synthetic-loopback-v16").unwrap();
        let mut config = ProjectConfig::default();
        config.auto_index.enabled = false;
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.network_opt_in = true;
        config.semantic.allow_query_network = true;
        config.semantic.allow_http = true;
        config.semantic.endpoint = probe.endpoint.clone();
        config.semantic.api_key_ref = Some(format!("file:{}", key.display()));
        config.semantic.model_id = format!("synthetic/v16-a-{seed}");
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.max_concurrent = 2;
        let config_path = root.path().join(".codecortex.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        for offset in 0..DOCS.len() {
            let (path, marker, _, _) = DOCS[(seed + offset) % DOCS.len()];
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let text = if path.extension().unwrap() == "py" {
                format!("def {marker}():\n    return 42\n")
            } else {
                format!("pub fn {marker}() -> u32 {{ 42 }}\n")
            };
            std::fs::write(path, text).unwrap();
        }
        let index = Arc::new(RwLock::new(CodeIndex::new(Some(root.path())).unwrap()));
        build(&index, true).await;
        ready(&index).await;
        let scope = HardScope {
            path_prefix: Some("scope/".into()),
            languages: Some(vec![Language::Rust]),
            file_paths: None,
        };
        let before = probe.queries.load(Ordering::SeqCst);
        let first = public(&index, &scope).await;
        assert_eq!(
            probe.queries.load(Ordering::SeqCst),
            before + 1,
            "cold public query must really encode"
        );
        let mut cases = Vec::new();
        for (name, scope, k) in [
            ("filter_before_top1", scope.clone(), 1),
            ("hand_top3_tie", scope.clone(), 3),
            ("hand_orthogonal_zero", scope.clone(), 4),
            (
                "language_path_file_intersection",
                HardScope {
                    file_paths: Some(vec![
                        "scope/tie_b.rs".into(),
                        "outside/strong.rs".into(),
                        "scope/near.py".into(),
                    ]),
                    ..scope.clone()
                },
                2,
            ),
            (
                "python_scope",
                HardScope {
                    path_prefix: Some("scope/".into()),
                    languages: Some(vec![Language::Python]),
                    file_paths: None,
                },
                2,
            ),
            (
                "some_empty",
                HardScope {
                    file_paths: Some(vec![]),
                    ..scope.clone()
                },
                2,
            ),
        ] {
            let outcome = recall(&index, scope.clone(), k).await;
            cases.push(json!({"name":name,"scope":scope,"k":k,"result":check_gold(&index,&outcome,&scope,k)}));
        }
        public(&index, &scope).await;
        assert_eq!(
            probe.queries.load(Ordering::SeqCst),
            before + 1,
            "all scope variants and warm query must reuse encoded vector"
        );
        std::fs::remove_file(root.path().join("scope/top.rs")).unwrap();
        build(&index, false).await;
        ready(&index).await;
        let after_delete = recall(&index, scope.clone(), 2).await;
        let deletion = check_gold(&index, &after_delete, &scope, 2);
        let hydrated = public(&index, &scope).await;
        assert!(hydrated["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["file_path"] != "scope/top.rs"));
        let old = index.read().unwrap().semantic_subsystem().unwrap();
        config.semantic.model_id = format!("synthetic/v16-b-{seed}");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        index
            .write()
            .unwrap()
            .set_project(root.path(), false)
            .unwrap();
        build(&index, false).await;
        ready(&index).await;
        let db = index.read().unwrap().index_db().unwrap().clone();
        let rejected = old
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY.into(),
                    scope: scope.clone(),
                    limit: 8,
                    policy_fingerprint: "old-space-proof".into(),
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
        public(&index, &scope).await;
        assert_eq!(
            probe.queries.load(Ordering::SeqCst),
            before + 2,
            "new model spec must not reuse old query vector"
        );
        observations.push(json!({"seed":seed,"cold_public":first,"cases":cases,"delete":deletion,"after_delete_hydrate":hydrated,"old_space_rejection":rejected,"query_posts":2}));
        index.write().unwrap().close();
    }
    if let Ok(path) = std::env::var("P7_V16_ORACLE_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
}
