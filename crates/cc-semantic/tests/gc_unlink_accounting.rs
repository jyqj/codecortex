//! P7-016 subset: real unlink errors and recovery at the public collect/sweep
//! boundary. Fixed seeds describe owned temporary cache objects; no process
//! kill, provider call, permission change, or internal mark/unlink hook is used.
//! An error can follow a successful first unlink. This is not atomic deletion
//! or a proof against concurrent publication/replacement after the DB mark.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use cc_db::index_db::IndexDb;
use cc_model::{CcError, CcResult};
use cc_semantic::{
    cache::{ArtifactCache, CacheRead},
    gc::{collect_candidates, run_gc_pass, sweep_batch, GcConfig, GcCounters, GcEntryKind},
    spec::{DocumentEncodingSpec, VectorSpace},
    types::{DocSpecDigest, InputDigest},
};
use serde_json::{json, Value};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);
impl TempDir {
    fn new(seed: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-gc-unlink-{seed}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct World {
    _root: TempDir,
    db: IndexDb,
    cache: ArtifactCache,
    space: VectorSpace,
    input: InputDigest,
    spec: DocSpecDigest,
    bin: PathBuf,
    meta: PathBuf,
}
impl World {
    fn new(seed: u32) -> Self {
        let root = TempDir::new(seed);
        let (db, _) = IndexDb::open(&root.0.join("index.sqlite3")).unwrap();
        let cache = ArtifactCache::open(root.0.join("cache"), format!("unlink-{seed}")).unwrap();
        let space = VectorSpace::new("fake/gc-unlink-accounting", 2).unwrap();
        let input = InputDigest::of_input(format!("gc-unlink-seed-{seed}").as_bytes()).unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "fixed-tokenizer")
            .unwrap()
            .digest()
            .unwrap();
        cache
            .put(&space, &input, &spec, &[1.0, seed as f32], 1000)
            .unwrap();
        assert!(matches!(
            cache.get(&space, &input, &spec).unwrap(),
            CacheRead::Hit(_)
        ));
        let leaf = cache
            .root()
            .join(format!("namespace-{}", cache.namespace()))
            .join(space.digest().unwrap().as_str())
            .join(input.as_str())
            .join(spec.as_str());
        let bin = leaf.join(format!("{}.bin", spec.as_str()));
        let meta = leaf.join(format!("{}.meta.json", spec.as_str()));
        Self {
            _root: root,
            db,
            cache,
            space,
            input,
            spec,
            bin,
            meta,
        }
    }

    fn assert_miss(&self) {
        assert!(matches!(
            self.cache
                .get(&self.space, &self.input, &self.spec)
                .unwrap(),
            CacheRead::Miss
        ));
    }
}

fn aged() -> GcConfig {
    GcConfig {
        min_retention_secs: 0,
        batch_entries: 8,
        now_unix: i64::MAX / 2,
    }
}

fn counters(value: &GcCounters) -> Value {
    json!({
        "kept_fresh": value.kept_fresh,
        "kept_referenced": value.kept_referenced,
        "kept_live_task": value.kept_live_task,
        "deleted_objects": value.deleted_objects,
        "deleted_halves": value.deleted_halves,
        "deleted_temps": value.deleted_temps,
        "pruned_dirs": value.pruned_dirs,
    })
}

fn outcome(value: &CcResult<GcCounters>) -> Value {
    match value {
        Ok(value) => json!({"status": "ok", "counters": counters(value)}),
        Err(error) => json!({"status": "error", "error": error.to_string()}),
    }
}

fn trace(seed: u32, phase: &str, value: Value) {
    if let Some(root) = std::env::var_os("CODECORTEX_GC_UNLINK_EVIDENCE_DIR") {
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(format!("seed-{seed}-{phase}.json")),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}

fn replace_with_directory(path: &Path) -> Vec<u8> {
    let original = std::fs::read(path).unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::create_dir(path).unwrap();
    original
}

fn restore_file(path: &Path, original: &[u8]) {
    std::fs::remove_dir(path).unwrap();
    std::fs::write(path, original).unwrap();
}

fn object_failure(seed: u32, fail_second_half: bool) {
    let world = World::new(seed);
    let generation = world.db.reads().read_generation().unwrap();
    let batch = collect_candidates(&world.cache, None, 8).unwrap();
    assert_eq!(batch.entries.len(), 1);
    assert!(matches!(batch.entries[0].kind, GcEntryKind::Object { .. }));
    let failed_path = if fail_second_half {
        &world.meta
    } else {
        &world.bin
    };
    let original = replace_with_directory(failed_path);
    let result = sweep_batch(&world.db, &world.cache, &aged(), &batch);
    trace(
        seed,
        "failed-unlink",
        json!({
            "seed": seed, "fault": "directory in collected file position", "fail_second_half": fail_second_half,
            "sweep": outcome(&result), "bin_is_file": world.bin.is_file(), "meta_is_file": world.meta.is_file(),
            "failed_path_is_directory": failed_path.is_dir(),
        }),
    );
    assert!(
        matches!(&result, Err(CcError::Io(_))),
        "unlink failure was hidden: {result:?}"
    );
    assert!(failed_path.is_dir());
    if fail_second_half {
        assert!(
            !world.bin.exists(),
            "the successful first unlink is not rolled back"
        );
    } else {
        assert!(
            world.meta.is_file(),
            "a failed first unlink stops before its counterpart"
        );
    }
    assert_eq!(world.db.reads().read_generation().unwrap(), generation);

    restore_file(failed_path, &original);
    // Retry from the previous cursor, with a new collection and DB mark. A
    // partial object is now a half; do not replay the old deletion verdict.
    let retry_batch = collect_candidates(&world.cache, None, 8).unwrap();
    assert_eq!(retry_batch.entries.len(), 1);
    assert_eq!(
        matches!(retry_batch.entries[0].kind, GcEntryKind::Half { .. }),
        fail_second_half
    );
    let (deleted, resume, exhausted) = run_gc_pass(&world.db, &world.cache, &aged(), None).unwrap();
    trace(
        seed,
        "recovered",
        json!({"seed": seed, "counters": counters(&deleted), "exhausted": exhausted, "resume_is_none": resume.is_none()}),
    );
    assert_eq!(deleted.deleted_objects, usize::from(!fail_second_half));
    assert_eq!(deleted.deleted_halves, usize::from(fail_second_half));
    assert_eq!(deleted.deleted_temps, 0);
    assert!(exhausted && resume.is_none());
    assert!(!world.bin.exists() && !world.meta.exists());
    world.assert_miss();
    assert_eq!(world.db.reads().read_generation().unwrap(), generation);
    let (again, resume, exhausted) = run_gc_pass(&world.db, &world.cache, &aged(), None).unwrap();
    assert_eq!(again, GcCounters::default());
    assert!(exhausted && resume.is_none());
}

#[test]
fn failed_first_object_unlink_preserves_counterpart_and_retries() {
    object_failure(17, false);
}

#[test]
fn failed_second_object_unlink_exposes_partial_deletion_and_retries_as_half() {
    object_failure(29, true);
}

fn single_file_failure(seed: u32, temp: bool) {
    let world = World::new(seed);
    let generation = world.db.reads().read_generation().unwrap();
    std::fs::remove_file(&world.bin).unwrap();
    let path = if temp {
        std::fs::remove_file(&world.meta).unwrap();
        let path = world.meta.parent().unwrap().join("leftover.bin.tmp-1-0");
        std::fs::write(&path, format!("leftover-{seed}")).unwrap();
        path
    } else {
        world.meta.clone()
    };
    let batch = collect_candidates(&world.cache, None, 8).unwrap();
    assert_eq!(batch.entries.len(), 1);
    assert_eq!(
        matches!(batch.entries[0].kind, GcEntryKind::Temp { .. }),
        temp
    );
    let original = replace_with_directory(&path);
    let result = sweep_batch(&world.db, &world.cache, &aged(), &batch);
    trace(
        seed,
        "failed-unlink",
        json!({"seed": seed, "temp": temp, "sweep": outcome(&result), "path_is_directory": path.is_dir()}),
    );
    assert!(
        matches!(&result, Err(CcError::Io(_))),
        "unlink failure was hidden: {result:?}"
    );
    assert!(path.is_dir());
    restore_file(&path, &original);
    let (deleted, resume, exhausted) = run_gc_pass(&world.db, &world.cache, &aged(), None).unwrap();
    trace(
        seed,
        "recovered",
        json!({"seed": seed, "counters": counters(&deleted), "exhausted": exhausted, "resume_is_none": resume.is_none()}),
    );
    assert_eq!(deleted.deleted_halves, usize::from(!temp));
    assert_eq!(deleted.deleted_temps, usize::from(temp));
    assert_eq!(deleted.deleted_objects, 0);
    assert!(exhausted && resume.is_none());
    assert!(!path.exists());
    assert_eq!(world.db.reads().read_generation().unwrap(), generation);
}

#[test]
fn failed_half_unlink_is_an_error_and_fresh_collection_recovers() {
    single_file_failure(43, false);
}

#[test]
fn failed_temp_unlink_is_an_error_and_fresh_collection_recovers() {
    single_file_failure(59, true);
}

#[test]
fn already_absent_halves_are_idempotent_and_never_count_as_unlinks() {
    for (seed, keep_bin, keep_meta) in [
        (61, false, false),
        (67, false, true),
        (71, true, false),
        (73, true, true),
    ] {
        let world = World::new(seed);
        let generation = world.db.reads().read_generation().unwrap();
        let batch = collect_candidates(&world.cache, None, 8).unwrap();
        assert_eq!(batch.entries.len(), 1);
        if !keep_bin {
            std::fs::remove_file(&world.bin).unwrap();
        }
        if !keep_meta {
            std::fs::remove_file(&world.meta).unwrap();
        }
        let result = sweep_batch(&world.db, &world.cache, &aged(), &batch);
        trace(
            seed,
            "absent-half-control",
            json!({"seed": seed, "keep_bin": keep_bin, "keep_meta": keep_meta, "sweep": outcome(&result)}),
        );
        let deleted = result.unwrap();
        assert_eq!(deleted.deleted_objects, usize::from(keep_bin || keep_meta));
        assert_eq!(deleted.deleted_halves + deleted.deleted_temps, 0);
        assert!(!world.bin.exists() && !world.meta.exists());
        world.assert_miss();
        let again = sweep_batch(&world.db, &world.cache, &aged(), &batch).unwrap();
        assert_eq!(again, GcCounters::default());
        assert_eq!(world.db.reads().read_generation().unwrap(), generation);
    }
}
