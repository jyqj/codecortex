//! Keep incremental test-edge cleanup/epoch semantics while eliminating empty scans.
use cc_db::index_db::IndexDb;
use std::time::Instant;

fn fixture(paths: &[(&str, i64)]) -> (tempfile::TempDir, IndexDb, rusqlite::Connection) {
    let root = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
    let c = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;BEGIN;").unwrap();
    for (path, flag) in paths {
        c.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at,is_test_file) VALUES(?1,'rust','synthetic',1,1,'2026-10-03',?2)",rusqlite::params![path,flag]).unwrap();
    }
    c.execute_batch("COMMIT;").unwrap();
    (root, db, c)
}
fn epoch(db: &IndexDb) -> u64 {
    db.reads().read_generation().unwrap().index_epoch
}
#[test]
fn no_tests_preserves_scoped_deletion_and_single_epoch_commit() {
    let (_root, db, c) = fixture(&[("a.rs", 0), ("b.rs", 0), ("c.rs", 0)]);
    for (id, a, b) in [("remove", "a.rs", "b.rs"), ("keep", "b.rs", "c.rs")] {
        c.execute("INSERT INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,'old-fixture',0.7)",rusqlite::params![id,a,b]).unwrap();
    }
    let old = epoch(&db);
    db.writes()
        .rebuild_test_edges_for_files(&["a.rs".into(), "absent.rs".into()])
        .unwrap();
    let ids: Vec<String> = c
        .prepare("SELECT edge_id FROM test_edges ORDER BY edge_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, ["keep"]);
    assert_eq!(epoch(&db), old + 1);
    db.writes().rebuild_test_edges_for_files(&[]).unwrap();
    assert_eq!(epoch(&db), old + 1);
}
#[test]
fn a_real_test_file_retains_both_incremental_branches() {
    let (_root, db, c) = fixture(&[("src/service.rs", 0), ("tests/service_test.rs", 1)]);
    for changed in ["src/service.rs", "tests/service_test.rs"] {
        db.writes()
            .rebuild_test_edges_for_files(&[changed.into()])
            .unwrap();
        let row: (String, String, String, f64) = c
            .query_row(
                "SELECT test_file_path,code_file_path,reason,confidence FROM test_edges",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "tests/service_test.rs".into(),
                "src/service.rs".into(),
                "same-basename".into(),
                0.9
            )
        );
    }
}
#[test]
fn malformed_flag_keeps_existing_error_and_transaction_rollback() {
    let (_root, db, c) = fixture(&[("a.rs", 0), ("b.rs", 0)]);
    c.execute(
        "UPDATE files SET is_test_file='malformed' WHERE file_path='a.rs'",
        [],
    )
    .unwrap();
    c.execute("INSERT INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES('old','a.rs','b.rs','old-fixture',0.7)",[]).unwrap();
    let old = epoch(&db);
    assert!(db
        .writes()
        .rebuild_test_edges_for_files(&["a.rs".into()])
        .is_err());
    assert_eq!(epoch(&db), old);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM test_edges", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn fifty_thousand_non_test_additions_have_no_edges_and_one_commit() {
    let paths: Vec<String> = (0..50_000).map(|i| format!("src/file_{i:05}.rs")).collect();
    let rows: Vec<_> = paths.iter().map(|p| (p.as_str(), 0)).collect();
    let (_root, db, c) = fixture(&rows);
    let old = epoch(&db);
    let start = Instant::now();
    db.writes().rebuild_test_edges_for_files(&paths).unwrap();
    assert_eq!(epoch(&db), old + 1);
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM test_edges", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        50_000
    );
    assert_eq!(
        c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    println!(
        "EMPTY_TEST_POPULATION {}",
        serde_json::json!({"files":50_000,"test_files":0,"elapsed_ms":start.elapsed().as_millis(),"wall_threshold_asserted":false})
    );
}
