//! Atomic debt acknowledgement, stale basis and corrupt durable state.
use cc_db::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
use cc_model::{freshness::*, public_surface::PublicSurface, Language, ParseOutcome};
fn unit(path: &str) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Rust,
        content_hash: "hash".into(),
        mtime: 1.0,
        size: 0,
        outcome: ParseOutcome {
            public_surface: PublicSurface::new("rust", path, "test"),
            ..Default::default()
        },
    }
}
fn pending(epoch: u64) -> ReconcileUpdate {
    let mut state = ReconcileState::new(epoch);
    state.roots.insert("root.rs".into());
    state.stop = ReconcileStop::BudgetExceeded;
    ReconcileUpdate {
        expected_index_epoch: epoch,
        next: Some(state),
    }
}
fn write(
    db: &IndexDb,
    units: &[FileWriteUnit],
    update: &ReconcileUpdate,
) -> cc_model::CcResult<cc_db::index_db::SeedTokenSpan> {
    db.writes().write_reconciled_batch(
        &[],
        units,
        &[],
        &[],
        &[],
        &PrecompressedChunks::new(),
        Some(update),
    )
}
#[test]
fn debt_and_file_facts_share_epoch_and_survive_reopen() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("index.db");
    let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
    let before = db.reads().generation().unwrap();
    let update = pending(before.index_epoch);
    write(&db, &[unit("root.rs")], &update).unwrap();
    let after = db.reads().generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(db.reads().resolution_frontier().unwrap(), update.next);
    let status = db.reads().resolution_freshness().unwrap();
    assert!(!status.complete);
    assert_eq!(status.index_epoch, after.index_epoch);
    drop(db);
    let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
    assert_eq!(db.reads().resolution_frontier().unwrap(), update.next);
    // Removing the root cannot delete the only explanation of its consumers.
    db.writes().remove_files_batch(&["root.rs".into()]).unwrap();
    assert!(!db.reads().resolution_freshness().unwrap().complete);
    let epoch = db.reads().generation().unwrap().index_epoch;
    write(
        &db,
        &[],
        &ReconcileUpdate {
            expected_index_epoch: epoch,
            next: None,
        },
    )
    .unwrap();
    assert!(db.reads().resolution_freshness().unwrap().complete);
    assert_eq!(db.reads().generation().unwrap().index_epoch, epoch + 1);
}
#[test]
fn failed_file_batch_never_acknowledges_or_publishes_debt() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&d.path().join("index.db")).unwrap().0;
    let epoch = db.reads().generation().unwrap().index_epoch;
    let mut bad = unit("bad.rs");
    bad.outcome.public_surface.module = "wrong.rs".into();
    assert!(write(&db, &[unit("root.rs"), bad.clone()], &pending(epoch)).is_err());
    assert_eq!(db.reads().generation().unwrap().index_epoch, epoch);
    assert!(db.reads().resolution_frontier().unwrap().is_none());
    assert!(db
        .reads()
        .public_surfaces(&["root.rs".into()])
        .unwrap()
        .is_empty());
    write(&db, &[unit("root.rs")], &pending(epoch)).unwrap();
    let before = db.reads().generation().unwrap();
    assert!(write(
        &db,
        &[bad],
        &ReconcileUpdate {
            expected_index_epoch: before.index_epoch,
            next: None
        }
    )
    .is_err());
    assert_eq!(db.reads().generation().unwrap(), before);
    assert!(!db.reads().resolution_freshness().unwrap().complete);
}
#[test]
fn stale_prepare_is_fenced_inside_the_writer_transaction() {
    let d = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&d.path().join("index.db")).unwrap().0;
    let epoch = db.reads().generation().unwrap().index_epoch;
    db.writes()
        .replace_files_batch(&[unit("other.rs")])
        .unwrap();
    assert!(matches!(
        write(&db, &[unit("root.rs")], &pending(epoch)),
        Err(cc_model::CcError::StalePreparedBuild { .. })
    ));
    assert!(db.reads().resolution_frontier().unwrap().is_none());
}
#[test]
fn invalid_frontier_fails_before_file_commit_and_bad_hash_is_not_ready() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("index.db");
    let db = IndexDb::open(&path).unwrap().0;
    let epoch = db.reads().generation().unwrap().index_epoch;
    let mut update = pending(epoch);
    update
        .next
        .as_mut()
        .unwrap()
        .roots
        .insert("../escape.rs".into());
    assert!(write(&db, &[unit("root.rs")], &update).is_err());
    assert_eq!(db.reads().generation().unwrap().index_epoch, epoch);
    write(&db, &[unit("root.rs")], &pending(epoch)).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE resolution_frontier SET digest='bad'", [])
        .unwrap();
    assert!(db.reads().resolution_frontier().is_err());
    assert!(!db.reads().resolution_freshness().unwrap().complete);
}

#[test]
fn invalid_frontier_precedes_a_file_mutation_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    let db = IndexDb::open(&path).unwrap().0;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_file BEFORE INSERT ON files BEGIN
         SELECT RAISE(ABORT, 'file mutation reached before frontier validation'); END;",
    )
    .unwrap();
    let before = db.reads().generation().unwrap();
    let mut update = pending(before.index_epoch);
    let state = update.next.as_mut().unwrap();
    state.roots.insert("../bad.rs".into());
    assert!(matches!(
        write(&db, &[unit("root.rs")], &update),
        Err(cc_model::CcError::InvalidParams(_))
    ));
    assert_eq!(db.reads().generation().unwrap(), before);
    assert!(db.reads().resolution_frontier().unwrap().is_none());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn prepared_frontier_keeps_legacy_bytes_and_late_failure_rolls_back_facts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.db");
    let db = IndexDb::open(&path).unwrap().0;
    let mut initial = pending(db.reads().generation().unwrap().index_epoch);
    let state = initial.next.as_mut().unwrap();
    state.roots.insert("café.rs".into());
    state.completed.insert("done.rs".into());
    let legacy_payload = initial.next.as_ref().unwrap().payload().unwrap();
    let legacy_digest = blake3::hash(legacy_payload.as_bytes()).to_hex().to_string();
    write(&db, &[unit("root.rs")], &initial).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let stored: (String, String, i64, i64) = conn
        .query_row(
            "SELECT payload,digest,root_count,completed_files FROM resolution_frontier WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(stored, (legacy_payload, legacy_digest, 2, 1));
    let before = db.reads().generation().unwrap();
    let frontier_before = db.reads().resolution_frontier().unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_frontier BEFORE INSERT ON resolution_frontier BEGIN
         SELECT RAISE(ABORT, 'late frontier publication failure'); END;",
    )
    .unwrap();
    let error = write(&db, &[unit("later.rs")], &pending(before.index_epoch)).unwrap_err();
    assert!(error.to_string().contains("late frontier publication failure"));
    assert_eq!(db.reads().generation().unwrap(), before);
    assert_eq!(db.reads().resolution_frontier().unwrap(), frontier_before);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(db
        .reads()
        .public_surfaces(&["later.rs".into()])
        .unwrap()
        .is_empty());
}
