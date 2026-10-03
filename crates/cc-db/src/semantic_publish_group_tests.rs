//! Independent temporary-DB oracles. Artifact refs are DB inputs; no cache claim.
use super::*;
use crate::semantic_outbox::{
    claim_next_on, supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert,
};
use std::sync::{mpsc, Arc};
use std::time::Duration;

struct Fixture {
    _dir: tempfile::TempDir,
    db: Arc<IndexDb>,
    conn: Connection,
    tasks: Vec<ClaimedTask>,
    incarnation: [u8; 16],
}
impl Fixture {
    fn new(n: usize) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (db, _) = IndexDb::open(&dir.path().join("index.sqlite3")).unwrap();
        let conn = Connection::open(db.admin().db_path()).unwrap();
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('sp','{}','active')",
            [],
        )
        .unwrap();
        for i in 0..n {
            let doc = format!("d{i}");
            let path = format!("src/{i}.rs");
            conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','hash',1,1,'2026-01-01')", [&path]).unwrap();
            conn.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,2,'body')", rusqlite::params![doc, path]).unwrap();
            conn.execute("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,'v1',?2,?1,'enc','{}','{\"input\":{\"input_hash\":\"input\"}}')", rusqlite::params![doc, path]).unwrap();
            supersede_and_enqueue_on(
                &conn,
                &OutboxPlan {
                    upserts: &[OutboxUpsert {
                        doc_key: doc,
                        doc_version: "v1".into(),
                        input_digest: "input".into(),
                    }],
                    removals: &[],
                    now_unix: 900.0,
                },
            )
            .unwrap();
        }
        let tasks = (0..n)
            .map(|_| {
                claim_next_on(&conn, "sp", "worker", 1000.0, 60.0)
                    .unwrap()
                    .unwrap()
            })
            .collect();
        let incarnation = db.reads().read_generation().unwrap().incarnation;
        Self {
            _dir: dir,
            db: Arc::new(db),
            conn,
            tasks,
            incarnation,
        }
    }
    fn requests(&self) -> Vec<PublishRequest<'_>> {
        self.tasks
            .iter()
            .map(|t| PublishRequest {
                task_id: t.task_id,
                lease_token: &t.token,
                doc_key: &t.doc_key,
                doc_version: &t.doc_version,
                input_digest: &t.input_digest,
                space_id: "sp",
                artifact_ref: "db-only-ref",
                expected_incarnation: self.incarnation,
                retry_backoff_secs: 5.0,
                max_attempts: 3,
                // Expired but unreclaimed is allowed: no wall-clock lease gate.
                now_unix: 2000.0,
            })
            .collect()
    }
    fn snapshot(
        &self,
    ) -> (
        Vec<(
            i64,
            String,
            Option<String>,
            i64,
            Option<f64>,
            Option<String>,
        )>,
        Vec<(String, String)>,
        Option<u64>,
    ) {
        let mut tasks = self.conn.prepare("SELECT task_id,state,lease_token,attempt_count,available_at,last_error FROM semantic_outbox ORDER BY task_id").unwrap();
        let tasks = tasks
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let mut rows = self
            .conn
            .prepare("SELECT doc_key,artifact_ref FROM semantic_manifest ORDER BY doc_key")
            .unwrap();
        let rows = rows
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        (
            tasks,
            rows,
            self.db.reads().read_generation().unwrap().semantic_epoch,
        )
    }
}

#[test]
fn group_four_changes_epoch_by_four_and_preserves_other_clocks() {
    let f = Fixture::new(4);
    let before = f.db.reads().read_generation().unwrap();
    let out =
        f.db.publish_semantic_group(&f.requests(), &LifecycleFence::default())
            .unwrap()
            .unwrap();
    assert_eq!(out.len(), 4);
    assert!(out.iter().all(|o| o.published && o.visible_set_changed));
    let after = f.db.reads().read_generation().unwrap();
    assert_eq!(after.semantic_epoch, Some(4));
    assert_eq!(
        (after.incarnation, after.index_epoch, after.evidence_epoch),
        (
            before.incarnation,
            before.index_epoch,
            before.evidence_epoch
        )
    );
    let (tasks, rows, _) = f.snapshot();
    assert!(tasks.iter().all(|t| t.1 == "done" && t.3 == 1));
    assert_eq!(rows.len(), 4);
    for (task, original) in tasks.iter().zip(&f.tasks) {
        assert_eq!(task.2.as_deref(), Some(original.token.as_str()));
    }
}

#[test]
fn group_mixes_success_rejection_and_q4_duplicate_in_input_order() {
    let mut f = Fixture::new(3);
    f.db.publish_semantic(&f.requests()[2]).unwrap();
    let t = &f.tasks[2];
    supersede_and_enqueue_on(
        &f.conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: t.doc_key.clone(),
                doc_version: t.doc_version.clone(),
                input_digest: t.input_digest.clone(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
    f.tasks[2] = claim_next_on(&f.conn, "sp", "worker", 1000.0, 60.0)
        .unwrap()
        .unwrap();
    let mut req = f.requests();
    req[1].doc_version = "stale";
    req[1].input_digest = "wrong"; // Version must retain priority over digest.
    req[1].space_id = "inactive";
    let out =
        f.db.publish_semantic_group(
            &[req[2].clone(), req[1].clone(), req[0].clone()],
            &LifecycleFence::default(),
        )
        .unwrap()
        .unwrap();
    assert!(out[0].published && !out[0].visible_set_changed);
    assert_eq!(out[1].rejection, Some(PublishRejection::DocVersionStale));
    assert!(out[2].published && out[2].visible_set_changed);
    let (tasks, rows, epoch) = f.snapshot();
    assert_eq!(epoch, Some(2));
    assert_eq!(rows.len(), 2);
    let rejected = tasks.iter().find(|t| t.0 == req[1].task_id).unwrap();
    assert_eq!(
        (
            rejected.1.as_str(),
            rejected.2.as_deref(),
            rejected.3,
            rejected.4,
            rejected.5.as_deref()
        ),
        (
            "pending",
            None,
            1,
            Some(2005.0),
            Some(PublishRejection::DocVersionStale.as_str())
        )
    );
}

#[test]
fn group_second_sql_error_rolls_back_success_and_rejection() {
    let f = Fixture::new(3);
    // First rejection changes retry state; second valid item changes manifest,
    // ack and epoch; third item's controlled SQL failure rolls all back.
    f.conn.execute_batch("CREATE TRIGGER fail_publish BEFORE INSERT ON semantic_manifest WHEN NEW.doc_key='d2' BEGIN SELECT RAISE(ABORT,'controlled publish error'); END;").unwrap();
    let before = f.snapshot();
    let mut req = f.requests();
    req[0].doc_version = "stale";
    assert!(f
        .db
        .publish_semantic_group(&req[1..], &LifecycleFence::default())
        .is_err()); // second item fails
    assert_eq!(f.snapshot(), before);
    assert!(f
        .db
        .publish_semantic_group(&req, &LifecycleFence::default())
        .is_err());
    assert_eq!(f.snapshot(), before);
    f.conn.execute_batch("DROP TRIGGER fail_publish;").unwrap();
    assert!(f
        .db
        .publish_semantic_group(&f.requests(), &LifecycleFence::default())
        .unwrap()
        .is_some());
}

#[test]
fn group_ack_epoch_and_commit_errors_return_no_partial_outcomes() {
    for sql in [
        "CREATE TRIGGER fail_ack BEFORE UPDATE OF state ON semantic_outbox WHEN NEW.state='done' AND NEW.doc_key='d1' BEGIN SELECT RAISE(ABORT,'controlled ack error'); END;",
        "CREATE TRIGGER fail_epoch BEFORE UPDATE ON metadata WHEN NEW.key='semantic_epoch' AND NEW.value='2' BEGIN SELECT RAISE(ABORT,'controlled epoch error'); END;",
        "CREATE TABLE test_parent(id INTEGER PRIMARY KEY); CREATE TABLE test_deferred(id INTEGER REFERENCES test_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_commit AFTER INSERT ON semantic_manifest WHEN NEW.doc_key='d1' BEGIN INSERT INTO test_deferred VALUES(1); END;",
    ] {
        let f = Fixture::new(2);
        f.conn.execute_batch(sql).unwrap();
        let before = f.snapshot();
        assert!(f.db.publish_semantic_group(&f.requests(), &LifecycleFence::default()).is_err());
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn group_closed_lifecycle_writes_nothing_and_single_api_still_works() {
    let f = Fixture::new(2);
    let fence = LifecycleFence::default();
    fence.close();
    let before = f.snapshot();
    assert_eq!(
        f.db.publish_semantic_group(&f.requests(), &fence).unwrap(),
        None
    );
    assert_eq!(f.snapshot(), before);
    assert!(f.db.publish_semantic(&f.requests()[0]).unwrap().published);
}

#[test]
fn invalid_groups_and_empty_do_not_acquire_db_lock() {
    let f = Fixture::new(5);
    let req = f.requests();
    // Poison only this fixture's write mutex: touching it would give Database.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = f.db.write_conn.lock().unwrap();
        panic!("controlled mutex poisoning");
    }));
    assert!(f.db.write_conn.is_poisoned());
    let fence = LifecycleFence::default();
    fence.close();
    assert_eq!(
        f.db.publish_semantic_group(&[], &fence).unwrap(),
        Some(vec![])
    );
    let mut same_doc = req[..2].to_vec();
    same_doc[1].doc_key = same_doc[0].doc_key;
    let mut same_task = req[..2].to_vec();
    same_task[1].task_id = same_task[0].task_id;
    let mut same_token = req[..2].to_vec();
    same_token[1].lease_token = same_token[0].lease_token;
    let mut different_incarnation = req[..2].to_vec();
    different_incarnation[1].expected_incarnation = [7; 16];
    for invalid in [
        &req[..],
        &same_doc[..],
        &same_task[..],
        &same_token[..],
        &different_incarnation[..],
    ] {
        assert!(matches!(
            f.db.publish_semantic_group(invalid, &fence),
            Err(CcError::InvalidParams(_))
        ));
    }
}

#[test]
fn group_permit_holds_close_until_commit_and_readers_see_whole_group() {
    let f = Fixture::new(4);
    let before = f.snapshot();
    let fence = Arc::new(LifecycleFence::default());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (closing_tx, closing_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let publish_fence = fence.clone();
        let publish_db = f.db.clone();
        let owned_tasks = f.tasks.clone();
        let incarnation = f.incarnation;
        let publisher = scope.spawn(move || {
            let requests: Vec<_> = owned_tasks
                .iter()
                .map(|t| PublishRequest {
                    task_id: t.task_id,
                    lease_token: &t.token,
                    doc_key: &t.doc_key,
                    doc_version: &t.doc_version,
                    input_digest: &t.input_digest,
                    space_id: "sp",
                    artifact_ref: "db-only-ref",
                    expected_incarnation: incarnation,
                    retry_backoff_secs: 5.0,
                    max_attempts: 3,
                    now_unix: 2000.0,
                })
                .collect();
            publish_db.publish_semantic_group_after_item(&requests, &publish_fence, |_, index| {
                if index == 0 {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(f.snapshot(), before);
        let close_fence = fence.clone();
        let closer = scope.spawn(move || {
            closing_tx.send(()).unwrap();
            close_fence.close();
            closed_tx.send(()).unwrap();
        });
        closing_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            closed_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release_tx.send(()).unwrap();
        closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        // Close returned: all four writes must already be committed.
        let (tasks, rows, epoch) = f.snapshot();
        assert_eq!(rows.len(), 4);
        assert!(tasks.iter().all(|t| t.1 == "done"));
        assert_eq!(epoch, Some(4));
        assert_eq!(publisher.join().unwrap().unwrap().unwrap().len(), 4);
        closer.join().unwrap();
    });
    assert_eq!(
        f.db.publish_semantic_group(&f.requests(), &fence).unwrap(),
        None
    );
}

#[test]
fn group_keeps_live_fences_rejection_priority_and_retry_exhaustion() {
    for expected in [
        PublishRejection::LeaseLost,
        PublishRejection::DocVersionStale,
        PublishRejection::InputDigestMismatch,
        PublishRejection::SpaceNotActive,
        PublishRejection::DocumentMissing,
    ] {
        let f = Fixture::new(2);
        let mut req = f.requests();
        let bad_id = req[1].task_id;
        req[1].max_attempts = 1; // Current claimed attempt must exhaust once.
        match expected {
            PublishRejection::LeaseLost => {
                req[1].lease_token = "forged-token";
                req[1].doc_version = "stale";
                req[1].input_digest = "wrong";
                req[1].space_id = "inactive";
            }
            PublishRejection::DocVersionStale => {
                req[1].doc_version = "stale";
                req[1].input_digest = "wrong";
                req[1].space_id = "inactive";
            }
            PublishRejection::InputDigestMismatch => {
                // A source that lost its embeddable encoding must reject even
                // with an otherwise matching digest, ahead of inactive space.
                f.conn
                    .execute(
                        "UPDATE document_manifest SET encoding_key=NULL WHERE doc_key=?1",
                        [req[1].doc_key],
                    )
                    .unwrap();
                req[1].space_id = "inactive";
            }
            PublishRejection::SpaceNotActive => req[1].space_id = "inactive",
            PublishRejection::DocumentMissing => {
                f.conn
                    .execute(
                        "DELETE FROM document_manifest WHERE doc_key=?1",
                        [req[1].doc_key],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.snapshot();
        let out =
            f.db.publish_semantic_group(&req, &LifecycleFence::default())
                .unwrap()
                .unwrap();
        assert!(out[0].published);
        assert_eq!(out[1].rejection, Some(expected));
        let (tasks, rows, epoch) = f.snapshot();
        assert_eq!(rows.len(), 1);
        assert_eq!(epoch, Some(1));
        let bad = tasks.iter().find(|t| t.0 == bad_id).unwrap();
        if expected == PublishRejection::LeaseLost {
            assert_eq!(bad, before.0.iter().find(|t| t.0 == bad_id).unwrap());
        } else {
            assert_eq!(bad.1, "failed");
            assert_eq!(bad.2, None);
            assert_eq!(bad.3, 1);
            assert_eq!(bad.5.as_deref(), Some(expected.as_str()));
        }
    }
    // Uniform old incarnation remains a live CAS rejection, not invalid input;
    // it takes priority even over a forged token and a stale source version.
    let f = Fixture::new(2);
    let mut req = f.requests();
    for r in &mut req {
        r.expected_incarnation = [7; 16];
        r.doc_version = "stale";
    }
    req[0].lease_token = "forged-token";
    let before = f.snapshot();
    let out =
        f.db.publish_semantic_group(&req, &LifecycleFence::default())
            .unwrap()
            .unwrap();
    assert!(out
        .iter()
        .all(|o| o.rejection == Some(PublishRejection::IncarnationMismatch)));
    let (tasks, rows, epoch) = f.snapshot();
    assert_eq!(tasks[0], before.0[0]); // fenced retry cannot affect real claimant
    assert_eq!(tasks[1].1, "pending");
    assert_eq!(tasks[1].3, 1);
    assert!(rows.is_empty());
    assert_eq!(epoch, None);
}

#[test]
fn group_reclaimed_token_cannot_ack_or_retry_successor() {
    let f = Fixture::new(2);
    assert_eq!(
        semantic_outbox::reclaim_expired_on(&f.conn, 1061.0).unwrap(),
        2
    );
    let successors: Vec<_> = (0..2)
        .map(|_| {
            claim_next_on(&f.conn, "sp", "successor", 1062.0, 60.0)
                .unwrap()
                .unwrap()
        })
        .collect();
    assert!(successors
        .iter()
        .zip(&f.tasks)
        .all(|(new, old)| new.token != old.token));
    let before = f.snapshot();
    let out =
        f.db.publish_semantic_group(&f.requests(), &LifecycleFence::default())
            .unwrap()
            .unwrap();
    assert!(out
        .iter()
        .all(|o| o.rejection == Some(PublishRejection::LeaseLost)));
    assert_eq!(f.snapshot(), before);
    let mut req = f.requests();
    for (r, successor) in req.iter_mut().zip(&successors) {
        r.lease_token = &successor.token;
    }
    assert!(f
        .db
        .publish_semantic_group(&req, &LifecycleFence::default())
        .unwrap()
        .unwrap()
        .iter()
        .all(|o| o.published));
    assert_eq!(f.snapshot().2, Some(2));
}
