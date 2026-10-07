//! Newly authored, normal-API oracle, identical on fixed source and candidate.
//! Expiration makes a claim reclaimable; token replacement is the publish fence.
use cc_db::{
    index_db::IndexDb,
    semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert},
    semantic_publish::PublishRejection,
};
use cc_semantic::{
    cache::ArtifactCache,
    ports::DocumentInput,
    publish::{PublishVerdict, Publisher},
    queue::{drain_pending_parallel_with_lifecycle, TaskExit, WorkerLimits},
    spec::{DocumentEncodingSpec, VectorSpace},
};
use std::{
    sync::{Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Hold {
    state: Mutex<(usize, bool)>,
    wake: Condvar,
}
impl Hold {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 += 1;
        self.wake.notify_all();
        let end = Instant::now() + Duration::from_secs(5);
        while !state.1 {
            let (next, timeout) = self
                .wake
                .wait_timeout(state, end.saturating_duration_since(Instant::now()))
                .unwrap();
            state = next;
            assert!(!timeout.timed_out(), "controlled provider hold expired");
        }
    }
    fn wait(&self, width: usize) {
        let mut state = self.state.lock().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while state.0 < width {
            let (next, timeout) = self
                .wake
                .wait_timeout(state, end.saturating_duration_since(Instant::now()))
                .unwrap();
            state = next;
            assert!(!timeout.timed_out(), "expected local width did not enter");
        }
        assert_eq!(state.0, width);
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
}
struct Release<'a>(&'a Hold);
impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[test]
fn expiry_reclaim_and_replacement_token_publication_contract() {
    let width: usize = std::env::var("CC_CONTRACT_EXPECT_WIDTH")
        .unwrap_or_else(|_| "4".into())
        .parse()
        .unwrap();
    assert!(width == 2 || width == 4);
    for mode in ["expired-unreclaimed", "reclaimed", "replacement-token"] {
        let root = std::path::PathBuf::from("/tmp").join(format!(
            "cc-new-token-oracle-{}-{width}-{mode}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let db = IndexDb::open(&root.join("index.db")).unwrap().0;
        let spec = DocumentEncodingSpec::new(
            VectorSpace::new("synthetic/token-oracle", 2).unwrap(),
            None,
            8192,
            "fake-tokenizer",
        )
        .unwrap();
        cc_semantic::space_switch::register_backfill_space(&db, &spec).unwrap();
        cc_semantic::space_switch::activate_space(&db, spec.space(), "new oracle").unwrap();
        let input = DocumentInput::from_bytes(b"new finite token oracle").unwrap();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        for i in 0..width {
            let path = format!("oracle_{i}.rs");
            conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','synthetic',1,1,'2026-10-03')", [&path]).unwrap();
            conn.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,1,'synthetic')", rusqlite::params![format!("chunk{i}"),path]).unwrap();
            conn.execute("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,'v1',?2,?3,'synthetic','{}',?4)", rusqlite::params![format!("doc{i}"),path,format!("chunk{i}"),serde_json::json!({"input":{"input_hash":input.input_digest.as_str()}}).to_string()]).unwrap();
            supersede_and_enqueue_on(
                &conn,
                &OutboxPlan {
                    upserts: &[OutboxUpsert {
                        doc_key: format!("doc{i}"),
                        doc_version: "v1".into(),
                        input_digest: input.input_digest.as_str().into(),
                    }],
                    removals: &[],
                    now_unix: now(),
                },
            )
            .unwrap();
        }
        drop(conn);
        let cache = ArtifactCache::open(root.join("cache"), "new-oracle".into()).unwrap();
        let digest = spec.digest().unwrap();
        let publisher = Publisher::new(
            &db,
            &cache,
            spec.space(),
            &digest,
            db.reads().read_generation().unwrap().incarnation,
        )
        .unwrap();
        let hold = Hold::default();
        let tokens = Mutex::new(Vec::new());
        let verdicts = Mutex::new(Vec::new());
        let limits = WorkerLimits::validated(width, 0.15, 30.0, 3).unwrap();
        let mut replacements = Vec::new();
        std::thread::scope(|scope| {
            let _release = Release(&hold);
            let job = scope.spawn(|| {
                drain_pending_parallel_with_lifecycle(
                    &db,
                    "new-oracle",
                    &limits,
                    None,
                    2,
                    &|guard| {
                        tokens.lock().unwrap().push(guard.task().clone());
                        hold.enter(); // controlled in-flight provider result, not DB fault injection
                        let verdict =
                            publisher.publish_embedding(guard.task(), &[1.0, 0.0], now() as i64)?;
                        verdicts.lock().unwrap().push(verdict);
                        Ok(TaskExit::Disposed)
                    },
                )
            });
            hold.wait(width);
            let expires: f64 = db
                .read_conn()
                .unwrap()
                .query_row(
                    "SELECT max(lease_expires_at) FROM semantic_outbox",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let end = Instant::now() + Duration::from_secs(5);
            while now() <= expires {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(2));
            }
            if mode != "expired-unreclaimed" {
                assert_eq!(db.reclaim_expired_semantic().unwrap(), width);
                if mode == "replacement-token" {
                    for _ in 0..width {
                        replacements.push(db.claim_semantic("new-owner", 60.0).unwrap().unwrap());
                    }
                    for old in tokens.lock().unwrap().iter() {
                        let replacement = replacements
                            .iter()
                            .find(|task| task.task_id == old.task_id)
                            .unwrap();
                        assert_ne!(old.token, replacement.token);
                    }
                }
                for old in tokens.lock().unwrap().iter() {
                    assert!(!db
                        .renew_semantic_lease(old.task_id, &old.token, 60.0)
                        .unwrap());
                    assert!(!db
                        .retry_semantic_task(old.task_id, &old.token, "stale oracle", 0.0, 3)
                        .unwrap());
                }
            }
            hold.release();
            assert_eq!(job.join().unwrap().unwrap().claimed, width);
        });
        let outcomes = verdicts.into_inner().unwrap();
        assert_eq!(outcomes.len(), width);
        if mode == "expired-unreclaimed" {
            assert!(outcomes
                .iter()
                .all(|v| matches!(v, PublishVerdict::Published { .. })));
        } else {
            assert!(outcomes
                .iter()
                .all(|v| *v == PublishVerdict::Rejected(PublishRejection::LeaseLost)));
        }
        let publications = || {
            db.read_conn()
                .unwrap()
                .query_row("SELECT count(*) FROM semantic_manifest", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(
            publications(),
            if mode == "expired-unreclaimed" {
                width as i64
            } else {
                0
            }
        );
        for task in &replacements {
            assert!(db
                .renew_semantic_lease(task.task_id, &task.token, 60.0)
                .unwrap());
            assert!(matches!(
                publisher
                    .publish_embedding(task, &[1.0, 0.0], now() as i64)
                    .unwrap(),
                PublishVerdict::Published { .. }
            ));
        }
        if mode == "replacement-token" {
            assert_eq!(publications(), width as i64);
        }
        println!("TOKEN_ORACLE width={width} mode={mode} old_published={} old_lease_lost={} replacements_published={}", if mode == "expired-unreclaimed" { width } else { 0 }, if mode == "expired-unreclaimed" { 0 } else { width }, replacements.len());
        drop(publisher);
        drop(cache);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
