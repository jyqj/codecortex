//! Cross-build cache of the `files` table snapshot (`get_file_state`).
//!
//! Every incremental build's scan/diff phase loads the full file-state map
//! (path → content_hash/mtime/size) to diff the scan against — an O(repo)
//! SQL load per build that dominates the scan phase on large repositories
//! once the tree walk itself is event-scoped. This module keeps the loaded
//! snapshot alive on the [`IndexDb`] handle (the only host that survives
//! across builds) behind an `Arc`, so subsequent builds get the map without
//! touching SQLite.
//!
//! # Validity (no manual invalidation hooks)
//!
//! Same contract as the seed-symbol cache (`crate::seed_symbol_cache`): the
//! cache is keyed on the write-time-maintained `files_state` aggregate (see
//! [`crate::signature_agg`]), a `(count, sum-of-row-hashes)` multiset
//! homomorphism over exactly the four `FileState` columns. Every production
//! write path that mutates `files` rows keeps the aggregates in sync inside
//! its own transaction, so an equal token means an equal file-state multiset
//! and any mutation — in-process or cross-process — moves the token and the
//! next read reloads. Rewrites that leave the projection unchanged (the
//! config linker re-applying identical units, `indexed_at` refreshes) keep
//! the token, so the cache survives them.
//!
//! # Maintenance
//!
//! The hot path ([`IndexDb::write_incremental_batch`]) updates the snapshot
//! in place after its transaction commits: remove the removed paths, upsert
//! the replaced units' `FileState` projections (dirty re-resolve units never
//! touch `files` rows). All other writers simply move the token; the next
//! read repopulates.
//!
//! # Memory
//!
//! One `FileState` (a 64-hex hash string + two numbers) plus the path string
//! per file — roughly 150–250 B/entry, ~10 MB at 50k files. Repositories
//! above `CODECORTEX_FILE_STATE_CACHE_MAX_FILES` (default 1_000_000, `0`
//! disables) skip the cache and keep the per-build direct load.

use std::collections::HashMap;
use std::sync::Arc;

use cc_model::CcResult;

use crate::index_db::{FileState, FileWriteUnit, IndexDb};
use crate::signature_agg::RowAgg;

/// Cached snapshot: the `files_state` aggregate it corresponds to plus the
/// shared map. Readers get an `Arc` clone; the batch delta uses
/// `Arc::make_mut`, so a snapshot still held by an in-flight build is never
/// mutated under it.
pub(crate) struct FileStateCache {
    token: RowAgg,
    map: Arc<HashMap<String, FileState>>,
}

/// Cache capacity in files; `0` disables the cache entirely.
fn file_state_cache_max_files() -> usize {
    std::env::var("CODECORTEX_FILE_STATE_CACHE_MAX_FILES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1_000_000)
}

impl IndexDb {
    /// The file-state snapshot behind an `Arc`: served from the cache when
    /// the persisted `files_state` token matches, otherwise loaded directly
    /// (and cached when the token is provably stable across the load).
    pub(crate) fn get_file_state_snapshot(&self) -> CcResult<Arc<HashMap<String, FileState>>> {
        let conn = self.read_conn()?;
        let stored = crate::signature_agg::load_on(&conn)?.map(|aggs| aggs.files_state);
        if let Some(token) = stored {
            if let Ok(guard) = self.file_state_cache.lock() {
                if let Some(cache) = guard.as_ref() {
                    if cache.token == token {
                        return Ok(Arc::clone(&cache.map));
                    }
                }
            }
        }

        let map = Arc::new(self.get_file_state()?);

        // Cache only when the token did not move during the load (both
        // reads on the same pooled connection, but separate statements — a
        // concurrent writer could commit in between).
        if let Some(token) = stored {
            let after = crate::signature_agg::load_on(&conn)?.map(|aggs| aggs.files_state);
            if after == Some(token) && map.len() <= file_state_cache_max_files() {
                if let Ok(mut guard) = self.file_state_cache.lock() {
                    *guard = Some(FileStateCache {
                        token,
                        map: Arc::clone(&map),
                    });
                }
            }
        }
        Ok(map)
    }

    /// Carry the cache across one committed incremental batch. `pre`/`post`
    /// are the `files_state` aggregates read inside the batch transaction
    /// before and after its mutations; `None` (no stored baseline) drops the
    /// cache. Mirrors `seed_cache_apply_batch`'s basis-proof structure.
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
            // before this batch (the count half of the aggregate is exact).
            None if pre.count == 0 => FileStateCache {
                token: pre,
                map: Arc::new(HashMap::new()),
            },
            // Unknown basis: stay cold, the next read repopulates.
            _ => return,
        };
        // Clone-on-write: an in-flight build may still hold the previous
        // snapshot Arc (the commit stages carry it), in which case make_mut
        // copies — still far cheaper than the SQL reload it replaces.
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
        if cache.map.len() <= file_state_cache_max_files() {
            *guard = Some(cache);
        }
    }

    /// Cached file count, for tests asserting the cache is engaged.
    #[doc(hidden)]
    pub fn file_state_cache_len(&self) -> Option<usize> {
        self.file_state_cache.lock().ok()?.as_ref().map(|c| c.map.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_db::PrecompressedChunks;

    fn open_db() -> (tempfile::TempDir, IndexDb) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("file_state.db")).unwrap().0;
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

    /// Cached snapshot must equal a direct SQL load.
    fn assert_cache_equals_direct(db: &IndexDb, label: &str) {
        let cached = db.get_file_state_snapshot().unwrap();
        let direct = db.get_file_state().unwrap();
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
            &[unit("a.rs", "h-a1", 1.0, 10), unit("b.rs", "h-b1", 2.0, 20)],
        );
        assert_eq!(
            db.file_state_cache_len(),
            Some(2),
            "cold-start must warm the cache"
        );
        assert_cache_equals_direct(&db, "after add batch");

        // Modify a.rs (hash/mtime/size all move).
        write_batch(&db, &[], &[unit("a.rs", "h-a2", 3.5, 11)]);
        assert_eq!(db.file_state_cache_len(), Some(2));
        assert_cache_equals_direct(&db, "after modify batch");

        // Remove b.rs.
        write_batch(&db, &["b.rs".to_string()], &[]);
        assert_eq!(db.file_state_cache_len(), Some(1));
        assert_cache_equals_direct(&db, "after remove batch");
    }

    /// A snapshot Arc held across a batch (an in-flight build) must keep its
    /// pre-batch content while the cache advances (clone-on-write).
    #[test]
    fn held_snapshot_is_immutable_across_batches() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[]);
        write_batch(&db, &[], &[unit("a.rs", "h-a1", 1.0, 10)]);
        let held = db.get_file_state_snapshot().unwrap();
        assert_eq!(held.get("a.rs").map(|s| s.content_hash.as_str()), Some("h-a1"));

        write_batch(&db, &[], &[unit("a.rs", "h-a2", 2.0, 11)]);
        assert_eq!(
            held.get("a.rs").map(|s| s.content_hash.as_str()),
            Some("h-a1"),
            "held snapshot must not observe the later batch"
        );
        let fresh = db.get_file_state_snapshot().unwrap();
        assert_eq!(fresh.get("a.rs").map(|s| s.content_hash.as_str()), Some("h-a2"));
        assert_cache_equals_direct(&db, "after held-arc batch");
    }

    /// A files write outside the incremental batch (here: whole-file
    /// replace) moves the persisted token — the next read must miss, return
    /// fresh content, and re-warm the cache.
    #[test]
    fn foreign_files_writer_invalidates_then_rewarns() {
        let (_tmp, db) = open_db();
        write_batch(&db, &[], &[]);
        write_batch(&db, &[], &[unit("a.rs", "h-a1", 1.0, 10)]);
        db.get_file_state_snapshot().unwrap();
        assert_eq!(db.file_state_cache_len(), Some(1));

        db.replace_files_batch(&[unit("c.rs", "h-c1", 4.0, 40)]).unwrap();
        assert_cache_equals_direct(&db, "after foreign writer");
        assert_eq!(
            db.file_state_cache_len(),
            Some(2),
            "read after the miss must re-warm with the fresh snapshot"
        );
    }
}
