use super::*;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

fn staging(db: &IndexDb, seed: &str) -> (PathBuf, IndexGeneration) {
    let path = db.rebuild_staging_path();
    let floor = db
        .build_rebuild_staging(&path, |path| {
            IndexDb::execute_temp_db_staging_build(path, |conn| {
                conn.execute(
                    "INSERT INTO metadata(key,value) VALUES('new-sentinel',?1)",
                    [seed],
                )
                .map_err(db_err)?;
                Ok(())
            })
        })
        .unwrap();
    (path, floor)
}

fn durable_signal(root: &Path, name: &str, bytes: &[u8]) {
    let temporary = root.join(format!("{name}.tmp"));
    let mut file = std::fs::File::create(&temporary).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::rename(temporary, root.join(name)).unwrap();
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "child entry; executed by the owning test with a private temporary database"]
fn rebuild_swap_child() {
    let root = PathBuf::from(std::env::var_os("CC_REBUILD_FAULT_ROOT").unwrap());
    let point = std::env::var("CC_REBUILD_FAULT_POINT").unwrap();
    let seed = std::env::var("CC_REBUILD_FAULT_SEED").unwrap();
    let db = IndexDb::open(&root.join("index.sqlite3")).unwrap().0;
    db.set_metadata("old-sentinel", &seed).unwrap();
    let generation = db.reads().read_generation().unwrap();
    durable_signal(
        &root,
        "before.json",
        &serde_json::to_vec(&serde_json::json!({
            "incarnation": generation.incarnation,
            "index_epoch": generation.index_epoch,
            "seed": seed,
        }))
        .unwrap(),
    );
    let (path, floor) = staging(&db, &seed);
    db.swap_rebuild_staging_with_hook(&path, floor, "owned process crash", |observed| {
        let selected = match point.as_str() {
            "sidecars-removed" => RebuildSwapPoint::SidecarsRemoved,
            "renamed" => RebuildSwapPoint::Renamed,
            _ => panic!("unknown crash point {point}"),
        };
        if observed == selected {
            durable_signal(&root, "ready", point.as_bytes());
            loop {
                std::thread::park_timeout(Duration::from_secs(1));
            }
        }
    })
    .unwrap();
    panic!("selected crash point was not reached");
}

#[test]
fn owned_process_kill_between_sidecar_cleanup_rename_and_reopen_preserves_database() {
    for seed in [163, 167, 173] {
        for point in ["sidecars-removed", "renamed"] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path();
            let log = std::fs::File::create(root.join("child.log")).unwrap();
            let mut child = OwnedChild(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "index_db_rebuild::fault_tests::rebuild_swap_child",
                        "--nocapture",
                    ])
                    .env("CC_REBUILD_FAULT_ROOT", root)
                    .env("CC_REBUILD_FAULT_POINT", point)
                    .env("CC_REBUILD_FAULT_SEED", seed.to_string())
                    .stdin(Stdio::null())
                    .stdout(log.try_clone().unwrap())
                    .stderr(log)
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(15);
            while !root.join("ready").exists() {
                if let Some(status) = child.0.try_wait().unwrap() {
                    panic!(
                        "child exited before {point}: {status}; {}",
                        std::fs::read_to_string(root.join("child.log")).unwrap()
                    );
                }
                assert!(
                    Instant::now() < deadline,
                    "seed {seed} {point}: ready timeout"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(std::fs::read_to_string(root.join("ready")).unwrap(), point);
            child.0.kill().unwrap();
            assert!(!child.0.wait().unwrap().success());
            let before: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("before.json")).unwrap()).unwrap();
            let path = root.join("index.sqlite3");
            let db = IndexDb::open(&path).unwrap().0;
            let generation = db.reads().read_generation().unwrap();
            if point == "sidecars-removed" {
                assert_eq!(
                    db.get_metadata("old-sentinel").unwrap(),
                    Some(seed.to_string())
                );
                assert_eq!(db.get_metadata("new-sentinel").unwrap(), None);
                assert_eq!(
                    serde_json::json!(generation.incarnation),
                    before["incarnation"]
                );
            } else {
                assert_eq!(
                    db.get_metadata("new-sentinel").unwrap(),
                    Some(seed.to_string())
                );
                assert_eq!(db.get_metadata("old-sentinel").unwrap(), None);
                assert_ne!(
                    serde_json::json!(generation.incarnation),
                    before["incarnation"]
                );
                assert!(generation.index_epoch > before["index_epoch"].as_u64().unwrap());
            }
            let conn = db.read_conn().unwrap();
            let integrity: String = conn
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .unwrap();
            assert_eq!(integrity, "ok");
            let foreign_key_failures: i64 = conn
                .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(foreign_key_failures, 0);
            drop(conn);
            db.set_metadata("after-recovery", "persisted").unwrap();
            drop(db);
            let reopened = IndexDb::open(&path).unwrap().0;
            assert_eq!(
                reopened.get_metadata("after-recovery").unwrap().as_deref(),
                Some("persisted")
            );
            eprintln!(
                "{}",
                serde_json::json!({"seed": seed, "point": point, "recovered": true, "integrity": "ok", "foreign_key_errors": 0})
            );
        }
    }
}

#[test]
fn writer_mutex_spans_rename_reopen_and_connection_installation() {
    let directory = tempfile::tempdir().unwrap();
    let db = Arc::new(
        IndexDb::open(&directory.path().join("index.sqlite3"))
            .unwrap()
            .0,
    );
    let (path, floor) = staging(&db, "179");
    let mut writer = None;
    let mut observed = Vec::new();
    db.swap_rebuild_staging_with_hook(&path, floor, "writer serialization", |point| {
        if matches!(
            point,
            RebuildSwapPoint::Renamed | RebuildSwapPoint::WriterReopened
        ) {
            assert!(matches!(
                db.write_conn.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            observed.push(point);
        }
        if point == RebuildSwapPoint::Renamed {
            let (entered, waiting) = mpsc::channel();
            let other = Arc::clone(&db);
            writer = Some(std::thread::spawn(move || {
                entered.send(()).unwrap();
                other.set_metadata("concurrent-writer", "new-database")
            }));
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    })
    .unwrap();
    assert_eq!(
        observed,
        vec![RebuildSwapPoint::Renamed, RebuildSwapPoint::WriterReopened]
    );
    writer.unwrap().join().unwrap().unwrap();
    let conn = Connection::open(db.admin().db_path()).unwrap();
    let value: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='concurrent-writer'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(value, "new-database");
    drop(conn);
    let path = db.admin().db_path().to_path_buf();
    drop(db);
    assert_eq!(
        IndexDb::open(&path)
            .unwrap()
            .0
            .get_metadata("concurrent-writer")
            .unwrap()
            .as_deref(),
        Some("new-database")
    );
}

fn failed_reopen_refuses_detached_handle(point: RebuildSwapPoint, seed: &str) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("index.sqlite3");
    let parked = directory.path().join("published.sqlite3");
    let db = IndexDb::open(&path).unwrap().0;
    db.set_metadata("old-sentinel", seed).unwrap();
    let (staged, floor) = staging(&db, seed);
    let mut injected = false;
    let result = db.swap_rebuild_staging_with_hook(&staged, floor, "reopen failure", |seen| {
        if seen == point {
            // A real filesystem failure after publication. SQLite must
            // reject opening a directory; its verdict is not mocked.
            std::fs::rename(&path, &parked).unwrap();
            std::fs::create_dir(&path).unwrap();
            injected = true;
        }
    });
    assert!(injected);
    assert!(result.is_err(), "opening the directory must fail");
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&parked, &path).unwrap();

    // Restoring the path does not repair the already returned handle.
    // Neither a successful write to its detached writer nor a read from
    // its previous pool may masquerade as authoritative state.
    assert!(
        db.set_metadata("after-failed-reopen", "must-not-commit")
            .is_err(),
        "a failed reopen left a writable detached or partially installed connection"
    );
    assert!(
        db.read_conn().is_err(),
        "a failed reopen left an old-generation read pool available"
    );
    drop(db);

    let reopened = IndexDb::open(&path).unwrap().0;
    assert_eq!(reopened.get_metadata("old-sentinel").unwrap(), None);
    assert_eq!(
        reopened.get_metadata("new-sentinel").unwrap().as_deref(),
        Some(seed)
    );
    assert_eq!(reopened.get_metadata("after-failed-reopen").unwrap(), None);
    reopened.set_metadata("explicitly-reopened", seed).unwrap();
    drop(reopened);
    assert_eq!(
        IndexDb::open(&path)
            .unwrap()
            .0
            .get_metadata("explicitly-reopened")
            .unwrap()
            .as_deref(),
        Some(seed)
    );
}

#[test]
fn failed_writer_reopen_leaves_no_writable_ghost_or_stale_read_pool() {
    failed_reopen_refuses_detached_handle(RebuildSwapPoint::Renamed, "181");
}

#[test]
fn failed_read_pool_reopen_leaves_no_partially_installed_writer_or_stale_reads() {
    failed_reopen_refuses_detached_handle(RebuildSwapPoint::WriterReopened, "191");
}
