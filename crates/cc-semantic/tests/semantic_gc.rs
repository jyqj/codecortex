//! P6-016 integration: GC 与发布协调.
//!
//! Covers what the unit-level mark predicates cannot see:
//!
//! 1. **引用保护** — a manifest-referenced object is never swept, whatever its
//!    age (V17: "manifest 引用永远可解析");
//! 2. **窗口竞争** — the artifact-before-manifest window: an aged candidate
//!    collected before a publish is kept by the sweep's DB snapshot
//!    synchronization point (the brief's race negative test), while a fresh
//!    unpublished object is protected by the retention grace and reclaimed
//!    after it;
//! 3. **有界性** — a backlog larger than the batch converges over explicit
//!    rounds with the keyset cursor, referenced objects surviving every round;
//! 4. **损坏衔接** — a P6-008 `Corrupt` detection is cleaned by the sweep and
//!    reads as `Miss` afterwards (the `discard()`-equivalent observable);
//! 5. **temp/半文件清扫** — `atomic_write` leftovers and half-written objects
//!    are swept, emptied directories pruned, the namespace and the P6-018
//!    quarantine area untouched;
//! 6. **全 space 引用** — a manifest row of a non-active space still protects
//!    its artifact (P6-017 rollback reuse).
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{claim_next_on, supersede_and_enqueue_on, OutboxPlan, OutboxUpsert};
use cc_semantic::cache::{ArtifactCache, CacheRead};
use cc_semantic::gc::{collect_candidates, run_gc_pass, sweep_batch, GcConfig, GcPosition};
use cc_semantic::publish::{PublishVerdict, Publisher};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};

// ── harness ──────────────────────────────────────────────────────────────

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-p6016-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn space() -> VectorSpace {
    VectorSpace::new("fake/model-gc", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

const T0: i64 = 1_000;
const GRACE: i64 = 3_600;

fn fresh_cfg() -> GcConfig {
    GcConfig {
        min_retention_secs: GRACE,
        batch_entries: 16,
        now_unix: T0, // anything put at T0 is inside the grace
    }
}

fn aged_cfg() -> GcConfig {
    GcConfig {
        min_retention_secs: GRACE,
        batch_entries: 16,
        now_unix: T0 + GRACE, // a T0 put sits exactly at the grace boundary → aged
    }
}

/// Insert the full FK chain (files → chunks → document_manifest →
/// semantic_manifest) for one manifest-protected object.
fn seed_manifest_row(
    conn: &rusqlite::Connection,
    doc_key: &str,
    space_id: &str,
    input_digest: &str,
    artifact_ref: &str,
) {
    let file_path = format!("src/{doc_key}.rs");
    conn.execute(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES(?1,'rust','hash',1.0,1,'2026-01-01')",
        rusqlite::params![file_path],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES(?1,?2,'rust',0,1,2,'body')",
        rusqlite::params![format!("c-{doc_key}"), file_path],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES(?1,'v1',?2,?3,NULL,'{}','{}')",
        rusqlite::params![doc_key, file_path, format!("c-{doc_key}")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
           space_id,artifact_ref,published_at,published_incarnation) \
         VALUES(?1,'v1',?2,'enc',?3,?4,?5,'2026-01-01','inc')",
        rusqlite::params![doc_key, file_path, input_digest, space_id, artifact_ref],
    )
    .unwrap();
}

/// The exact `artifact_ref` string the cache's `put` returns for a tuple
/// (checksum read back from the sidecar — the P6-008 format).
fn artifact_ref_of(
    cache: &ArtifactCache,
    space: &VectorSpace,
    input: &InputDigest,
    spec: &DocSpecDigest,
) -> String {
    let meta_path =
        object_dir(cache, space, input, spec).join(format!("{}.meta.json", spec.as_str()));
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    format!(
        "cas.v1:{}:{}:{}:{}:{}",
        cache.namespace(),
        space.digest().unwrap().as_str(),
        input.as_str(),
        spec.as_str(),
        value["checksum"].as_str().unwrap()
    )
}

/// The P6-008 object directory:
/// `<root>/namespace-<ns>/<space>/<input>/<spec>/`.
fn object_dir(
    cache: &ArtifactCache,
    space: &VectorSpace,
    input: &InputDigest,
    spec: &DocSpecDigest,
) -> PathBuf {
    cache
        .root()
        .join(format!("namespace-{}", cache.namespace()))
        .join(space.digest().unwrap().as_str())
        .join(input.as_str())
        .join(spec.as_str())
}

fn object_bin_path(
    cache: &ArtifactCache,
    space: &VectorSpace,
    input: &InputDigest,
    spec: &DocSpecDigest,
) -> PathBuf {
    object_dir(cache, space, input, spec).join(format!("{}.bin", spec.as_str()))
}

fn manifest_ref(conn: &rusqlite::Connection, doc_key: &str) -> String {
    conn.query_row(
        "SELECT artifact_ref FROM semantic_manifest WHERE doc_key=?1",
        [doc_key],
        |r| r.get(0),
    )
    .unwrap()
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space: VectorSpace,
    spec: DocSpecDigest,
    input: InputDigest,
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let (db, _) = IndexDb::open(&dir.0.join("index.sqlite3")).expect("open index");
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let space = space();
        let spec = doc_spec(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        let input = InputDigest::of_input(b"body text").expect("input digest");

        // Active space + one desired embed task (the publish pipeline world).
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space.digest().unwrap().as_str()],
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
                "{{\"input\":{{\"input_hash\":\"{}\"}}}}",
                input.as_str()
            )],
        )
        .unwrap();
        supersede_and_enqueue_on(
            &conn,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: "d1".into(),
                    doc_version: "v1".into(),
                    input_digest: input.as_str().into(),
                }],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();

        Self {
            _dir: dir,
            db,
            conn,
            cache,
            space,
            spec,
            input,
        }
    }

    fn incarnation(&self) -> [u8; 16] {
        let hex: String = self
            .conn
            .query_row(
                "SELECT value FROM metadata WHERE key='index_incarnation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut bytes = [0u8; 16];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    fn publisher(&self) -> Publisher<'_> {
        Publisher::new(
            &self.db,
            &self.cache,
            &self.space,
            &self.spec,
            self.incarnation(),
        )
        .expect("publisher")
    }

    fn claim(&self, now: f64) -> cc_db::semantic_outbox::ClaimedTask {
        claim_next_on(
            &self.conn,
            self.space.digest().unwrap().as_str(),
            "worker",
            now,
            60.0,
        )
        .unwrap()
        .expect("a ready task")
    }

    /// Put one orphan vector under a distinct input (kept clear of the seeded
    /// live task's input digest, which would lease-protect it).
    fn put_orphan(&self, tag: &str, created_at: i64) -> InputDigest {
        let input = InputDigest::of_input(tag.as_bytes()).expect("input digest");
        self.cache
            .put(&self.space, &input, &self.spec, &[1.0, 0.0], created_at)
            .expect("put orphan");
        input
    }
}

// ── 1. 引用保护 ──────────────────────────────────────────────────────────

#[test]
fn manifest_referenced_object_is_never_swept() {
    let world = World::new("referenced");
    let task = world.claim(T0 as f64);
    let verdict = world
        .publisher()
        .publish_embedding(&task, &[1.0, 0.0], T0)
        .expect("publish");
    assert!(matches!(verdict, PublishVerdict::Published { .. }));

    // Far beyond the retention grace: age alone never deletes a referenced
    // object (V17: the manifest reference must stay resolvable).
    let (counters, resume, exhausted) =
        run_gc_pass(&world.db, &world.cache, &aged_cfg(), None).expect("gc pass");
    assert_eq!(counters.kept_referenced, 1, "the published object is held");
    assert_eq!(counters.deleted_objects, 0);
    assert!(exhausted);
    assert!(resume.is_none());

    let published_ref = manifest_ref(&world.conn, "d1");
    match world
        .cache
        .get(&world.space, &world.input, &world.spec)
        .unwrap()
    {
        CacheRead::Hit(verified) => {
            assert_eq!(verified.artifact_ref.as_str(), published_ref);
        }
        other => panic!("published object must resolve: {other:?}"),
    }
}

// ── 2. 保留期宽限 ────────────────────────────────────────────────────────

#[test]
fn fresh_unpublished_object_is_graced_then_reclaimed() {
    let world = World::new("grace");
    let orphan = world.put_orphan("orphan-grace", T0);

    // Inside the retention grace: no manifest row, no live task for this
    // input — but the fresh timestamp exempts it (a publish may be in flight;
    // every publish re-puts, refreshing created_at).
    let (counters, _resume, exhausted) =
        run_gc_pass(&world.db, &world.cache, &fresh_cfg(), None).expect("gc pass");
    assert_eq!(counters.kept_fresh, 1);
    assert_eq!(counters.deleted_objects, 0);
    assert!(exhausted);
    assert!(matches!(
        world.cache.get(&world.space, &orphan, &world.spec).unwrap(),
        CacheRead::Hit(_)
    ));

    // After the grace, the same unreferenced object is reclaimable — the
    // orphan does not survive forever.
    let (counters, _resume, _exhausted) =
        run_gc_pass(&world.db, &world.cache, &aged_cfg(), None).expect("gc pass");
    assert_eq!(counters.deleted_objects, 1);
    assert_eq!(
        counters.pruned_dirs, 3,
        "spec leaf dir, input dir and space dir pruned"
    );
    assert_eq!(
        world.cache.get(&world.space, &orphan, &world.spec).unwrap(),
        CacheRead::Miss
    );
}

// ── 3. 窗口竞争（V17 竞态负测试）──────────────────────────────────────────

#[test]
fn publish_committing_between_collect_and_sweep_is_protected_by_the_snapshot() {
    let world = World::new("race");
    let task = world.claim(T0 as f64);

    // A stale worker attempt left the artifact long ago; by GC's clock it is
    // aged past the grace and unreferenced — a textbook sweep candidate.
    world
        .cache
        .put(
            &world.space,
            &world.input,
            &world.spec,
            &[1.0, 0.0],
            T0 - GRACE,
        )
        .expect("stale put");
    let cfg = GcConfig {
        min_retention_secs: GRACE,
        batch_entries: 16,
        now_unix: T0 + 1,
    };
    let batch = collect_candidates(&world.cache, None, cfg.batch_entries).expect("collect");
    assert_eq!(batch.entries.len(), 1, "the stale object is a candidate");

    // INTERLEAVE: the publisher re-puts (refreshing created_at) and its CAS
    // commits BEFORE the GC sweep's snapshot — artifact-before-manifest, with
    // the manifest row now referencing exactly this object.
    let verdict = world
        .publisher()
        .publish_embedding(&task, &[1.0, 0.0], T0)
        .expect("publish");
    assert!(matches!(
        verdict,
        PublishVerdict::Published {
            visible_set_changed: true
        }
    ));

    // The sweep decides under a snapshot that is LATER than the publish
    // commit (brief: "删除决定基于晚于 publish 提交的快照"), so the candidate
    // is re-marked referenced and kept.
    let counters = sweep_batch(&world.db, &world.cache, &cfg, &batch).expect("sweep");
    assert_eq!(counters.kept_referenced, 1);
    assert_eq!(counters.deleted_objects, 0);

    // V17: the manifest reference still resolves to a verified object.
    let published_ref = manifest_ref(&world.conn, "d1");
    match world
        .cache
        .get(&world.space, &world.input, &world.spec)
        .unwrap()
    {
        CacheRead::Hit(verified) => {
            assert_eq!(verified.artifact_ref.as_str(), published_ref);
        }
        other => panic!("manifest reference must stay resolvable: {other:?}"),
    }
}

#[test]
fn publish_committing_after_the_sweep_is_covered_by_the_grace() {
    let world = World::new("grace-race");
    let task = world.claim(T0 as f64);

    // The publisher's put lands AFTER the GC sweep already decided: the
    // object it is about to reference did not exist as an aged candidate at
    // sweep time, and the fresh `created_at` of the just-put object keeps it
    // exempt from every later pass until the CAS commits.
    let (counters, _resume, _exhausted) =
        run_gc_pass(&world.db, &world.cache, &aged_cfg(), None).expect("gc pass");
    assert_eq!(counters.deleted_objects, 0, "nothing to delete");

    let verdict = world
        .publisher()
        .publish_embedding(&task, &[1.0, 0.0], T0)
        .expect("publish");
    assert!(matches!(verdict, PublishVerdict::Published { .. }));

    // A GC pass immediately after: the object is referenced now anyway, but
    // assert the fresh-put path independently — zero age, zero risk.
    let (counters, _resume, _exhausted) =
        run_gc_pass(&world.db, &world.cache, &fresh_cfg(), None).expect("gc pass");
    assert_eq!(counters.kept_fresh + counters.kept_referenced, 1);
    assert_eq!(counters.deleted_objects, 0);
    assert!(manifest_ref(&world.conn, "d1").starts_with("cas.v1:"));
}

// ── 4. 有界性：超上限存量分轮收敛 ────────────────────────────────────────

#[test]
fn bounded_backlog_converges_over_rounds_and_reference_survives_every_round() {
    let world = World::new("bounded");

    // One manifest-protected object among the backlog.
    let kept_input = InputDigest::of_input(b"kept body").unwrap();
    world
        .cache
        .put(&world.space, &kept_input, &world.spec, &[1.0, 0.0], T0)
        .expect("put kept");
    let kept_ref = artifact_ref_of(&world.cache, &world.space, &kept_input, &world.spec);
    seed_manifest_row(
        &world.conn,
        "keep1",
        world.space.digest().unwrap().as_str(),
        kept_input.as_str(),
        &kept_ref,
    );

    // Seven aged, unreferenced orphans — more than one batch of 3.
    for n in 0..7 {
        world.put_orphan(&format!("orphan-{n}"), T0);
    }

    let cfg = GcConfig {
        min_retention_secs: GRACE,
        batch_entries: 3,
        now_unix: T0 + GRACE,
    };
    let mut cursor: Option<GcPosition> = None;
    let mut rounds = 0;
    let mut total_deleted = 0;
    let mut kept_seen = 0;
    loop {
        let (counters, resume, exhausted) =
            run_gc_pass(&world.db, &world.cache, &cfg, cursor.as_ref()).expect("gc pass");
        rounds += 1;
        total_deleted += counters.deleted_objects;
        kept_seen += counters.kept_referenced;
        cursor = resume;
        assert!(
            rounds < 20,
            "bounded rounds must converge, not loop forever"
        );
        if exhausted {
            break;
        }
    }

    assert_eq!(total_deleted, 7, "every orphan is eventually reclaimed");
    assert_eq!(kept_seen, 1, "the referenced object survives every round");
    assert!(rounds >= 3, "the backlog needed multiple bounded rounds");
    match world
        .cache
        .get(&world.space, &kept_input, &world.spec)
        .unwrap()
    {
        CacheRead::Hit(verified) => {
            assert_eq!(verified.artifact_ref.as_str(), kept_ref.as_str());
        }
        other => panic!("the referenced object must survive: {other:?}"),
    }
}

// ── 5. 损坏对象清扫衔接 P6-008 discard ───────────────────────────────────

#[test]
fn corrupt_object_is_swept_after_grace_and_reads_miss_like_discard() {
    let world = World::new("corrupt");
    let orphan = world.put_orphan("orphan-corrupt", T0);

    // Tamper the payload: P6-008 detects corruption on read (never serves).
    let bin = object_bin_path(&world.cache, &world.space, &orphan, &world.spec);
    let mut bytes = std::fs::read(&bin).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&bin, &bytes).unwrap();
    assert!(matches!(
        world.cache.get(&world.space, &orphan, &world.spec).unwrap(),
        CacheRead::Corrupt(_)
    ));

    // Still inside the grace: conservative — corrupt or not, it stays.
    let (counters, _resume, _exhausted) =
        run_gc_pass(&world.db, &world.cache, &fresh_cfg(), None).expect("gc pass");
    assert_eq!(counters.kept_fresh, 1);
    assert_eq!(counters.deleted_objects, 0);

    // After the grace the sweep unlinks both halves — the physical twin of
    // P6-008's discard(): the next read is a plain Miss.
    let (counters, _resume, _exhausted) =
        run_gc_pass(&world.db, &world.cache, &aged_cfg(), None).expect("gc pass");
    assert_eq!(counters.deleted_objects, 1);
    assert_eq!(
        world.cache.get(&world.space, &orphan, &world.spec).unwrap(),
        CacheRead::Miss
    );
    assert!(!bin.exists());
}

// ── 6. temp/半文件清扫 + 目录修剪 + quarantine 隔离 ──────────────────────

#[test]
fn temps_and_halves_are_swept_and_emptied_dirs_pruned_without_touching_quarantine() {
    let world = World::new("temp-half");

    // A complete object whose sidecar was lost → bin half.
    let binned = world.put_orphan("lost-meta", T0);
    let bin = object_bin_path(&world.cache, &world.space, &binned, &world.spec);
    let input_dir = bin.parent().unwrap().to_path_buf();
    let lost_meta = input_dir.join(format!("{}.meta.json", world.spec.as_str()));
    std::fs::remove_file(&lost_meta).unwrap();

    // A sidecar without a payload → meta half.
    let meta_half = input_dir.join("deadbeef.meta.json");
    std::fs::write(&meta_half, b"{\"format_version\":1}").unwrap();

    // An atomic_write temp leftover.
    let temp = input_dir.join(format!("{}.bin.tmp-4242-0", world.spec.as_str()));
    std::fs::write(&temp, b"half-written payload").unwrap();

    // A file in the P6-018 quarantine area — outside every namespace.
    let quarantine_file = world.cache.root().join("quarantine").join("bad.bin");
    std::fs::create_dir_all(quarantine_file.parent().unwrap()).unwrap();
    std::fs::write(&quarantine_file, b"quarantined").unwrap();

    // Fresh sweep (now=0): everything inside the grace — kept.
    let cfg_fresh = GcConfig {
        min_retention_secs: GRACE,
        batch_entries: 16,
        now_unix: 0,
    };
    let counters = {
        let batch = collect_candidates(&world.cache, None, 16).unwrap();
        sweep_batch(&world.db, &world.cache, &cfg_fresh, &batch).unwrap()
    };
    assert_eq!(counters.kept_fresh, 3, "bin half + meta half + temp");
    assert_eq!(counters.deleted_temps + counters.deleted_halves, 0);
    assert!(temp.exists() && meta_half.exists() && bin.exists());

    // Aged sweep (min_retention=0, now far beyond any mtime): all leftovers
    // swept, the emptied input/space directories pruned, quarantine intact.
    let cfg_aged = GcConfig {
        min_retention_secs: 0,
        batch_entries: 16,
        now_unix: i64::MAX / 2,
    };
    let counters = {
        let batch = collect_candidates(&world.cache, None, 16).unwrap();
        sweep_batch(&world.db, &world.cache, &cfg_aged, &batch).unwrap()
    };
    assert_eq!(counters.deleted_halves, 2);
    assert_eq!(counters.deleted_temps, 1);
    assert_eq!(counters.deleted_objects, 0);
    assert!(counters.pruned_dirs >= 2, "input dir and space dir pruned");
    assert!(!temp.exists() && !meta_half.exists() && !bin.exists());
    assert!(quarantine_file.exists(), "quarantine is never touched");
    assert!(
        world
            .cache
            .root()
            .join(format!("namespace-{}", world.cache.namespace()))
            .is_dir(),
        "the namespace directory itself survives"
    );
}

// ── 7. 全 space 引用（含非 active）────────────────────────────────────────

#[test]
fn revoked_space_object_stays_protected_by_its_manifest_row() {
    let world = World::new("revoked-space");

    // A space with NO semantic_spaces row at all (i.e. not active — the
    // P6-017 rollback-reuse shape): its object must still be held by the
    // manifest row of the space it was published under.
    let old_space = VectorSpace::new("fake/model-old-space", 2).expect("old space");
    let old_input = InputDigest::of_input(b"old body").unwrap();
    world
        .cache
        .put(&old_space, &old_input, &world.spec, &[0.5, 0.5], T0)
        .expect("put old-space object");
    let old_ref = artifact_ref_of(&world.cache, &old_space, &old_input, &world.spec);
    seed_manifest_row(
        &world.conn,
        "old1",
        old_space.digest().unwrap().as_str(),
        old_input.as_str(),
        &old_ref,
    );

    let (counters, _resume, exhausted) =
        run_gc_pass(&world.db, &world.cache, &aged_cfg(), None).expect("gc pass");
    assert_eq!(counters.kept_referenced, 1);
    assert_eq!(counters.deleted_objects, 0);
    assert!(exhausted);
    assert_eq!(
        manifest_ref(&world.conn, "old1"),
        old_ref,
        "the non-active space manifest row still resolves"
    );
}
