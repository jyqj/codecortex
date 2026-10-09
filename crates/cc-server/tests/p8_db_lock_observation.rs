//! The opt-in status wire separates observer acquisitions without issuing SQL
//! for the counter snapshot. These controls are not performance measurements.
use cc_server::{engine::CodeIndex, tools::StatusParams};

#[cfg(not(feature = "p8-db-lock-observation"))]
#[test]
fn default_status_rejects_diagnostic_aspect_and_has_no_observation_field() {
    let mut params = StatusParams {
        aspect: "lock_observation".into(),
        ..Default::default()
    };
    assert!(params.sanitize().is_err());
    let index = CodeIndex::new(None).unwrap();
    assert!(index
        .diagnostics_info()
        .get("db_lock_observation")
        .is_none());
}

#[cfg(feature = "p8-db-lock-observation")]
#[test]
fn opt_in_snapshot_runs_no_sql_and_status_acquisitions_have_their_own_role() {
    use cc_server::handlers::facade::handle_status;
    use std::sync::{Arc, RwLock};

    fn attempts(value: &serde_json::Value, role: &str, metric: &str) -> u64 {
        value[role][metric]["attempts"].as_u64().unwrap()
    }

    let root = tempfile::tempdir().unwrap();
    let sample = root.path().join("sample.py");
    std::fs::write(sample, "def stable():\n    return 1\n").unwrap();
    let mut index = CodeIndex::new(Some(root.path())).unwrap();
    index.build_index(false).unwrap();
    let runtime = Arc::new(RwLock::new(index));
    let mut params = StatusParams {
        aspect: "lock_observation".into(),
        ..Default::default()
    };
    params.sanitize().unwrap();
    let before = handle_status(runtime.clone(), &params.aspect).unwrap();
    let repeat = handle_status(runtime.clone(), &params.aspect).unwrap();
    // A second snapshot must not acquire a database connection.
    assert_eq!(before, repeat);
    assert_eq!(before["scope"], "process_lifetime_all_index_db_handles");
    assert_eq!(before["sqlite_busy_wait_observed"], false);
    assert_eq!(before["coherent"], true);
    assert_eq!(before["overflowed"], false);

    handle_status(runtime.clone(), "index").unwrap();
    let after_status = handle_status(runtime.clone(), &params.aspect).unwrap();
    assert_eq!(before["workload"], after_status["workload"]);
    let pool = "read_pool_lock_acquire";
    assert!(attempts(&after_status, "observer", pool) > attempts(&before, "observer", pool));

    runtime.read().unwrap().index_status().unwrap();
    let after_work = handle_status(runtime, &params.aspect).unwrap();
    assert_eq!(after_status["observer"], after_work["observer"]);
    let checkout = "read_connection_checkout";
    let work_after = attempts(&after_work, "workload", checkout);
    let work_before = attempts(&after_status, "workload", checkout);
    assert!(work_after > work_before);
}
