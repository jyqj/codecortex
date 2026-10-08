//! Deterministic mark/unlink interleavings use the real production path and
//! its per-call no-op seam. Child processes operate only on this test's
//! private cache/SQLite fixture, with fixed seeds and atomic ready markers.

use super::*;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use crate::cache::CacheRead;
use crate::publish::{PublishVerdict, Publisher};
use crate::spec::{DocumentEncodingSpec, VectorSpace};
use crate::types::{DocSpecDigest, InputDigest};
use cc_db::semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    root: PathBuf,
    db: IndexDb,
    cache: ArtifactCache,
    space: VectorSpace,
    input: InputDigest,
    spec: DocSpecDigest,
}

fn inputs() -> (VectorSpace, InputDigest, DocSpecDigest) {
    let space = VectorSpace::new("fake/gc-mutation-process", 2).unwrap();
    let input = InputDigest::of_input(b"fixed private input").unwrap();
    let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "fixed-tokenizer")
        .unwrap()
        .digest()
        .unwrap();
    (space, input, spec)
}

fn cache(root: &Path) -> ArtifactCache {
    ArtifactCache::open(root.join("cache"), "private-gc-process".to_string()).unwrap()
}

impl Fixture {
    fn new(seed: u32) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cc-gc-mutation-{seed}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let db = IndexDb::open(&root.join("index.sqlite3")).unwrap().0;
        let cache = cache(&root);
        let (space, input, spec) = inputs();
        cache.put(&space, &input, &spec, &[1.0, 2.0], 1000).unwrap();
        Self {
            root,
            db,
            cache,
            space,
            input,
            spec,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn child(root: &Path, role: &str) -> OwnedChild {
    let log = std::fs::File::create(root.join(format!("{role}.log"))).unwrap();
    OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "gc::concurrency_tests::mutation_child",
                "--nocapture",
            ])
            .env("CC_GC_MUTATION_CHILD_ROOT", root)
            .env("CC_GC_MUTATION_CHILD_ROLE", role)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    )
}

fn signal(path: &Path) {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary).unwrap();
    file.write_all(b"ready\n").unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::rename(temporary, path).unwrap();
}

fn wait_ready(child: &mut OwnedChild, root: &Path, role: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("ready").exists() {
        if let Some(status) = child.0.try_wait().unwrap() {
            panic!(
                "child {role} exited before handshake: {status}; {}",
                std::fs::read_to_string(root.join(format!("{role}.log"))).unwrap()
            );
        }
        assert!(Instant::now() < deadline, "child {role} ready timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_success(child: &mut OwnedChild, root: &Path, role: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "child {role} failed: {status}; {}",
                std::fs::read_to_string(root.join(format!("{role}.log"))).unwrap()
            );
            return;
        }
        assert!(Instant::now() < deadline, "child {role} completion timeout");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn seed_task(db: &IndexDb, space: &VectorSpace, input: &InputDigest) {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at)
         VALUES('src/d.rs','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text)
         VALUES('c-d','src/d.rs','rust',0,1,2,'body');",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space.digest().unwrap().as_str()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json)
         VALUES('d','v1','src/d.rs','c-d','enc','{}',?1)",
        [format!("{{\"input\":{{\"input_hash\":\"{}\"}}}}", input.as_str())],
    )
    .unwrap();
    supersede_and_enqueue_on(
        &conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d".to_string(),
                doc_version: "v1".to_string(),
                input_digest: input.as_str().to_string(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
}

#[test]
#[ignore = "child entry; executed by the owning process with a private fixture"]
fn mutation_child() {
    let root = PathBuf::from(std::env::var_os("CC_GC_MUTATION_CHILD_ROOT").unwrap());
    let role = std::env::var("CC_GC_MUTATION_CHILD_ROLE").unwrap();
    let cache = cache(&root);
    if role == "hold" {
        let _mutation = cache.lock_mutation().unwrap();
        signal(&root.join("ready"));
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }

    assert_eq!(role, "publish");
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(cache.namespace_dir().join(".mutation.lock"))
        .unwrap();
    assert!(matches!(
        probe.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    signal(&root.join("ready"));
    drop(probe);

    let db = IndexDb::open(&root.join("index.sqlite3")).unwrap().0;
    let (space, input, spec) = inputs();
    seed_task(&db, &space, &input);
    let task = db
        .claim_semantic("private-publish-child", 30.0)
        .unwrap()
        .unwrap();
    let publisher = Publisher::new(
        &db,
        &cache,
        &space,
        &spec,
        db.reads().read_generation().unwrap().incarnation,
    )
    .unwrap();
    assert!(matches!(
        publisher
            .publish_embedding(&task, &[2.0, 1.0], 1000)
            .unwrap(),
        PublishVerdict::Published { .. }
    ));
}

#[test]
fn namespace_lock_spans_mark_and_unlink_before_concurrent_publish() {
    for seed in [137, 139, 149] {
        let fixture = Fixture::new(seed);
        let batch = collect_candidates(&fixture.cache, None, 8).unwrap();
        assert_eq!(batch.entries.len(), 1);
        let config = GcConfig {
            min_retention_secs: 3600,
            batch_entries: 8,
            now_unix: 4600,
        };
        let mut publisher = None;
        let counters =
            sweep_batch_with_mark_hook(&fixture.db, &fixture.cache, &config, &batch, || {
                let mut process = child(&fixture.root, "publish");
                wait_ready(&mut process, &fixture.root, "publish");
                publisher = Some(process);
            })
            .unwrap();
        assert_eq!(
            counters.deleted_objects, 1,
            "old unreferenced object must be reclaimed"
        );
        wait_success(publisher.as_mut().unwrap(), &fixture.root, "publish");
        let hit = fixture
            .cache
            .get(&fixture.space, &fixture.input, &fixture.spec)
            .unwrap();
        let CacheRead::Hit(hit) = hit else {
            panic!("newly published object missing: {hit:?}")
        };
        assert_eq!(hit.data, vec![2.0, 1.0]);
        let conn = fixture.db.read_conn().unwrap();
        let reference: String = conn
            .query_row("SELECT artifact_ref FROM semantic_manifest", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(hit.artifact_ref.as_str(), reference);
        drop(conn);
        let kept = run_gc_pass(&fixture.db, &fixture.cache, &config, None)
            .unwrap()
            .0;
        assert_eq!(kept.kept_referenced, 1);
    }
}

#[test]
fn killed_mutation_owner_releases_cross_process_lock() {
    let fixture = Fixture::new(151);
    let mut process = child(&fixture.root, "hold");
    wait_ready(&mut process, &fixture.root, "hold");
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.cache.namespace_dir().join(".mutation.lock"))
        .unwrap();
    assert!(matches!(
        probe.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    process.0.kill().unwrap();
    assert!(!process.0.wait().unwrap().success());
    probe
        .try_lock()
        .expect("OS lock survives as an unlocked persistent inode after child exit");
    probe.unlock().unwrap();
    fixture
        .cache
        .put(
            &fixture.space,
            &fixture.input,
            &fixture.spec,
            &[3.0, 4.0],
            4600,
        )
        .unwrap();
    assert!(matches!(
        fixture
            .cache
            .get(&fixture.space, &fixture.input, &fixture.spec)
            .unwrap(),
        CacheRead::Hit(_)
    ));
}

#[test]
fn publisher_keeps_namespace_lease_between_verified_bytes_and_manifest_cas() {
    let fixture = Fixture::new(157);
    seed_task(&fixture.db, &fixture.space, &fixture.input);
    let task = fixture
        .db
        .claim_semantic("verify-window", 30.0)
        .unwrap()
        .unwrap();
    let publisher = Publisher::new(
        &fixture.db,
        &fixture.cache,
        &fixture.space,
        &fixture.spec,
        fixture.db.reads().read_generation().unwrap().incarnation,
    )
    .unwrap();
    let mut verified_window_observed = false;
    let verdict = publisher
        .publish_embedding_with_verify_hook(&task, &[4.0, 3.0], 1000, || {
            let probe = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(fixture.cache.namespace_dir().join(".mutation.lock"))
                .unwrap();
            assert!(matches!(
                probe.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            let conn = fixture.db.read_conn().unwrap();
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "the lease is observed before the manifest CAS");
            verified_window_observed = true;
        })
        .unwrap();
    assert!(verified_window_observed);
    assert!(matches!(verdict, PublishVerdict::Published { .. }));
    let config = GcConfig {
        min_retention_secs: 3600,
        batch_entries: 8,
        now_unix: 4600,
    };
    let counters = run_gc_pass(&fixture.db, &fixture.cache, &config, None)
        .unwrap()
        .0;
    assert_eq!(counters.kept_referenced, 1);
    assert!(matches!(
        fixture
            .cache
            .get(&fixture.space, &fixture.input, &fixture.spec)
            .unwrap(),
        CacheRead::Hit(_)
    ));
}
