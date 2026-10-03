//! Independent prepared staging / interprocess GC boundary evidence, not full V17.
#![cfg(unix)]
use cc_db::{
    index_db::IndexDb,
    semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert},
};
use cc_semantic::{
    cache::{ArtifactCache, CacheRead},
    gc::{collect_candidates, sweep_batch, GcConfig},
    ports::{DocumentInput, EmbeddingProvider},
    providers::fake::{FakeProvider, FakeProviderConfig},
    publish::{PublishVerdict, Publisher},
    queue::LeaseGuard,
    reconcile::{reconcile_after_rebuild, ReconcileOptions, ReconcileVerdict},
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
    durable(
        &root.join("ready"),
        std::process::id().to_string().as_bytes(),
    );
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

fn projection(conn: &rusqlite::Connection, doc: &str, input: &InputDigest) {
    let file = format!("src/{doc}.rs");
    let chunk = format!("c-{doc}");
    conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','hash',1.0,1,'2026-01-01')",[&file]).unwrap();
    conn.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,2,'body')",rusqlite::params![chunk,file]).unwrap();
    conn.execute("INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,'v1',?2,?3,'enc','{}',?4)",rusqlite::params![doc,file,chunk,format!("{{\"input\":{{\"input_hash\":\"{}\"}}}}",input.as_str())]).unwrap();
}
fn enqueue(conn: &rusqlite::Connection, doc: &str, input: &InputDigest) {
    supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: doc.into(),
                doc_version: "v1".into(),
                input_digest: input.as_str().into(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
}
fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
fn publish_cached(
    db: &IndexDb,
    cache: &ArtifactCache,
    space: &VectorSpace,
    spec: &DocSpecDigest,
    guard: &LeaseGuard<'_>,
    input: &InputDigest,
) {
    let hit = match cache.get(space, input, spec).unwrap() {
        CacheRead::Hit(h) => h,
        _ => panic!("missing precomputed fake vector"),
    };
    let p = Publisher::new(
        db,
        cache,
        space,
        spec,
        db.reads().read_generation().unwrap().incarnation,
    )
    .unwrap();
    assert_eq!(
        p.publish_embedding(guard.task(), &hit.data, 1000).unwrap(),
        PublishVerdict::Published {
            visible_set_changed: true
        }
    );
}
fn wait_file(path: &Path) {
    let end = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(Instant::now() < end, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "owned subprocess entry"]
fn staging_gc_child() {
    let root = PathBuf::from(std::env::var_os("P7_SG_ROOT").unwrap());
    let role = std::env::var("P7_SG_ROLE").unwrap();
    let (db, conn, cache, space, spec, input) = open(&root);
    if role.starts_with("staging") {
        let floor=db.admin().build_temp_db_staging(|tmp|{
   projection(tmp,"d1",&input);
   if role=="staging-writing"{
    // Callback is inside the real staging transaction, before its commit.
    durable(&root.join("physical-point.json"),serde_json::to_string(&serde_json::json!({"role":role,"staging_path":db.admin().rebuild_staging_path(),"inside_transaction":!tmp.is_autocommit()})).unwrap().as_bytes());
    assert!(!tmp.is_autocommit());signal_and_wait(&root);
   }
   Ok(())
  }).unwrap();
        if role == "staging-swapped" {
            db.admin().swap_rebuild_staging(floor).unwrap();
        }
        durable(&root.join("physical-point.json"),serde_json::to_string(&serde_json::json!({"role":role,"staging_exists":db.admin().rebuild_staging_path().exists(),"incarnation":db.reads().read_generation().unwrap().incarnation})).unwrap().as_bytes());
        signal_and_wait(&root);
    }
    if role == "gc" {
        let cfg = GcConfig {
            min_retention_secs: 3600,
            batch_entries: 16,
            now_unix: 5000,
        };
        let batch = collect_candidates(&cache, None, 16).unwrap();
        assert_eq!(batch.entries.len(), 3);
        durable(
            &root.join("collected"),
            std::process::id().to_string().as_bytes(),
        );
        wait_file(&root.join("sweep-release"));
        let c = sweep_batch(&db, &cache, &cfg, &batch).unwrap();
        assert_eq!(c.deleted_objects, 1); // independently unreferenced control
        assert_eq!(c.kept_referenced + c.kept_live_task, 2); // original plus target
        durable(&root.join("gc-counters.json"),serde_json::to_string(&serde_json::json!({"deleted_objects":c.deleted_objects,"kept_referenced":c.kept_referenced,"kept_live_task":c.kept_live_task,"kept_fresh":c.kept_fresh,"epoch":db.reads().read_generation().unwrap().semantic_epoch})).unwrap().as_bytes());
        signal_and_wait(&root);
    }
    assert!(role == "publisher-held" || role == "publisher-committed");
    let guard = LeaseGuard::claim(&db, "owned-publisher", 2.0)
        .unwrap()
        .unwrap();
    assert_eq!(guard.task().doc_key, "d2");
    let input = InputDigest::of_input(&std::fs::read(root.join("target.body")).unwrap()).unwrap();
    assert_eq!(input.as_str(), guard.task().input_digest);
    if role == "publisher-committed" {
        publish_cached(&db, &cache, &space, &spec, &guard, &input);
    }
    durable(&root.join("publisher-ready"),serde_json::to_string(&serde_json::json!({"pid":std::process::id(),"role":role,"task_id":guard.task().task_id,"token":guard.task().token,"state":conn.query_row("SELECT state FROM semantic_outbox WHERE doc_key='d2'",[],|r|r.get::<_,String>(0)).unwrap()})).unwrap().as_bytes());
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(root: &Path, role: &str) -> Owned {
    let f = std::fs::File::create(root.join(format!("{role}.log"))).unwrap();
    Owned(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "staging_gc_child", "--ignored", "--nocapture"])
            .env("P7_SG_ROOT", root)
            .env("P7_SG_ROLE", role)
            .stdout(f.try_clone().unwrap())
            .stderr(f)
            .spawn()
            .unwrap(),
    )
}
fn kill(child: &mut Owned) -> u32 {
    let pid = child.0.id();
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    pid
}
fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn intact(
    db: &IndexDb,
    cache: &ArtifactCache,
    space: &VectorSpace,
    spec: &DocSpecDigest,
    input: &InputDigest,
) -> String {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    let value: String = conn
        .query_row(
            "SELECT artifact_ref FROM semantic_manifest WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let hit = match cache.get(space, input, spec).unwrap() {
        CacheRead::Hit(h) => h,
        _ => panic!("lost original"),
    };
    assert_eq!(value, hit.artifact_ref.as_str());
    value
}
#[test]
fn staging_and_gc_interprocess_preparation() {
    let output = std::env::var_os("P7_SG_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("p7-sg-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    let mut rows = vec![];
    for seed in [17, 29, 43] {
        for point in [
            "staging-writing",
            "staging-built",
            "staging-swapped",
            "gc-held",
            "gc-committed",
        ] {
            let root = output.join(format!("{seed}-{point}"));
            std::fs::create_dir(&root).unwrap();
            durable(
                &root.join("input.body"),
                format!("fn staging_seed_{seed}()->u32{{{seed}}}\n").as_bytes(),
            );
            let original_ref;
            let original_inc;
            let original_epoch;
            {
                let (db, conn, cache, space, spec, input) = open(&root);
                seed_world(&conn, space.digest().unwrap().as_str(), input.as_str());
                let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
                let vector = provider
                    .embed_documents(&[DocumentInput::from_bytes(
                        &std::fs::read(root.join("input.body")).unwrap(),
                    )
                    .unwrap()])
                    .unwrap()
                    .remove(0);
                assert_eq!(provider.call_count(), 1);
                cache.put(&space, &input, &spec, &vector, 1000).unwrap();
                let guard = LeaseGuard::claim(&db, "original", 30.0).unwrap().unwrap();
                publish_cached(&db, &cache, &space, &spec, &guard, &input);
                original_ref = intact(&db, &cache, &space, &spec, &input);
                original_inc = db.reads().read_generation().unwrap().incarnation;
                original_epoch = db.reads().read_generation().unwrap().semantic_epoch;
                durable(&root.join("original.json"),serde_json::to_string(&serde_json::json!({"artifact_ref":original_ref,"incarnation":original_inc,"epoch":original_epoch,"fake_calls":provider.call_count()})).unwrap().as_bytes());
                if point.starts_with("gc") {
                    durable(
                        &root.join("target.body"),
                        format!("target-{seed}").as_bytes(),
                    );
                    // Pre-existing cached target and deletable orphan; collection happens before new desired task.
                    let target =
                        InputDigest::of_input(format!("target-{seed}").as_bytes()).unwrap();
                    let target_vector = provider
                        .embed_documents(&[DocumentInput::from_bytes(
                            format!("target-{seed}").as_bytes(),
                        )
                        .unwrap()])
                        .unwrap()
                        .remove(0);
                    cache
                        .put(&space, &target, &spec, &target_vector, 1000)
                        .unwrap();
                    let orphan =
                        InputDigest::of_input(format!("orphan-{seed}").as_bytes()).unwrap();
                    cache
                        .put(&space, &orphan, &spec, &[1.0, 0.0], 1000)
                        .unwrap();
                    assert_eq!(provider.call_count(), 2);
                    durable(&root.join("target.json"),serde_json::to_string(&serde_json::json!({"input_digest":target.as_str(),"orphan":orphan.as_str(),"fake_calls":provider.call_count()})).unwrap().as_bytes());
                }
            }
            let mut killed = vec![];
            if point.starts_with("staging") {
                let mut child = spawn(&root, point);
                wait_file(&root.join("ready"));
                assert_eq!(
                    std::fs::read_to_string(root.join("ready"))
                        .unwrap()
                        .parse::<u32>()
                        .unwrap(),
                    child.0.id()
                );
                killed.push(kill(&mut child));
                let (db, conn, cache, space, spec, input) = open(&root);
                if point != "staging-swapped" {
                    let staged = rusqlite::Connection::open_with_flags(
                        db.admin().rebuild_staging_path(),
                        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                    )
                    .unwrap();
                    assert_eq!(
                        count(&staged, "document_manifest"),
                        i64::from(point == "staging-built")
                    );
                    assert_eq!(count(&staged, "semantic_spaces"), 0);
                    assert_eq!(count(&staged, "semantic_manifest"), 0);
                }
                let generation = db.reads().read_generation().unwrap();
                assert_eq!(
                    count(&conn, "semantic_spaces"),
                    if point == "staging-swapped" { 0 } else { 1 }
                );
                if point != "staging-swapped" {
                    assert_eq!(generation.incarnation, original_inc);
                    assert_eq!(intact(&db, &cache, &space, &spec, &input), original_ref);
                    assert_eq!(generation.semantic_epoch, original_epoch);
                } else {
                    assert_ne!(generation.incarnation, original_inc);
                    assert_eq!(count(&conn, "semantic_manifest"), 0);
                    assert_eq!(count(&conn, "semantic_outbox"), 0);
                    conn.execute("INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",[space.digest().unwrap().as_str()]).unwrap();
                    match reconcile_after_rebuild(
                        &db,
                        &cache,
                        &space,
                        &spec,
                        generation.incarnation,
                        &ReconcileOptions {
                            lease_secs: 2.0,
                            retry_backoff_secs: 0.0,
                            max_attempts: 3,
                        },
                    )
                    .unwrap()
                    {
                        ReconcileVerdict::Applied(r) => assert_eq!((r.reused, r.requeued), (1, 0)),
                        _ => panic!("fenced"),
                    };
                    assert_eq!(intact(&db, &cache, &space, &spec, &input), original_ref);
                }
                assert_eq!(count(&conn, "semantic_spaces"), 1);
                assert_eq!(count(&conn, "semantic_manifest"), 1);
                let integrity: String = conn
                    .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(integrity, "ok");
                rows.push(serde_json::json!({"seed":seed,"point":point,"killed_pids":killed,"signal":9,"integrity":integrity,"original_ref_preserved":original_ref,"old_incarnation":original_inc,"new_incarnation":generation.incarnation,"initial_spaces":if point=="staging-swapped"{0}else{1},"final_spaces":1,"reuse_provider_calls":0,"total_fake_calls":1}));
            } else {
                let mut gc = spawn(&root, "gc");
                wait_file(&root.join("collected"));
                assert_eq!(
                    std::fs::read_to_string(root.join("collected"))
                        .unwrap()
                        .parse::<u32>()
                        .unwrap(),
                    gc.0.id()
                );
                let target_value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("target.json")).unwrap())
                        .unwrap();
                let target =
                    InputDigest::of_input(&std::fs::read(root.join("target.body")).unwrap())
                        .unwrap();
                assert_eq!(
                    target.as_str(),
                    target_value["input_digest"].as_str().unwrap()
                );
                {
                    let (_db, conn, _cache, _space, _spec, _input) = open(&root);
                    projection(&conn, "d2", &target);
                    enqueue(&conn, "d2", &target);
                }
                let mut publisher = spawn(
                    &root,
                    if point == "gc-held" {
                        "publisher-held"
                    } else {
                        "publisher-committed"
                    },
                );
                wait_file(&root.join("publisher-ready"));
                let pv: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("publisher-ready")).unwrap())
                        .unwrap();
                assert_eq!(pv["pid"].as_u64().unwrap(), u64::from(publisher.0.id()));
                durable(&root.join("sweep-release"), b"release");
                wait_file(&root.join("ready"));
                killed.push(kill(&mut gc));
                killed.push(kill(&mut publisher));
                let (db, conn, cache, space, spec, input) = open(&root);
                assert_eq!(intact(&db, &cache, &space, &spec, &input), original_ref);
                assert!(matches!(
                    cache.get(&space, &target, &spec).unwrap(),
                    CacheRead::Hit(_)
                ));
                let initial_manifest = count(&conn, "semantic_manifest");
                assert_eq!(initial_manifest, if point == "gc-held" { 1 } else { 2 });
                if point == "gc-held" {
                    let expiry: f64 = conn
                        .query_row(
                            "SELECT lease_expires_at FROM semantic_outbox WHERE doc_key='d2'",
                            [],
                            |r| r.get(0),
                        )
                        .unwrap();
                    let end = Instant::now() + Duration::from_secs(6);
                    while now() <= expiry + 1.0 {
                        assert!(Instant::now() < end);
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    match recover_scan(
                        &db,
                        &cache,
                        &space,
                        &spec,
                        db.reads().read_generation().unwrap().incarnation,
                        &RecoveryOptions {
                            scan_batch: 1,
                            lease_secs: 2.0,
                            retry_backoff_secs: 0.0,
                            max_attempts: 3,
                        },
                    )
                    .unwrap()
                    {
                        RecoveryVerdict::Applied(r) => {
                            assert_eq!((r.reclaimed, r.replayed, r.requeued), (1, 1, 0))
                        }
                        _ => panic!("fenced"),
                    };
                }
                let target_hit = match cache.get(&space, &target, &spec).unwrap() {
                    CacheRead::Hit(h) => h,
                    _ => panic!("target missing"),
                };
                let target_ref: String = conn
                    .query_row(
                        "SELECT artifact_ref FROM semantic_manifest WHERE doc_key='d2'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(target_ref, target_hit.artifact_ref.as_str());
                assert_eq!(count(&conn, "semantic_manifest"), 2);
                assert_eq!(count(&conn, "semantic_spaces"), 1);
                assert_eq!(
                    conn.query_row(
                        "SELECT COUNT(*) FROM semantic_outbox WHERE state='done'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )
                    .unwrap(),
                    2
                );
                let cfg = GcConfig {
                    min_retention_secs: 3600,
                    batch_entries: 16,
                    now_unix: 5000,
                };
                let batch = collect_candidates(&cache, None, 16).unwrap();
                let c = sweep_batch(&db, &cache, &cfg, &batch).unwrap();
                assert_eq!((c.deleted_objects, c.kept_referenced), (0, 2));
                assert_eq!(
                    db.reads().read_generation().unwrap().semantic_epoch,
                    Some(original_epoch.unwrap() + 1)
                ); // Fixture projection/enqueue SQL does not bump; one actual publication.
                let integrity: String = conn
                    .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(integrity, "ok");
                let marks: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("gc-counters.json")).unwrap())
                        .unwrap();
                assert_eq!(
                    marks["kept_live_task"].as_u64().unwrap(),
                    u64::from(point == "gc-held")
                );
                rows.push(serde_json::json!({"seed":seed,"point":point,"killed_pids":killed,"signal":9,"integrity":integrity,"gc":marks,"original_ref_preserved":original_ref,"initial_manifest":initial_manifest,"final_manifest":2,"active_spaces":1,"reuse_provider_calls":0,"total_fake_calls":2,"orphan_control_deleted":true,"postrestart_gc_deleted":0}));
            }
        }
    }
    durable(
        &output.join("results.json"),
        serde_json::to_string_pretty(&rows).unwrap().as_bytes(),
    );
}
