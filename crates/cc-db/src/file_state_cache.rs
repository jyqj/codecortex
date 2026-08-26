//! Cross-build cache of the scan/diff file-state snapshot.
//!
//! `get_file_state` historically re-loaded every `files` row from SQLite on
//! each incremental build to drive the scan/diff skip decisions — an O(repo)
//! floor paid even for a single-file change (the dominant share of the
//! 50k-file single-file incremental benchmark). This module keeps the loaded
//! map alive on the [`IndexDb`] handle (the only host that survives across
//! builds) and serves subsequent loads from memory, mirroring
//! [`crate::seed_symbol_cache`].
//!
//! # Validity (no manual invalidation hooks)
//!
//! The cache is keyed on the write-time-maintained `files_state` aggregate
//! (see [`crate::signature_agg`]): a persisted `(count, sum-of-row-hashes)`
//! multiset homomorphism over exactly the scan-diff projection
//! `(file_path, content_hash, mtime, size)`. Every production write path
//! that mutates `files` rows keeps the aggregates in sync inside its own
//! transaction (path-scoped deltas for incremental writers, baseline
//! recompute for full rebuilds), so:
//!
//! - equal token ⇒ equal file-state multiset (64-bit collision odds, the
//!   same guarantee class as the other aggregate consumers);
//! - any files mutation — in-process or cross-process — moves the persisted
//!   token, and the next read misses and reloads;
//! - writes that do not touch `files` rows (edge/symbol rewrites,
//!   postprocess, evidence, metadata) leave the token unchanged, so the
//!   cache survives a build's post-batch write traffic.
//!
//! # Maintenance
//!
//! The hot path ([`IndexDb::write_incremental_batch`]) updates the snapshot
//! in place after its transaction commits: remove the removed paths, upsert
//! the written units' `(hash, mtime, size)` — the exact file-scoped delta
//! the transaction applied to `files`. All other writers simply move the
//! token; the next read repopulates.
//!
//! # Sharing & memory
//!
//! The map is handed out as `Arc<HashMap>` — a cache hit is one Arc clone,
//! no per-build copy. The write-path delta uses `Arc::make_mut`, which
//! clones only while an in-flight build still holds the previous snapshot.
//! One entry is ~100 bytes (path + 64-hex hash + metadata): ~10 MB at 50k
//! files — small enough to skip a capacity knob.

use std::collections::HashMap;
use std::sync::Arc;

use crate::index_db::{FileState, FileWriteUnit, IndexDb};
use crate::signature_agg::RowAgg;

/// Cached file-state snapshot plus the `files_state` aggregate it matches.
pub(crate) struct FileStateCache {
    token: RowAgg,
    map: Arc<HashMap<String, FileState>>,
}

impl IndexDb {
    /// Serve the file-state map from the cache when the persisted token
    /// matches. A hit is one `Arc` clone.
    pub(crate) fn file_state_cache_materialize(
        &self,
        token: RowAgg,
    ) -> Option<Arc<HashMap<String, FileState>>> {
        let guard = self.file_state_cache.lock().ok()?;
        let cache = guard.as_ref()?;
        if cache.token != token {
            return None;
        }
        Some(Arc::clone(&cache.map))
    }

    /// Populate the cache from a full direct load whose token was verified
    /// stable across the load (caller re-reads the stored aggregate).
    pub(crate) fn file_state_cache_store(
        &self,
        token: RowAgg,
        map: Arc<HashMap<String, FileState>>,
    ) {
        if let Ok(mut guard) = self.file_state_cache.lock() {
            *guard = Some(FileStateCache { token, map });
        }
    }

    /// Carry the cache across one committed incremental batch. `pre`/`post`
    /// are the `files_state` aggregates read inside the batch transaction
    /// before and after its mutations; `None` (no stored baseline) drops
    /// the cache. Dirty units never rewrite `files` rows, so only removals
    /// and normal (re-parsed) units participate.
    pub(crate) fn file_state_cache_apply_batch(
        &self,
        pre: Option<RowAgg>,
        post: Option<RowAgg>,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
    ) {
        let Ok(mut guard) = self.file_state_cache.lock() else {
            return;
        };
        let (Some(pre), Some(post)) = (pre, post) else {
            *guard = None;
            return;
        };
        let mut cache = match guard.take() {
            // A concurrent reader already refilled against the committed
            // state; keep it.
            Some(cache) if cache.token == post => {
                *guard = Some(cache);
                return;
            }
            Some(cache) if cache.token == pre => cache,
            // Cold start: `count == 0` proves the files table was empty
            // before this batch, so the batch delta alone reconstructs the
            // snapshot.
            None if pre.count == 0 => FileStateCache {
                token: pre,
                map: Arc::new(HashMap::new()),
            },
            // Unknown basis (stale snapshot or cold cache on a non-empty
            // table): stay cold, the next read repopulates.
            _ => return,
        };
        // Clones only while an in-flight build still holds the previous
        // snapshot Arc (bounded: one map copy per batch, far below the SQL
        // reload it replaces).
        let map = Arc::make_mut(&mut cache.map);
        for path in to_remove {
            map.remove(path);
        }
        for unit in normal_units {
            map.insert(
                unit.rel_path.clone(),
                FileState {
                    content_hash: unit.content_hash.clone(),
                    mtime: unit.mtime,
                    size: unit.size,
                },
            );
        }
        cache.token = post;
        *guard = Some(cache);
    }

    /// Cached file count, for tests asserting the cache is engaged.
    #[doc(hidden)]
    pub fn file_state_cache_len(&self) -> Option<usize> {
        self.file_state_cache
            .lock()
            .ok()?
            .as_ref()
            .map(|c| c.map.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_db::PrecompressedChunks;

    fn open_db() -> (tempfile::TempDir, IndexDb) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("fs_cache.db")).unwrap().0;
        (tmp, db)
    }

    fn unit(file: &str, hash: &str, mtime: f64, size: u64) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: file.to_string(),
            language: cc_model::Language::Rust,
            content_hash: hash.to_string(),
            mtime,
            size,
            outcome: cc_model::parse::ParseOutcome::default(),
        }
    }

    fn write_batch(db: &IndexDb, to_remove: &[String], normal: &[FileWriteUnit]) {
        db.write_incremental_batch(
            to_remove,
            normal,
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
        )
        .unwrap();
    }

    /// Cached read must equal a direct SQL load after every batch shape.
    fn assert_cache_equals_direct(db: &IndexDb, label: &str) {
        let cached = db.get_file_state().unwrap();
        let conn = db.read_conn().unwrap();
        let direct = IndexDb::load_file_state_on(&conn).unwrap();
        assert_eq!(
            *cached, direct,
            "{label}: cached file state diverged from a direct load"
        );
    }

    /// The committed-batch delta must keep the cached snapshot identical to
    /// a direct load across add / modify / remove batches, without ever
    /// reloading (the cache stays warm through every step).
    #[test]
    fn batch_delta_matches_direct_load() {
        let (_tmp, db) = open_db();
        // No-op batch initializes the aggregate baseline; the first real
        // batch then cold-starts the cache from its own delta.
        write_batch(&db, &[], &[]);
        write_batch(
            &db,
            &[],
            &[
                unit("a.rs", "hash-a1", 1.0, 100),
                unit("b.rs", "hash-b1", 2.0, 200),
            ],
        );
        assert_eq!(
            db.file_state_cache_len(),
            Some(2),
            "cold-start must warm the cache"
        );
        assert_cache_equals_direct(&db, "after add batch");

        // Modify a.rs (hash/mtime/size all move).
        write_batch(&db, &[], &[unit("a.rs", "hash-a2", 3.5, 150)]);
        assert_eq!(db.file_state_cache_len(), Some(2));
        assert_cache_equals_direct(&db, "after modify batch");
        let state = db.get_file_state().unwrap();
        assert_eq!(state["a.rs"].content_hash, "hash-a2");
        assert_eq!(state["a.rs"].mtime, 3.5);
        assert_eq!(state["a.rs"].size, 150);

        // Remove b.rs.
        write_batch(&db, &["b.rs".to_string()], &[]);
        assert_eq!(
            db.file_state_cache_len(),
            Some(1),
            "removal must drop the file"
        );
        assert_cache_equals_direct(&db, "after remove batch");
    }

    /// A files write outside the incremental batch moves the persisted
    /// token without touching the snapshot — the next read must miss,
    /// return fresh content, and re-warm the cache.
    #[test]
    fn foreign_files_writer_invalidates_then_rewarns() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[]);
        write_batch(&db, &[], &[unit("a.rs", "hash-a1", 1.0, 100)]);
        assert_eq!(db.file_state_cache_len(), Some(1));

        db.replace_files_batch(&[unit("c.rs", "hash-c1", 2.0, 50)])
            .unwrap();
        assert_cache_equals_direct(&db, "after foreign writer");
        assert_eq!(
            db.file_state_cache_len(),
            Some(2),
            "read after the miss must re-warm with the fresh snapshot"
        );
    }

    /// A cache hit must not clone the map: consecutive reads on an
    /// unchanged database return the same allocation.
    #[test]
    fn cache_hit_shares_the_same_allocation() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[]);
        write_batch(&db, &[], &[unit("a.rs", "hash-a1", 1.0, 100)]);
        let first = db.get_file_state().unwrap();
        let second = db.get_file_state().unwrap();
        assert!(
            Arc::ptr_eq(&first, &second),
            "consecutive hits must share one snapshot allocation"
        );
    }
}
