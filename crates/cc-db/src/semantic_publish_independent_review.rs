//! Review-only independent oracles for fixed PR127 head 9f47a21.
//! All databases are fresh temporary files. SQL is fixture/fault setup only.
use super::*;
use crate::semantic_outbox::{
    claim_next_on, supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert,
};
use std::{
    ffi::{c_char, c_int, c_void, CStr},
    sync::{mpsc, Arc},
    time::Duration,
};

struct Lab {
    _dir: tempfile::TempDir,
    db: Arc<IndexDb>,
    observer: Connection,
    tasks: Vec<ClaimedTask>,
}
impl Lab {
    fn new(n: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (db, _) = IndexDb::open(&dir.path().join("review.sqlite")).unwrap();
        let observer = Connection::open(db.admin().db_path()).unwrap();
        observer.execute_batch("PRAGMA foreign_keys=ON; INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('live','{}','active');").unwrap();
        for i in 0..n {
            let key = format!("review-{i}");
            let path = format!("review/{i}.rs");
            observer.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','h',1,2,'2026-10-03')", [&path]).unwrap();
            observer.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,1,'review')", rusqlite::params![key,path]).unwrap();
            observer.execute("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,'version',?2,?1,'encoding','{}','{\"input\":{\"input_hash\":\"digest\"}}')", rusqlite::params![key,path]).unwrap();
            Self::enqueue(&observer, &key);
        }
        let tasks = (0..n)
            .map(|_| {
                claim_next_on(&observer, "live", "reviewer", 20.0, 1.0)
                    .unwrap()
                    .unwrap()
            })
            .collect();
        Self {
            _dir: dir,
            db: Arc::new(db),
            observer,
            tasks,
        }
    }
    fn enqueue(conn: &Connection, key: &str) {
        supersede_and_enqueue_on(
            conn,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: key.into(),
                    doc_version: "version".into(),
                    input_digest: "digest".into(),
                }],
                removals: &[],
                now_unix: 10.0,
            },
        )
        .unwrap();
    }
    fn requests(&self) -> Vec<PublishRequest<'_>> {
        let incarnation = self.db.reads().read_generation().unwrap().incarnation;
        self.tasks
            .iter()
            .map(|t| PublishRequest {
                task_id: t.task_id,
                lease_token: &t.token,
                doc_key: &t.doc_key,
                doc_version: &t.doc_version,
                input_digest: &t.input_digest,
                space_id: "live",
                artifact_ref: "verified-upstream-ref",
                expected_incarnation: incarnation,
                retry_backoff_secs: 7.0,
                max_attempts: 4,
                now_unix: 100.0,
            })
            .collect()
    }
    // Serialize *every column* of affected tables, not just counts/selected fields.
    fn snapshot(&self) -> Vec<Vec<Vec<String>>> {
        ["semantic_outbox", "semantic_manifest", "metadata"]
            .iter()
            .map(|table| {
                let mut stmt = self
                    .observer
                    .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
                    .unwrap();
                let cols = stmt.column_count();
                stmt.query_map([], |row| {
                    Ok((0..cols)
                        .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                        .collect())
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
            })
            .collect()
    }
    fn health(&self) {
        assert!(
            self.db.write_conn.lock().unwrap().is_autocommit(),
            "group left a transaction open"
        );
        let id = self
            .db
            .reads()
            .get_metadata("index_incarnation")
            .unwrap()
            .unwrap();
        self.db
            .writes()
            .set_metadata("index_incarnation", &id)
            .unwrap();
        assert_eq!(
            self.db.reads().get_metadata("index_incarnation").unwrap(),
            Some(id)
        );
        self.observer
            .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
            .unwrap();
    }
    fn epoch(&self) -> u64 {
        self.db
            .reads()
            .read_generation()
            .unwrap()
            .semantic_epoch
            .unwrap_or(0)
    }
}

#[test]
fn review_validation_is_upfront_even_with_lifecycle_permit_held() {
    let lab = Lab::new(5);
    let fence = LifecycleFence::default();
    let _permit = fence.enter().unwrap(); // touching lifecycle would deadlock
    let req = lab.requests();
    let before = lab.snapshot();
    let mut cases = vec![req.clone()];
    for field in 0..4 {
        let mut pair = req[..2].to_vec();
        match field {
            0 => pair[1].task_id = pair[0].task_id,
            1 => pair[1].lease_token = pair[0].lease_token,
            2 => pair[1].doc_key = pair[0].doc_key,
            _ => pair[1].expected_incarnation = [42; 16],
        }
        cases.push(pair);
    }
    for case in cases {
        assert!(matches!(
            lab.db.publish_semantic_group(&case, &fence),
            Err(CcError::InvalidParams(_))
        ));
        assert_eq!(lab.snapshot(), before);
        lab.health();
    }
    assert_eq!(
        lab.db.publish_semantic_group(&[], &fence).unwrap(),
        Some(vec![])
    );
    assert_eq!(lab.snapshot()[..2], before[..2]);
}

#[test]
fn review_mixed_content_four_items_replay_does_not_charge_again() {
    let mut lab = Lab::new(4);
    let fence = LifecycleFence::default();
    lab.db.publish_semantic(&lab.requests()[3]).unwrap();
    let duplicate_before: String = lab
        .observer
        .query_row("SELECT published_at FROM semantic_manifest", [], |r| {
            r.get(0)
        })
        .unwrap();
    Lab::enqueue(&lab.observer, &lab.tasks[3].doc_key);
    lab.tasks[3] = claim_next_on(&lab.observer, "live", "duplicate", 20.0, 1.0)
        .unwrap()
        .unwrap();
    let mut req = lab.requests();
    req[1].input_digest = "wrong-digest";
    req[2].artifact_ref = "different-content";
    let before = lab.db.reads().read_generation().unwrap();
    let out = lab
        .db
        .publish_semantic_group(&req, &fence)
        .unwrap()
        .unwrap();
    assert_eq!(
        out.iter()
            .map(|o| (o.published, o.visible_set_changed))
            .collect::<Vec<_>>(),
        vec![(true, true), (false, false), (true, true), (true, false)]
    );
    assert_eq!(
        out[1].rejection,
        Some(PublishRejection::InputDigestMismatch)
    );
    let after = lab.db.reads().read_generation().unwrap();
    assert_eq!(
        after.semantic_epoch,
        Some(before.semantic_epoch.unwrap() + 2)
    );
    assert_eq!(
        (after.incarnation, after.index_epoch, after.evidence_epoch),
        (
            before.incarnation,
            before.index_epoch,
            before.evidence_epoch
        )
    );
    let content: Vec<(String,String,String)> = lab.observer.prepare("SELECT doc_key,artifact_ref,published_incarnation FROM semantic_manifest ORDER BY doc_key").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert_eq!(
        content.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
        vec![req[0].doc_key, req[2].doc_key, req[3].doc_key]
    );
    assert_eq!(content[1].1, "different-content");
    assert!(content
        .iter()
        .all(|r| r.2 == incarnation_hex(before.incarnation)));
    assert_eq!(
        lab.observer
            .query_row(
                "SELECT published_at FROM semantic_manifest WHERE doc_key=?1",
                [req[3].doc_key],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        duplicate_before
    );
    let retry: (String, i64, f64) = lab
        .observer
        .query_row(
            "SELECT state,attempt_count,available_at FROM semantic_outbox WHERE task_id=?1",
            [req[1].task_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(retry, ("pending".into(), 1, 107.0));
    let snapshot = lab.snapshot();
    for _ in 0..3 {
        assert!(lab
            .db
            .publish_semantic_group(&req, &fence)
            .unwrap()
            .unwrap()
            .iter()
            .all(|o| o.rejection == Some(PublishRejection::LeaseLost)));
        assert_eq!(lab.snapshot(), snapshot);
    }
    lab.health();
}

#[test]
fn review_live_changes_after_request_creation_and_expired_unreclaimed() {
    for mode in 0..6 {
        let lab = Lab::new(2);
        let req = lab.requests(); // create before mutating live authority
        match mode {
            0 => {
                lab.observer
                    .execute(
                        "UPDATE document_manifest SET doc_version='replacement' WHERE doc_key=?1",
                        [req[1].doc_key],
                    )
                    .unwrap();
            }
            1 => {
                lab.observer.execute("UPDATE document_manifest SET record_json='{\"input\":{\"input_hash\":\"new-digest\"}}' WHERE doc_key=?1",[req[1].doc_key]).unwrap();
            }
            2 => {
                lab.observer.execute_batch("UPDATE semantic_spaces SET state='revoked'; INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('replacement','{}','active');").unwrap();
            }
            3 => {
                lab.db
                    .writes()
                    .set_metadata("index_incarnation", &incarnation_hex([42; 16]))
                    .unwrap();
            }
            4 => {
                lab.observer
                    .execute(
                        "UPDATE semantic_outbox SET lease_token='successor' WHERE task_id=?1",
                        [req[1].task_id],
                    )
                    .unwrap();
            }
            _ => {} // lease deadline 21; publish at 100 with no reclaim is valid
        }
        let out = lab
            .db
            .publish_semantic_group(&req, &LifecycleFence::default())
            .unwrap()
            .unwrap();
        let expected = [
            Some(PublishRejection::DocVersionStale),
            Some(PublishRejection::InputDigestMismatch),
            Some(PublishRejection::SpaceNotActive),
            Some(PublishRejection::IncarnationMismatch),
            Some(PublishRejection::LeaseLost),
            None,
        ][mode];
        assert_eq!(out[1].rejection, expected);
        assert_eq!(
            lab.epoch(),
            if mode == 2 || mode == 3 {
                0
            } else if mode == 5 {
                2
            } else {
                1
            }
        );
        lab.health();
    }
}

#[test]
fn review_each_recoverable_error_cleans_transaction_and_next_publish_works() {
    // Errors in CAS read, retry, manifest, ack, epoch, and COMMIT (deferred FK).
    let injections = [
        "UPDATE document_manifest SET record_json='broken-json' WHERE doc_key='review-1';",
        "CREATE TRIGGER review_fault BEFORE UPDATE ON semantic_outbox WHEN NEW.doc_key='review-1' AND NEW.state='pending' BEGIN SELECT RAISE(ABORT,'retry fault'); END;",
        "CREATE TRIGGER review_fault BEFORE INSERT ON semantic_manifest WHEN NEW.doc_key='review-1' BEGIN SELECT RAISE(ABORT,'manifest fault'); END;",
        "CREATE TRIGGER review_fault BEFORE UPDATE ON semantic_outbox WHEN NEW.doc_key='review-1' AND NEW.state='done' BEGIN SELECT RAISE(ABORT,'ack fault'); END;",
        "CREATE TRIGGER review_fault BEFORE UPDATE ON metadata WHEN NEW.key='semantic_epoch' AND NEW.value='2' BEGIN SELECT RAISE(ABORT,'epoch fault'); END;",
        "CREATE TABLE review_parent(id PRIMARY KEY); CREATE TABLE review_child(id REFERENCES review_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER review_fault AFTER INSERT ON semantic_manifest WHEN NEW.doc_key='review-1' BEGIN INSERT INTO review_child VALUES(99); END;",
    ];
    for (mode, sql) in injections.iter().enumerate() {
        let lab = Lab::new(2);
        lab.observer.execute_batch(sql).unwrap();
        let before = lab.snapshot();
        let mut req = lab.requests();
        if mode == 1 {
            req[1].doc_version = "stale";
        }
        let error = lab
            .db
            .publish_semantic_group(&req, &LifecycleFence::default())
            .unwrap_err();
        if mode == 5 {
            assert!(error
                .to_string()
                .contains("outcome may be uncertain; reconcile state"));
        }
        assert_eq!(lab.snapshot(), before, "mode {mode}");
        lab.health();
        if mode == 0 {
            lab.observer.execute_batch("UPDATE document_manifest SET record_json='{\"input\":{\"input_hash\":\"digest\"}}';").unwrap();
        } else {
            lab.observer
                .execute_batch("DROP TRIGGER review_fault;")
                .unwrap();
        }
        assert!(lab
            .db
            .publish_semantic_group(&lab.requests(), &LifecycleFence::default())
            .unwrap()
            .unwrap()
            .iter()
            .all(|o| o.published));
        assert_eq!(lab.epoch(), 2);
        lab.health();
    }
    // Non-SQL per-item failure also occurs after the first item's writes.
    let lab = Lab::new(2);
    let before = lab.snapshot();
    let mut req = lab.requests();
    req[1].now_unix = f64::MAX;
    assert!(matches!(
        lab.db
            .publish_semantic_group(&req, &LifecycleFence::default()),
        Err(CcError::InvalidParams(_))
    ));
    assert_eq!(lab.snapshot(), before);
    lab.health();
    assert!(lab
        .db
        .publish_semantic_group(&lab.requests(), &LifecycleFence::default())
        .unwrap()
        .is_some());
}

#[test]
fn review_begin_busy_closed_and_poisoned_lifecycle_leave_connection_healthy() {
    let lab = Lab::new(2);
    lab.db
        .write_conn
        .lock()
        .unwrap()
        .busy_timeout(Duration::ZERO)
        .unwrap();
    let before = lab.snapshot();
    lab.observer.execute_batch("BEGIN IMMEDIATE;").unwrap();
    assert!(lab
        .db
        .publish_semantic_group(&lab.requests(), &LifecycleFence::default())
        .unwrap_err()
        .to_string()
        .contains("begin publish group"));
    lab.observer.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(lab.snapshot(), before);
    lab.health();
    let closed = LifecycleFence::default();
    closed.close();
    assert_eq!(
        lab.db
            .publish_semantic_group(&lab.requests(), &closed)
            .unwrap(),
        None
    );
    lab.health();
    let poisoned = LifecycleFence::default();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = poisoned.enter().unwrap();
        panic!("review lifecycle poison");
    }));
    assert_eq!(
        lab.db
            .publish_semantic_group(&lab.requests(), &poisoned)
            .unwrap(),
        None
    );
    lab.health();
    assert!(lab
        .db
        .publish_semantic_group(&lab.requests(), &LifecycleFence::default())
        .unwrap()
        .is_some());
}

#[test]
fn review_close_during_sqlite_contention_precedes_all_group_writes() {
    let lab = Lab::new(2);
    let fence = Arc::new(LifecycleFence::default());
    let before = lab.snapshot();
    lab.observer.execute_batch("BEGIN IMMEDIATE;").unwrap();
    std::thread::scope(|s| {
        let req = lab.requests();
        let (started_tx, started_rx) = mpsc::channel();
        let db = lab.db.clone();
        let pf = fence.clone();
        let publisher = s.spawn(move || {
            started_tx.send(()).unwrap();
            db.publish_semantic_group(&req, &pf)
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // Confirm facade owns its connection mutex and is blocked at BEGIN.
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while lab.db.write_conn.try_lock().is_ok() {
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
        fence.close(); // must return while SQLite writer remains held
        lab.observer.execute_batch("ROLLBACK;").unwrap();
        assert_eq!(publisher.join().unwrap().unwrap(), None);
    });
    assert_eq!(lab.snapshot(), before);
    lab.health();
}

#[test]
fn review_permit_is_retained_after_last_item_through_commit_and_cleanup() {
    for fail_commit in [false, true] {
        let lab = Lab::new(2);
        if fail_commit {
            lab.observer.execute_batch("CREATE TABLE review_parent(id PRIMARY KEY); CREATE TABLE review_child(id REFERENCES review_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER review_fault AFTER INSERT ON semantic_manifest BEGIN INSERT INTO review_child VALUES(99); END;").unwrap();
        }
        let before = lab.snapshot();
        let fence = Arc::new(LifecycleFence::default());
        let (at_end_tx, at_end_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (closing_tx, closing_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        std::thread::scope(|s| {
            let db = lab.db.clone();
            let pf = fence.clone();
            let req = lab.requests();
            let publisher = s.spawn(move || {
                db.publish_semantic_group_after_item(&req, &pf, |conn, i| {
                    if i == 1 {
                        assert!(!conn.is_autocommit());
                        assert!(pf.0.try_lock().is_err());
                        at_end_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    }
                })
            });
            at_end_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(lab.snapshot(), before);
            let cf = fence.clone();
            let closer = s.spawn(move || {
                closing_tx.send(()).unwrap();
                cf.close();
                closed_tx.send(()).unwrap();
            });
            closing_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert!(matches!(
                closed_rx.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            release_tx.send(()).unwrap();
            let result = publisher.join().unwrap();
            if fail_commit {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("commit publish group"));
            } else {
                assert!(result.unwrap().unwrap().iter().all(|o| o.published));
            }
            closed_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            closer.join().unwrap();
        });
        if fail_commit {
            assert_eq!(lab.snapshot(), before); // observed state for this FK failure only
        } else {
            assert_eq!(lab.epoch(), 2);
            assert_eq!(
                lab.observer
                    .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                2
            );
        }
        lab.health();
    }
}

// SQLite's supported authorizer seam; deny only transaction control in this
// fresh fixture. No dependency features, provider, disk faults, or crash seam.
unsafe extern "C" fn deny_rollback(
    _: *mut c_void,
    action: c_int,
    arg: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action == rusqlite::ffi::SQLITE_TRANSACTION
        && !arg.is_null()
        && unsafe { CStr::from_ptr(arg) }.to_bytes() == b"ROLLBACK"
    {
        rusqlite::ffi::SQLITE_DENY
    } else {
        rusqlite::ffi::SQLITE_OK
    }
}
fn authorizer(lab: &Lab, enable: bool) {
    let conn = lab.db.write_conn.lock().unwrap();
    assert_eq!(
        unsafe {
            rusqlite::ffi::sqlite3_set_authorizer(
                conn.handle(),
                if enable { Some(deny_rollback) } else { None },
                std::ptr::null_mut(),
            )
        },
        rusqlite::ffi::SQLITE_OK
    );
}

#[test]
fn review_counterexample_rollback_failure_leaves_transaction_and_blocks_next_publish() {
    for mode in 0..4 {
        let lab = Lab::new(2);
        let fence = LifecycleFence::default();
        match mode {
            0 => fence.close(),
            1 => lab.observer.execute_batch("CREATE TRIGGER review_fault BEFORE INSERT ON semantic_manifest WHEN NEW.doc_key='review-1' BEGIN SELECT RAISE(ABORT,'item failure'); END;").unwrap(),
            2 => lab.observer.execute_batch("CREATE TRIGGER review_fault BEFORE UPDATE ON metadata WHEN NEW.key='semantic_epoch' AND NEW.value='2' BEGIN SELECT RAISE(ABORT,'epoch failure'); END;").unwrap(),
            _ => lab.observer.execute_batch("CREATE TABLE review_parent(id PRIMARY KEY); CREATE TABLE review_child(id REFERENCES review_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER review_fault AFTER INSERT ON semantic_manifest BEGIN INSERT INTO review_child VALUES(99); END;").unwrap(),
        }
        authorizer(&lab, true);
        assert!(lab
            .db
            .publish_semantic_group(&lab.requests(), &fence)
            .is_err());
        authorizer(&lab, false); // remove fault; surviving transaction is facade state
        assert!(!lab.db.write_conn.lock().unwrap().is_autocommit());
        let next = lab
            .db
            .publish_semantic_group(&lab.requests(), &LifecycleFence::default())
            .unwrap_err();
        assert!(next
            .to_string()
            .contains("cannot start a transaction within a transaction"));
        assert!(lab
            .observer
            .execute_batch("PRAGMA busy_timeout=0; BEGIN IMMEDIATE;")
            .is_err());
        // Explicit test cleanup only; facade itself has no recovery branch.
        lab.db
            .write_conn
            .lock()
            .unwrap()
            .execute_batch("ROLLBACK;")
            .unwrap();
        lab.health();
    }
}
