//! Behavioral evidence for the relocated assembly-generation guard.
//! All sources and recall outcomes are synthetic; no provider or network.
use cc_model::{
    query::{QueryControl, RetrievalStrategy},
    retrieval::LaneOutcome,
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    CcError, CcResult,
};
use cc_server::{engine::CodeIndex, handlers::SharedCodeIndex};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

struct HeldRecall {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}
impl SemanticRecall for HeldRecall {
    fn recall(
        &self,
        request: SemanticRequest,
        control: QueryControl,
    ) -> Pin<Box<dyn Future<Output = CcResult<LaneOutcome>> + Send + '_>> {
        Box::pin(async move {
            let release = self.release.lock().unwrap().take();
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            if let Some(release) = release {
                release.await.unwrap();
            }
            control.check()?;
            assert!(!request.query.is_empty());
            Ok(LaneOutcome::disabled("semantic", 1.0))
        })
    }
}
fn fixture() -> (tempfile::TempDir, SharedCodeIndex) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("fixture.rs"),
        "pub fn guarded_needle() -> u32 { 101 }\n",
    )
    .unwrap();
    std::fs::write(root.path().join(".codecortex.json"),r#"{"auto_index":{"enabled":false},"query":{"deadline_ms":10000,"semantic_timeout_ms":5000}}"#).unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(true).unwrap();
    (root, Arc::new(RwLock::new(index)))
}
async fn query(index: SharedCodeIndex) -> CcResult<serde_json::Value> {
    cc_server::handlers::context::search_async(
        index,
        "guarded_needle".into(),
        5,
        None,
        SearchRequest {
            retrieval_strategy: Some(RetrievalStrategy::Auto),
            ..Default::default()
        },
    )
    .await
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stable_async_handler_returns_one_accepted_generation_and_verified_source() {
    let (root, index) = fixture();
    index
        .read()
        .unwrap()
        .set_semantic_recall(Some(Arc::new(HeldRecall {
            entered: Mutex::new(None),
            release: Mutex::new(None),
        })));
    let db = index.read().unwrap().index_db().unwrap().clone();
    let accepted = db.reads().read_generation().unwrap();
    let envelope = query(index).await.unwrap();
    assert_eq!(
        envelope["evidence_summary"]["source_freshness"]["generation"],
        serde_json::json!(accepted)
    );
    let hits = envelope["machine_pack"]["hits"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| hit["file_path"] == "fixture.rs"));
    assert!(hits
        .iter()
        .any(|hit| hit["text"].as_str().unwrap().contains("101")));
    let (mut checked, _) = cc_eval::benchmark::normalizer::mcp(&envelope).unwrap();
    for hit in &mut checked {
        cc_eval::benchmark::normalizer::verify_source(hit, root.path()).unwrap();
        assert_eq!(hit.evidence_valid, Some(true));
    }
    assert_eq!(db.reads().read_generation().unwrap(), accepted);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_real_rebuild_rejects_the_pre_rebuild_recall_generation() {
    let (root, index) = fixture();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, receiver) = tokio::sync::oneshot::channel();
    index
        .read()
        .unwrap()
        .set_semantic_recall(Some(Arc::new(HeldRecall {
            entered: Mutex::new(Some(entered)),
            release: Mutex::new(Some(receiver)),
        })));
    let db = index.read().unwrap().index_db().unwrap().clone();
    let before = db.reads().read_generation().unwrap();
    let request = tokio::spawn(query(index.clone()));
    tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(
        root.path().join("fixture.rs"),
        "pub fn guarded_needle() -> u32 { 202 }\n",
    )
    .unwrap();
    let write = index.clone();
    tokio::task::spawn_blocking(move || cc_server::handlers::core::build_index(write, false))
        .await
        .unwrap()
        .unwrap();
    let after = db.reads().read_generation().unwrap();
    assert_ne!(
        before, after,
        "actual typed rebuild must commit a new generation"
    );
    release.send(()).unwrap();
    let result = request.await.unwrap();
    assert!(
        matches!(result, Err(CcError::RetrievalChanged { .. })),
        "must reject stale recall generation instead of returning mixed evidence: {result:?}"
    );
}
