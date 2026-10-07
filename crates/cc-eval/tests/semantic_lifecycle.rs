//! Deterministic production scheduling contention; synthetic inputs only.
#![cfg(feature = "semantic")]

use cc_model::{config::ProjectConfig, query::RetrievalStrategy, search::SearchRequest};
use cc_semantic::{
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::fake::{FakeProvider, FakeProviderConfig},
    types::VectorSpace,
};
use cc_server::{engine::CodeIndex, handlers::core, query_handle::QueryHandle};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

struct HeldProvider {
    inner: FakeProvider,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Arc<(Mutex<bool>, Condvar)>,
}
impl EmbeddingProvider for HeldProvider {
    fn space(&self) -> &VectorSpace {
        self.inner.space()
    }
    fn embed_documents(&self, inputs: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        if let Some(tx) = self.entered.lock().unwrap().take() {
            let _ = tx.send(());
        }
        let (lock, wake) = &*self.release;
        let released = lock.lock().unwrap();
        let _released = wake.wait_while(released, |released| !*released).unwrap();
        self.inner.embed_documents(inputs)
    }
    fn embed_queries(&self, inputs: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.inner.embed_queries(inputs)
    }
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap() = true;
        self.0 .1.notify_all();
    }
}
async fn build(index: Arc<RwLock<CodeIndex>>, full: bool) {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || core::build_index(index, full)),
    )
    .await
    .expect("local indexing must progress during provider wait")
    .unwrap()
    .unwrap();
}

fn write_observations(kind: &str, observations: &[serde_json::Value]) {
    if let Ok(dir) = std::env::var("CODECORTEX_LIFECYCLE_RECEIPT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{kind}.json")),
            serde_json::to_vec_pretty(observations).unwrap(),
        )
        .unwrap();
    }
}

async fn run_cases(switch_model: bool) {
    let kind = if switch_model {
        "model-switch"
    } else {
        "local-cancel"
    };
    let mut observations = Vec::new();
    for seed in [7_u64, 19, 43] {
        let before_snapshot = cc_eval::benchmark::sampler::process_snapshot(std::process::id());
        let fixture = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.model_id = format!("fake/old-{seed}");
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://synthetic.invalid/v1".into();
        let config_path = fixture.path().join(".codecortex.json");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::write(
            fixture.path().join("keep.rs"),
            "pub fn before_mutation() -> u32 { 1 }\n",
        )
        .unwrap();
        std::fs::write(
            fixture.path().join("delete.rs"),
            "pub fn deleted_symbol() -> u32 { 2 }\n",
        )
        .unwrap();
        let mut index = CodeIndex::new(Some(fixture.path())).unwrap();
        let old_db = index.index_db().unwrap().clone();
        let (tx, entered) = tokio::sync::oneshot::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let release_guard = Release(release.clone());
        let old_space = index.semantic_subsystem().unwrap().space.clone();
        let old_provider = Arc::new(HeldProvider {
            inner: FakeProvider::new(FakeProviderConfig::new(old_space.clone())),
            entered: Mutex::new(Some(tx)),
            release,
        });
        index
            .install_semantic_provider(old_provider.clone())
            .unwrap();
        let shared = Arc::new(RwLock::new(index));
        build(shared.clone(), true).await;
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            old_db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .published,
            0
        );
        let started = Instant::now();
        let query_index = shared.clone();
        let found = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                query_index
                    .read()
                    .unwrap()
                    .graph()
                    .find_symbol("before_mutation", true, 10, false)
            }),
        )
        .await
        .expect("foreground local query must progress")
        .unwrap()
        .unwrap();
        assert!(!found.as_array().unwrap().is_empty());
        let query = QueryHandle::capture(&shared).unwrap();
        let envelope = tokio::time::timeout(
            Duration::from_secs(2),
            query.search_async(
                "before_mutation".into(),
                10,
                None,
                SearchRequest {
                    retrieval_strategy: Some(RetrievalStrategy::Local),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect("real local retrieval must progress during held document embedding")
        .unwrap();
        assert!(!envelope.machine_pack["hits"].as_array().unwrap().is_empty());
        drop(query);
        let query_ms = started.elapsed().as_secs_f64() * 1000.0;
        let blocked_snapshot = cc_eval::benchmark::sampler::process_snapshot(std::process::id());
        let blocked_queue = {
            let conn = old_db.read_conn().unwrap();
            conn.query_row("SELECT COUNT(*),COALESCE(SUM(state='claimed'),0),COALESCE(SUM(attempt_count),0) FROM semantic_outbox", [], |row| Ok((row.get::<_, i64>(0)?,row.get::<_, i64>(1)?,row.get::<_, i64>(2)?))).unwrap()
        };
        assert_eq!(blocked_queue.1, 1);
        let write_started = Instant::now();
        std::fs::write(
            fixture.path().join("keep.rs"),
            format!("pub fn after_mutation() -> u32 {{ {seed} }}\n"),
        )
        .unwrap();
        std::fs::remove_file(fixture.path().join("delete.rs")).unwrap();
        build(shared.clone(), false).await;
        let write_ms = write_started.elapsed().as_secs_f64() * 1000.0;
        {
            let index = shared
                .try_read()
                .expect("no CodeIndex lock held by provider");
            assert!(index
                .graph()
                .find_symbol("before_mutation", true, 10, false)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty());
            assert!(index
                .graph()
                .find_symbol("deleted_symbol", true, 10, false)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty());
            assert!(!index
                .graph()
                .find_symbol("after_mutation", true, 10, false)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty());
        }
        let base_observation = serde_json::json!({"seed":seed,"local_query_ms":query_ms,"incremental_write_delete_ms":write_ms,"blocked_queue":{"tasks":blocked_queue.0,"claimed":blocked_queue.1,"total_attempt_count":blocked_queue.2},"before_snapshot":before_snapshot,"blocked_snapshot":blocked_snapshot,"resource_attribution":"test runner and server share this PID; no subprocess/server-tree or sampled-peak claim; unavailable stays null","scope":"in-process synthetic functional contention; not full V20 performance certification"});
        if !switch_model {
            shared.write().unwrap().close();
            drop(release_guard);
            tokio::time::timeout(Duration::from_secs(5), async {
                while shared.read().unwrap().query_pins() != 0 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("cancelled worker must physically exit");
            let published = old_db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .published;
            assert_eq!(
                published, 0,
                "late old input cannot publish after mutation/delete/close"
            );
            let calls = old_provider.inner.call_count();
            let charged: i64 = old_db
                .read_conn()
                .unwrap()
                .query_row(
                    "SELECT COALESCE(SUM(attempt_count),0) FROM semantic_outbox",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(calls, 1);
            assert!(
                charged <= calls as i64,
                "unstarted work cannot consume retry budget after cancellation"
            );
            let mut row = base_observation;
            row["provider_calls"] = calls.into();
            row["charged_attempts_after_close"] = charged.into();
            row["published_after_close"] = published.into();
            row["pins_after_exit"] = shared.read().unwrap().query_pins().into();
            row["after_snapshot"] = serde_json::to_value(
                cc_eval::benchmark::sampler::process_snapshot(std::process::id()),
            )
            .unwrap();
            observations.push(row);
            write_observations(kind, &observations);
            continue;
        }
        config.semantic.model_id = format!("fake/new-{seed}");
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let replacement = {
            let mut index = shared
                .try_write()
                .expect("model switch cannot wait for provider");
            index.set_project(fixture.path(), false).unwrap();
            let space = index.semantic_subsystem().unwrap().space.clone();
            assert_ne!(space, old_space);
            let provider = Arc::new(FakeProvider::new(FakeProviderConfig::new(space)));
            index.install_semantic_provider(provider.clone()).unwrap();
            provider
        };
        build(shared.clone(), false).await;
        drop(release_guard);
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let done = {
                    let index = shared.read().unwrap();
                    let coverage = index
                        .index_db()
                        .unwrap()
                        .reads()
                        .semantic_coverage()
                        .unwrap()
                        .coverage;
                    coverage.published > 0 && index.query_pins() == 0
                };
                if done {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        if completed.is_err() {
            let mut row = base_observation.clone();
            row["failure"] =
                "replacement backfill did not complete within unchanged5s bound".into();
            row["active_space"] = old_db.semantic_active_space().unwrap().into();
            row["old_space"] = old_space.digest().unwrap().as_str().into();
            row["configured_new_space"] = replacement.space().digest().unwrap().as_str().into();
            row["new_provider_calls"] = replacement.call_count().into();
            row["capabilities"] = shared.read().unwrap().capabilities_info();
            observations.push(row);
            write_observations(kind, &observations);
        }
        completed.expect("replacement backfill must finish and old worker physically exit");
        let conn = old_db.read_conn().unwrap();
        let stale: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_manifest WHERE space_id=?1",
                [old_space.digest().unwrap().as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            stale, 0,
            "old model must never publish after mutation/delete/switch"
        );
        assert!(replacement.call_count() > 0);
        let mut row = base_observation;
        row["old_space_publications"] = stale.into();
        row["new_provider_calls"] = replacement.call_count().into();
        row["after_snapshot"] = serde_json::to_value(
            cc_eval::benchmark::sampler::process_snapshot(std::process::id()),
        )
        .unwrap();
        observations.push(row);
        write_observations(kind, &observations);
        shared.write().unwrap().close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn partial_backfill_allows_real_local_query_mutation_delete_and_cancel() {
    run_cases(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn partial_backfill_allows_model_switch_and_new_space_progress() {
    run_cases(true).await;
}
