use super::*;
use cc_semantic::{
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::fake::{FakeProvider, FakeProviderConfig},
};
use cc_server::{engine::CodeIndex, semantic_runtime::SemanticRuntime};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

struct HeldProvider {
    inner: FakeProvider,
    entered: Arc<AtomicUsize>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl EmbeddingProvider for HeldProvider {
    fn space(&self) -> &cc_semantic::types::VectorSpace {
        self.inner.space()
    }
    fn embed_documents(&self, input: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let mut released = self.gate.0.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !*released {
            assert!(
                Instant::now() < deadline,
                "held synthetic worker exceeded original bound"
            );
            released = self
                .gate
                .1
                .wait_timeout(released, Duration::from_millis(2))
                .unwrap()
                .0;
        }
        self.inner.embed_documents(input)
    }
    fn embed_queries(&self, input: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.inner.embed_queries(input)
    }
}
struct World {
    root: tempfile::TempDir,
    index: CodeIndex,
    db: Arc<IndexDb>,
    config: ProjectConfig,
    services: QueryServices,
    worker: Arc<SemanticRuntime>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    entered: Arc<AtomicUsize>,
}
impl Drop for World {
    fn drop(&mut self) {
        self.release();
        self.worker.close();
        self.index.close();
    }
}
impl World {
    async fn held() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut config = ProjectConfig::default();
        config.auto_index.enabled = false;
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.network_opt_in = false;
        config.semantic.allow_query_network = false;
        config.semantic.model_id = "fake/independent-status".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        std::fs::write(
            root.path().join(".codecortex.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        std::fs::write(root.path().join("one.rs"), "pub fn one() -> u32 { 947 }\n").unwrap();
        let mut index = CodeIndex::new(Some(root.path())).unwrap();
        index.build_index(false).unwrap();
        let db = index.index_db().unwrap().clone();
        let actual = Arc::new(cc_server::service_factory::QueryServices::default());
        let services = QueryServices(actual.clone());
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
        actual.set_semantic(Some(subsystem.recall.clone()));
        actual.set_semantic_wired(Some(cc_server::service_factory::SemanticWiredInfo {
            model_id: subsystem.space.model_id().to_owned(),
            dimensions: subsystem.space.dimension(),
        }));
        actual.set_semantic_degradation(Some(
            cc_server::service_factory::SemanticDegradation::from(subsystem.ledger.snapshot()),
        ));
        let entered = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let worker = SemanticRuntime::new(
            db.clone(),
            subsystem.clone(),
            actual,
            Arc::new(HeldProvider {
                inner: FakeProvider::new(FakeProviderConfig::new(subsystem.space.clone())),
                entered: entered.clone(),
                gate: gate.clone(),
            }),
        )
        .unwrap();
        assert!(worker.schedule());
        tokio::time::timeout(Duration::from_secs(5), async {
            while entered.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            db.reads().semantic_coverage().unwrap().coverage.published,
            0
        );
        Self {
            root,
            index,
            db,
            config,
            services,
            worker,
            gate,
            entered,
        }
    }
    fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
    fn published(&self, minimum_calls: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let c = self.db.reads().semantic_coverage().unwrap().coverage;
            let p = self.db.reads().semantic_outbox_pending().unwrap();
            if self.entered.load(Ordering::SeqCst) >= minimum_calls
                && c.published > 0
                && c.uncovered == 0
                && p == 0
                && self.services.query_pins() == 0
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "publication not complete: {c:?} pending={p}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
fn trace(label: &str, value: Value) {
    if let Some(root) = std::env::var_os("P7_STATUS_REVIEW_EVIDENCE_DIR") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(format!("{}-{label}.json", std::process::id())),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    } else {
        println!("STATUS_REVIEW {label} {value}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forced_real_worker_publication_invalidates_first_snapshot_and_accepts_second() {
    let world = World::held().await;
    let before = world.db.reads().read_generation().unwrap();
    let mut attempts = 0;
    let status = super::snapshot_observing(
        Some(world.root.path()),
        Some(&world.db),
        Some(&world.config),
        &world.services,
        || {
            attempts += 1;
            if attempts == 1 {
                world.release();
                world.published(1);
            }
        },
    );
    let after = world.db.reads().read_generation().unwrap();
    let coverage = world.db.reads().semantic_coverage().unwrap();
    assert_eq!(before.incarnation, after.incarnation);
    assert_eq!(before.index_epoch, after.index_epoch);
    assert_eq!(before.evidence_epoch, after.evidence_epoch);
    assert!(after.semantic_epoch > before.semantic_epoch);
    assert_eq!(attempts, 2);
    assert_eq!(status["retrieval"]["generation"], json!(after));
    assert_eq!(json!(coverage.generation), json!(after));
    assert_eq!(status["retrieval"]["dense_state"], "ready");
    assert_eq!(
        status["retrieval"]["dense_published"],
        coverage.coverage.published
    );
    assert_eq!(
        status["retrieval"]["dense_desired"],
        coverage.coverage.eligible
    );
    assert_eq!(status["retrieval"]["semantic_pending"], 0);
    assert!(!status["retrieval"]["error"].is_object());
    trace(
        "forced-worker-crossing",
        json!({"before":before,"after":after,"coverage":{"generation":coverage.generation,"published":coverage.coverage.published,"eligible":coverage.coverage.eligible,"uncovered":coverage.coverage.uncovered,"failed":coverage.coverage.failed},"status":status,"root_attempts":attempts,"provider_calls":world.entered.load(Ordering::SeqCst),"real_worker":true,"level":"L2 exact-source observer"}),
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_status_counterexample_pairs_old_root_with_new_real_publication() {
    let world = World::held().await;
    let before = world.db.reads().read_generation().unwrap();
    let mut callbacks = 0;
    let status = crate::legacy_status::snapshot_observing_legacy(
        Some(world.root.path()),
        Some(&world.db),
        Some(&world.config),
        &world.services,
        || {
            callbacks += 1;
            world.release();
            world.published(1);
        },
    );
    let after = world.db.reads().read_generation().unwrap();
    assert_eq!(callbacks, 1);
    assert!(after.semantic_epoch > before.semantic_epoch);
    assert_eq!(status["retrieval"]["generation"], json!(before));
    assert_ne!(status["retrieval"]["generation"], json!(after));
    assert_eq!(status["retrieval"]["dense_state"], "ready");
    assert_eq!(status["retrieval"]["dense_published"], 1);
    trace(
        "legacy-counterexample",
        json!({"before":before,"after":after,"status":status,"callbacks":callbacks,"real_worker":true,"legacy_code_with_observer_only":true,"old_ready_generation_inconsistent":true}),
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_attempt_real_rebuild_and_worker_publish_exhausts_at_three_without_ready() {
    let mut world = World::held().await;
    world.release();
    world.published(1);
    let mut attempts = 0;
    let mut committed = Vec::new();
    let start = Instant::now();
    let db = world.db.clone();
    let config = world.config.clone();
    let root = world.root.path().to_path_buf();
    let services = QueryServices(world.services.0.clone());
    let status =
        super::snapshot_observing(Some(&root), Some(&db), Some(&config), &services, || {
            attempts += 1;
            std::fs::write(
                root.join("one.rs"),
                format!("pub fn one() -> u32 {{ {} }}\n", 947 + attempts),
            )
            .unwrap();
            world.index.build_index(false).unwrap();
            world.worker.schedule();
            world.published(attempts + 1);
            committed.push(db.reads().read_generation().unwrap());
        });
    assert_eq!(attempts, 3);
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(world.entered.load(Ordering::SeqCst), 4);
    assert_eq!(status["retrieval"]["index_state"], "error");
    assert_eq!(status["retrieval"]["default_query_state"], "unavailable");
    assert_eq!(status["retrieval"]["error"]["retryable"], true);
    assert!(status["retrieval"]["generation"].is_null());
    assert_ne!(status["retrieval"]["dense_state"], "ready");
    assert_ne!(status["retrieval"]["semantic_state"], "ready");
    assert!(status["retrieval"]["dense_published"].is_null());
    trace(
        "real-worker-churn",
        json!({"root_attempts":attempts,"committed":committed,"status":status,"provider_calls":world.entered.load(Ordering::SeqCst),"elapsed_ms":start.elapsed().as_millis(),"real_worker":true}),
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stable_status_matches_coverage_and_configured_space_mismatch_stays_partial() {
    let mut world = World::held().await;
    world.release();
    world.published(1);
    let before = world.db.reads().read_generation().unwrap();
    let status = super::snapshot(
        Some(world.root.path()),
        Some(&world.db),
        Some(&world.config),
        &world.services,
    );
    let coverage = world.db.reads().semantic_coverage().unwrap();
    assert_eq!(
        json!(coverage.generation),
        status["retrieval"]["generation"]
    );
    assert_eq!(status["retrieval"]["generation"], json!(before));
    assert_eq!(status["retrieval"]["dense_state"], "ready");
    assert_eq!(
        status["retrieval"]["dense_published"],
        coverage.coverage.published
    );
    assert_eq!(
        status["retrieval"]["dense_desired"],
        coverage.coverage.eligible
    );
    world.config.semantic.model_id = "fake/intended-next-space".into();
    world
        .services
        .set_semantic_wired(Some(crate::service_factory::SemanticWiredInfo {
            model_id: world.config.semantic.model_id.clone(),
            dimensions: 2,
        }));
    let mismatch = super::snapshot(
        Some(world.root.path()),
        Some(&world.db),
        Some(&world.config),
        &world.services,
    );
    assert_eq!(mismatch["retrieval"]["generation"], json!(before));
    assert_eq!(mismatch["retrieval"]["semantic_state"], "backfilling");
    assert_eq!(mismatch["retrieval"]["dense_state"], "partial");
    assert_eq!(mismatch["retrieval"]["dense_published"], 0);
    assert_eq!(
        mismatch["retrieval"]["dense_reason"],
        "semantic_configured_space_pending"
    );
    trace(
        "stable-configured-space",
        json!({"coverage":{"generation":coverage.generation,"published":coverage.coverage.published,"eligible":coverage.coverage.eligible,"uncovered":coverage.coverage.uncovered,"failed":coverage.coverage.failed},"stable":status,"mismatch":mismatch,"query_network_path_excluded":true}),
    );
}
