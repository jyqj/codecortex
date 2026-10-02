//! Explicit configured transition fences; no provider or network.
use cc_db::{
    index_db::IndexDb, semantic_publish::LifecycleFence, semantic_space_switch::SpaceState,
};
use std::sync::Arc;
fn fixture() -> (tempfile::TempDir, Arc<IndexDb>, String, String, [u8; 16]) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(IndexDb::open(&dir.path().join("transition.db")).unwrap().0);
    let old = "a".repeat(64);
    let new = "b".repeat(64);
    db.register_semantic_space(&old, "{}").unwrap();
    db.switch_semantic_active_space(&old, "initial-operator-revision")
        .unwrap();
    let incarnation = db.reads().read_generation().unwrap().incarnation;
    (dir, db, old, new, incarnation)
}
#[test]
fn explicit_transition_registers_activates_and_records_revision_atomically() {
    let (_dir, db, old, new, incarnation) = fixture();
    let fence = LifecycleFence::default();
    assert!(db
        .prepare_semantic_configured_space(
            &new,
            "{}",
            "operator-new-model-revision",
            incarnation,
            &fence
        )
        .unwrap());
    assert_eq!(db.semantic_active_space().unwrap(), Some(new.clone()));
    assert_eq!(
        db.semantic_space_state(&old).unwrap().unwrap().0,
        SpaceState::Revoked
    );
    let conn = db.read_conn().unwrap();
    let log: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='semantic_space_switch_log'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let log: serde_json::Value = serde_json::from_str(&log).unwrap();
    assert_eq!(
        log.as_array().unwrap().last().unwrap()["revision"],
        "operator-new-model-revision"
    );
    assert_eq!(log.as_array().unwrap().last().unwrap()["pinned"], false);
}
#[test]
fn retired_or_stale_incarnation_cannot_register_or_switch() {
    for retired in [false, true] {
        let (_dir, db, old, new, mut incarnation) = fixture();
        let fence = LifecycleFence::default();
        if retired {
            fence.close();
        } else {
            incarnation[0] ^= 1;
        }
        let generation = db.reads().read_generation().unwrap();
        assert!(!db
            .prepare_semantic_configured_space(&new, "{}", "retired", incarnation, &fence)
            .unwrap());
        assert_eq!(db.semantic_active_space().unwrap(), Some(old));
        assert!(db.semantic_space_state(&new).unwrap().is_none());
        assert_eq!(db.reads().read_generation().unwrap(), generation);
    }
}
#[test]
fn external_writer_wait_does_not_block_close_or_allow_late_switch() {
    let (_dir, db, old, new, incarnation) = fixture();
    let fence = Arc::new(LifecycleFence::default());
    let writer = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let (entered, rx) = std::sync::mpsc::channel();
    let (job_db, job_fence, target) = (db.clone(), fence.clone(), new.clone());
    let job = std::thread::spawn(move || {
        entered.send(()).unwrap();
        job_db
            .prepare_semantic_configured_space(&target, "{}", "late", incarnation, &job_fence)
            .unwrap()
    });
    rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let started = std::time::Instant::now();
    fence.close();
    assert!(started.elapsed() < std::time::Duration::from_millis(100));
    writer.execute_batch("COMMIT;").unwrap();
    assert!(!job.join().unwrap());
    assert_eq!(db.semantic_active_space().unwrap(), Some(old));
    assert!(db.semantic_space_state(&new).unwrap().is_none());
}
