//! Independent lifecycle counterexamples; synthetic providers, no network.
#![cfg(feature = "semantic")]

use cc_model::config::ProjectConfig;
use cc_semantic::{
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::fake::{FakeProvider, FakeProviderConfig},
    types::{InputDigest, VectorSpace},
};
use cc_server::{engine::CodeIndex, handlers::SharedCodeIndex};
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};

struct HeldProvider {
    inner: FakeProvider,
    calls: AtomicUsize,
    input: Mutex<Option<InputDigest>>,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl EmbeddingProvider for HeldProvider {
    fn space(&self) -> &VectorSpace {
        self.inner.space()
    }
    fn embed_documents(&self, inputs: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        *self.input.lock().unwrap() = Some(inputs[0].input_digest.clone());
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(15))
                .expect("test must release synthetic provider");
        }
        self.inner.embed_documents(inputs)
    }
    fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.inner.embed_queries(inputs)
    }
}
struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

fn fixture(
    count: usize,
) -> (
    tempfile::TempDir,
    SharedCodeIndex,
    Arc<HeldProvider>,
    tokio::sync::oneshot::Receiver<()>,
    ReleaseOnDrop,
) {
    let root = tempfile::tempdir().unwrap();
    let mut config = ProjectConfig::default();
    config.auto_index.enabled = false;
    config.indexing.db_read_pool_size = Some(1);
    config.semantic.enabled = true;
    config.semantic.model_id = "fake/independent-review".into();
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(16);
    config.semantic.endpoint = "https://semantic.invalid/v1".into();
    std::fs::write(
        root.path().join(".codecortex.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    for n in 0..count {
        std::fs::write(
            root.path().join(format!("fixture_{n}.rs")),
            format!("pub fn fixture_{n}() -> u32 {{ {n} }}\n"),
        )
        .unwrap();
    }
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    let (entered, receiver) = tokio::sync::oneshot::channel();
    let (release, release_receiver) = std::sync::mpsc::channel();
    let provider = Arc::new(HeldProvider {
        inner: FakeProvider::new(FakeProviderConfig::new(
            index.semantic_subsystem().unwrap().space.clone(),
        )),
        calls: AtomicUsize::new(0),
        input: Mutex::new(None),
        entered: Mutex::new(Some(entered)),
        release: Mutex::new(release_receiver),
    });
    index.install_semantic_provider(provider.clone()).unwrap();
    (
        root,
        Arc::new(RwLock::new(index)),
        provider,
        receiver,
        ReleaseOnDrop(Some(release)),
    )
}
async fn build_and_wait(index: &SharedCodeIndex, entered: tokio::sync::oneshot::Receiver<()>) {
    let index = index.clone();
    tokio::task::spawn_blocking(move || cc_server::handlers::core::build_index(index, true))
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), entered)
        .await
        .unwrap()
        .unwrap();
}
async fn wait_pins(index: &SharedCodeIndex, expected: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while index.read().unwrap().query_pins() != expected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("physical worker must release its project pin");
}
fn evidence(name: &str, value: serde_json::Value) {
    if let Ok(dir) = std::env::var("P7_REVIEW_EVIDENCE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_one_inflight_call_does_not_charge_unstarted_documents() {
    let (_root, index, provider, entered, release) = fixture(130);
    let db = index.read().unwrap().index_db().unwrap().clone();
    build_and_wait(&index, entered).await;
    // A real writer and the only read-pool connection remain available while
    // the provider waits. This is independent of the old unit-test fixture.
    let lock_db = db.clone();
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            lock_db.reads().read_generation().unwrap();
            lock_db.enqueue_semantic_rebuild_plan(&[]).unwrap();
        }),
    )
    .await
    .unwrap()
    .unwrap();
    index
        .try_write()
        .expect("no CodeIndex lock across provider wait")
        .close();
    drop(release);
    wait_pins(&index, 0).await;
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    let (tasks, charged, unavailable): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*),COALESCE(SUM(attempt_count>0),0),COALESCE(SUM(last_error='embed input unavailable for task'),0) FROM semantic_outbox", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    let calls = provider.calls.load(Ordering::Acquire);
    evidence(
        "cancel-budget",
        json!({"fixture_documents":130,"queued_tasks":tasks,"provider_calls":calls,"tasks_charged":charged,"unstarted_input_unavailable_retries":unavailable,"published":db.reads().semantic_coverage().unwrap().coverage.published,"pins_after_exit":index.read().unwrap().query_pins()}),
    );
    assert_eq!(calls, 1);
    assert!(tasks >= 64, "fixture must cross the enqueue page boundary");
    assert_eq!(
        db.reads().semantic_coverage().unwrap().coverage.published,
        0
    );
    assert_eq!(
        unavailable, 0,
        "close must not claim/retry work whose input was deliberately withheld by cancellation"
    );
    assert!(
        charged <= calls as i64,
        "unstarted documents must retain their durable retry budget"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retired_worker_cannot_clear_reopened_degradation_projection() {
    let (_root, index, _old_provider, entered, release) = fixture(1);
    build_and_wait(&index, entered).await;
    let reopened_ledger = {
        let mut index = index
            .try_write()
            .expect("close/reopen may acquire the index lock");
        index.close();
        index.reopen().unwrap();
        let subsystem = index.semantic_subsystem().unwrap();
        // Inject a sanctioned corruption event at the new ledger boundary;
        // the real replacement worker, not a test setter, projects this state.
        subsystem
            .ledger
            .note_corrupt(&InputDigest::of_input(b"synthetic reopened corruption event").unwrap());
        let ledger = subsystem.ledger.clone();
        index
            .install_semantic_provider(Arc::new(FakeProvider::new(FakeProviderConfig::new(
                subsystem.space.clone(),
            ))))
            .unwrap();
        assert!(index.semantic_runtime().unwrap().schedule());
        ledger
    };
    // New worker exits; old provider is still physically blocked with its pin.
    wait_pins(&index, 1).await;
    let before = index.read().unwrap().capabilities_info();
    assert_eq!(before["retrieval"]["semantic_state"], "degraded");
    assert!(reopened_ledger.snapshot().degraded);
    drop(release);
    wait_pins(&index, 0).await;
    let after = index.read().unwrap().capabilities_info();
    evidence(
        "retired-projection",
        json!({"before_old_exit":before,"after_old_exit":after,"reopened_ledger_still_degraded":reopened_ledger.snapshot().degraded}),
    );
    assert!(reopened_ledger.snapshot().degraded);
    assert_eq!(
        after["retrieval"]["semantic_state"], "degraded",
        "retired clean ledger must not clear the live reopened ledger projection"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_after_response_before_publish_still_fences_old_worker() {
    let (_root, index, provider, entered, release) = fixture(1);
    let db = index.read().unwrap().index_db().unwrap().clone();
    let subsystem = index.read().unwrap().semantic_subsystem().unwrap();
    build_and_wait(&index, entered).await;
    let input = provider.input.lock().unwrap().clone().unwrap();
    assert!(matches!(
        subsystem
            .cache
            .get(&subsystem.space, &input, &subsystem.doc_spec)
            .unwrap(),
        cc_semantic::cache::CacheRead::Miss
    ));
    // Legitimate SQLite writer contention freezes the publication boundary,
    // without touching production code or changing task identity/state.
    let writer = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    drop(release);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                subsystem
                    .cache
                    .get(&subsystem.space, &input, &subsystem.doc_spec)
                    .unwrap(),
                cc_semantic::cache::CacheRead::Hit(_)
            ) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("durable artifact proves provider returned past its close check");
    assert_eq!(
        db.reads().semantic_coverage().unwrap().coverage.published,
        0
    );
    assert_eq!(index.read().unwrap().query_pins(), 1);
    index
        .try_write()
        .expect("close cannot wait for the worker's publication transaction")
        .close();
    writer.execute_batch("ROLLBACK").unwrap();
    wait_pins(&index, 0).await;
    let published = db.reads().semantic_coverage().unwrap().coverage.published;
    evidence(
        "close-publish-window",
        json!({"provider_calls":provider.calls.load(Ordering::Acquire),"artifact_durable_before_close":true,"published_before_close":0,"published_after_close":published,"index_closed":index.read().unwrap().is_closed(),"pins_after_exit":index.read().unwrap().query_pins()}),
    );
    assert_eq!(
        published, 0,
        "close must fence publication even if it follows the provider's post-response check"
    );
}
