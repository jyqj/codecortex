//! Regression coverage for the incremental no-testfile fast path.
//! Fixtures exercise the public write facet and persist through a reopened DB.
use cc_db::index_db::IndexDb;
use rusqlite::{params, Connection};
use std::time::Instant;

struct Fixture {
    db: IndexDb,
    conn: Connection,
    path: std::path::PathBuf,
    _tmp: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, bool)]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("index.sqlite3");
        let db = IndexDb::open(&path).unwrap().0;
        let mut conn = Connection::open(&path).unwrap();
        let tx = conn.transaction().unwrap();
        for (path, is_test) in files {
            tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,is_test_file,indexed_at) \
                 VALUES(?1,'Python','h',1,100,?2,'2026-10-03T00:00:00Z')",
                params![path, is_test],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        Self {
            db,
            conn,
            path,
            _tmp: tmp,
        }
    }

    fn seed_edge(&self, id: &str, test: &str, code: &str) {
        self.conn
            .execute(
                "INSERT INTO test_edges VALUES(?1,?2,?3,'path-overlap',0.7)",
                params![id, test, code],
            )
            .unwrap();
    }

    fn edges(&self) -> Vec<(String, String, String, String, f64)> {
        self.conn
            .prepare(
                "SELECT edge_id,test_file_path,code_file_path,reason,confidence \
             FROM test_edges ORDER BY edge_id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn rebuild(&self, changed: &[&str]) {
        self.db
            .writes()
            .rebuild_test_edges_for_files(
                &changed.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            )
            .unwrap();
    }
}

#[test]
fn no_tests_deletes_both_changed_endpoints_and_commits_one_epoch() {
    let f = Fixture::new(&[("src/user.py", false), ("src/other.py", false)]);
    f.seed_edge("changed-code", "gone_test.py", "src/user.py");
    f.seed_edge("changed-test", "gone_test.py", "src/other.py");
    f.seed_edge("unaffected", "other_gone_test.py", "src/other.py");
    let before = f.db.reads().read_generation().unwrap();
    f.rebuild(&["src/user.py", "gone_test.py", "src/user.py"]);
    assert_eq!(
        f.edges().iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        ["unaffected"]
    );
    let after = f.db.reads().read_generation().unwrap();
    assert_eq!(after.index_epoch, before.index_epoch + 1);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    let reopened = IndexDb::open(&f.path).unwrap().0;
    assert_eq!(reopened.reads().read_generation().unwrap(), after);
    assert_eq!(
        reopened
            .read_conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM test_edges", [], |r| r
                .get::<_, i64>(0),)
            .unwrap(),
        1
    );
}

#[test]
fn empty_changed_is_a_noop_even_with_stale_edges() {
    let f = Fixture::new(&[]);
    f.seed_edge("stale", "missing_test.py", "missing_code.py");
    let before = f.db.reads().read_generation().unwrap();
    f.rebuild(&[]);
    assert_eq!(f.db.reads().read_generation().unwrap(), before);
    assert_eq!(f.edges().len(), 1);
}

#[test]
fn empty_database_nonempty_changed_still_commits_epoch() {
    let f = Fixture::new(&[]);
    f.seed_edge("stale", "missing_test.py", "missing_code.py");
    let before = f.db.reads().read_generation().unwrap();
    f.rebuild(&["missing_test.py", "missing_code.py"]);
    assert!(f.edges().is_empty());
    assert_eq!(
        f.db.reads().read_generation().unwrap().index_epoch,
        before.index_epoch + 1
    );
}

#[test]
fn deleting_last_test_row_cleans_edges_without_resurrection() {
    let f = Fixture::new(&[("src/user.py", false), ("tests/user_test.py", true)]);
    f.rebuild(&["tests/user_test.py"]);
    assert_eq!(f.edges().len(), 1);
    f.conn
        .execute("DELETE FROM files WHERE is_test_file = 1", [])
        .unwrap();
    f.rebuild(&["tests/user_test.py", "src/user.py"]);
    assert!(f.edges().is_empty());
}

#[test]
fn present_testfile_preserves_code_test_and_mixed_changed_behavior() {
    for changed in [
        vec!["src/user.py"],
        vec!["tests/user_test.py"],
        vec!["src/user.py", "tests/user_test.py"],
    ] {
        let f = Fixture::new(&[
            ("src/user.py", false),
            ("tests/user_test.py", true),
            ("tests/unrelated_test.py", true),
        ]);
        f.rebuild(&changed);
        let edges = f.edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0],
            (
                "test:tests/user_test.py:src/user.py".into(),
                "tests/user_test.py".into(),
                "src/user.py".into(),
                "same-basename".into(),
                0.9
            )
        );
        let before = f.db.reads().read_generation().unwrap();
        f.rebuild(&changed);
        assert_eq!(f.edges(), edges);
        assert_eq!(
            f.db.reads().read_generation().unwrap().index_epoch,
            before.index_epoch + 1
        );
    }
}

#[test]
fn delete_failure_rolls_back_previous_deletes_and_epoch() {
    let f = Fixture::new(&[("src/user.py", false)]);
    f.seed_edge("first", "gone.py", "src/user.py");
    f.seed_edge("fail", "other.py", "missing.py");
    f.conn
        .execute_batch(
            "CREATE TRIGGER fail_delete BEFORE DELETE ON test_edges \
        WHEN OLD.edge_id = 'fail' BEGIN SELECT RAISE(ABORT,'injected delete failure'); END;",
        )
        .unwrap();
    let edges = f.edges();
    let before = f.db.reads().read_generation().unwrap();
    assert!(f
        .db
        .writes()
        .rebuild_test_edges_for_files(&["src/user.py".into(), "missing.py".into()],)
        .is_err());
    assert_eq!(f.edges(), edges);
    assert_eq!(f.db.reads().read_generation().unwrap(), before);
}

#[test]
fn epoch_failure_rolls_back_no_testfile_edge_deletion() {
    let f = Fixture::new(&[("src/user.py", false)]);
    f.seed_edge("stale", "gone.py", "src/user.py");
    f.conn
        .execute_batch(
            "CREATE TRIGGER fail_epoch BEFORE INSERT ON metadata \
         BEGIN SELECT RAISE(ABORT,'injected epoch failure'); END;",
        )
        .unwrap();
    let edges = f.edges();
    let before = f.db.reads().read_generation().unwrap();
    assert!(f
        .db
        .writes()
        .rebuild_test_edges_for_files(&["src/user.py".into()])
        .is_err());
    assert_eq!(f.edges(), edges);
    assert_eq!(f.db.reads().read_generation().unwrap(), before);
}

#[test]
fn noncanonical_integer_test_flag_keeps_existing_branch_asymmetry() {
    let f = Fixture::new(&[("src/user.py", false), ("tests/user_test.py", true)]);
    f.conn
        .execute("UPDATE files SET is_test_file=2 WHERE is_test_file=1", [])
        .unwrap();
    // Existing code-side SQL selects exactly 1; the changed-test decoder
    // treats any nonzero integer as true. The guard must preserve both.
    f.rebuild(&["src/user.py"]);
    assert!(f.edges().is_empty());
    f.rebuild(&["tests/user_test.py"]);
    assert_eq!(f.edges().len(), 1);
    assert_eq!(f.edges()[0].3, "same-basename");
}

#[test]
fn malformed_changed_flags_keep_error_and_rollback() {
    for value in [
        rusqlite::types::Value::Text("malformed".into()),
        rusqlite::types::Value::Blob(vec![0]),
        rusqlite::types::Value::Real(0.5),
    ] {
        let f = Fixture::new(&[("src/user.py", false)]);
        f.seed_edge("stale", "gone.py", "src/user.py");
        f.conn
            .execute("UPDATE files SET is_test_file=?1", [value])
            .unwrap();
        let before = f.db.reads().read_generation().unwrap();
        let edges = f.edges();
        assert!(f
            .db
            .writes()
            .rebuild_test_edges_for_files(&["src/user.py".into()])
            .is_err());
        assert_eq!(f.edges(), edges);
        assert_eq!(f.db.reads().read_generation().unwrap(), before);
    }
}

fn profile_no_testfiles(count: usize) {
    let paths: Vec<String> = (0..count)
        .map(|i| format!("src/module_{:03}/file_{i:05}.py", i % 200))
        .collect();
    let files: Vec<(&str, bool)> = paths.iter().map(|p| (p.as_str(), false)).collect();
    let f = Fixture::new(&files);
    let before = f.db.reads().read_generation().unwrap();
    let start = Instant::now();
    f.db.writes().rebuild_test_edges_for_files(&paths).unwrap();
    eprintln!(
        "cold_no_tests files={count} changed={count} elapsed_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    assert!(f.edges().is_empty());
    assert_eq!(
        f.db.reads().read_generation().unwrap().index_epoch,
        before.index_epoch + 1
    );
    assert_eq!(
        f.conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        count as i64
    );
    assert_eq!(
        f.conn
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(f
        .conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
#[ignore = "1k old/new production-method profile; run explicitly with --release"]
fn profile_cold_1k_no_testfiles() {
    profile_no_testfiles(1_000);
}

#[test]
#[ignore = "50k production-method profile; run explicitly with --release and external 300s timeout"]
fn profile_cold_50k_no_testfiles() {
    profile_no_testfiles(50_000);
}
