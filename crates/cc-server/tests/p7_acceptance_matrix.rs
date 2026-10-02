//! Independent L2 acceptance for P7-011/012/013. All vectors and transports
//! are synthetic; these tests do not certify live providers or corpus quality.
use cc_model::{
    query::{QueryControl, RetrievalStrategy},
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus, LANE_OUTCOME_SCHEMA_VERSION},
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    CcError, CcResult, ContextEnvelope,
};
use cc_server::{engine::CodeIndex, handlers::SharedCodeIndex, query_handle::QueryHandle};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
};

#[cfg(feature = "semantic")]
use std::time::Duration;

fn fixture() -> (tempfile::TempDir, SharedCodeIndex) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".codecortex.json"), json!({"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"deadline_ms":5000,"semantic_timeout_ms":25}}).to_string()).unwrap();
    for (path, text) in [
        ("scope/target.py", "def needle():\n    return 731\n"),
        ("scope/bias.py", "def unrelated():\n    return 997\n"),
        ("outside/high.rs", "fn strongest() { }\n"),
    ] {
        let p = root.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    assert!(index.build_index(true).unwrap().parse_errors.is_empty());
    (root, Arc::new(RwLock::new(index)))
}
fn empty(status: LaneStatus, reason: Option<&str>) -> LaneOutcome {
    LaneOutcome {
        schema_version: LANE_OUTCOME_SCHEMA_VERSION,
        lane_id: "semantic".into(),
        weight: 1.0,
        status,
        elapsed_us: 0,
        candidate_count: 0,
        coverage: if status == LaneStatus::Complete {
            LaneCoverage::complete(Some(0), 0)
        } else {
            LaneCoverage::not_run()
        },
        truncation_reason: reason.map(str::to_owned),
        candidates: vec![],
    }
}
fn lane(value: &ContextEnvelope) -> &Value {
    value.evidence_summary["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["lane_id"] == "semantic")
        .unwrap()
}
#[derive(Clone)]
enum Reply {
    Outcome(LaneOutcome),
    Error,
    Pending,
}
struct SequencedPort {
    replies: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<SemanticRequest>>,
}
impl SemanticRecall for SequencedPort {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(request);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected extra recall");
            match reply {
                Reply::Outcome(o) => {
                    control.check()?;
                    Ok(o)
                }
                Reply::Error => Err(CcError::Search("offline injected provider failure".into())),
                Reply::Pending => std::future::pending().await,
            }
        })
    }
}
fn ranking(v: &ContextEnvelope) -> Vec<Value> {
    v.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            json!([
                h["chunk_id"],
                h["rerank_score"],
                h["score_trace"],
                h["text"]
            ])
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_request_fault_recovery_and_repeated_failure_never_reuses_complete_success() {
    for strategy in [RetrievalStrategy::Auto, RetrievalStrategy::Semantic] {
        for (failure, status, reason) in [
            (Reply::Pending, "timeout", "semantic_deadline"),
            (Reply::Error, "error", "semantic_read_error"),
            (
                Reply::Outcome(empty(LaneStatus::Unavailable, Some("semantic_capacity"))),
                "unavailable",
                "semantic_capacity",
            ),
        ] {
            let (_root, runtime) = fixture();
            let port = Arc::new(SequencedPort {
                replies: Mutex::new(VecDeque::from([
                    failure.clone(),
                    Reply::Outcome(empty(LaneStatus::Complete, None)),
                    failure,
                ])),
                calls: Mutex::new(vec![]),
            });
            runtime
                .read()
                .unwrap()
                .set_semantic_recall(Some(port.clone()));
            let handle = QueryHandle::capture(&runtime).unwrap();
            let mut results = vec![];
            for expected in [status, "complete", status] {
                let v = handle
                    .search_async(
                        "needle".into(),
                        1,
                        None,
                        SearchRequest {
                            retrieval_strategy: Some(strategy),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                assert_eq!(lane(&v)["status"], expected);
                assert_eq!(lane(&v)["candidate_count"], 0);
                if expected == "complete" {
                    assert!(lane(&v)["truncation_reason"].is_null());
                    assert_eq!(lane(&v)["coverage"]["complete"], true);
                } else {
                    assert_eq!(lane(&v)["truncation_reason"], reason);
                    assert_eq!(lane(&v)["coverage"]["complete"], false);
                }
                assert_eq!(v.machine_pack["hits"][0]["file_path"], "scope/target.py");
                results.push(ranking(&v));
            }
            assert_eq!(
                results[0], results[1],
                "failed optional lane must cast zero votes"
            );
            assert_eq!(
                results[1], results[2],
                "warm history must not alter local ranking/source"
            );
            assert_eq!(
                port.calls.lock().unwrap().len(),
                3,
                "each identical query must obtain a fresh lane receipt"
            );
        }
    }
}

#[tokio::test]
async fn explicit_empty_hard_scopes_never_invoke_optional_port() {
    let (_root, runtime) = fixture();
    let port = Arc::new(SequencedPort {
        replies: Mutex::new(VecDeque::new()),
        calls: Mutex::new(vec![]),
    });
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(port.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    for request in [
        SearchRequest {
            file_paths: Some(vec![]),
            ..Default::default()
        },
        SearchRequest {
            languages: Some(vec![]),
            ..Default::default()
        },
        SearchRequest {
            path_prefix: Some("scope".into()),
            file_paths: Some(vec!["outside/high.rs".into()]),
            ..Default::default()
        },
    ] {
        let v = handle
            .search_async(
                "needle".into(),
                5,
                None,
                SearchRequest {
                    retrieval_strategy: Some(RetrievalStrategy::Semantic),
                    ..request
                },
            )
            .await
            .unwrap();
        assert!(v.machine_pack["hits"].as_array().unwrap().is_empty());
        assert_eq!(lane(&v)["status"], "disabled");
    }
    assert!(port.calls.lock().unwrap().is_empty());
}

#[cfg(feature = "semantic")]
mod wired {
    use super::*;
    use cc_model::{config::ProjectConfig, retrieval::HardScope, Language};
    use cc_semantic::{
        cache::{QueryCacheKey, QueryVector},
        ports::QueryInput,
        types::InputDigest,
    };
    use cc_server::semantic_wiring::{assemble_with, SemanticSubsystem};

    struct World {
        _root: tempfile::TempDir,
        _cache: tempfile::TempDir,
        runtime: SharedCodeIndex,
        subsystem: SemanticSubsystem,
    }
    impl World {
        fn new() -> Self {
            let (root, runtime) = fixture();
            let cache = tempfile::tempdir().unwrap();
            let db = runtime.read().unwrap().index_db().unwrap().clone();
            let mut config = ProjectConfig::default();
            config.semantic.enabled = true;
            config.semantic.model_id = "synthetic/p7-acceptance".into();
            config.semantic.dimensions = Some(2);
            config.semantic.max_input_tokens = Some(8192);
            config.semantic.max_batch_items = Some(16);
            config.semantic.endpoint = "https://offline.invalid/v1".into();
            let subsystem = assemble_with(
                &root.path().to_string_lossy(),
                &config,
                db.clone(),
                |name| {
                    if name == cc_semantic::cache::CACHE_ROOT_ENV {
                        Some(cache.path().to_string_lossy().into_owned())
                    } else {
                        None
                    }
                },
                false,
            )
            .unwrap()
            .unwrap();
            let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
            let space = subsystem.space.digest().unwrap();
            conn.execute(
                "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
                [space.as_str()],
            )
            .unwrap();
            for (symbol, path, vector) in [
                ("needle", "scope/target.py", vec![0.8, 0.6]),
                ("unrelated", "scope/bias.py", vec![0.1, 1.0]),
                ("strongest", "outside/high.rs", vec![1.0, 0.0]),
            ] {
                let engine = cc_search::SearchEngine::new(db.clone(), &Default::default(), None);
                let result = engine
                    .search_with_diagnostics(&SearchRequest {
                        query: symbol.into(),
                        top_k: 5,
                        ..Default::default()
                    })
                    .unwrap();
                let candidate = result
                    .lanes
                    .iter()
                    .find(|l| l.lane_id == "exact_symbol")
                    .unwrap()
                    .candidates
                    .iter()
                    .find(|c| {
                        let row: String = conn
                            .query_row(
                                "SELECT file_path FROM document_manifest WHERE doc_key=?1",
                                [&c.document.doc_key],
                                |r| r.get(0),
                            )
                            .unwrap();
                        row == path
                    })
                    .unwrap();
                let input = InputDigest::of_input(candidate.document.doc_key.as_bytes()).unwrap();
                let artifact = subsystem
                    .cache
                    .put(&subsystem.space, &input, &subsystem.doc_spec, &vector, 1000)
                    .unwrap();
                conn.execute("INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,space_id,artifact_ref,published_at,published_incarnation) VALUES(?1,?2,?3,?4,?5,?6,?7,'2026-01-01','synthetic')",rusqlite::params![candidate.document.doc_key,candidate.document.doc_version,path,candidate.document.encoding_key,input.as_str(),space.as_str(),artifact.as_str()]).unwrap();
            }
            let input = QueryInput::from_bytes(b"conceptual acceptance query").unwrap();
            let key =
                QueryCacheKey::new(&subsystem.namespace, &subsystem.query_spec, &input).unwrap();
            subsystem
                .query_cache
                .put(
                    key,
                    QueryVector {
                        dimension: 2,
                        data: vec![1.0, 0.0],
                    },
                )
                .unwrap();
            runtime
                .read()
                .unwrap()
                .set_semantic_recall(Some(subsystem.recall.clone()));
            Self {
                _root: root,
                _cache: cache,
                runtime,
                subsystem,
            }
        }
        async fn recall(&self, scope: HardScope) -> LaneOutcome {
            let generation = self
                .runtime
                .read()
                .unwrap()
                .index_db()
                .unwrap()
                .reads()
                .read_generation()
                .unwrap();
            self.subsystem
                .recall
                .recall(
                    SemanticRequest {
                        query: "conceptual acceptance query".into(),
                        scope,
                        limit: 1,
                        policy_fingerprint: "p7-acceptance-synthetic".into(),
                        generation,
                    },
                    QueryControl::new(Duration::from_secs(5)).unwrap(),
                )
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn hard_scope_filters_stronger_outsider_before_top_k_and_intersects_language() {
        let world = World::new();
        let all = world.recall(HardScope::default()).await;
        let db = world.runtime.read().unwrap().index_db().unwrap().clone();
        let conn = db.read_conn().unwrap();
        let path = |o: &LaneOutcome| {
            conn.query_row(
                "SELECT file_path FROM document_manifest WHERE doc_key=?1",
                [&o.candidates[0].document.doc_key],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(path(&all), "outside/high.rs");
        drop(conn);
        let scoped = world
            .recall(HardScope {
                path_prefix: Some("scope".into()),
                languages: Some(vec![Language::Python]),
                file_paths: Some(vec!["scope/target.py".into(), "outside/high.rs".into()]),
            })
            .await;
        scoped.validate().unwrap();
        assert_eq!(scoped.status, LaneStatus::Complete);
        assert_eq!(scoped.candidate_count, 1);
        let x = f64::from(0.8_f32);
        let y = f64::from(0.6_f32);
        assert!(
            (scoped.candidates[0].raw_score - x / (x * x + y * y).sqrt()).abs() < 1e-12,
            "cosine must match the hand calculation on the stored f32 vector"
        );
        let conn = db.read_conn().unwrap();
        let selected: String = conn
            .query_row(
                "SELECT file_path FROM document_manifest WHERE doc_key=?1",
                [&scoped.candidates[0].document.doc_key],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(selected, "scope/target.py");
    }

    #[tokio::test]
    async fn dense_recall_outside_soft_preselection_survives_final_source_hydration() {
        let world = World::new();
        let handle = QueryHandle::capture(&world.runtime).unwrap();
        let v = handle
            .search_async(
                "conceptual acceptance query".into(),
                5,
                None,
                SearchRequest {
                    retrieval_strategy: Some(RetrievalStrategy::Auto),
                    path_prefix: Some("scope".into()),
                    languages: Some(vec![Language::Python]),
                    boost_file_paths: Some(vec!["scope/bias.py".into()]),
                    recent_file_paths: Some(vec!["scope/bias.py".into()]),
                    pinned_file_paths: Some(vec!["scope/bias.py".into()]),
                    overlay_file_paths: Some(vec!["scope/bias.py".into()]),
                    file_preselect_limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let hits = v.machine_pack["hits"].as_array().unwrap();
        assert!(
            hits.iter().any(|h| h["file_path"] == "scope/target.py"
                && h["text"].as_str().unwrap().contains("return 731")),
            "soft hints cannot erase an independently recalled dense source: {v:?}"
        );
        assert!(hits
            .iter()
            .all(|h| h["file_path"].as_str().unwrap().starts_with("scope/")));
        assert_eq!(
            v.evidence_summary["source_freshness"]["dense_manifest_fence"]["skipped"],
            0
        );
    }

    #[tokio::test]
    async fn uncovered_language_is_partial_and_deleted_document_never_returns_on_second_query() {
        let world = World::new();
        let db = world.runtime.read().unwrap().index_db().unwrap().clone();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        conn.execute(
            "DELETE FROM semantic_manifest WHERE file_path='outside/high.rs'",
            [],
        )
        .unwrap();
        let uncovered = world
            .recall(HardScope {
                languages: Some(vec![Language::Rust]),
                ..Default::default()
            })
            .await;
        assert_eq!(uncovered.status, LaneStatus::Partial);
        assert!(!uncovered.coverage.complete);
        assert_eq!(
            uncovered.truncation_reason.as_deref(),
            Some("semantic_coverage_uncovered")
        );
        assert_eq!(uncovered.candidate_count, 0);
        let python = HardScope {
            languages: Some(vec![Language::Python]),
            file_paths: Some(vec!["scope/target.py".into()]),
            ..Default::default()
        };
        assert_eq!(world.recall(python.clone()).await.candidate_count, 1);
        db.writes()
            .remove_files_batch(&["scope/target.py".to_owned()])
            .unwrap();
        let deleted = world.recall(python).await;
        assert_eq!(deleted.status, LaneStatus::Complete);
        assert_eq!(deleted.candidate_count, 0);
        assert!(deleted.coverage.complete);
    }
}

#[cfg(feature = "semantic")]
mod transport {
    use super::*;
    use cc_semantic::{
        admission::InputBudget,
        cache::{encode_queries, namespace_key, QueryCacheKey, QueryVectorCache},
        ports::QueryInput,
        providers::openai_compatible::{
            EmbeddingApiKey, EmbeddingHttpTransport, HttpRequest, HttpResponse,
            OpenAiCompatibleConfig, OpenAiCompatibleProvider, TransportError,
        },
        spec::{QueryEncodingSpec, VectorSpace},
    };
    struct MemoryTransport {
        responses: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
        calls: Mutex<usize>,
    }
    impl EmbeddingHttpTransport for MemoryTransport {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            assert_eq!(request.url, "https://offline.invalid/v1/embeddings");
            assert_eq!(request.timeout, Duration::from_millis(40));
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["input"], json!(["synthetic alpha", "synthetic beta"]));
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected transport call")
        }
    }
    fn response(status: u16, data: Value) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: serde_json::to_vec(&json!({"model":"synthetic/transport","data":data})).unwrap(),
        })
    }
    fn success() -> Result<HttpResponse, TransportError> {
        // Out-of-order indexes must be restored before entering the cache.
        response(
            200,
            json!([{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]),
        )
    }
    #[test]
    fn rejected_transport_batch_never_poison_caches_and_recovery_is_reused_without_another_call() {
        let faults = vec![
            ("429", response(429, json!([]))),
            ("500", response(500, json!([]))),
            ("auth", response(401, json!([]))),
            ("timeout", Err(TransportError::Timeout)),
            ("cancelled", Err(TransportError::Cancelled)),
            (
                "missing-index",
                response(200, json!([{"index":0,"embedding":[1.0,0.0]}])),
            ),
            (
                "duplicate-index",
                response(
                    200,
                    json!([{"index":0,"embedding":[1.0,0.0]},{"index":0,"embedding":[0.0,1.0]}]),
                ),
            ),
            (
                "bad-dimension",
                response(
                    200,
                    json!([{"index":0,"embedding":[1.0,0.0]},{"index":1,"embedding":[1.0]}]),
                ),
            ),
            (
                "zero",
                response(
                    200,
                    json!([{"index":0,"embedding":[1.0,0.0]},{"index":1,"embedding":[0.0,0.0]}]),
                ),
            ),
            (
                "non-numeric",
                response(
                    200,
                    json!([{"index":0,"embedding":[1.0,0.0]},{"index":1,"embedding":["NaN",1.0]}]),
                ),
            ),
        ];
        for (case, fault) in faults {
            let seam = Arc::new(MemoryTransport {
                responses: Mutex::new(VecDeque::from([fault, success()])),
                calls: Mutex::new(0),
            });
            let space = VectorSpace::new("synthetic/transport", 2).unwrap();
            let mut config = OpenAiCompatibleConfig::new(
                space.clone(),
                "https://offline.invalid/v1",
                EmbeddingApiKey::new("synthetic-unusable-key"),
            );
            // In-memory seam only; this opt-in assembles a fake implementation
            // and never authorizes or constructs a socket/live transport.
            config.egress.network_opt_in = true;
            config.timeout = Duration::from_millis(40);
            config.transport = Some(seam.clone());
            let provider = OpenAiCompatibleProvider::new(config);
            let namespace = namespace_key(case).unwrap();
            let spec =
                QueryEncodingSpec::new(space, None, 8192, cc_model::chunk_policy::TOKEN_ESTIMATOR)
                    .unwrap();
            let cache = QueryVectorCache::new(8, 4096);
            let budget = InputBudget::validated(2, 4096, 1024).unwrap();
            let inputs = [
                (0, b"synthetic alpha".to_vec()),
                (1, b"synthetic beta".to_vec()),
            ];
            let error =
                encode_queries(&provider, &cache, &namespace, &spec, &budget, &inputs).unwrap_err();
            if case == "timeout" {
                assert!(matches!(error, CcError::QueryTimedOut));
            }
            if case == "cancelled" {
                assert!(matches!(error, CcError::QueryCancelled));
            }
            for (_, bytes) in &inputs {
                let input = QueryInput::from_bytes(bytes).unwrap();
                let key = QueryCacheKey::new(&namespace, &spec, &input).unwrap();
                assert!(
                    cache.get(&key).is_none(),
                    "whole rejected batch must remain uncached: {case}"
                );
            }
            assert_eq!(*seam.calls.lock().unwrap(), 1);
            let recovered =
                encode_queries(&provider, &cache, &namespace, &spec, &budget, &inputs).unwrap();
            let warmed =
                encode_queries(&provider, &cache, &namespace, &spec, &budget, &inputs).unwrap();
            assert_eq!(
                recovered, warmed,
                "cache history must preserve exact vectors/order: {case}"
            );
            for (index, expected) in [(0, vec![1.0, 0.0]), (1, vec![0.0, 1.0])] {
                let input = QueryInput::from_bytes(&inputs[index].1).unwrap();
                let key = QueryCacheKey::new(&namespace, &spec, &input).unwrap();
                assert_eq!(cache.get(&key).unwrap().data, expected);
            }
            assert_eq!(
                *seam.calls.lock().unwrap(),
                2,
                "second recovery must be warm: {case}"
            );
            assert!(seam.responses.lock().unwrap().is_empty());
        }
    }

    struct BlockingTransport {
        started: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl EmbeddingHttpTransport for BlockingTransport {
        fn post_json(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
            assert_eq!(request.url, "https://offline.invalid/v1/embeddings");
            self.started
                .lock()
                .unwrap()
                .take()
                .unwrap()
                .send(())
                .unwrap();
            // Safety bound only: test controls release; this is no socket or
            // assertion about a real HTTP client's deadline implementation.
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            Err(TransportError::Cancelled)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn slow_offline_transport_cancellation_keeps_worker_capacity_until_physical_exit() {
        use cc_search::execution::{until, ExecutionPool};
        use cc_semantic::ports::EmbeddingProvider;
        for cancel_parent in [false, true] {
            let (_root, runtime) = fixture();
            let pool = ExecutionPool::new(1, 0, 1).unwrap();
            let parent = QueryControl::new(Duration::from_secs(5)).unwrap();
            let admission = QueryControl::new(Duration::from_secs(5)).unwrap();
            let (start_tx, mut start_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let mut config = OpenAiCompatibleConfig::new(
                VectorSpace::new("synthetic/transport", 2).unwrap(),
                "https://offline.invalid/v1",
                EmbeddingApiKey::new("synthetic-unusable-key"),
            );
            config.egress.network_opt_in = true;
            config.transport = Some(Arc::new(BlockingTransport {
                started: Mutex::new(Some(start_tx)),
                release: Mutex::new(release_rx),
            }));
            let provider = OpenAiCompatibleProvider::new(config);
            let mut work = Box::pin(pool.run_cpu(admission, move || {
                provider
                    .embed_queries(&[QueryInput::from_bytes(b"synthetic slow query").unwrap()])
                    .map_err(|_| CcError::QueryCancelled)
            }));
            // Start the worker before starting the short lane clock; scheduler
            // startup latency cannot substitute for a real in-flight cancel.
            tokio::select! {
                ready = &mut start_rx => ready.unwrap(),
                result = work.as_mut() => panic!("transport returned before release: {result:?}"),
                _ = tokio::time::sleep(Duration::from_secs(1)) => panic!("worker did not start"),
            }
            let child = parent.child(Duration::from_millis(20));
            if cancel_parent {
                parent.cancel();
            }
            let result = until(&child, work.as_mut()).await;
            assert!(if cancel_parent {
                matches!(result, Err(CcError::QueryCancelled))
            } else {
                matches!(result, Err(CcError::QueryTimedOut))
            });
            drop(work);
            assert_eq!(
                pool.stats().cpu_in_flight,
                1,
                "future drop is not transport termination"
            );
            assert!(matches!(
                pool.run_cpu(
                    QueryControl::new(Duration::from_secs(1)).unwrap(),
                    || Ok(())
                )
                .await,
                Err(CcError::QueryBusy)
            ));
            // Real fixture runtime + only read-pool connection remain usable
            // during the isolated transport worker's blocked interval.
            {
                let guard = runtime
                    .try_write()
                    .expect("transport retained CodeIndex lock");
                let db = guard.index_db().unwrap();
                let conn = db.read_conn().unwrap();
                assert_eq!(
                    conn.query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
                        .unwrap(),
                    1
                );
            }
            if !cancel_parent {
                parent
                    .check()
                    .expect("lane timeout must preserve total/local budget");
            }
            release_tx.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while {
                    let stats = pool.stats();
                    stats.cpu_in_flight != 0 || stats.cpu_admitted != 0
                } {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(
                pool.run_cpu(QueryControl::new(Duration::from_secs(1)).unwrap(), || Ok(
                    42
                ))
                .await
                .unwrap(),
                42
            );
        }
    }
}
