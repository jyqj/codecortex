//! Independent preparation only: real SIGKILL at completed persistence boundaries.
//! Does not certify P7-016, production stdio startup recovery, or power loss.
#![cfg(unix)]
use cc_db::{
    index_db::IndexDb,
    semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert},
};
use cc_semantic::{
    cache::ArtifactCache,
    ports::{DocumentInput, EmbeddingProvider},
    providers::fake::{FakeProvider, FakeProviderConfig},
    publish::{PublishVerdict, Publisher},
    queue::{drain_pending, EmbedHandler, LeaseGuard, WorkerLimits},
    recovery::{recover_scan, RecoveryOptions, RecoveryVerdict},
    spec::{DocumentEncodingSpec, VectorSpace},
    types::{DocSpecDigest, InputDigest},
};
use std::os::unix::process::ExitStatusExt;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

fn seed_world(conn: &rusqlite::Connection, space_id: &str, input_digest: &str) {
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('src/d1.rs','rust','hash',1.0,1,'2026-01-01')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c-d1','src/d1.rs','rust',0,1,2,'body')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES('d1','v1','src/d1.rs','c-d1','enc','{}',?1)",
        rusqlite::params![format!(
            "{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}"
        )],
    )
    .unwrap();
    let stats = supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d1".into(),
                doc_version: "v1".into(),
                input_digest: input_digest.into(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
}

fn open(
    root: &Path,
) -> (
    IndexDb,
    rusqlite::Connection,
    ArtifactCache,
    VectorSpace,
    DocSpecDigest,
    InputDigest,
) {
    let (db, _) = IndexDb::open(&root.join("index.sqlite3")).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    let space = VectorSpace::new("fake/crash-preparation", 2).unwrap();
    let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "fake-tokenizer")
        .unwrap()
        .digest()
        .unwrap();
    let input = InputDigest::of_input(&std::fs::read(root.join("input.body")).unwrap()).unwrap();
    let cache = ArtifactCache::open(root.join("cache"), "synthetic-crash-only".into()).unwrap();
    (db, conn, cache, space, spec, input)
}
fn durable(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
}
fn signal_and_wait(root: &Path) -> ! {
    let pid = std::process::id();
    let ready = root.join("ready");
    let staging = root.join(format!("ready.{pid}.tmp"));
    durable(&staging, pid.to_string().as_bytes());
    std::fs::rename(&staging, &ready).unwrap();
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}
#[test]
#[ignore = "child entry; invoked only by the owning parent with an isolated directory"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("P7_CRASH_CHILD_ROOT").expect("owned root"));
    let point = std::env::var("P7_CRASH_CHILD_POINT").unwrap();
    let (db, conn, cache, space, spec, input) = open(&root);
    if point == "uncommitted" {
        conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        seed_world(&conn, space.digest().unwrap().as_str(), input.as_str());
        signal_and_wait(&root);
    }
    seed_world(&conn, space.digest().unwrap().as_str(), input.as_str());
    let guard = LeaseGuard::claim(&db, "owned-crash-child", 2.0)
        .unwrap()
        .unwrap();
    durable(&root.join("claimed.json"), serde_json::to_string(&serde_json::json!({"task_id":guard.task().task_id,"token":guard.task().token,"epoch":db.reads().read_generation().unwrap().semantic_epoch})).unwrap().as_bytes());
    if point == "claimed" {
        signal_and_wait(&root);
    }
    let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
    let vector = provider
        .embed_documents(&[DocumentInput::from_bytes(
            &std::fs::read(root.join("input.body")).unwrap(),
        )
        .unwrap()])
        .unwrap()
        .remove(0);
    durable(
        &root.join("provider-calls"),
        provider.call_count().to_string().as_bytes(),
    );
    if point == "artifact" {
        cache.put(&space, &input, &spec, &vector, 1_000).unwrap();
        signal_and_wait(&root);
    }
    assert_eq!(point, "published");
    let publisher = Publisher::new(
        &db,
        &cache,
        &space,
        &spec,
        db.reads().read_generation().unwrap().incarnation,
    )
    .unwrap();
    assert_eq!(
        publisher
            .publish_embedding(guard.task(), &vector, 1_000)
            .unwrap(),
        PublishVerdict::Published {
            visible_set_changed: true
        }
    );
    signal_and_wait(&root);
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn task(conn: &rusqlite::Connection) -> (String, i64) {
    conn.query_row("SELECT state,attempt_count FROM semantic_outbox", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .unwrap()
}
#[test]
fn isolated_sigkill_reopen_replay_preparation() {
    let output = std::env::var_os("P7_CRASH_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("p7-crash-preparation-{}", std::process::id()))
        });
    std::fs::create_dir_all(&output).unwrap();
    let mut rows = vec![];
    for seed in [17, 29, 43] {
        for point in ["uncommitted", "claimed", "artifact", "published"] {
            let root = output.join(format!("seed-{seed}-{point}"));
            // Refuse to overwrite another run or a previously retained failure.
            std::fs::create_dir(&root).unwrap();
            durable(
                &root.join("input.body"),
                format!("fn synthetic_seed_{seed}() -> u32 {{ {seed} }}\n").as_bytes(),
            );
            let log = std::fs::File::create(root.join("child.log")).unwrap();
            let mut child = OwnedChild(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", "crash_child", "--nocapture"])
                    .env("P7_CRASH_CHILD_ROOT", &root)
                    .env("P7_CRASH_CHILD_POINT", point)
                    .stdout(log.try_clone().unwrap())
                    .stderr(log)
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(15);
            while !root.join("ready").exists() {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child failed: {}",
                    root.display()
                );
                assert!(Instant::now() < deadline, "child did not reach boundary");
                std::thread::sleep(Duration::from_millis(10));
            }
            let ready_pid: u32 = std::fs::read_to_string(root.join("ready"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(ready_pid, child.0.id());
            child.0.kill().unwrap();
            let status = child.0.wait().unwrap();
            assert_eq!(status.signal(), Some(9));
            let (db, conn, cache, space, spec, _input) = open(&root);
            let integrity: String = conn
                .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                .unwrap();
            assert_eq!(integrity, "ok");
            let fk_rows = conn
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .query([])
                .unwrap()
                .next()
                .unwrap()
                .is_some();
            assert!(!fk_rows);
            let initial_epoch = db.reads().read_generation().unwrap().semantic_epoch;
            let initial_manifest = count(&conn, "semantic_manifest");
            if point == "uncommitted" {
                for table in [
                    "files",
                    "chunks",
                    "document_manifest",
                    "semantic_spaces",
                    "semantic_outbox",
                    "semantic_manifest",
                ] {
                    assert_eq!(count(&conn, table), 0);
                }
                rows.push(serde_json::json!({"seed":seed,"input_digest":_input.as_str(),"point":point,"pid":ready_pid,"signal":9,"integrity":integrity,"transaction_rollback":true}));
                continue;
            }
            assert_eq!(initial_manifest, i64::from(point == "published"));
            assert_eq!(
                task(&conn),
                (
                    if point == "published" {
                        "done"
                    } else {
                        "claimed"
                    }
                    .into(),
                    1
                )
            );
            let claimed: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join("claimed.json")).unwrap()).unwrap();
            let before_publish_epoch: Option<u64> =
                serde_json::from_value(claimed["epoch"].clone()).unwrap();
            if point != "published" {
                assert_eq!(initial_epoch, before_publish_epoch);
            } else {
                assert_eq!(initial_epoch, Some(before_publish_epoch.unwrap_or(0) + 1));
            }
            // Natural lease expiry; never alter DB lease timestamps to simulate a crash.
            let start = Instant::now();
            if point != "published" {
                let expires: f64 = conn
                    .query_row("SELECT lease_expires_at FROM semantic_outbox", [], |r| {
                        r.get(0)
                    })
                    .unwrap();
                while std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64()
                    <= expires + 1.0
                {
                    assert!(start.elapsed() < Duration::from_secs(6));
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let options = RecoveryOptions {
                scan_batch: 1,
                lease_secs: 2.0,
                retry_backoff_secs: 0.0,
                max_attempts: 3,
            };
            let generation = db.reads().read_generation().unwrap();
            let first =
                match recover_scan(&db, &cache, &space, &spec, generation.incarnation, &options)
                    .unwrap()
                {
                    RecoveryVerdict::Applied(r) => r,
                    _ => panic!("unexpected fence"),
                };
            assert_eq!(first.reclaimed, usize::from(point != "published"));
            assert_eq!(first.replayed, usize::from(point == "artifact"));
            assert_eq!(first.requeued, usize::from(point == "claimed"));
            let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
            let publisher =
                Publisher::new(&db, &cache, &space, &spec, generation.incarnation).unwrap();
            let resolver = |_: &cc_db::semantic_outbox::ClaimedTask| {
                DocumentInput::from_bytes(&std::fs::read(root.join("input.body")).unwrap())
                    .map(Some)
            };
            let handler = EmbedHandler::new(publisher, &provider, &resolver);
            drain_pending(
                &db,
                "reopened-worker",
                &WorkerLimits::validated(1, 2.0, 0.0, 3).unwrap(),
                &mut |g| handler.handle(g),
            )
            .unwrap();
            assert_eq!(provider.call_count(), usize::from(point == "claimed"));
            let expected_claims = match point {
                "claimed" => 3,
                "artifact" => 2,
                "published" => 1,
                _ => unreachable!(),
            };
            assert_eq!(task(&conn), ("done".into(), expected_claims));
            assert_eq!(count(&conn, "semantic_manifest"), 1);
            let final_epoch = db.reads().read_generation().unwrap().semantic_epoch;
            assert_eq!(final_epoch, Some(before_publish_epoch.unwrap_or(0) + 1));
            for _ in 0..3 {
                let r = match recover_scan(
                    &db,
                    &cache,
                    &space,
                    &spec,
                    generation.incarnation,
                    &options,
                )
                .unwrap()
                {
                    RecoveryVerdict::Applied(r) => r,
                    _ => panic!("fenced"),
                };
                assert_eq!(
                    (r.reclaimed, r.replayed, r.requeued, r.dead_letters),
                    (0, 0, 0, 0)
                );
                assert!(r.converged);
                assert_eq!(
                    db.reads().read_generation().unwrap().semantic_epoch,
                    final_epoch
                );
            }
            let first_calls = if root.join("provider-calls").exists() {
                std::fs::read_to_string(root.join("provider-calls"))
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            } else {
                0
            };
            assert_eq!(first_calls + provider.call_count(), 1);
            rows.push(serde_json::json!({"seed":seed,"input_digest":_input.as_str(),"point":point,"pid":ready_pid,"signal":9,"integrity":integrity,"initial_manifest":initial_manifest,"initial_epoch":initial_epoch,"reclaimed":first.reclaimed,"replayed":first.replayed,"requeued":first.requeued,"reopen_provider_calls":provider.call_count(),"total_fake_provider_calls":first_calls+provider.call_count(),"attempt_count":expected_claims,"final_epoch":final_epoch,"no_op_replay_rounds":3,"lease_wait_millis":start.elapsed().as_millis()}));
        }
    }
    durable(
        &output.join("results.json"),
        serde_json::to_string_pretty(&rows).unwrap().as_bytes(),
    );
}
