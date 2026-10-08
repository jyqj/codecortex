//! P7-016: replayable filesystem/SQLite interleavings at collect → sweep.
//! The publisher's vector and caller clock deliberately vary independently:
//! freshness is not a substitute for revalidating the current artifact ref.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use cc_db::index_db::IndexDb;
use cc_semantic::cache::{ArtifactCache, CacheRead};
use cc_semantic::gc::{collect_candidates, run_gc_pass, sweep_batch, GcConfig, GcCounters};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};

static SEQUENCE: AtomicU32 = AtomicU32::new(0);

struct World {
    db: IndexDb,
    cache: ArtifactCache,
    space: VectorSpace,
    input: InputDigest,
    spec: DocSpecDigest,
    root: PathBuf,
}

impl World {
    fn new(seed: u32) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cc-semantic-gc-replacement-{seed}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let db = IndexDb::open(&root.join("index.sqlite3")).unwrap().0;
        let cache = ArtifactCache::open(root.join("cache"), format!("seed-{seed}")).unwrap();
        let space = VectorSpace::new("fake/gc-replacement", 2).unwrap();
        let input = InputDigest::of_input(format!("seed-{seed}").as_bytes()).unwrap();
        let spec = DocumentEncodingSpec::new(space.clone(), None, 8192, "fixed-tokenizer")
            .unwrap()
            .digest()
            .unwrap();
        Self {
            db,
            cache,
            space,
            input,
            spec,
            root,
        }
    }

    fn put(&self, vector: &[f32], timestamp: i64) -> String {
        self.cache
            .put(&self.space, &self.input, &self.spec, vector, timestamp)
            .unwrap()
            .as_str()
            .to_string()
    }

    fn assert_hit(&self, reference: &str, vector: &[f32]) {
        let hit = self
            .cache
            .get(&self.space, &self.input, &self.spec)
            .unwrap();
        match hit {
            CacheRead::Hit(hit) => {
                assert_eq!(hit.artifact_ref.as_str(), reference);
                assert_eq!(hit.data, vector);
            }
            other => panic!("current replacement was lost: {other:?}"),
        }
    }

    fn publish_reference(&self, reference: &str) {
        let conn = rusqlite::Connection::open(self.db.admin().db_path()).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at)
             VALUES('src/d.rs','rust','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text)
             VALUES('c-d','src/d.rs','rust',0,1,2,'body');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json)
             VALUES('d','v1','src/d.rs','c-d','enc','{}','{}');",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,
                 space_id,artifact_ref,published_at,published_incarnation)
             VALUES('d','v1','src/d.rs','enc',?1,?2,?3,'2026-01-01','inc')",
            rusqlite::params![
                self.input.as_str(),
                self.space.digest().unwrap().as_str(),
                reference,
            ],
        )
        .unwrap();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn config() -> GcConfig {
    GcConfig {
        min_retention_secs: 3600,
        batch_entries: 8,
        now_unix: 4600,
    }
}

#[test]
fn collected_old_timestamp_cannot_delete_a_fresh_reput() {
    for seed in [101, 103, 107] {
        let world = World::new(seed);
        let before = world.db.reads().read_generation().unwrap();
        world.put(&[1.0, 2.0], 1000);
        let collected = collect_candidates(&world.cache, None, 8).unwrap();
        assert_eq!(collected.entries.len(), 1);

        // Same pathname, same checksum, new filesystem publication and age.
        let reference = world.put(&[1.0, 2.0], 4600);
        let counters = sweep_batch(&world.db, &world.cache, &config(), &collected).unwrap();
        assert_eq!(counters.kept_fresh, 1, "seed {seed}: {counters:?}");
        assert_eq!(counters.deleted_objects, 0);
        world.assert_hit(&reference, &[1.0, 2.0]);
        assert_eq!(world.db.reads().read_generation().unwrap(), before);
    }
}

#[test]
fn collected_old_checksum_cannot_delete_current_manifest_reference() {
    for seed in [109, 113, 127] {
        let world = World::new(seed);
        let old = world.put(&[1.0, 2.0], 1000);
        let collected = collect_candidates(&world.cache, None, 8).unwrap();
        assert_eq!(collected.entries.len(), 1);

        // The new vector is also old enough to sweep: only the *current*
        // checksum's manifest mark may protect it, not the retention grace.
        let new = world.put(&[2.0, 1.0], 1000);
        assert_ne!(old, new);
        world.publish_reference(&new);
        let before = world.db.reads().read_generation().unwrap();
        let counters = sweep_batch(&world.db, &world.cache, &config(), &collected).unwrap();
        assert_eq!(counters.kept_referenced, 1, "seed {seed}: {counters:?}");
        assert_eq!(counters.kept_fresh, 0);
        assert_eq!(counters.deleted_objects, 0);
        world.assert_hit(&new, &[2.0, 1.0]);
        assert_eq!(world.db.reads().read_generation().unwrap(), before);
    }
}

#[test]
fn nonexistent_namespace_gc_remains_side_effect_free() {
    let world = World::new(131);
    assert!(!world.cache.root().exists());
    let (counters, cursor, exhausted) =
        run_gc_pass(&world.db, &world.cache, &config(), None).unwrap();
    assert_eq!(counters, GcCounters::default());
    assert!(exhausted && cursor.is_none());
    assert!(!world.cache.root().exists());
}
