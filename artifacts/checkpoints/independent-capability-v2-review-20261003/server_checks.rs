use super::*;
#[cfg(feature = "semantic")]
use crate::service_factory::{SemanticDegradation, SemanticWiredInfo, SemanticWorkerStatus};
#[cfg(feature = "semantic")]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(feature = "semantic")]
use std::time::{Duration, Instant};

#[cfg(feature = "semantic")]
fn database_fields(v: &Value) -> Value {
    json!({"files":v["indexed_files"],"symbols":v["indexed_symbols"],"generation":v["retrieval"]["generation"],"freshness":v["retrieval"]["resolution_freshness"],"active":v["retrieval"]["semantic_active_space"],"pending":v["retrieval"]["semantic_pending"],"failed":v["retrieval"]["semantic_failed"],"desired":v["retrieval"]["dense_desired"],"published":v["retrieval"]["dense_published"],"dense":v["retrieval"]["dense_state"],"local":v["retrieval"]["local_state"]})
}

#[test]
fn independent_default_no_project_closed_unwired_and_identity_exhaustion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("review.db");
    let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
    let services = QueryServices::default();
    for (project, database, state) in [
        (None, None, "no_project"),
        (Some(dir.path()), None, "closed"),
    ] {
        let s = snapshot(project, database, None, &services);
        assert_eq!(s["retrieval"]["index_state"], state);
        assert!(s["retrieval"]["generation"].is_null());
        assert!(s["indexed_files"].is_null());
        assert_eq!(s["retrieval"]["identity_validation"], "not_observed");
        assert_ne!(s["retrieval"]["dense_state"], "ready");
    }
    let s = snapshot(Some(dir.path()), Some(&db), None, &services);
    assert_eq!(s["retrieval"]["index_state"], "empty");
    assert_eq!(s["retrieval"]["semantic_state"], "not_configured");
    assert_eq!(s["retrieval"]["dense_state"], "disabled");
    assert!(s["retrieval"]["semantic_active_space"].is_null());
    let mut calls = 0;
    let failed = snapshot_observing(Some(dir.path()), Some(&db), None, &services, || {
        calls += 1;
        db.writes()
            .set_metadata("index_incarnation", &format!("{:032x}", 200 + calls))
            .unwrap();
    });
    assert_eq!(calls, 3);
    assert_eq!(failed["retrieval"]["index_state"], "error");
    assert!(failed["retrieval"]["generation"].is_null());
    assert!(failed["indexed_symbols"].is_null());
    assert_eq!(failed["retrieval"]["default_query_state"], "unavailable");
    assert_eq!(failed["retrieval"]["identity_validation"], "not_observed");
    assert_eq!(failed["retrieval"]["error"]["retryable"], true);
    eprintln!("BOUNDARIES {failed}");
}

#[cfg(feature = "semantic")]
fn assemble(
    dir: &std::path::Path,
    db: &Arc<IndexDb>,
    services: &QueryServices,
) -> (
    ProjectConfig,
    Arc<crate::semantic_wiring::SemanticSubsystem>,
) {
    let mut config = ProjectConfig::default();
    config.semantic.enabled = true;
    config.semantic.model_id = "fake/independent-v2".into();
    config.semantic.dimensions = Some(2);
    config.semantic.max_input_tokens = Some(8192);
    config.semantic.max_batch_items = Some(8);
    config.semantic.endpoint = "https://semantic.invalid/v1".into();
    let subsystem = Arc::new(
        crate::semantic_wiring::assemble_with(
            &dir.to_string_lossy(),
            &config,
            db.clone(),
            |key| {
                (key == cc_semantic::cache::CACHE_ROOT_ENV)
                    .then(|| dir.join("cache").to_string_lossy().into_owned())
            },
            false,
        )
        .unwrap()
        .unwrap(),
    );
    crate::semantic_wiring::attach(services, &subsystem);
    (config, subsystem)
}

#[cfg(feature = "semantic")]
#[test]
fn independent_active_configured_space_and_service_priority() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("review.db");
    let db = Arc::new(IndexDb::open_with_read_pool_size(&path, 1).unwrap().0);
    let services = QueryServices::default();
    let (config, sub) = assemble(dir.path(), &db, &services);
    let w = rusqlite::Connection::open(&path).unwrap();
    w.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [sub.space.digest().unwrap().as_str()],
    )
    .unwrap();
    // Remove only synthetic active designation through ordinary SQL; no rebuild/fault.
    w.execute("UPDATE semantic_spaces SET state='revoked'", [])
        .unwrap();
    let no_active = snapshot(Some(dir.path()), Some(&db), Some(&config), &services);
    assert!(no_active["retrieval"]["semantic_active_space"].is_null());
    assert_eq!(
        no_active["retrieval"]["semantic_state"],
        "port_attached_unverified"
    );
    assert_eq!(
        no_active["retrieval"]["dense_reason"],
        "semantic_no_active_space"
    );
    w.execute(
        "UPDATE semantic_spaces SET state='active' WHERE space_id=?1",
        [sub.space.digest().unwrap().as_str()],
    )
    .unwrap();
    let zero = snapshot(Some(dir.path()), Some(&db), Some(&config), &services);
    assert_eq!(zero["retrieval"]["dense_state"], "partial");
    assert_eq!(
        zero["retrieval"]["dense_reason"],
        "semantic_no_eligible_documents"
    );
    services.set_semantic_wired(Some(SemanticWiredInfo {
        model_id: "fake/other-config".into(),
        dimensions: 2,
    }));
    let mismatch = snapshot(Some(dir.path()), Some(&db), Some(&config), &services);
    assert_eq!(mismatch["retrieval"]["semantic_state"], "backfilling");
    assert_eq!(
        mismatch["retrieval"]["dense_reason"],
        "semantic_configured_space_pending"
    );
    assert_eq!(mismatch["retrieval"]["dense_published"], 0);
    services.set_semantic_wired(Some(SemanticWiredInfo {
        model_id: config.semantic.model_id.clone(),
        dimensions: 2,
    }));
    let worker = Arc::new(SemanticWorkerStatus::default());
    services.set_semantic_worker(Some(worker.clone()));
    worker.assembly_failed();
    let failed = snapshot(Some(dir.path()), Some(&db), Some(&config), &services);
    assert_eq!(failed["retrieval"]["semantic_state"], "failed");
    assert_eq!(
        failed["retrieval"]["semantic_worker_reason"],
        "semantic_provider_assembly_failed"
    );
    worker.round_failed();
    services.set_semantic_degradation(Some(SemanticDegradation {
        degraded: true,
        degraded_reasons: vec!["independent-process-degraded".into()],
        ..Default::default()
    }));
    let degraded = snapshot(Some(dir.path()), Some(&db), Some(&config), &services);
    assert_eq!(degraded["retrieval"]["semantic_state"], "degraded");
    assert_eq!(
        degraded["retrieval"]["semantic_worker_reason"],
        "semantic_worker_round_failed"
    );
    assert_eq!(
        degraded["retrieval"]["service_state_scope"],
        "process_observed_separately"
    );
    assert_eq!(database_fields(&degraded), database_fields(&failed));
    // Independent process observation can change AFTER the DB transaction.
    services.set_semantic_degradation(None);
    worker.clear();
    let crossing = snapshot_observing(
        Some(dir.path()),
        Some(&db),
        Some(&config),
        &services,
        || worker.round_failed(),
    );
    assert_eq!(crossing["retrieval"]["semantic_state"], "failed");
    assert_eq!(database_fields(&crossing), database_fields(&failed));
    for project in [None, Some(dir.path())] {
        let closed = snapshot(project, None, Some(&config), &services);
        assert_ne!(closed["retrieval"]["dense_state"], "ready");
    }
    eprintln!("SERVICE_PRIORITY {degraded}");
}

#[cfg(feature = "semantic")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_real_publish_old_or_new_full_tuple_no_rescan_and_mutant() {
    use cc_semantic::{
        ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
        providers::fake::{FakeProvider, FakeProviderConfig},
    };
    struct Gated {
        fake: FakeProvider,
        entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }
    impl EmbeddingProvider for Gated {
        fn space(&self) -> &cc_semantic::types::VectorSpace {
            self.fake.space()
        }
        fn embed_documents(&self, docs: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            if let Some(tx) = self.entered.lock().unwrap().take() {
                tx.send(()).unwrap();
            }
            let mut open = self.gate.0.lock().unwrap();
            let bound = Instant::now() + Duration::from_secs(5);
            while !*open {
                assert!(Instant::now() < bound);
                open = self
                    .gate
                    .1
                    .wait_timeout(open, Duration::from_millis(2))
                    .unwrap()
                    .0;
            }
            self.fake.embed_documents(docs)
        }
        fn embed_queries(&self, q: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.fake.embed_queries(q)
        }
    }
    struct Unlock(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Unlock {
        fn drop(&mut self) {
            *self.0 .0.lock().unwrap() = true;
            self.0 .1.notify_all();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    for i in 0..3 {
        std::fs::write(
            dir.path().join(format!("r{i}.rs")),
            format!("pub fn r{i}() -> u32 {{ {} }}\n", 673 + i),
        )
        .unwrap();
    }
    let mut cfg = ProjectConfig::default();
    cfg.indexing.db_read_pool_size = Some(1);
    std::fs::write(
        dir.path().join(".codecortex.json"),
        serde_json::to_vec(&cfg).unwrap(),
    )
    .unwrap();
    let mut index = crate::engine::CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(false).unwrap();
    let handle = index.query_handle().unwrap();
    let (config, sub) = assemble(dir.path(), &handle.db, &handle.services);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let guard = Unlock(gate.clone());
    let runtime = crate::semantic_runtime::SemanticRuntime::new(
        handle.db.clone(),
        sub.clone(),
        handle.services.clone(),
        Arc::new(Gated {
            fake: FakeProvider::new(FakeProviderConfig::new(sub.space.clone())),
            entered: Mutex::new(Some(tx)),
            gate: gate.clone(),
        }),
    )
    .unwrap();
    assert!(runtime.schedule());
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .unwrap()
        .unwrap();
    let old = snapshot(
        Some(dir.path()),
        Some(&handle.db),
        Some(&config),
        &handle.services,
    );
    assert_eq!(old["retrieval"]["dense_published"], 0);
    assert!(old["retrieval"]["semantic_pending"].as_u64().unwrap() > 0);
    let mut calls = 0;
    let crossed = snapshot_observing(
        Some(dir.path()),
        Some(&handle.db),
        Some(&config),
        &handle.services,
        || {
            calls += 1;
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            let bound = Instant::now() + Duration::from_secs(5);
            loop {
                let s = handle.db.reads().capability_snapshot(true).unwrap();
                let c = s.semantic.unwrap();
                if c.pending == 0 && c.eligible > 0 && c.published == c.eligible {
                    break;
                }
                assert!(Instant::now() < bound);
                std::thread::sleep(Duration::from_millis(1));
            }
        },
    );
    assert_eq!(calls, 1, "ordinary real publish cannot rescan");
    let new = snapshot(
        Some(dir.path()),
        Some(&handle.db),
        Some(&config),
        &handle.services,
    );
    assert_eq!(new["retrieval"]["dense_state"], "ready");
    assert_eq!(new["retrieval"]["semantic_state"], "ready");
    assert_eq!(new["retrieval"]["semantic_pending"], 0);
    assert_eq!(
        new["retrieval"]["dense_desired"],
        new["retrieval"]["dense_published"]
    );
    assert_ne!(
        old["retrieval"]["generation"],
        new["retrieval"]["generation"]
    );
    assert_eq!(
        database_fields(&crossed),
        database_fields(&old),
        "old observation after committed real publish must remain complete"
    );
    for v in [&old, &crossed, &new] {
        assert_eq!(v["retrieval"]["spec"], "retrieval-capabilities-v2");
        assert_eq!(v["retrieval"]["consistency"], "point_in_time");
        assert_eq!(
            v["retrieval"]["generation_scope"],
            "observed_database_snapshot"
        );
        assert_eq!(
            v["retrieval"]["identity_validation"],
            "checked_at_observation_boundary"
        );
        assert_eq!(
            v["retrieval"]["resolution_freshness"]["index_epoch"],
            v["retrieval"]["generation"]["index_epoch"]
        );
    }
    // Actual legacy split algorithm executed against the committed normal publish.
    let mut mutant = new.clone();
    mutant["retrieval"]["generation"] = old["retrieval"]["generation"].clone();
    assert!(
        database_fields(&mutant) != database_fields(&old)
            && database_fields(&mutant) != database_fields(&new)
    );
    eprintln!(
        "NORMAL_PUBLISH {}",
        json!({"old":old,"crossed":crossed,"new":new,"split_mutant":mutant,"attempts":calls})
    );
    // Ready is DB observation only: process failure must still override it.
    let state = Arc::new(SemanticWorkerStatus::default());
    state.round_failed();
    handle.services.set_semantic_worker(Some(state));
    let failed = snapshot(
        Some(dir.path()),
        Some(&handle.db),
        Some(&config),
        &handle.services,
    );
    assert_eq!(failed["retrieval"]["semantic_state"], "failed");
    assert_eq!(failed["retrieval"]["dense_state"], "ready");
    handle
        .services
        .set_semantic_degradation(Some(SemanticDegradation {
            degraded: true,
            degraded_reasons: vec!["independent-ready-process-failure".into()],
            ..Default::default()
        }));
    assert_eq!(
        snapshot(
            Some(dir.path()),
            Some(&handle.db),
            Some(&config),
            &handle.services
        )["retrieval"]["semantic_state"],
        "degraded"
    );
    drop(guard);
    runtime.close();
    index.close();
}

#[test]
fn independent_ordinary_epochs_do_not_rescan_with_single_pool() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("epochs.db");
    let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
    let w = rusqlite::Connection::open(&path).unwrap();
    let services = QueryServices::default();
    for i in 1..=6 {
        let old = db.reads().capability_snapshot(false).unwrap();
        let mut attempts = 0;
        let view = snapshot_observing(Some(dir.path()), Some(&db), None, &services, || {
            attempts += 1;
            w.execute_batch(&format!("BEGIN; INSERT INTO metadata(key,value) VALUES('index_epoch','{}'),('evidence_epoch','{}'),('semantic_epoch','{}') ON CONFLICT(key) DO UPDATE SET value=excluded.value; COMMIT;",i*11,i*13,i*17)).unwrap();
        });
        assert_eq!(attempts, 1);
        assert_eq!(view["retrieval"]["generation"], json!(old.generation));
        assert_eq!(
            view["retrieval"]["resolution_freshness"],
            json!(old.resolution_freshness)
        );
        assert_eq!(view["indexed_files"], old.indexed_files);
        assert_eq!(view["indexed_symbols"], old.indexed_symbols);
        assert!(!view["retrieval"]["error"].is_object());
    }
    eprintln!("ORDINARY_EPOCH_POLLS 6 one attempt each; pool 1");
}
