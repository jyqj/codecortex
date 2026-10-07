//! P7-014: the public positive GC grace must remain positive in the runtime.
//! Synthetic cache objects only; no provider, credentials or network calls.
#![cfg(feature = "semantic")]

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use cc_db::index_db::IndexDb;
use cc_model::{config::ProjectConfig, CcError};
use cc_semantic::{
    cache::{CacheRead, CACHE_ROOT_ENV},
    types::InputDigest,
};
use cc_server::semantic_wiring::{assemble_with, run_gc_until_exhausted};

fn config(retention: u64, enabled: bool) -> ProjectConfig {
    serde_json::from_value(serde_json::json!({
        "semantic": {
            "enabled": enabled,
            "model_id": "fake/gc-retention-config",
            "dimensions": 2,
            "max_input_tokens": 8192,
            "max_batch_items": 16,
            "endpoint": "https://semantic.invalid/v1",
            "gc_min_retention_secs": retention
        }
    }))
    .unwrap()
}

fn assert_rejected(retention: u64) {
    let root = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
    let cache = root.path().join("cache");
    let lookups = AtomicUsize::new(0);
    let result = assemble_with(
        "synthetic/gc-retention-rejection",
        &config(retention, true),
        Arc::new(db),
        |key| {
            lookups.fetch_add(1, Ordering::SeqCst);
            (key == CACHE_ROOT_ENV).then(|| cache.to_string_lossy().into_owned())
        },
        false,
    );
    match result {
        Err(CcError::Config(message)) => {
            assert!(message.contains("semantic.gc_min_retention_secs"));
        }
        Ok(Some(subsystem)) => panic!(
            "positive configured grace {retention} became accepted runtime grace {}",
            subsystem.gc_min_retention_secs
        ),
        Ok(None) => panic!("enabled invalid config was silently disabled"),
        Err(other) => panic!("expected key-specific config error, received {other}"),
    }
    assert_eq!(lookups.load(Ordering::SeqCst), 0);
    assert!(!cache.exists());
}

#[test]
fn overflowing_gc_grace_is_rejected_before_cache_root_resolution() {
    for retention in [i64::MAX as u64 + 1, u64::MAX] {
        assert_rejected(retention);
    }
}

#[test]
fn zero_gc_grace_is_still_rejected_before_cache_root_resolution() {
    assert_rejected(0);
}

#[test]
fn representable_grace_keeps_fresh_objects_and_expires_at_its_exact_boundary() {
    for retention in [1, 3_600, i64::MAX as u64] {
        let root = tempfile::tempdir().unwrap();
        let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
        let db = Arc::new(db);
        let cache = root.path().join("cache");
        let subsystem = assemble_with(
            "synthetic/gc-retention-boundary",
            &config(retention, true),
            db.clone(),
            |key| (key == CACHE_ROOT_ENV).then(|| cache.to_string_lossy().into_owned()),
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(subsystem.gc_min_retention_secs as u64, retention);
        let generation = db.reads().read_generation().unwrap();
        let input = InputDigest::of_input(b"owned synthetic fresh object").unwrap();
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

        let fresh = run_gc_until_exhausted(&db, &subsystem, 0).unwrap();
        assert_eq!(fresh.kept_fresh, 1);
        assert_eq!(fresh.deleted_objects, 0);
        assert!(matches!(
            subsystem
                .cache
                .get(&subsystem.space, &input, &subsystem.doc_spec)
                .unwrap(),
            CacheRead::Hit(_)
        ));

        let expired = run_gc_until_exhausted(&db, &subsystem, retention as i64).unwrap();
        assert_eq!(expired.kept_fresh, 0);
        assert_eq!(expired.deleted_objects, 1);
        assert!(matches!(
            subsystem
                .cache
                .get(&subsystem.space, &input, &subsystem.doc_spec)
                .unwrap(),
            CacheRead::Miss
        ));
        assert_eq!(db.reads().read_generation().unwrap(), generation);
    }
}

#[test]
fn disabled_semantic_config_remains_inert_even_for_unusable_gc_graces() {
    for retention in [0, i64::MAX as u64 + 1, u64::MAX] {
        let root = tempfile::tempdir().unwrap();
        let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
        let lookups = AtomicUsize::new(0);
        let result = assemble_with(
            "synthetic/gc-retention-disabled",
            &config(retention, false),
            Arc::new(db),
            |_| {
                lookups.fetch_add(1, Ordering::SeqCst);
                None
            },
            false,
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(lookups.load(Ordering::SeqCst), 0);
    }
}
