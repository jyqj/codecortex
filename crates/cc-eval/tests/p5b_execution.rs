//! P5-B real parser/SQLite integration with explicitly fake async recall.
//! Fake success is not a provider/model certification.
use cc_model::{
    query::{QueryControl, RetrievalStrategy},
    retrieval::{LaneCoverage, LaneOutcome, LaneStatus, LANE_OUTCOME_SCHEMA_VERSION},
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    CcError, CcResult,
};
use cc_server::{
    engine::CodeIndex,
    handlers::{self, SharedCodeIndex},
    query_handle::QueryHandle,
};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, RwLock,
    },
    time::Duration,
};
use tokio::sync::Notify;

fn fixture(query: Value) -> (tempfile::TempDir, SharedCodeIndex) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join(".codecortex.json"),
        json!({"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":query})
            .to_string(),
    )
    .unwrap();
    std::fs::write(d.path().join("target.py"), "def needle():\n    return 7\n").unwrap();
    std::fs::write(d.path().join("other.py"), "def other():\n    return 8\n").unwrap();
    let mut index = CodeIndex::new(Some(d.path())).unwrap();
    assert!(index.build_index(true).unwrap().parse_errors.is_empty());
    (d, Arc::new(RwLock::new(index)))
}
fn empty() -> LaneOutcome {
    LaneOutcome {
        schema_version: LANE_OUTCOME_SCHEMA_VERSION,
        lane_id: "semantic".into(),
        weight: 1.0,
        status: LaneStatus::Complete,
        elapsed_us: 0,
        candidate_count: 0,
        coverage: LaneCoverage::complete(Some(0), 0),
        truncation_reason: None,
        candidates: vec![],
    }
}
fn semantic_candidate(runtime: &SharedCodeIndex) -> LaneOutcome {
    let index = runtime.read().unwrap();
    let engine =
        cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &Default::default(), None);
    let result = engine
        .search_with_diagnostics(&SearchRequest {
            query: "needle".into(),
            top_k: 5,
            ..Default::default()
        })
        .unwrap();
    let mut candidate = result
        .lanes
        .iter()
        .find(|l| l.lane_id == "exact_symbol")
        .unwrap()
        .candidates[0]
        .clone();
    candidate.lane_id = "semantic".into();
    candidate.lane_rank = 1;
    candidate.exact_identity = false;
    candidate.raw_score = 0.8;
    candidate.scoring_spec = "fake-recall-test-v1".into();
    // The dense hydration fence requires a current publication even for a
    // synthetic recall port. Seed metadata only; no provider/model is called.
    let conn = rusqlite::Connection::open(index.index_db().unwrap().admin().db_path()).unwrap();
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('fake-recall-test','{}','active')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
         input_digest,space_id,artifact_ref,published_at,published_incarnation) \
         VALUES(?1,?2,'target.py',?3,'synthetic-input','fake-recall-test',\
         'synthetic-artifact','2026-01-01','synthetic-incarnation')",
        rusqlite::params![
            candidate.document.doc_key,
            candidate.document.doc_version,
            candidate.document.encoding_key,
        ],
    )
    .unwrap();
    LaneOutcome {
        candidate_count: 1,
        candidates: vec![candidate],
        coverage: LaneCoverage::complete(Some(1), 1),
        ..empty()
    }
}
struct DropCount(Arc<AtomicUsize>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Fake {
    reply: LaneOutcome,
    hold: bool,
    started: Notify,
    release: Notify,
    calls: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}
impl Fake {
    fn new(reply: LaneOutcome, hold: bool) -> Arc<Self> {
        Arc::new(Self {
            reply,
            hold,
            started: Notify::new(),
            release: Notify::new(),
            calls: AtomicUsize::new(0),
            dropped: Arc::new(AtomicUsize::new(0)),
        })
    }
}
impl SemanticRecall for Fake {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            let _drop = DropCount(self.dropped.clone());
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(!request.policy_fingerprint.is_empty());
            self.started.notify_one();
            if self.hold {
                self.release.notified().await;
            }
            control.check()?;
            Ok(self.reply.clone())
        })
    }
}
fn lane(value: &cc_model::ContextEnvelope, id: &str) -> Value {
    value.evidence_summary["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["lane_id"] == id)
        .unwrap()
        .clone()
}
fn override_strategy(strategy: RetrievalStrategy) -> SearchRequest {
    SearchRequest {
        retrieval_strategy: Some(strategy),
        ..Default::default()
    }
}
async fn started(fake: &Fake) {
    tokio::time::timeout(Duration::from_secs(5), fake.started.notified())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_never_invokes_installed_port_and_missing_auto_is_local() {
    let (_d, runtime) = fixture(json!({}));
    let fake = Fake::new(empty(), false);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let value = handle
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
    assert_eq!(value.machine_pack["hits"][0]["file_path"], "target.py");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        value.evidence_summary["retrieval"]["policy"]["semantic_state"],
        "disabled"
    );
    runtime.read().unwrap().set_semantic_recall(None);
    let auto = handle
        .search_async(
            "needle".into(),
            1,
            None,
            override_strategy(RetrievalStrategy::Auto),
        )
        .await
        .unwrap();
    assert_eq!(
        auto.evidence_summary["retrieval"]["policy"]["effective"],
        "local"
    );
    assert_eq!(
        auto.evidence_summary["retrieval"]["policy"]["semantic_state"],
        "not_configured"
    );
    assert!(matches!(
        handle
            .search_async(
                "needle".into(),
                1,
                None,
                override_strategy(RetrievalStrategy::Semantic)
            )
            .await,
        Err(CcError::SemanticUnavailable)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slow_fake_does_not_hold_codeindex_lock_or_the_only_sql_connection() {
    let (d, runtime) =
        fixture(json!({"strategy":"auto","deadline_ms":10000,"semantic_timeout_ms":8000}));
    let fake = Fake::new(empty(), true);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let worker = handle.clone();
    let query = tokio::spawn(async move {
        worker
            .search_async("needle".into(), 5, None, Default::default())
            .await
    });
    started(&fake).await;
    {
        let guard = runtime
            .try_write()
            .expect("query retained CodeIndex guard across await");
        let db = guard.index_db().unwrap();
        let connection = db.read_conn().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT 1", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            1
        );
    }
    let status_runtime = runtime.clone();
    let status = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || {
            handlers::facade::handle_status(status_runtime, "index")
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert!(status.is_object());
    std::fs::write(d.path().join("target.py"), "def needle():\n    return 99\n").unwrap();
    let index_runtime = runtime.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            handlers::core::build_index_scoped(index_runtime, false, None)
        }),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    fake.release.notify_one();
    assert!(
        matches!(query.await.unwrap(), Err(CcError::RetrievalChanged { .. })),
        "changed provider basis cannot be accepted"
    );
    runtime.read().unwrap().set_semantic_recall(None);
    let fresh = handle
        .search_async(
            "needle".into(),
            1,
            None,
            override_strategy(RetrievalStrategy::Local),
        )
        .await
        .unwrap();
    assert!(fresh.machine_pack["hits"][0]["text"]
        .as_str()
        .unwrap()
        .contains("99"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn semantic_timeout_keeps_local_result_visible_and_does_not_cache_failure() {
    let (_d, runtime) =
        fixture(json!({"strategy":"auto","deadline_ms":3000,"semantic_timeout_ms":15}));
    let fake = Fake::new(empty(), true);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    for _ in 0..2 {
        let value = handle
            .search_async("needle".into(), 1, None, Default::default())
            .await
            .unwrap();
        assert_eq!(value.machine_pack["hits"][0]["file_path"], "target.py");
        assert_eq!(lane(&value, "semantic")["status"], "timeout");
        assert_eq!(lane(&value, "semantic")["candidate_count"], 0);
        let (_, status) =
            cc_eval::benchmark::normalizer::mcp(&serde_json::to_value(value).unwrap()).unwrap();
        assert_eq!(status, cc_eval::benchmark::schema::ResultStatus::Partial);
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(fake.dropped.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn semantic_candidates_are_hydrated_from_current_scoped_documents() {
    let (_d, runtime) = fixture(json!({"strategy":"auto"}));
    let reply = semantic_candidate(&runtime);
    let fake = Fake::new(reply.clone(), false);
    runtime.read().unwrap().set_semantic_recall(Some(fake));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let value = handle
        .search_async("conceptual question".into(), 5, None, Default::default())
        .await
        .unwrap();
    assert!(value.machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["file_path"] == "target.py"));
    assert_eq!(lane(&value, "semantic")["candidate_count"], 1);
    assert_eq!(
        value.evidence_summary["source_freshness"]["dense_manifest_fence"]["checked"],
        1
    );
    assert_eq!(
        value.evidence_summary["source_freshness"]["dense_manifest_fence"]["skipped"], 0,
        "synthetic candidate must pass the real publication fence"
    );
    for hit in value.machine_pack["hits"].as_array().unwrap() {
        let sum: f64 = hit["score_trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x[1].as_f64().unwrap())
            .sum();
        assert!((sum - hit["rerank_score"].as_f64().unwrap()).abs() < 1e-9);
    }
    let scoped = SearchRequest {
        file_paths: Some(vec!["other.py".into()]),
        ..Default::default()
    };
    assert!(handle
        .search_async("conceptual question".into(), 5, None, scoped)
        .await
        .is_err());
    for mutate in 0..4 {
        let mut malformed = reply.clone();
        match mutate {
            0 => malformed.candidates[0].document.doc_version = "0".repeat(64),
            1 => malformed.candidates[0].raw_score = f64::NAN,
            2 => malformed.candidates[0].exact_identity = true,
            _ => {
                let mut c = malformed.candidates[0].clone();
                c.lane_rank = 2;
                malformed.candidates.push(c);
                malformed.candidate_count = 2;
                malformed.coverage.total_lower_bound = 2;
            }
        }
        runtime
            .read()
            .unwrap()
            .set_semantic_recall(Some(Fake::new(malformed, false)));
        assert!(
            handle
                .search_async("conceptual question".into(), 5, None, Default::default())
                .await
                .is_err(),
            "malformed fake={mutate}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_and_future_drop_release_async_work_without_a_detached_task() {
    let (_d, runtime) =
        fixture(json!({"strategy":"auto","deadline_ms":10000,"semantic_timeout_ms":8000}));
    let fake = Fake::new(empty(), true);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let control = QueryControl::new(Duration::from_secs(5)).unwrap();
    let worker = handle.clone();
    let supplied = control.clone();
    let first = tokio::spawn(async move {
        worker
            .search_async(
                "needle".into(),
                1,
                None,
                SearchRequest {
                    control: Some(supplied),
                    ..Default::default()
                },
            )
            .await
    });
    started(&fake).await;
    control.cancel();
    assert!(matches!(first.await.unwrap(), Err(CcError::QueryCancelled)));
    assert_eq!(fake.dropped.load(Ordering::SeqCst), 1);
    let worker = handle.clone();
    let second = tokio::spawn(async move {
        worker
            .search_async("needle".into(), 1, None, Default::default())
            .await
    });
    started(&fake).await;
    second.abort();
    assert!(second.await.unwrap_err().is_cancelled());
    assert_eq!(fake.dropped.load(Ordering::SeqCst), 2);
    runtime.read().unwrap().set_semantic_recall(None);
    handle
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_handle_survives_idle_close_but_poison_recovery_invalidates_old_engine() {
    let (_d, runtime) = fixture(json!({}));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let original = handle.db_instance_id();
    runtime.write().unwrap().close();
    let value = handle
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
    assert_eq!(value.machine_pack["hits"][0]["file_path"], "target.py");
    runtime.write().unwrap().reopen().unwrap();
    let replacement = QueryHandle::capture(&runtime).unwrap();
    assert_ne!(replacement.db_instance_id(), original);
    runtime
        .write()
        .unwrap()
        .invalidate_search_cache_after_poison();
    assert!(matches!(
        replacement
            .search_async("needle".into(), 1, None, Default::default())
            .await,
        Err(CcError::QueryInvalidated)
    ));
    QueryHandle::capture(&runtime)
        .unwrap()
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn async_dispatch_preserves_resolution_freshness_and_project_identity() {
    let (_d, runtime) = fixture(json!({}));
    let search = handlers::context::search_async(
        runtime.clone(),
        "needle".into(),
        1,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    assert_eq!(search["resolution_freshness"]["complete"], true);
    let context =
        handlers::context::context_async(runtime.clone(), "needle".into(), None, false, None)
            .await
            .unwrap();
    assert_eq!(context["resolution_freshness"]["complete"], true);
    let old = QueryHandle::capture(&runtime).unwrap();
    let fake = Fake::new(empty(), false);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let (other_dir, _other) = fixture(json!({"strategy":"auto"}));
    runtime
        .write()
        .unwrap()
        .set_project(other_dir.path(), false)
        .unwrap();
    let new = QueryHandle::capture(&runtime).unwrap();
    assert_ne!(old.project_path(), new.project_path());
    let value = new
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
    assert_eq!(
        value.evidence_summary["retrieval"]["policy"]["semantic_state"],
        "not_configured"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    old.search_async(
        "needle".into(),
        1,
        None,
        override_strategy(RetrievalStrategy::Auto),
    )
    .await
    .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_empty_scope_disables_optional_port_without_invoking_it() {
    let (_d, runtime) = fixture(json!({"strategy":"auto"}));
    let fake = Fake::new(semantic_candidate(&runtime), false);
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(fake.clone()));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let value = handle
        .search_async(
            "needle".into(),
            1,
            None,
            SearchRequest {
                file_paths: Some(vec![]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert!(value.machine_pack["hits"].as_array().unwrap().is_empty());
    assert!(value.evidence_summary["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|l| l["status"] == "disabled"));
}

struct PanickingPort {
    on_construction: bool,
}
impl SemanticRecall for PanickingPort {
    fn recall(
        &self,
        _request: SemanticRequest,
        _control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        assert!(!self.on_construction, "injected port construction panic");
        Box::pin(async { panic!("injected port future panic") })
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partial_receipt_and_provider_panics_remain_visible_not_complete_empty() {
    let (_d, runtime) = fixture(json!({"strategy":"auto"}));
    let handle = QueryHandle::capture(&runtime).unwrap();
    let mut reply = semantic_candidate(&runtime);
    reply.status = LaneStatus::Partial;
    reply.coverage.complete = false;
    reply.truncation_reason = Some("candidate_limit".into());
    reply.weight = 999.0;
    runtime
        .read()
        .unwrap()
        .set_semantic_recall(Some(Fake::new(reply, false)));
    let value = handle
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
    assert_eq!(lane(&value, "semantic")["status"], "partial");
    assert_eq!(lane(&value, "semantic")["weight"], 1.0);
    for on_construction in [false, true] {
        runtime
            .read()
            .unwrap()
            .set_semantic_recall(Some(Arc::new(PanickingPort { on_construction })));
        let value = handle
            .search_async("needle".into(), 1, None, Default::default())
            .await
            .unwrap();
        assert_eq!(lane(&value, "semantic")["status"], "error");
        assert_eq!(value.machine_pack["hits"][0]["file_path"], "target.py");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; real stdio query policy/handles"]
async fn real_mcp_preserves_tool_schema_and_exposes_offline_query_policy() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let (d, _runtime) = fixture(json!({"strategy":"auto"}));
    let binary = std::path::PathBuf::from(
        std::env::var("CODECORTEX_BENCH_BINARY").expect("explicit built binary"),
    );
    let mut client = McpStdio::spawn(&binary, d.path(), Duration::from_secs(20))
        .await
        .unwrap();
    client
        .call("index", json!({"path":d.path(),"full":true}))
        .await
        .unwrap();
    let search = client
        .call(
            "search",
            json!({"query":"needle","top_k":1,"project_path":d.path()}),
        )
        .await
        .unwrap();
    assert_eq!(search["resolution_freshness"]["complete"], true);
    assert_eq!(search["machine_pack"]["hits"][0]["file_path"], "target.py");
    assert_eq!(
        search["evidence_summary"]["retrieval"]["policy"]["effective"],
        "local"
    );
    assert_eq!(
        search["evidence_summary"]["retrieval"]["policy"]["semantic_state"],
        "not_configured"
    );
    assert_eq!(
        search["evidence_summary"]["retrieval"]["lanes"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let status = client
        .call("status", json!({"aspect":"index"}))
        .await
        .unwrap();
    assert_eq!(status["diagnostics"]["query_execution"]["cpu_limit"], 4);
    assert_eq!(status["diagnostics"]["query_execution"]["queue_limit"], 32);
    assert_eq!(status["diagnostics"]["semantic_configured"], false);
    client.close().await.unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("p5b-stdio.json"),
            serde_json::to_vec_pretty(&json!({"search":search,"status":status})).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_expired_request_cannot_return_a_warm_cache_hit() {
    let (_d, runtime) = fixture(json!({}));
    let handle = QueryHandle::capture(&runtime).unwrap();
    handle
        .search_async("needle".into(), 1, None, Default::default())
        .await
        .unwrap();
    let request = SearchRequest {
        control: Some(QueryControl::new(Duration::ZERO).unwrap()),
        ..Default::default()
    };
    assert!(matches!(
        handle.search_async("needle".into(), 1, None, request).await,
        Err(CcError::QueryTimedOut)
    ));
}
