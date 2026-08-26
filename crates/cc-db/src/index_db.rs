//! IndexDatabase — the index.sqlite3 connection manager.
//!
//! Read: pool of connections (one per query, no manual refresh needed).
//! Write: single Mutex<Connection> for exclusive writes.
//! FTS sync: application-layer, in the same transaction as base table writes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{types::Type, Connection};

use cc_model::config::ProjectStats;
use cc_model::{CcError, CcResult, ParserTier};

use crate::index_migrate::{migrate_index_db, SchemaStatus};
use crate::sql_util::db_err;

/// Read a chunk text column using the explicit `chunks.text_encoding` marker.
///
/// Supported values:
/// - `plain`: `text` is stored as normal UTF-8 TEXT.
/// - `zstd`: `text` is stored as a zstd-compressed BLOB.
/// - `legacy_auto` / unknown / missing: auto-detect for migrated pre-v16 rows.
pub fn read_chunk_text_with_encoding(
    row: &rusqlite::Row,
    text_col_idx: usize,
    encoding_col_idx: usize,
) -> rusqlite::Result<String> {
    let encoding = row
        .get::<_, Option<String>>(encoding_col_idx)
        .ok()
        .flatten()
        .unwrap_or_else(|| "legacy_auto".to_string());
    match encoding.as_str() {
        "plain" => read_plain_chunk_text(row, text_col_idx),
        "zstd" => read_zstd_chunk_text(row, text_col_idx),
        _ => read_chunk_text_auto(row, text_col_idx),
    }
}

fn read_plain_chunk_text(row: &rusqlite::Row, col_idx: usize) -> rusqlite::Result<String> {
    row.get::<_, String>(col_idx).or_else(|text_err| {
        // Some old SQLite files may still report a BLOB storage class even when
        // the marker says plain. Fall back to raw UTF-8 before surfacing an
        // error so migrated databases remain readable.
        let blob: Vec<u8> = row.get(col_idx)?;
        String::from_utf8(blob).map_err(|utf8_err| {
            rusqlite::Error::FromSqlConversionFailure(
                col_idx,
                Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "plain chunk text is not valid UTF-8: {utf8_err}; sqlite error: {text_err}"
                    ),
                )),
            )
        })
    })
}

fn read_zstd_chunk_text(row: &rusqlite::Row, col_idx: usize) -> rusqlite::Result<String> {
    let blob: Vec<u8> = row.get(col_idx)?;
    let decoded = zstd::decode_all(blob.as_slice()).map_err(|zstd_err| {
        rusqlite::Error::FromSqlConversionFailure(
            col_idx,
            Type::Blob,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("zstd chunk text decompression failed: {zstd_err}"),
            )),
        )
    })?;
    String::from_utf8(decoded).map_err(|utf8_err| {
        rusqlite::Error::FromSqlConversionFailure(
            col_idx,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("zstd chunk text is not valid UTF-8: {utf8_err}"),
            )),
        )
    })
}

fn read_chunk_text_auto(row: &rusqlite::Row, col_idx: usize) -> rusqlite::Result<String> {
    match row.get::<_, String>(col_idx) {
        Ok(s) => Ok(s),
        Err(_) => {
            let blob: Vec<u8> = row.get(col_idx)?;
            match zstd::decode_all(blob.as_slice()) {
                Ok(decompressed) => match String::from_utf8(decompressed) {
                    Ok(s) => Ok(s),
                    Err(e) => {
                        tracing::warn!(
                            col_idx,
                            error = %e,
                            "read_chunk_text: UTF-8 conversion failed after zstd decompression"
                        );
                        Err(rusqlite::Error::FromSqlConversionFailure(
                            col_idx,
                            Type::Blob,
                            Box::new(e),
                        ))
                    }
                },
                Err(zstd_err) => {
                    tracing::warn!(
                        col_idx,
                        error = %zstd_err,
                        "read_chunk_text: zstd decompression failed, attempting raw UTF-8"
                    );
                    match String::from_utf8(blob) {
                        Ok(s) => Ok(s),
                        Err(e) => {
                            tracing::warn!(
                                col_idx,
                                error = %e,
                                "read_chunk_text: raw UTF-8 conversion also failed"
                            );
                            Err(rusqlite::Error::FromSqlConversionFailure(
                                col_idx,
                                Type::Blob,
                                Box::new(e),
                            ))
                        }
                    }
                }
            }
        }
    }
}

pub use crate::index_db_types::*;

/// The index database handle.
pub struct IndexDb {
    pub(crate) db_path: PathBuf,
    pub(crate) pool: RwLock<Pool<SqliteConnectionManager>>,
    pub(crate) write_conn: Mutex<Connection>,
    pub(crate) read_pool_size: u32,
    /// Process-unique handle identity assigned at open from a monotonic
    /// counter. Unlike `Arc::as_ptr`, it is never reused after a handle is
    /// dropped, so caches keyed on it cannot alias across project instances.
    instance_id: u64,
    /// Cross-build resolver seed snapshot, validated against the persisted
    /// `symbols_seed` aggregate (see `crate::seed_symbol_cache`).
    pub(crate) seed_cache: Mutex<Option<crate::seed_symbol_cache::SeedSymbolCache>>,
    /// Cross-build resolver *catalog* slot: cc-index parks its built
    /// `SymbolCatalog` here between builds (same host rationale as
    /// `seed_cache` — the handle is the only object that survives across
    /// builds). Type-erased because cc-db must not depend on cc-index; the
    /// owner validates content against the persisted `symbols_seed`
    /// aggregate exactly like the seed cache does.
    pub(crate) resolver_catalog_slot: Mutex<Option<Box<dyn std::any::Any + Send>>>,
    /// Cross-build file-state snapshot for scan/diff, validated against the
    /// persisted `files_state` aggregate (see `crate::file_state_cache`).
    pub(crate) file_state_cache: Mutex<Option<crate::file_state_cache::FileStateCache>>,
}

/// Process-wide monotonic source for [`IndexDb::instance_id`].
static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Read-only facet over [`IndexDb`]: queries, graph/retrieval reads, stats,
/// and metadata reads. Obtained via [`IndexDb::reads()`]; zero-cost borrow.
///
/// Same facet pattern as the server-side `CodeIndex` (`.search()` /
/// `.graph()` / `.impact()`): the public surface of `IndexDb` is split by
/// capability so callers state their intent at the call site and write
/// access is impossible to reach from a read facet at compile time.
#[derive(Clone, Copy)]
pub struct ReadOps<'a>(pub(crate) &'a IndexDb);

/// Write facet over [`IndexDb`]: every method that mutates index content or
/// runtime evidence (and therefore bumps `index_epoch` / `evidence_epoch` —
/// see [`crate::epoch_rules`]). Obtained via [`IndexDb::writes()`]; this is
/// the only public path to the write methods.
#[derive(Clone, Copy)]
pub struct WriteOps<'a>(pub(crate) &'a IndexDb);

/// Maintenance facet over [`IndexDb`]: full-rebuild protocols, WAL
/// checkpointing, and handle identity. Obtained via [`IndexDb::admin()`].
#[derive(Clone, Copy)]
pub struct MaintenanceOps<'a>(pub(crate) &'a IndexDb);

impl IndexDb {
    /// Read-only view: queries, graph/retrieval reads, stats, metadata reads.
    pub fn reads(&self) -> ReadOps<'_> {
        ReadOps(self)
    }

    /// Write view: batch writes, edge/evidence mutations, unit-of-work.
    pub fn writes(&self) -> WriteOps<'_> {
        WriteOps(self)
    }

    /// Maintenance view: rebuild protocols, WAL checkpoints, handle identity.
    pub fn admin(&self) -> MaintenanceOps<'_> {
        MaintenanceOps(self)
    }
}

impl IndexDb {
    /// Open (or create) the index database at the given path using the default
    /// read pool size.
    /// If the schema version doesn't match, the database is reset in place and rebuilt.
    ///
    /// Returns the database handle together with the [`SchemaStatus`] so callers
    /// can tell whether the database was freshly initialized, migrated, or
    /// already up-to-date. This is used by the auto-index logic to decide
    /// whether a first-connect build is needed.
    pub fn open(path: &Path) -> CcResult<(Self, SchemaStatus)> {
        Self::open_with_read_pool_size(path, 4)
    }

    /// Open (or create) the index database with an explicit SQLite reader pool size.
    pub fn open_with_read_pool_size(
        path: &Path,
        read_pool_size: u32,
    ) -> CcResult<(Self, SchemaStatus)> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let (write_conn, schema_status) = Self::open_and_ensure_schema(path)?;

        let read_pool_size = read_pool_size.clamp(1, 64);
        let pool = Self::build_read_pool(path, read_pool_size)?;
        tracing::debug!(read_pool_size, "index db read pool initialized");

        Ok((
            Self {
                db_path: path.to_path_buf(),
                pool: RwLock::new(pool),
                write_conn: Mutex::new(write_conn),
                read_pool_size,
                instance_id: NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed),
                seed_cache: Mutex::new(None),
                resolver_catalog_slot: Mutex::new(None),
                file_state_cache: Mutex::new(None),
            },
            schema_status,
        ))
    }

    /// Process-unique, never-reused identity of this database handle.
    pub(crate) fn instance_id(&self) -> u64 {
        self.instance_id
    }

    pub(crate) fn build_read_pool(
        path: &Path,
        read_pool_size: u32,
    ) -> CcResult<Pool<SqliteConnectionManager>> {
        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            // `query_only=ON` turns the read pool's type-level write isolation
            // into a hard guarantee: any INSERT/UPDATE/DELETE/CREATE through a
            // pooled connection fails with SQLITE_READONLY instead of silently
            // bypassing the WriteOps epoch bump (which would let epoch-keyed
            // caches serve stale data). It must stay last in the batch so the
            // WAL/journal pragmas still apply on a fresh file.
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA query_only=ON;",
            )?;
            register_regexp_function(conn)?;
            // The hot read paths keep ~25+ distinct constant statements alive
            // via prepare_cached; rusqlite's default capacity of 16 would make
            // them evict each other under LRU.
            conn.set_prepared_statement_cache_capacity(64);
            Ok(())
        });
        Pool::builder()
            .max_size(read_pool_size)
            .min_idle(Some(read_pool_size.min(2)))
            .idle_timeout(Some(Duration::from_secs(300)))
            .build(manager)
            .map_err(db_err)
    }

    /// Open the database, check schema version, and rebuild if mismatched.
    pub(crate) fn open_and_ensure_schema(path: &Path) -> CcResult<(Connection, SchemaStatus)> {
        let (conn, status) = Self::open_and_ensure_schema_inner(path)?;
        // The batch write path cycles through ~20+ distinct prepare_cached
        // statements per file (inserts across 12 tables plus FTS mirrors and
        // batched deletes); rusqlite's default capacity of 16 makes that
        // rotation evict every statement right before its reuse, degrading
        // every "cached" execute into a full re-prepare. Same fix as the
        // read pool (see `build_read_pool`).
        conn.set_prepared_statement_cache_capacity(64);
        Ok((conn, status))
    }

    fn open_and_ensure_schema_inner(path: &Path) -> CcResult<(Connection, SchemaStatus)> {
        // This is the single incremental write connection (the read pool is
        // built separately in `build_read_pool`, which keeps the lean default).
        // Every INSERT/DELETE in a batch navigates the secondary-index B-trees;
        // on a multi-hundred-MB DB the default 2 MB page cache misses on nearly
        // every index page, so give this one connection a 64 MB cache plus a
        // 512 MB mmap window to cut the random-page-read amplification. Bounded
        // to one connection, so total memory stays predictable. A full rebuild
        // re-opens through this same path (the bulk pragmas apply only to the
        // throwaway temp connection), so the cache is never silently dropped.
        let pragmas = "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; \
                       PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; \
                       PRAGMA cache_size=-65536; PRAGMA mmap_size=536870912;";

        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch(pragmas).map_err(db_err)?;

        match migrate_index_db(&conn)? {
            status @ (SchemaStatus::UpToDate | SchemaStatus::Initialized) => Ok((conn, status)),
            SchemaStatus::Mismatch { stored } => {
                tracing::warn!(
                    stored_version = stored,
                    "resetting index database for schema rebuild"
                );
                // Export persistent assets before destroying the database.
                let preserved = Self::export_persistent_assets(&conn);
                // Snapshot the old epoch vector so the rebuilt database never
                // rolls back below a generation consumers may have cached.
                let prev_generation = Self::read_generation_on(&conn).ok();

                // Reset the schema in place instead of deleting the file.
                // Unlinking and recreating the path splits SQLite's per-inode
                // lock coordination with any connection that still has the old
                // file open: when such a connection closes later, it unlinks
                // the *new* database's `-wal`/`-shm` by name, after which every
                // fresh connection fails with SQLITE_IOERR while the write
                // connection lives. r2d2 pools close their connections
                // asynchronously after drop (an in-flight background
                // `add_connection` task keeps the shared pool alive), so a
                // just-dropped `IndexDb` on the same path is exactly such a
                // lingering connection. An in-place reset keeps the inode, so
                // SQLite's own locking stays coherent with any straggler.
                let conn = match Self::reset_schema_in_place(&conn) {
                    Ok(()) => conn,
                    Err(reset_err) => {
                        // Unreadable/corrupt database: fall back to deleting
                        // the file. The lingering-connection race is accepted
                        // here — the alternative is failing the open outright.
                        tracing::warn!(
                            err = %reset_err,
                            "in-place schema reset failed; deleting index database file"
                        );
                        drop(conn);
                        let _ = std::fs::remove_file(path);
                        let mut wal = path.as_os_str().to_owned();
                        wal.push("-wal");
                        let mut shm = path.as_os_str().to_owned();
                        shm.push("-shm");
                        let _ = std::fs::remove_file(&wal);
                        let _ = std::fs::remove_file(&shm);
                        let new_conn = Connection::open(path).map_err(db_err)?;
                        new_conn.execute_batch(pragmas).map_err(db_err)?;
                        new_conn
                    }
                };
                migrate_index_db(&conn)?;

                // Re-import preserved assets into the fresh database.
                if let Ok(assets) = preserved {
                    Self::import_persistent_assets(&conn, &assets)?;
                }
                // Epoch floor after the destructive rebuild (which also just
                // re-imported runtime evidence without bumping): old + 1 keeps
                // the "rebuilt generation exceeds every observed value"
                // invariant. When the old metadata was unreadable, seed from
                // wall-clock seconds — far above any realistic write-counter
                // value, so no previously cached small integer can collide.
                let next = match prev_generation {
                    Some(prev) => IndexGeneration {
                        index_epoch: prev.index_epoch.saturating_add(1),
                        evidence_epoch: prev.evidence_epoch.saturating_add(1),
                    },
                    None => {
                        let seed = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(1)
                            .max(1);
                        IndexGeneration {
                            index_epoch: seed,
                            evidence_epoch: seed,
                        }
                    }
                };
                Self::write_generation_on(&conn, next)?;
                // After mismatch rebuild the DB is empty — report as Initialized
                // so callers know an index build is needed.
                Ok((conn, SchemaStatus::Initialized))
            }
        }
    }

    /// Destructively drop every SQL object in the open database without
    /// touching the file itself, leaving an empty database (`user_version` 0)
    /// ready for [`migrate_index_db`] to re-apply the full schema.
    ///
    /// The `writable_schema` route removes tables (including FTS5 virtual
    /// tables and their shadow tables), indexes, triggers, and views in one
    /// pass without dependency-ordering concerns; `VACUUM` then reclaims the
    /// orphaned pages and rebuilds the file compactly.
    fn reset_schema_in_place(conn: &Connection) -> CcResult<()> {
        conn.execute_batch(
            "PRAGMA writable_schema = 1;
             DELETE FROM sqlite_master;
             PRAGMA writable_schema = RESET;
             PRAGMA user_version = 0;",
        )
        .map_err(|e| CcError::Database(format!("reset schema in place: {}", e)))?;
        conn.execute_batch("VACUUM;")
            .map_err(|e| CcError::Database(format!("vacuum after schema reset: {}", e)))?;
        Ok(())
    }

    /// Export ADR and runtime_evidence rows as JSON before a schema rebuild.
    fn export_persistent_assets(conn: &Connection) -> CcResult<serde_json::Value> {
        let mut adrs = Vec::new();
        let has_adr = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='adr'")
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if has_adr {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT adr_id, title, status, context, decision, created_at, updated_at FROM adr",
            ) {
                let rows = stmt.query_map([], |row| {
                    Ok(serde_json::json!({
                        "adr_id": row.get::<_, String>(0)?,
                        "title": row.get::<_, String>(1)?,
                        "status": row.get::<_, String>(2)?,
                        "context": row.get::<_, String>(3)?,
                        "decision": row.get::<_, String>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                        "updated_at": row.get::<_, String>(6)?,
                    }))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        adrs.push(row);
                    }
                }
            }
        }

        let mut evidence = Vec::new();
        let has_evidence = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='runtime_evidence'",
            )
            .and_then(|mut s| s.exists([]))
            .unwrap_or(false);
        if has_evidence {
            if let Ok(mut stmt) = conn.prepare(
                "SELECT evidence_id, service_name, method, path, status_code, observed_count, first_seen, last_seen FROM runtime_evidence",
            ) {
                let rows = stmt.query_map([], |row| {
                    Ok(serde_json::json!({
                        "evidence_id": row.get::<_, String>(0)?,
                        "service_name": row.get::<_, String>(1)?,
                        "method": row.get::<_, Option<String>>(2)?,
                        "path": row.get::<_, String>(3)?,
                        "status_code": row.get::<_, Option<String>>(4)?,
                        "observed_count": row.get::<_, u32>(5)?,
                        "first_seen": row.get::<_, String>(6)?,
                        "last_seen": row.get::<_, String>(7)?,
                    }))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        evidence.push(row);
                    }
                }
            }
        }

        let count = adrs.len() + evidence.len();
        if count > 0 {
            tracing::info!(
                adrs = adrs.len(),
                evidence = evidence.len(),
                "exported persistent assets before schema rebuild"
            );
        }
        Ok(serde_json::json!({ "adrs": adrs, "runtime_evidence": evidence }))
    }

    /// Re-import ADR and runtime_evidence rows after a schema rebuild.
    fn import_persistent_assets(conn: &Connection, assets: &serde_json::Value) -> CcResult<()> {
        let mut adr_failed: usize = 0;
        let mut adr_total: usize = 0;
        if let Some(adrs) = assets.get("adrs").and_then(|v| v.as_array()) {
            adr_total = adrs.len();
            for adr in adrs {
                if let Err(err) = conn.execute(
                    "INSERT OR REPLACE INTO adr(adr_id, title, status, context, decision, created_at, updated_at)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        adr["adr_id"].as_str().unwrap_or_default(),
                        adr["title"].as_str().unwrap_or_default(),
                        adr["status"].as_str().unwrap_or_default(),
                        adr["context"].as_str().unwrap_or_default(),
                        adr["decision"].as_str().unwrap_or_default(),
                        adr["created_at"].as_str().unwrap_or_default(),
                        adr["updated_at"].as_str().unwrap_or_default(),
                    ],
                ) {
                    adr_failed += 1;
                    tracing::warn!(
                        adr_id = adr["adr_id"].as_str().unwrap_or("?"),
                        error = %err,
                        "failed to re-import ADR row"
                    );
                }
            }
        }

        let mut ev_failed: usize = 0;
        let mut ev_total: usize = 0;
        if let Some(evidence) = assets.get("runtime_evidence").and_then(|v| v.as_array()) {
            ev_total = evidence.len();
            for ev in evidence {
                if let Err(err) = conn.execute(
                    "INSERT OR REPLACE INTO runtime_evidence(evidence_id, service_name, method, path, status_code, observed_count, first_seen, last_seen)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        ev["evidence_id"].as_str().unwrap_or_default(),
                        ev["service_name"].as_str().unwrap_or_default(),
                        ev["method"].as_str(),
                        ev["path"].as_str().unwrap_or_default(),
                        ev["status_code"].as_str(),
                        ev["observed_count"].as_i64().unwrap_or(1),
                        ev["first_seen"].as_str().unwrap_or_default(),
                        ev["last_seen"].as_str().unwrap_or_default(),
                    ],
                ) {
                    ev_failed += 1;
                    tracing::warn!(
                        evidence_id = ev["evidence_id"].as_str().unwrap_or("?"),
                        error = %err,
                        "failed to re-import runtime_evidence row"
                    );
                }
            }
        }

        let total_failed = adr_failed + ev_failed;
        if total_failed > 0 {
            tracing::warn!(
                adr_failed,
                adr_total,
                ev_failed,
                ev_total,
                "partial failure during persistent asset re-import"
            );
            return Err(CcError::Database(format!(
                "persistent asset re-import failed for {total_failed}/{} rows",
                adr_total + ev_total
            )));
        }
        if adr_total + ev_total > 0 {
            tracing::info!(
                adrs_ok = adr_total - adr_failed,
                adrs_total = adr_total,
                evidence_ok = ev_total - ev_failed,
                evidence_total = ev_total,
                "re-imported persistent assets after schema rebuild"
            );
        }
        Ok(())
    }

    /// Read the persisted epoch vector. Missing keys (old databases) read as 0.
    ///
    /// One metadata SELECT covering both keys — cheap enough to call on every
    /// search/graph read, which is how consumers observe invalidation.
    pub(crate) fn generation(&self) -> CcResult<IndexGeneration> {
        let conn = self.read_conn()?;
        Self::read_generation_on(&conn)
    }

    /// Read the epoch vector from an arbitrary connection (e.g. a soon-to-be
    /// destroyed mismatched-schema database).
    fn read_generation_on(conn: &Connection) -> CcResult<IndexGeneration> {
        let mut stmt = conn
            .prepare_cached("SELECT key, value FROM metadata WHERE key IN (?1, ?2)")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![INDEX_EPOCH_KEY, EVIDENCE_EPOCH_KEY],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(db_err)?;
        let mut generation = IndexGeneration::default();
        for row in rows {
            let (key, value) = row.map_err(db_err)?;
            let parsed = value.parse::<u64>().unwrap_or(0);
            match key.as_str() {
                INDEX_EPOCH_KEY => generation.index_epoch = parsed,
                EVIDENCE_EPOCH_KEY => generation.evidence_epoch = parsed,
                _ => {}
            }
        }
        Ok(generation)
    }

    /// Increment the index-content epoch on the given connection/transaction.
    ///
    /// Every cc-db method that commits index content MUST call this inside the
    /// same transaction as the data write, so callers can never forget to
    /// invalidate downstream caches.
    pub(crate) fn bump_index_epoch_on(conn: &Connection) -> CcResult<()> {
        Self::bump_epoch_on(conn, INDEX_EPOCH_KEY)
    }

    /// Increment the runtime-evidence epoch on the given connection/transaction.
    pub(crate) fn bump_evidence_epoch_on(conn: &Connection) -> CcResult<()> {
        Self::bump_epoch_on(conn, EVIDENCE_EPOCH_KEY)
    }

    /// Persist the post-rebuild epoch vector into the finished temp database.
    ///
    /// Must be called while the write lock is held, immediately before the
    /// atomic swap: the live generation is re-read at that point so writers
    /// that advanced the epochs during the rebuild (including other
    /// processes) can never produce a "same generation, different content"
    /// collision with the floor snapshot taken when the rebuild started.
    /// A full rebuild can change arbitrary index content and drops runtime
    /// evidence rows, so both epochs advance past `max(floor, live)`.
    pub(crate) fn finalize_rebuild_generation(
        &self,
        tmp_path: &Path,
        floor: IndexGeneration,
    ) -> CcResult<()> {
        let live = self.generation().unwrap_or_default();
        let next = IndexGeneration {
            index_epoch: floor.index_epoch.max(live.index_epoch).saturating_add(1),
            evidence_epoch: floor
                .evidence_epoch
                .max(live.evidence_epoch)
                .saturating_add(1),
        };
        let tmp_conn = Connection::open(tmp_path)
            .map_err(|e| CcError::Database(format!("open temp db for generation: {}", e)))?;
        Self::write_generation_on(&tmp_conn, next)?;
        // Fold the write back into the main file before the rename; the temp
        // db may be in WAL mode and only the main file is swapped in.
        let _ = tmp_conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        drop(tmp_conn);
        let mut tmp_wal = tmp_path.as_os_str().to_owned();
        tmp_wal.push("-wal");
        let mut tmp_shm = tmp_path.as_os_str().to_owned();
        tmp_shm.push("-shm");
        let _ = std::fs::remove_file(&tmp_wal);
        let _ = std::fs::remove_file(&tmp_shm);
        Ok(())
    }

    /// Persist an explicit epoch vector on the given connection/transaction.
    fn write_generation_on(conn: &Connection, generation: IndexGeneration) -> CcResult<()> {
        Self::set_metadata_on(conn, INDEX_EPOCH_KEY, &generation.index_epoch.to_string())?;
        Self::set_metadata_on(
            conn,
            EVIDENCE_EPOCH_KEY,
            &generation.evidence_epoch.to_string(),
        )
    }

    fn bump_epoch_on(conn: &Connection, key: &str) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT INTO metadata(key,value) VALUES(?1,'1') \
             ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
            rusqlite::params![key],
        )?;
        Ok(())
    }

    /// Begin a typed multi-statement write transaction.
    ///
    /// The returned [`crate::unit_of_work::UnitOfWork`] holds the write lock
    /// until it is committed or dropped (drop rolls back). See the module
    /// docs of [`crate::unit_of_work`] for the locking and epoch contract.
    pub(crate) fn begin_unit_of_work(&self) -> CcResult<crate::unit_of_work::UnitOfWork<'_>> {
        crate::unit_of_work::UnitOfWork::begin(self)
    }

    /// Get a read connection from the pool.
    pub(crate) fn read_conn(&self) -> CcResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        let pool = self
            .pool
            .read()
            .map_err(|e| CcError::Database(format!("read pool lock: {}", e)))?;
        pool.get()
            .map_err(|e| CcError::Database(format!("pool get: {}", e)))
    }

    /// Force a WAL checkpoint, truncating the WAL file.
    ///
    /// Call after large writes (e.g. full rebuild) to reclaim WAL disk space
    /// and ensure all data is flushed to the main database file.
    pub(crate) fn checkpoint_wal(&self) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| CcError::Database(format!("wal_checkpoint: {}", e)))?;
        Ok(())
    }

    /// Truncate the WAL only once it has grown past `max_bytes`.
    ///
    /// Incremental builds never swap the database file, so the WAL otherwise
    /// shrinks only via SQLite's page-count autocheckpoint and stays large in
    /// long watch sessions, amplifying reads. Returns whether a checkpoint ran.
    pub(crate) fn checkpoint_wal_if_large(&self, max_bytes: u64) -> CcResult<bool> {
        let mut wal_path = self.db_path.as_os_str().to_owned();
        wal_path.push("-wal");
        let wal_size = match std::fs::metadata(&wal_path) {
            Ok(meta) => meta.len(),
            Err(_) => return Ok(false),
        };
        if wal_size <= max_bytes {
            return Ok(false);
        }
        self.checkpoint_wal()?;
        tracing::debug!(wal_size, max_bytes, "truncated oversized WAL");
        Ok(true)
    }

    // ── File state ───────────────────────────────────────────────

    /// The scan/diff file-state map, served from the cross-build snapshot
    /// cache when the persisted `files_state` aggregate matches (see
    /// `crate::file_state_cache`); falls back to the full-table load and
    /// re-warms the cache when the token was stable across the load.
    pub(crate) fn get_file_state(&self) -> CcResult<Arc<HashMap<String, FileState>>> {
        let conn = self.read_conn()?;
        let pre_token = crate::signature_agg::load_on(&conn)?.map(|aggs| aggs.files_state);
        if let Some(token) = pre_token {
            if let Some(cached) = self.file_state_cache_materialize(token) {
                return Ok(cached);
            }
        }
        let map = Arc::new(Self::load_file_state_on(&conn)?);
        // Only cache a load whose token was provably stable across it (a
        // concurrent writer between the token read and the SELECT would
        // otherwise pin a torn snapshot under a valid token).
        let post_token = crate::signature_agg::load_on(&conn)?.map(|aggs| aggs.files_state);
        if let (Some(pre), Some(post)) = (pre_token, post_token) {
            if pre == post {
                self.file_state_cache_store(post, Arc::clone(&map));
            }
        }
        Ok(map)
    }

    /// Direct SQL load of the file-state map (the historical O(repo) path).
    pub(crate) fn load_file_state_on(conn: &Connection) -> CcResult<HashMap<String, FileState>> {
        let mut stmt = conn
            .prepare("SELECT file_path, content_hash, mtime, size FROM files")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FileState {
                        content_hash: row.get::<_, String>(1)?,
                        mtime: row.get::<_, f64>(2)?,
                        size: row.get::<_, i64>(3)?.max(0) as u64,
                    },
                ))
            })
            .map_err(db_err)?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, state) = row.map_err(db_err)?;
            map.insert(path, state);
        }
        Ok(map)
    }

    // ── Metadata ─────────────────────────────────────────────────

    pub(crate) fn get_metadata(&self, key: &str) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        Ok(conn
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                rusqlite::params![key],
                |r| r.get::<_, String>(0),
            )
            .ok())
    }

    pub(crate) fn set_metadata(&self, key: &str, value: &str) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key, value])
            .map_err(db_err)?;
        Ok(())
    }

    // ── Stats ────────────────────────────────────────────────────

    pub(crate) fn stats(&self, project_path: &Path) -> CcResult<ProjectStats> {
        let conn = self.read_conn()?;
        let count = |table: &str| -> usize {
            conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0) as usize
        };
        Ok(ProjectStats {
            project_path: project_path.display().to_string(),
            indexed_files: count("files"),
            indexed_chunks: count("chunks"),
            indexed_symbols: count("symbols"),
            indexed_symbol_refs: count("symbol_refs"),
            indexed_call_edges: count("call_edges"),
            indexed_test_edges: count("test_edges"),
            indexed_route_edges: count("routes"),
            indexed_literals: count("literal_index"),
            indexed_diagnostics: 0,
            last_indexed_at: self.get_metadata("last_indexed_at")?,
            index_version: self.get_metadata("index_version")?,
        })
    }

    // Methods split into separate files:
    // - index_db_graph.rs: graph traversal, community, framework
    // - index_db_query.rs: symbol/file queries, search, JSON
    // - index_db_frontier.rs: route, HTTP, diagnostic queries
    // - index_db_edges.rs: edge batch operations, dispatch sites, infra
    // - index_db_arch.rs: architecture analysis, ADR
}

/// Parse a parser_tier string back into the `ParserTier` enum.
pub(crate) fn parse_parser_tier(s: &str) -> ParserTier {
    match s {
        "generic" => ParserTier::Generic,
        "heuristic" => ParserTier::Heuristic,
        "tree_sitter" => ParserTier::TreeSitter,
        "semantic" => ParserTier::Semantic,
        "verified" => ParserTier::Verified,
        _ => ParserTier::Generic,
    }
}

// Read-only facet delegates (see `IndexDb::reads()`).
impl ReadOps<'_> {
    /// Read the persisted epoch vector. Missing keys (old databases) read as 0.
    pub fn generation(&self) -> CcResult<IndexGeneration> {
        self.0.generation()
    }

    /// The on-disk schema version (`PRAGMA user_version`) of this database.
    pub fn schema_version(&self) -> CcResult<u32> {
        let conn = self.0.read_conn()?;
        conn.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .map_err(|e| CcError::Database(e.to_string()))
    }

    // NOTE: there is deliberately no `read_conn()` delegate here. Handing a
    // raw pooled connection upward would let upper crates write ad-hoc SQL
    // against the schema, eroding the typed read-model seam (and the
    // aggregate/signature maintenance that depends on it — see STORAGE.md).
    // Upper-crate reads go through the typed facets (`ReadOps` methods,
    // `RetrievalReadModel`, `GraphReads`, `FrameworkScanSession`); tests
    // open their own seed connections.

    /// The scan/diff file-state map (`{file_path: hash/mtime/size}`),
    /// served from the cross-build snapshot cache when valid — a hit is one
    /// `Arc` clone instead of an O(repo) table load.
    pub fn get_file_state(&self) -> CcResult<Arc<HashMap<String, FileState>>> {
        self.0.get_file_state()
    }

    pub fn get_metadata(&self, key: &str) -> CcResult<Option<String>> {
        self.0.get_metadata(key)
    }

    pub fn stats(&self, project_path: &Path) -> CcResult<ProjectStats> {
        self.0.stats(project_path)
    }

    /// Graph-signature aggregates maintained by the write paths, or `None`
    /// when no baseline exists yet (see `signature_agg` module docs).
    pub fn stored_graph_signature_aggregates(
        &self,
    ) -> CcResult<Option<crate::GraphSignatureAggregates>> {
        let conn = self.0.read_conn()?;
        crate::signature_agg::load_on(&conn)
    }

    /// Recompute the graph-signature aggregates from the committed table
    /// contents (ground truth, O(repo)). Fallback for databases without a
    /// stored baseline; value-identical to the maintained aggregates.
    pub fn scan_graph_signature_aggregates(&self) -> CcResult<crate::GraphSignatureAggregates> {
        let conn = self.0.read_conn()?;
        crate::signature_agg::scan_on(&conn)
    }

    /// `call_synthetic` partial aggregate for the given `synthesized_by`
    /// kinds — the committed rows a staged synthesis action would delete.
    pub fn synthetic_call_kind_aggregate(&self, kinds: &[&str]) -> CcResult<crate::RowAgg> {
        let conn = self.0.read_conn()?;
        crate::signature_agg::synthetic_kind_agg_on(&conn, kinds)
    }
}

// Write facet delegates (see `IndexDb::writes()`).
impl<'a> WriteOps<'a> {
    /// Begin a typed multi-statement write transaction.
    pub fn begin_unit_of_work(&self) -> CcResult<crate::unit_of_work::UnitOfWork<'a>> {
        self.0.begin_unit_of_work()
    }

    pub fn replace_files_batch(&self, files: &[FileWriteUnit]) -> CcResult<()> {
        self.0.replace_files_batch(files)
    }

    /// Apply one round of config-linker output: row-preserving ref replacement
    /// for files that already have a parsed `files` row, whole-unit writes for
    /// the rest, and stale-ref cleanup for seen config files that resolved to
    /// zero links this round.
    pub fn apply_config_link_units(
        &self,
        units: &[FileWriteUnit],
        seen_config_files: &[String],
    ) -> CcResult<()> {
        self.0.apply_config_link_units(units, seen_config_files)
    }

    /// Update only the edge/resolution data for dirty (DirtyResolveOnly) files.
    pub fn replace_reresolved_edges_only(&self, units: &[FileWriteUnit]) -> CcResult<()> {
        self.0.replace_reresolved_edges_only(units)
    }

    /// Write one incremental index batch atomically: file removals, full file
    /// replacements, dirty re-resolution, route nodes and the batch files'
    /// hierarchy edges in one transaction.
    /// `precompressed` carries chunk payloads compressed during prepare (off
    /// the write lock); units without an entry compress inside the
    /// transaction with the same policy.
    /// Returns the in-transaction `symbols_seed` token span (see
    /// [`SeedTokenSpan`]).
    pub fn write_incremental_batch(
        &self,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
        dirty_units: &[FileWriteUnit],
        route_nodes: &[cc_model::edge::RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        precompressed: &PrecompressedChunks,
    ) -> CcResult<SeedTokenSpan> {
        self.0.write_incremental_batch(
            to_remove,
            normal_units,
            dirty_units,
            route_nodes,
            hierarchy_edges,
            precompressed,
        )
    }

    pub fn remove_files_batch(&self, paths: &[String]) -> CcResult<usize> {
        self.0.remove_files_batch(paths)
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> CcResult<()> {
        self.0.set_metadata(key, value)
    }
}

// Maintenance facet delegates (see `IndexDb::admin()`).
impl MaintenanceOps<'_> {
    /// Process-unique, never-reused identity of this database handle.
    pub fn instance_id(&self) -> u64 {
        self.0.instance_id()
    }

    /// Filesystem path of the SQLite database file (maintenance/diagnostics,
    /// e.g. sidecar inspection or opening a dedicated side connection).
    pub fn db_path(&self) -> &Path {
        &self.0.db_path
    }

    /// Force a WAL checkpoint, truncating the WAL file.
    pub fn checkpoint_wal(&self) -> CcResult<()> {
        self.0.checkpoint_wal()
    }

    /// Truncate the WAL only once it has grown past `max_bytes`.
    pub fn checkpoint_wal_if_large(&self, max_bytes: u64) -> CcResult<bool> {
        self.0.checkpoint_wal_if_large(max_bytes)
    }

    /// Perform a full rebuild using a temporary database file, then atomically
    pub fn rebuild_with_temp_db<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        self.0.rebuild_with_temp_db(write_fn)
    }

    /// High-speed full rebuild using DirectWriter.
    pub fn rebuild_with_direct_writer<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        self.0.rebuild_with_direct_writer(write_fn)
    }

    /// Standard temp-db staging path (`.sqlite3.tmp` beside the live db).
    pub fn rebuild_staging_path(&self) -> PathBuf {
        self.0.rebuild_staging_path()
    }

    /// Write a replacement database to the staging path without swapping.
    pub fn build_temp_db_staging<F>(&self, write_fn: F) -> CcResult<IndexGeneration>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.rebuild_staging_path();
        self.0.build_rebuild_staging(&tmp_path, |tmp_path| {
            IndexDb::execute_temp_db_staging_build(tmp_path, write_fn)
        })
    }

    /// Atomically swap a previously staged rebuild database into place.
    pub fn swap_rebuild_staging(&self, generation_floor: IndexGeneration) -> CcResult<()> {
        let tmp_path = self.rebuild_staging_path();
        self.0
            .swap_rebuild_staging(&tmp_path, generation_floor, "full rebuild")
    }
}

#[cfg(test)]
#[path = "index_db_tests.rs"]
mod tests;
