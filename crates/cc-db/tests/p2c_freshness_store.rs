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
