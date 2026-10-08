//! Real finite scheduler + owned cache fixtures. No network or fake GC result.
use super::*;
use cc_model::config::ProjectConfig;
use cc_semantic::{
    cache::{CacheRead, CACHE_ROOT_ENV, NAMESPACE_DIR_PREFIX},
    providers::fake::{FakeProvider, FakeProviderConfig},
    types::InputDigest,
};

struct Fixture {
    root: tempfile::TempDir,
    config: ProjectConfig,
    worker: Arc<SemanticRuntime>,
    provider: Arc<FakeProvider>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(IndexDb::open(&root.path().join("index.db")).unwrap().0);
        let mut config = ProjectConfig::default();
        config.semantic.enabled = true;
        config.semantic.model_id = "fake/maintenance-scheduling".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        let cache = root.path().join("cache");
        let subsystem = Arc::new(
            semantic_wiring::assemble_with(
                &root.path().to_string_lossy(),
                &config,
                db.clone(),
                |key| (key == CACHE_ROOT_ENV).then(|| cache.to_string_lossy().into_owned()),
                false,
            )
            .unwrap()
            .unwrap(),
        );
        let services = Arc::new(QueryServices::default());
        semantic_wiring::attach(&services, &subsystem);
        let provider = Arc::new(FakeProvider::new(FakeProviderConfig::new(
            subsystem.space.clone(),
        )));
        let worker = SemanticRuntime::new(db, subsystem, services, provider.clone()).unwrap();
        Self {
            root,
            config,
            worker,
            provider,
        }
    }

    fn put_orphan(&self, ordinal: usize) -> InputDigest {
        let input = InputDigest::of_input(format!("owned orphan {ordinal}").as_bytes()).unwrap();
        let subsystem = &self.worker.subsystem;
        subsystem
            .cache
            .put(
                &subsystem.space,
                &input,
                &subsystem.doc_spec,
                &[1.0, 0.0],
                0,
            )
            .unwrap();
        input
    }

    fn hit(&self, input: &InputDigest) -> bool {
        let subsystem = &self.worker.subsystem;
        matches!(
            subsystem
                .cache
                .get(&subsystem.space, input, &subsystem.doc_spec)
                .unwrap(),
            CacheRead::Hit(_)
        )
    }

    fn status(&self) -> serde_json::Value {
        crate::capability_status::snapshot(
            Some(self.root.path()),
            Some(&self.worker.db),
            Some(&self.config),
            &self.worker.services,
        )
    }

    async fn schedule_and_wait(&self) {
        assert!(self.worker.schedule());
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while self.worker.running.load(Ordering::Acquire)
                || self.worker.services.query_pins() != 0
            {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("finite maintenance job must finish");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.worker.close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_jobs_collect_one_page_and_status_never_drives_the_remainder() {
    let fixture = Fixture::new();
    let inputs: Vec<_> = (0..257).map(|n| fixture.put_orphan(n)).collect();
    // Read-only status cannot launch the first maintenance page.
    fixture.status();
    assert_eq!(inputs.iter().filter(|i| fixture.hit(i)).count(), 257);
    fixture.schedule_and_wait().await;
    assert_eq!(inputs.iter().filter(|i| fixture.hit(i)).count(), 1);
    assert!(fixture.worker.gc_cursor.lock().unwrap().is_some());
    let generation = fixture.worker.db.reads().read_generation().unwrap();
    for _ in 0..3 {
        fixture.status();
    }
    assert_eq!(inputs.iter().filter(|i| fixture.hit(i)).count(), 1);
    assert_eq!(fixture.provider.call_count(), 0);
    fixture.schedule_and_wait().await;
    assert_eq!(inputs.iter().filter(|i| fixture.hit(i)).count(), 0);
    assert!(fixture.worker.gc_cursor.lock().unwrap().is_none());
    assert_eq!(
        fixture.worker.db.reads().read_generation().unwrap(),
        generation
    );
    assert_eq!(fixture.provider.call_count(), 0);
    println!(
        "P7_014_MAINTENANCE {}",
        serde_json::json!({"case":"bounded_explicit_gc","initial":257,"after_first_job":1,
            "after_status":1,"after_second_job":0,"provider_calls":0,"gc_epoch_change":false})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_and_closed_jobs_do_not_create_or_touch_cache_objects() {
    let fixture = Fixture::new();
    let cache_root = fixture.worker.subsystem.cache.root().to_path_buf();
    assert!(!cache_root.exists());
    fixture.schedule_and_wait().await;
    assert!(
        !cache_root.exists(),
        "empty GC must preserve lazy cache creation"
    );
    let orphan = fixture.put_orphan(0);
    fixture.worker.close();
    assert!(!fixture.worker.schedule());
    fixture.worker.collect_cache_page().unwrap();
    assert!(fixture.hit(&orphan));
    assert_eq!(fixture.provider.call_count(), 0);
    assert_eq!(fixture.worker.services.query_pins(), 0);
    println!(
        "P7_014_MAINTENANCE {}",
        serde_json::json!({"case":"empty_and_closed","empty_created_cache":false,
            "closed_deleted_objects":0,"provider_calls":0,"query_pins":0})
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gc_error_is_public_and_the_next_explicit_request_retries_the_same_page() {
    let fixture = Fixture::new();
    let input = fixture.put_orphan(0);
    let subsystem = &fixture.worker.subsystem;
    let directory = subsystem
        .cache
        .root()
        .join(format!(
            "{NAMESPACE_DIR_PREFIX}{}",
            subsystem.cache.namespace()
        ))
        .join(subsystem.space.digest().unwrap().as_str())
        .join(input.as_str())
        .join(subsystem.doc_spec.as_str());
    let bin = directory.join(format!("{}.bin", subsystem.doc_spec.as_str()));
    let meta = directory.join(format!("{}.meta.json", subsystem.doc_spec.as_str()));
    std::fs::remove_file(&bin).unwrap();
    std::fs::create_dir(&bin).unwrap();
    fixture.schedule_and_wait().await;
    let failed = fixture.status();
    assert_eq!(failed["retrieval"]["semantic_state"], "failed");
    assert_eq!(
        failed["retrieval"]["semantic_worker_reason"],
        "semantic_worker_gc_failed"
    );
    assert!(fixture.worker.gc_cursor.lock().unwrap().is_none());
    assert!(
        meta.is_file(),
        "failed first unlink must not claim object deleted"
    );
    // Correct only this fixture's invalid path, then retry through the real
    // scheduler; a fresh put provides a valid expired object at the same key.
    std::fs::remove_dir(&bin).unwrap();
    fixture.put_orphan(0);
    fixture.schedule_and_wait().await;
    assert!(!fixture.hit(&input));
    assert!(!meta.exists());
    let recovered = fixture.status();
    assert_ne!(recovered["retrieval"]["semantic_state"], "failed");
    assert!(recovered["retrieval"]
        .get("semantic_worker_reason")
        .is_none());
    assert_eq!(fixture.provider.call_count(), 0);
    println!(
        "P7_014_MAINTENANCE {}",
        serde_json::json!({"case":"gc_failure_recovery","failed":failed,
            "recovered":recovered,"provider_calls":0,"actual_orphan_deleted":true})
    );
}
