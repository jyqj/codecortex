//! IndexDatabase — the index.sqlite3 connection manager.
//!
//! Read: pool of connections (one per query, no manual refresh needed).
//! Write: single Mutex<Connection> for exclusive writes.
//! FTS sync: application-layer, in the same transaction as base table writes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{types::Type, Connection};

use cc_model::config::ProjectStats;
use cc_model::edge::RouteNodeRecord;
use cc_model::parse::ParseOutcome;
use cc_model::{CcError, CcResult, Language, ParserTier};

use crate::index_migrate::{
    migrate_index_db, SchemaStatus, CURRENT_SCHEMA_VERSION, FULL_SCHEMA_SQL,
};
use crate::sql_util::{sql_in_placeholders, IN_BATCH_SIZE};

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

/// Metadata key for the index-content epoch counter.
pub const INDEX_EPOCH_KEY: &str = "index_epoch";
/// Metadata key for the runtime-evidence epoch counter.
pub const EVIDENCE_EPOCH_KEY: &str = "evidence_epoch";

/// Monotonic epoch vector persisted in the metadata KV table.
///
/// `index_epoch` advances whenever index content is committed (file batches,
/// postprocess edge rebuilds, full rebuilds). `evidence_epoch` advances on
/// runtime-evidence writes only, so evidence ingestion never invalidates
/// caches that depend solely on index content. Consumers key their caches on
/// these values and compare on read; any observed change forces a recompute.
/// Databases created before the epochs existed read as `0` until first write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IndexGeneration {
    pub index_epoch: u64,
    pub evidence_epoch: u64,
}

/// Persisted file metadata used to decide whether an incremental scan can skip
/// reading and hashing a file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub content_hash: String,
    pub mtime: f64,
    pub size: u64,
}

/// A single file's worth of data to write into the index.
#[derive(Clone)]
pub struct FileWriteUnit {
    pub rel_path: String,
    pub language: Language,
    pub content_hash: String,
    pub mtime: f64,
    pub size: u64,
    pub outcome: ParseOutcome,
}

/// Pre-compressed chunk payloads computed off the write lock, keyed by
/// `FileWriteUnit::rel_path` and index-aligned with `outcome.chunks`.
/// `Some(blob)` stores the zstd blob, `None` stores plain text — exactly the
/// decision [`compress_chunk_text`] would make inside the transaction, so the
/// on-disk bytes are identical whether or not a side-car entry is present.
pub type PrecompressedChunks = HashMap<String, Vec<Option<Vec<u8>>>>;

/// Deterministic chunk compression policy: zstd level 3, only for payloads
/// larger than 128 bytes, and only when compression actually saves space.
/// Returns `None` when the chunk should be stored as plain text. Shared by
/// the prepare-phase precompression (cc-index) and the in-transaction
/// fallback in [`IndexDb::insert_file_data`], so both produce byte-identical
/// rows.
pub fn compress_chunk_text(text: &str) -> Option<Vec<u8>> {
    let text_bytes = text.as_bytes();
    if text_bytes.len() <= 128 {
        return None;
    }
    match zstd::encode_all(std::io::Cursor::new(text_bytes), 3) {
        Ok(compressed) if compressed.len() < text_bytes.len() => Some(compressed),
        _ => None,
    }
}

pub type RepoFrameworkRecord = (String, f64, Vec<String>);
pub type FileFrameworkSignal = (String, f64, String);
pub type FileFrameworkRecord = (String, Vec<FileFrameworkSignal>);

#[derive(Debug, Clone)]
pub struct SymbolCoverRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CallEdgeLite {
    pub file_path: String,
    pub line: u32,
    pub caller_symbol: Option<String>,
    pub callee_symbol: String,
    pub caller_symbol_uid: Option<String>,
    pub callee_symbol_uid: Option<String>,
    pub resolution_kind: String,
    pub confidence: f64,
    pub dispatch_kind: String,
    pub synthesized_by: Option<String>,
    pub synthesis_key: Option<String>,
    pub registered_file: Option<String>,
    pub registered_line: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub container: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub qname: Option<String>,
    pub signature: Option<String>,
}

/// BFS-friendly edge info returned by call_uid_edges_lite.
#[derive(Debug, Clone)]
pub struct EdgeLiteBfs {
    pub caller_uid: String,
    pub callee_uid: String,
    pub dispatch_kind: String,
    pub synthesized_by: Option<String>,
    pub synthesis_key: Option<String>,
    pub confidence: f64,
    pub file_path: String,
    pub line: u32,
    pub registered_file: Option<String>,
    pub registered_line: Option<u32>,
    pub resolution_kind: Option<String>,
    pub parser_tier: Option<String>,
    pub resolution_strategy: Option<String>,
    pub parser_confidence: Option<f64>,
}

/// Symbol degree metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolDegreeInfo {
    pub in_degree: u32,
    pub out_degree: u32,
    pub caller_count: u32,
    pub callee_count: u32,
    pub ref_count: u32,
}

#[derive(Debug, Clone)]
pub struct SymbolTargetRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub qname: Option<String>,
    pub file_path: String,
}

/// Full chunk row with decoded text, returned by `chunk_rows_by_ids`.
#[derive(Debug, Clone)]
pub struct ChunkDetailRow {
    pub chunk_id: String,
    pub file_path: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileInfoRow {
    pub file_path: String,
    pub language: String,
    pub size: u64,
    pub parser_tier: String,
    pub indexed_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommunityRow {
    pub community_id: u32,
    pub label: String,
    pub member_count: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolRefLite {
    pub file_path: String,
    pub line: u32,
    pub symbol_name: String,
    pub target_symbol_uid: Option<String>,
    pub resolution_kind: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolutionAttemptRow {
    pub attempt_id: String,
    pub source_table: String,
    pub source_id: String,
    pub file_path: String,
    pub reference_name: String,
    pub reference_kind: String,
    pub line: u32,
    pub column_no: u32,
    pub container: Option<String>,
    pub candidates: serde_json::Value,
    pub failure_reason: String,
    pub resolution_strategy: String,
    pub parser_tier: String,
    pub parser_confidence: f64,
    pub language: Option<String>,
}

/// Register a `REGEXP(pattern, text)` scalar function on a SQLite connection.
///
/// This enables `column REGEXP ?` syntax in SQL (used by Cypher `=~` expressions).
/// The compiled `Regex` is cached as SQLite auxiliary data keyed on the pattern
/// argument, so a constant pattern is compiled once per statement rather than once
/// per row.
fn register_regexp_function(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;

    conn.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let re: std::sync::Arc<regex::Regex> = ctx.get_or_create_aux(
                0,
                |vr| -> Result<_, Box<dyn std::error::Error + Send + Sync + 'static>> {
                    Ok(regex::Regex::new(vr.as_str()?)?)
                },
            )?;
            let text: String = ctx.get(1)?;
            Ok(re.is_match(&text))
        },
    )
}

/// The index database handle.
pub struct IndexDb {
    pub(crate) db_path: PathBuf,
    pub(crate) pool: RwLock<Pool<SqliteConnectionManager>>,
    pub(crate) write_conn: Mutex<Connection>,
    read_pool_size: u32,
    /// Process-unique handle identity assigned at open from a monotonic
    /// counter. Unlike `Arc::as_ptr`, it is never reused after a handle is
    /// dropped, so caches keyed on it cannot alias across project instances.
    instance_id: u64,
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
            },
            schema_status,
        ))
    }

    /// Process-unique, never-reused identity of this database handle.
    pub(crate) fn instance_id(&self) -> u64 {
        self.instance_id
    }

    fn build_read_pool(
        path: &Path,
        read_pool_size: u32,
    ) -> CcResult<Pool<SqliteConnectionManager>> {
        let manager = SqliteConnectionManager::file(path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
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
            .map_err(|e| CcError::Database(e.to_string()))
    }

    /// Open the database, check schema version, and rebuild if mismatched.
    fn open_and_ensure_schema(path: &Path) -> CcResult<(Connection, SchemaStatus)> {
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
        let pragmas = "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;";

        let conn = Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute_batch(pragmas)
            .map_err(|e| CcError::Database(e.to_string()))?;

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
                        let new_conn =
                            Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
                        new_conn
                            .execute_batch(pragmas)
                            .map_err(|e| CcError::Database(e.to_string()))?;
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
                        ev["observed_count"].as_u64().unwrap_or(1),
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(
                rusqlite::params![INDEX_EPOCH_KEY, EVIDENCE_EPOCH_KEY],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut generation = IndexGeneration::default();
        for row in rows {
            let (key, value) = row.map_err(|e| CcError::Database(e.to_string()))?;
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
    fn finalize_rebuild_generation(&self, tmp_path: &Path, floor: IndexGeneration) -> CcResult<()> {
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
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
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

    // ── Bulk rebuild pragmas ────────────────────────────────────

    /// Apply aggressive pragmas for full rebuild (not safe for incremental).
    /// Only call on the write connection, never on read pool connections.
    fn set_bulk_rebuild_pragmas(conn: &Connection) -> CcResult<()> {
        conn.execute_batch(
            "PRAGMA synchronous = OFF;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -64000;
             PRAGMA mmap_size = 268435456;",
        )
        .map_err(|e| CcError::Database(format!("set_bulk_rebuild_pragmas: {}", e)))?;
        Ok(())
    }

    /// Restore normal pragmas after bulk operation.
    fn restore_normal_pragmas(conn: &Connection) -> CcResult<()> {
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = DEFAULT;
             PRAGMA cache_size = -2000;",
        )
        .map_err(|e| CcError::Database(format!("restore_normal_pragmas: {}", e)))?;
        Ok(())
    }

    pub(crate) fn execute_cached<P: rusqlite::Params>(
        conn: &Connection,
        sql: &str,
        params: P,
    ) -> CcResult<usize> {
        let mut stmt = conn
            .prepare_cached(sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        stmt.execute(params)
            .map_err(|e| CcError::Database(e.to_string()))
    }

    // ── Full rebuild protocol (shared by both rebuild paths) ────

    /// Derive a SQLite sidecar path (`-wal` / `-shm`) by appending to the
    /// full file name. SQLite appends to the complete name, so this must not
    /// go through `Path::with_extension` (which replaces the last extension).
    fn sidecar_path(db_path: &Path, suffix: &str) -> PathBuf {
        let mut os = db_path.as_os_str().to_owned();
        os.push(suffix);
        PathBuf::from(os)
    }

    /// Shared full-rebuild protocol behind [`Self::rebuild_with_temp_db`] and
    /// [`Self::rebuild_with_direct_writer`].
    ///
    /// The strategy-specific part is `build_temp`: it must leave a fully
    /// written, closed SQLite database file at `tmp_path`, or return an error
    /// (cleaning up after itself if it wants to — the protocol only sweeps
    /// stale temp artifacts at the *start* of the next rebuild). The protocol
    /// runs the invariant-bearing steps in order:
    ///
    /// 1. Snapshot the epoch floor.
    /// 2. Remove stale temp artifacts left by a previous crashed run.
    /// 3. `build_temp(tmp_path)` — produce the replacement database.
    /// 4. Under the write lock: finalize the generation into the temp file,
    ///    remove the live WAL/SHM sidecars, atomically rename temp → main,
    ///    remove the temp sidecars.
    /// 5. Reopen the write connection, rebuild the read pool, checkpoint the
    ///    fresh WAL (non-fatal on failure).
    ///
    /// # Invariants
    ///
    /// - **Epoch floor.** The generation snapshotted in step 1 is only a
    ///   *floor*: writers (including other processes) may advance the live
    ///   epochs while `build_temp` runs. The final epoch vector is therefore
    ///   written at swap time, under the write lock, as `max(floor, live) + 1`
    ///   per epoch (see [`Self::finalize_rebuild_generation`]), so a rebuild
    ///   can never produce a "same generation, different content" collision
    ///   with values observed before or during the rebuild.
    /// - **Lock scope / no reentrancy.** The `write_conn` mutex is held for
    ///   the entire finalize + rename window so no writer can commit between
    ///   epoch finalization and the file swap. The mutex is not reentrant:
    ///   `build_temp` (and any `write_fn` it wraps) writes only to the temp
    ///   file and must never take the write lock or call back into either
    ///   rebuild method on the same thread. Concurrent writes from *other*
    ///   threads during `build_temp` are allowed — the epoch floor exists
    ///   precisely to absorb them.
    /// - **FTS dual maintenance.** `symbols_fts` and `file_paths_fts` are
    ///   maintained by schema triggers on `symbols`/`files`, while
    ///   `chunks_fts`, `files_fts` and `literal_fts` have no triggers and are
    ///   maintained at the application layer ([`Self::delete_file_data`] and
    ///   the insert helpers). Rebuild strategies write into a fresh schema, so
    ///   both models hold automatically — but `build_temp` must populate data
    ///   through the shared insert helpers so the application-maintained FTS
    ///   tables stay in sync with their base tables, including the
    ///   FTS-rowid-equals-base-rowid alignment the indexed deletes rely on.
    fn run_rebuild_protocol(
        &self,
        tmp_path: &Path,
        label: &str,
        build_temp: impl FnOnce(&Path) -> CcResult<()>,
    ) -> CcResult<()> {
        // Floor snapshot of the epoch vector; the final value is written at
        // swap time (under the write lock) as max(floor, live) + 1.
        let generation_floor = self.generation().unwrap_or_default();

        // Clean up any stale temp artifacts from a previous crashed run.
        let _ = std::fs::remove_file(tmp_path);
        let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-wal"));
        let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-shm"));

        // Produce the replacement database (strategy-specific).
        build_temp(tmp_path)?;

        // Acquire write lock, do atomic swap while lock is held
        {
            // Lock the write connection to prevent concurrent writes.
            // The rename MUST happen inside this scope so no writer can
            // slip in between lock-release and file replacement.
            let _write_guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;

            // Write the final epoch vector now that no further writes can
            // land: max(floor, live) + 1 covers writes committed while the
            // rebuild ran (including from other processes).
            self.finalize_rebuild_generation(tmp_path, generation_floor)?;

            // Remove the old WAL/SHM files — the new file will create its own
            let wal = self.db_path.with_extension("sqlite3-wal");
            let shm = self.db_path.with_extension("sqlite3-shm");
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);

            // Atomic rename: temp → main (inside write lock)
            std::fs::rename(tmp_path, &self.db_path).map_err(|e| {
                CcError::Database(format!(
                    "atomic rename {} → {}: {}",
                    tmp_path.display(),
                    self.db_path.display(),
                    e
                ))
            })?;

            // Clean up temp WAL/SHM if any
            let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-wal"));
            let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-shm"));
        }

        // Reopen write connection
        let (new_write_conn, _status) = Self::open_and_ensure_schema(&self.db_path)?;
        {
            let mut guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            *guard = new_write_conn;
        }

        // Rebuild the read pool using the configured/adaptive pool size.
        let new_pool = Self::build_read_pool(&self.db_path, self.read_pool_size)?;
        {
            let mut pool_guard = self
                .pool
                .write()
                .map_err(|e| CcError::Database(format!("write pool lock: {}", e)))?;
            *pool_guard = new_pool;
        }

        // Checkpoint WAL to reclaim space after full rebuild
        if let Err(e) = self.checkpoint_wal() {
            tracing::warn!(err = %e, "{}: WAL checkpoint failed (non-fatal)", label);
        }

        Ok(())
    }

    /// Perform a full rebuild using a temporary database file, then atomically
    /// swap it into place. This avoids WAL contention and allows aggressive
    /// pragmas without risk to the live database.
    ///
    /// The `write_fn` closure receives a mutable reference to the temp
    /// `Connection` with bulk pragmas already applied and indexes dropped.
    /// It should insert all data. Indexes are recreated automatically after
    /// the closure returns.
    ///
    /// After successful write, the temp file is renamed over the main db file
    /// and all connections (pool + write) are re-opened.
    ///
    /// Thin adapter over [`Self::run_rebuild_protocol`]; only the temp-file
    /// build strategy lives here.
    pub(crate) fn rebuild_with_temp_db<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("sqlite3.tmp");
        self.run_rebuild_protocol(&tmp_path, "full rebuild", |tmp_path| {
            tracing::info!(
                tmp = %tmp_path.display(),
                "full rebuild: creating temp database"
            );

            // 1. Open temp db and apply schema
            let tmp_conn = Connection::open(tmp_path)
                .map_err(|e| CcError::Database(format!("open temp db: {}", e)))?;
            tmp_conn
                .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
                .map_err(|e| CcError::Database(format!("temp db pragmas: {}", e)))?;
            // Same statement-rotation hazard as the main write connection:
            // the per-file insert helpers keep ~20 cached statements alive.
            tmp_conn.set_prepared_statement_cache_capacity(64);

            // Apply full schema
            tmp_conn
                .execute_batch(FULL_SCHEMA_SQL)
                .map_err(|e| CcError::Database(format!("temp db schema init failed: {}", e)))?;
            tmp_conn
                .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .map_err(|e| CcError::Database(format!("temp db set version: {}", e)))?;

            // 2. Apply bulk pragmas
            Self::set_bulk_rebuild_pragmas(&tmp_conn)?;

            // 3. Drop non-PK indexes for faster bulk insert (derived from the schema
            //    so the set always matches index_v1.sql)
            tmp_conn
                .execute_batch(&crate::direct_writer::drop_index_statements(
                    FULL_SCHEMA_SQL,
                ))
                .map_err(|e| CcError::Database(format!("temp db drop indexes: {}", e)))?;

            tracing::info!("full rebuild: writing data to temp database");

            // 4. Write all data inside a single transaction
            {
                let tx_result = (|| -> CcResult<()> {
                    let tx = tmp_conn
                        .unchecked_transaction()
                        .map_err(|e| CcError::Database(format!("temp db transaction: {}", e)))?;
                    write_fn(&tx)?;
                    tx.commit()
                        .map_err(|e| CcError::Database(format!("temp db commit: {}", e)))?;
                    Ok(())
                })();

                if let Err(e) = tx_result {
                    tracing::warn!(err = %e, "full rebuild failed, cleaning up temp db");
                    drop(tmp_conn);
                    let _ = std::fs::remove_file(tmp_path);
                    return Err(e);
                }
            }

            // 5. Recreate indexes (same schema-derived set as the drop above)
            tracing::info!("full rebuild: recreating indexes");
            tmp_conn
                .execute_batch(&crate::direct_writer::extract_index_statements(
                    FULL_SCHEMA_SQL,
                ))
                .map_err(|e| CcError::Database(format!("temp db recreate indexes: {}", e)))?;

            // Restore normal pragmas before closing
            Self::restore_normal_pragmas(&tmp_conn)?;

            // 6. Close temp connection
            drop(tmp_conn);

            tracing::info!("full rebuild: swapping temp database into place");
            Ok(())
        })?;

        tracing::info!("full rebuild: temp-db swap complete");
        Ok(())
    }

    /// High-speed full rebuild using DirectWriter.
    ///
    /// Creates a fresh database with aggressive PRAGMAs (journal OFF, synchronous OFF,
    /// 64KB pages, exclusive locking), writes all data via the caller-supplied closure,
    /// creates indexes after data, validates with integrity_check, then atomically
    /// swaps the new file into place.
    ///
    /// The `write_fn` signature matches `rebuild_with_temp_db` so callers can switch
    /// between them trivially.
    ///
    /// Thin adapter over [`Self::run_rebuild_protocol`]; only the
    /// DirectWriter build strategy lives here.
    ///
    /// Enable via `IndexingConfig::use_direct_writer == true`.
    pub(crate) fn rebuild_with_direct_writer<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("direct-tmp.sqlite3");
        self.run_rebuild_protocol(&tmp_path, "direct writer", |tmp_path| {
            tracing::info!(
                tmp = %tmp_path.display(),
                "direct writer: creating high-speed temp database"
            );

            crate::direct_writer::DirectWriter::write_db(tmp_path, FULL_SCHEMA_SQL, |tx| {
                // Set schema version inside the transaction
                tx.pragma_update(
                    None,
                    "user_version",
                    crate::index_migrate::CURRENT_SCHEMA_VERSION,
                )
                .map_err(|e| format!("set user_version: {}", e))?;

                // Delegate to caller's write function.
                // Transaction derefs to Connection, so write_fn(&tx) works.
                write_fn(tx).map_err(|e| e.to_string())
            })
            .map_err(|e| CcError::Database(format!("direct writer: {}", e)))?;

            tracing::info!("direct writer: temp database written, swapping into place");
            Ok(())
        })?;

        tracing::info!("direct writer: swap complete");
        Ok(())
    }

    // ── File state ───────────────────────────────────────────────

    pub(crate) fn get_file_state(&self) -> CcResult<HashMap<String, FileState>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path, content_hash, mtime, size FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut map = HashMap::new();
        for row in rows {
            let (path, state) = row.map_err(|e| CcError::Database(e.to_string()))?;
            map.insert(path, state);
        }
        Ok(map)
    }

    // ── Batch write ──────────────────────────────────────────────

    pub(crate) fn replace_files_batch(&self, files: &[FileWriteUnit]) -> CcResult<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rel_paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        Self::delete_files_fts_batch(&tx, rel_paths.iter().copied())?;
        // Replacement keeps the path, so the path-derived test_edges
        // stay valid (see `delete_files_data_base_keep_test_edges_batch`).
        Self::delete_files_data_base_keep_test_edges_batch(&tx, &rel_paths)?;
        for file in files {
            Self::insert_file_data_deferred_fts(&tx, file, None)?;
        }
        Self::insert_files_literal_fts_batch(&tx, &rel_paths)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Apply one round of config-linker output (incremental path).
    ///
    /// Units whose file already has a `files` row (scanner-visible config
    /// files like yaml/toml, written as parsed units) only replace that
    /// file's config refs — the parsed representation (files row, chunks,
    /// FTS) stays intact. Units without a row are written whole, as before
    /// (non-scanner config files such as .ini/.env).
    ///
    /// `seen_config_files` are the config files covered by this round's scan
    /// (or token cache): a seen file without a unit resolved to ZERO links,
    /// so its leftover config refs from earlier rounds are deleted here —
    /// they would otherwise linger until the next full rebuild.
    pub(crate) fn apply_config_link_units(
        &self,
        units: &[FileWriteUnit],
        seen_config_files: &[String],
    ) -> CcResult<()> {
        if units.is_empty() && seen_config_files.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let linked_now: std::collections::HashSet<&str> =
            units.iter().map(|u| u.rel_path.as_str()).collect();
        let mut wrote = false;
        for path in seen_config_files {
            if linked_now.contains(path.as_str()) {
                continue;
            }
            // 零链接陈旧行清理：本轮没有替换单元的文件，旧 refs 直接删。
            wrote |= Self::delete_config_link_refs(&tx, path)? > 0;
        }
        for unit in units {
            let parsed_row_exists = {
                let mut stmt = tx
                    .prepare_cached("SELECT 1 FROM files WHERE file_path = ?1")
                    .map_err(|e| CcError::Database(e.to_string()))?;
                stmt.exists(rusqlite::params![unit.rel_path])
                    .map_err(|e| CcError::Database(e.to_string()))?
            };
            if parsed_row_exists {
                Self::delete_config_link_refs(&tx, &unit.rel_path)?;
                Self::insert_symbol_refs_on(&tx, &unit.outcome.symbol_refs)?;
            } else {
                Self::delete_file_data(&tx, &unit.rel_path)?;
                Self::insert_file_data(&tx, unit)?;
            }
            wrote = true;
        }
        if !wrote {
            // 真正零变化：丢弃事务，不 bump epoch（保持快速路径零写入语义）。
            return Ok(());
        }
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Append a config-link unit's refs without touching the file's parsed
    /// rows. Full-rebuild counterpart of the row-preserving branch in
    /// [`Self::apply_config_link_units`]: inside the rebuild closure the
    /// parsed unit was already inserted, so a second `insert_file_data`
    /// would violate the `files` primary key and lose the parsed chunks.
    pub fn insert_config_link_refs(conn: &Connection, unit: &FileWriteUnit) -> CcResult<()> {
        Self::insert_symbol_refs_on(conn, &unit.outcome.symbol_refs)
    }

    /// Delete the config-linker refs of `rel_path` (parser-produced refs
    /// untouched). Returns the number of deleted rows.
    fn delete_config_link_refs(conn: &Connection, rel_path: &str) -> CcResult<usize> {
        Self::execute_cached(
            conn,
            "DELETE FROM symbol_refs WHERE file_path = ?1 \
             AND ref_kind IN ('config_module','config_file','config_dependency')",
            rusqlite::params![rel_path],
        )
    }

    /// Insert symbol_refs rows (INSERT OR REPLACE on ref_id).
    fn insert_symbol_refs_on(
        conn: &Connection,
        refs: &[cc_model::symbol::SymbolRefRecord],
    ) -> CcResult<()> {
        for r in refs {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
            )?;
        }
        Ok(())
    }

    /// Update only the edge/resolution data for dirty (DirtyResolveOnly) files.
    /// Does NOT delete or modify: files row, chunks, FTS, route_nodes,
    /// http_call_edges, data_flow_edges, literals, file_frameworks,
    /// co_change_edges, test_edges.
    /// Only replaces: symbols, imports, call_edges, symbol_refs, semantic_edges,
    /// dispatch_sites, route_edges.
    pub(crate) fn replace_reresolved_edges_only(&self, units: &[FileWriteUnit]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for file in units {
            Self::replace_reresolved_edges_for_file(&tx, file)?;
        }
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Per-file body of [`Self::replace_reresolved_edges_only`], usable inside
    /// a caller-owned transaction.
    pub(crate) fn replace_reresolved_edges_for_file(
        tx: &Connection,
        file: &FileWriteUnit,
    ) -> CcResult<()> {
        {
            let rel = file.rel_path.as_str();
            let outcome = &file.outcome;

            // Delete only the re-resolvable tables
            for table in &[
                "call_edges",
                "symbol_refs",
                "symbols",
                "imports",
                "semantic_edges",
                "dispatch_sites",
                "routes",
            ] {
                Self::execute_cached(
                    tx,
                    &format!("DELETE FROM {} WHERE file_path = ?1", table),
                    rusqlite::params![rel],
                )?;
            }

            // Re-insert symbols
            for s in &outcome.symbols {
                Self::execute_cached(
                    tx,
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                    rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
                )?;
            }

            // Re-insert imports
            for i in &outcome.imports {
                Self::execute_cached(
                    tx,
                    "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![i.file_path, i.import_string, i.resolved_path, i.imported_name, i.alias, i.is_namespace as i32, i.is_default as i32, i.is_reexport as i32],
                )?;
            }

            // Re-insert symbol_refs
            for r in &outcome.symbol_refs {
                Self::execute_cached(
                    tx,
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
                )?;
            }

            // Re-insert call_edges
            for e in &outcome.call_edges {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                    rusqlite::params![e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol, e.line, e.start_col, e.end_line, e.end_col, e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id, e.caller_symbol_uid, e.callee_symbol_uid, e.dispatch_kind.as_str(), e.call_kind, e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy, e.receiver_expr, e.arg_count.map(|v| v as i32), e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32, e.parser_tier.as_str(), e.parser_confidence, e.synthesized_by, e.synthesis_key, e.registered_file, e.registered_line.map(|v| v as i32)],
                )?;
            }

            // Re-insert semantic_edges
            for se in &outcome.semantic_edges {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![se.edge_id, se.file_path, se.source_symbol, se.source_symbol_uid, se.target_symbol, se.target_symbol_uid, se.relation_kind.as_str(), se.line, se.confidence, se.parser_tier.as_str()],
                )?;
            }

            // Re-insert dispatch_sites
            for ds in &outcome.dispatch_sites {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
                )?;
            }

            // Re-insert route_edges
            for r in &outcome.route_edges {
                Self::execute_cached(
                    tx,
                    "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str(), r.resolution_strategy, r.resolution_confidence],
                )?;
            }
        }
        Ok(())
    }

    /// Write one incremental index batch atomically: file removals, full file
    /// replacements, dirty-file edge re-resolution, route nodes and the batch
    /// files' hierarchy edges share a single transaction, so a crash cannot
    /// leave files deleted with their edges still present — nor leave a batch
    /// file committed (content_hash persisted, so never re-batched) with its
    /// hierarchy edges missing (and the batch costs one WAL sync instead of
    /// four).
    pub(crate) fn write_incremental_batch(
        &self,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
        dirty_units: &[FileWriteUnit],
        route_nodes: &[cc_model::edge::RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        precompressed: &PrecompressedChunks,
    ) -> CcResult<()> {
        if to_remove.is_empty()
            && normal_units.is_empty()
            && dirty_units.is_empty()
            && route_nodes.is_empty()
            && hierarchy_edges.is_empty()
        {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        // Per-section timing: emitted as `tracing::debug!` "sub-phase timing"
        // events (same field style as cc-index's `time_step`) so a slow
        // `write.incremental_batch` aggregate can be attributed from logs.
        fn section_ms(step: &'static str, count: usize, start: std::time::Instant) {
            tracing::debug!(
                phase = "write",
                step,
                count,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "sub-phase timing"
            );
        }
        // One batched scan per FTS table for the whole batch (file_path is
        // UNINDEXED there, so per-file deletes would each scan the table).
        let section_start = std::time::Instant::now();
        Self::delete_files_fts_batch(
            &tx,
            to_remove
                .iter()
                .map(String::as_str)
                .chain(normal_units.iter().map(|f| f.rel_path.as_str())),
        )?;
        section_ms(
            "db_fts_delete",
            to_remove.len() + normal_units.len(),
            section_start,
        );
        let section_start = std::time::Instant::now();
        let remove_paths: Vec<&str> = to_remove.iter().map(String::as_str).collect();
        Self::delete_files_data_base_batch(&tx, &remove_paths)?;
        section_ms("db_remove_files", to_remove.len(), section_start);
        // Replacement keeps the path, so the path-derived test_edges
        // stay valid; only removals above cascade into test_edges.
        // Deletes run batched for the whole replacement set before any
        // insert: no inserted row is keyed by another batch file's path, so
        // the old per-file delete/insert interleaving carried no semantics.
        let section_start = std::time::Instant::now();
        let replace_paths: Vec<&str> = normal_units.iter().map(|f| f.rel_path.as_str()).collect();
        Self::delete_files_data_base_keep_test_edges_batch(&tx, &replace_paths)?;
        section_ms("db_replace_delete", normal_units.len(), section_start);
        let section_start = std::time::Instant::now();
        for file in normal_units {
            Self::insert_file_data_deferred_fts(
                &tx,
                file,
                precompressed.get(&file.rel_path).map(Vec::as_slice),
            )?;
        }
        Self::insert_files_literal_fts_batch(&tx, &replace_paths)?;
        section_ms("db_replace_insert", normal_units.len(), section_start);
        let section_start = std::time::Instant::now();
        for file in dirty_units {
            Self::replace_reresolved_edges_for_file(&tx, file)?;
        }
        section_ms("db_dirty_rewrite", dirty_units.len(), section_start);
        // Hierarchy edges for the batch files, inside the same transaction.
        // Must run after the dirty rewrite above: its per-file delete clears
        // each dirty file's semantic_edges rows, which include the hierarchy
        // edges being re-inserted here.
        let section_start = std::time::Instant::now();
        Self::insert_semantic_edges_batch_on(&tx, hierarchy_edges)?;
        section_ms("db_hierarchy_edges", hierarchy_edges.len(), section_start);
        let section_start = std::time::Instant::now();
        Self::insert_route_nodes_on(&tx, route_nodes)?;
        Self::bump_index_epoch_on(&tx)?;
        section_ms("db_routes_epoch", route_nodes.len(), section_start);
        let section_start = std::time::Instant::now();
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        section_ms("db_commit", 0, section_start);
        Ok(())
    }

    pub(crate) fn remove_files_batch(&self, paths: &[String]) -> CcResult<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rel_paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        Self::delete_files_fts_batch(&tx, rel_paths.iter().copied())?;
        Self::delete_files_data_base_batch(&tx, &rel_paths)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(paths.len())
    }

    /// Delete every row owned by `rel_path` across content, FTS and edge tables.
    ///
    /// FTS dual-maintenance model: `symbols_fts` and `file_paths_fts` are kept
    /// in sync by schema triggers (the `DELETE FROM files` in
    /// [`Self::delete_file_data_base`] cascades into `symbols` via
    /// `ON DELETE CASCADE`, firing those triggers), while `chunks_fts`,
    /// `files_fts` and `literal_fts` have no triggers and MUST be deleted at
    /// the application layer — *before* the base rows, in the same
    /// transaction (the rowid-aligned FTS delete resolves rowids through the
    /// still-present base rows). Multi-file callers should batch the FTS half
    /// via [`Self::delete_files_fts_batch`] instead of calling this in a loop.
    pub(crate) fn delete_file_data(conn: &Connection, rel_path: &str) -> CcResult<()> {
        Self::delete_files_fts_batch(conn, std::iter::once(rel_path))?;
        Self::delete_file_data_base(conn, rel_path)
    }

    /// Delete the app-maintained FTS rows (`chunks_fts`, `files_fts`,
    /// `literal_fts`) for a set of files using chunked `IN (...)` statements.
    ///
    /// FTS rowids are aligned with their base-table rowids (schema v5, see
    /// `index_v1.sql`), so each delete resolves the doomed rowids through the
    /// base table's `file_path` index and removes the FTS rows by rowid —
    /// O(log n) per row instead of the full FTS-content-table scan that a
    /// DELETE on the UNINDEXED `file_path` column degrades to. The `IN (...)`
    /// list stays chunked at [`IN_BATCH_SIZE`] to respect
    /// SQLITE_MAX_VARIABLE_NUMBER.
    ///
    /// MUST run before the base-table rows are deleted (the rowid subquery
    /// needs them), in the same transaction as those deletes.
    pub(crate) fn delete_files_fts_batch<'p>(
        conn: &Connection,
        rel_paths: impl IntoIterator<Item = &'p str>,
    ) -> CcResult<()> {
        let rel_paths: Vec<&str> = rel_paths.into_iter().collect();
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            for (fts_table, base_table) in &[
                ("chunks_fts", "chunks"),
                ("files_fts", "files"),
                ("literal_fts", "literal_index"),
            ] {
                Self::execute_cached(
                    conn,
                    &format!(
                        "DELETE FROM {} WHERE rowid IN \
                         (SELECT rowid FROM {} WHERE file_path IN ({}))",
                        fts_table, base_table, placeholders
                    ),
                    rusqlite::params_from_iter(batch.iter()),
                )?;
            }
        }
        Ok(())
    }

    /// Base-table half of [`Self::delete_file_data`]: everything except the
    /// app-maintained FTS mirrors, which multi-file callers batch separately.
    pub(crate) fn delete_file_data_base(conn: &Connection, rel_path: &str) -> CcResult<()> {
        Self::delete_files_data_base_batch(conn, &[rel_path])
    }

    /// Batched [`Self::delete_file_data_base`]: one chunked `IN (...)` DELETE
    /// per table for the whole removal set instead of per-file statements.
    /// The `OR`-predicate tables (`test_edges`, `co_change_edges`) split into
    /// one DELETE per endpoint column so each runs on its own index.
    pub(crate) fn delete_files_data_base_batch(
        conn: &Connection,
        rel_paths: &[&str],
    ) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            for column in &["test_file_path", "code_file_path"] {
                Self::execute_cached(
                    conn,
                    &format!(
                        "DELETE FROM test_edges WHERE {} IN ({})",
                        column, placeholders
                    ),
                    rusqlite::params_from_iter(batch.iter()),
                )?;
            }
            Self::delete_files_data_chunk_keep_test_edges(conn, batch, &placeholders)?;
        }
        Ok(())
    }

    /// [`Self::delete_files_data_base_batch`] minus the test_edges cascade,
    /// for replace-in-place writers. Test edges are path-derived: their
    /// endpoints are file paths and the matching depends only on the path set
    /// plus the `is_test_file` flag, which every parser computes from the
    /// path alone — so deleting and re-inserting a file under the SAME path
    /// cannot change its test edges. New paths have no edges to delete
    /// (postprocess builds them), and removed paths go through
    /// [`Self::delete_files_data_base_batch`]. Chunked at [`IN_BATCH_SIZE`]
    /// like the FTS half (which MUST run first — see
    /// [`Self::delete_files_fts_batch`]).
    pub(crate) fn delete_files_data_base_keep_test_edges_batch(
        conn: &Connection,
        rel_paths: &[&str],
    ) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            Self::delete_files_data_chunk_keep_test_edges(conn, batch, &placeholders)?;
        }
        Ok(())
    }

    /// One `IN (...)` chunk of the keep-test-edges delete: every per-file
    /// DELETE the old loop issued, as one statement per table. The `files`
    /// DELETE still cascades per row into chunks/symbols/imports/symbol_refs/
    /// call_edges/literal_index and fires the `symbols_fts` /
    /// `file_paths_fts` triggers row-by-row, exactly as before — no table's
    /// rows reference another file in the batch, so the per-file interleaving
    /// order carried no semantics.
    fn delete_files_data_chunk_keep_test_edges(
        conn: &Connection,
        batch: &[&str],
        placeholders: &str,
    ) -> CcResult<()> {
        Self::execute_cached(
            conn,
            &format!(
                "DELETE FROM frameworks WHERE scope='file' AND scope_id IN ({})",
                placeholders
            ),
            rusqlite::params_from_iter(batch.iter()),
        )?;
        for table in &[
            "routes",
            "data_flow_edges",
            "http_call_edges",
            "semantic_edges",
            "dispatch_sites",
        ] {
            Self::execute_cached(
                conn,
                &format!("DELETE FROM {} WHERE file_path IN ({})", table, placeholders),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        for column in &["file_a", "file_b"] {
            Self::execute_cached(
                conn,
                &format!(
                    "DELETE FROM co_change_edges WHERE {} IN ({})",
                    column, placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        Self::execute_cached(
            conn,
            &format!("DELETE FROM files WHERE file_path IN ({})", placeholders),
            rusqlite::params_from_iter(batch.iter()),
        )?;
        Ok(())
    }

    /// Insert a single file's data into the given connection.
    /// Accepts `&Connection` so it works with both `Transaction` (via Deref)
    /// and bare connections (e.g. inside `rebuild_with_temp_db`).
    pub fn insert_file_data(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
        Self::insert_file_data_precompressed(conn, file, None)
    }

    /// [`Self::insert_file_data`] with optional pre-compressed chunk payloads
    /// (index-aligned with `outcome.chunks`, see [`PrecompressedChunks`]).
    /// Chunks without a side-car entry fall back to [`compress_chunk_text`]
    /// inside the transaction — same policy, identical on-disk bytes.
    pub fn insert_file_data_precompressed(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
    ) -> CcResult<()> {
        Self::insert_file_data_impl(conn, file, chunk_blobs, false)
    }

    /// [`Self::insert_file_data_precompressed`] minus the per-row `files_fts`
    /// / `literal_fts` mirror inserts, for multi-file writers that mirror
    /// those tables afterwards in one shot via
    /// [`Self::insert_files_literal_fts_batch`]. `chunks_fts` stays per-row in
    /// both modes: its base column may hold a zstd BLOB while FTS needs the
    /// plain text, so a SELECT-based mirror would require a decompression UDF
    /// for no measurable gain.
    ///
    /// Public for rebuild closures and the write-path micro-benchmark; always
    /// pair with [`Self::insert_files_literal_fts_batch`] in the same
    /// transaction.
    pub fn insert_file_data_deferred_fts(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
    ) -> CcResult<()> {
        Self::insert_file_data_impl(conn, file, chunk_blobs, true)
    }

    /// Mirror freshly inserted `files` / `literal_index` rows into their FTS
    /// tables by selecting straight from the base tables, one chunked
    /// `IN (...)` statement per table — rowid alignment by construction, no
    /// per-row `last_insert_rowid()` round-trips. Selecting from the base
    /// table also inherits the literal `OR IGNORE` first-wins semantics:
    /// only the surviving base rows exist to be mirrored.
    ///
    /// MUST run inside the same transaction after every base-row insert for
    /// `rel_paths`, and only over paths whose previous rows were deleted in
    /// this batch (otherwise pre-existing rows would be mirrored twice).
    pub fn insert_files_literal_fts_batch(
        conn: &Connection,
        rel_paths: &[&str],
    ) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            Self::execute_cached(
                conn,
                &format!(
                    "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) \
                     SELECT rowid,file_path,summary,content_excerpt FROM files \
                     WHERE file_path IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
            Self::execute_cached(
                conn,
                &format!(
                    "INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) \
                     SELECT rowid,literal_id,file_path,literal,literal_kind FROM literal_index \
                     WHERE file_path IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        Ok(())
    }

    fn insert_file_data_impl(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
        defer_files_literal_fts: bool,
    ) -> CcResult<()> {
        let outcome = &file.outcome;
        let now = chrono::Utc::now().to_rfc3339();
        let excerpt: String = outcome
            .chunks
            .iter()
            .take(3)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(20000)
            .collect();

        // files + files_fts (FTS rowid aligned with files.rowid; SQLite
        // resets last_insert_rowid after the file_paths_fts_ai trigger, so
        // it reliably names the files row here). Batch writers defer the
        // files_fts mirror to `insert_files_literal_fts_batch`.
        Self::execute_cached(
            conn,
            "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![file.rel_path, file.language.as_str(), file.content_hash, file.mtime, file.size as i64, outcome.summary, excerpt, outcome.parser_tier.as_str(), outcome.parser_confidence, outcome.is_test_file as i32, now],
        )?;
        if !defer_files_literal_fts {
            Self::execute_cached(
                conn,
                "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) VALUES(?1,?2,?3,?4)",
                rusqlite::params![conn.last_insert_rowid(), file.rel_path, outcome.summary, excerpt],
            )?;
        }

        // chunks + chunks_fts
        for (chunk_idx, c) in outcome.chunks.iter().enumerate() {
            // Compress chunk text with zstd when it saves space. Prefer the
            // payload pre-compressed during prepare (off the write lock);
            // fall back to compressing here for callers without a side-car.
            let fallback;
            let use_compressed: Option<&[u8]> = match chunk_blobs.and_then(|b| b.get(chunk_idx)) {
                Some(precomputed) => precomputed.as_deref(),
                None => {
                    fallback = compress_chunk_text(&c.text);
                    fallback.as_deref()
                }
            };
            if let Some(blob) = use_compressed {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), blob, "zstd", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            } else {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), c.text, "plain", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            }
            // FTS always receives uncompressed text (rowid aligned with the
            // chunks row just inserted)
            Self::execute_cached(
                conn,
                "INSERT INTO chunks_fts(rowid,chunk_id,file_path,breadcrumb,symbol_name,text) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![conn.last_insert_rowid(), c.chunk_id, c.file_path, c.breadcrumb, c.symbol_name, c.text],
            )?;
        }

        // symbols
        for s in &outcome.symbols {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
            )?;
        }

        // imports
        for i in &outcome.imports {
            Self::execute_cached(conn, "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![i.file_path, i.import_string, i.resolved_path, i.imported_name, i.alias, i.is_namespace as i32, i.is_default as i32, i.is_reexport as i32],
            )?;
        }

        // symbol_refs
        Self::insert_symbol_refs_on(conn, &outcome.symbol_refs)?;

        // call_edges
        for e in &outcome.call_edges {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                rusqlite::params![e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol, e.line, e.start_col, e.end_line, e.end_col, e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id, e.caller_symbol_uid, e.callee_symbol_uid, e.dispatch_kind.as_str(), e.call_kind, e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy, e.receiver_expr, e.arg_count.map(|v| v as i32), e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32, e.parser_tier.as_str(), e.parser_confidence, e.synthesized_by, e.synthesis_key, e.registered_file, e.registered_line.map(|v| v as i32)],
            )?;
        }

        // test_edges
        for t in &outcome.test_edges {
            Self::execute_cached(conn, "INSERT OR IGNORE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![t.edge_id, t.test_file_path, t.code_file_path, t.reason, t.confidence],
            )?;
        }

        // route_edges
        for r in &outcome.route_edges {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str(), r.resolution_strategy, r.resolution_confidence],
            )?;
        }

        // http_call_edges
        for hce in &outcome.http_call_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,normalized_path,method,call_kind,line,confidence,parser_tier,broker_type) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![hce.edge_id, hce.file_path, hce.caller_symbol_uid, hce.url_or_path, hce.normalized_path, hce.method, hce.call_kind, hce.line, hce.confidence, hce.parser_tier.as_str(), hce.broker_type],
            )?;
        }

        // literal_index + literal_fts (FTS rowid aligned with the base row).
        // OR IGNORE instead of the previous OR REPLACE: a REPLACE would give
        // the surviving base row a fresh rowid and orphan the FTS row written
        // for the first occurrence. literal_id is derived from
        // (file_path,line,col), so a conflict can only be a duplicate
        // extraction of the same literal within this outcome — first one wins
        // and the duplicate is skipped on both sides. Batch writers defer the
        // FTS mirror; selecting from the base table preserves first-wins.
        for l in &outcome.literal_index {
            let inserted = Self::execute_cached(conn, "INSERT OR IGNORE INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence, l.enclosing_symbol_uid, l.key_path],
            )?;
            if !defer_files_literal_fts && inserted > 0 {
                Self::execute_cached(conn, "INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4,?5)", rusqlite::params![conn.last_insert_rowid(), l.literal_id, l.file_path, l.literal, l.literal_kind])?;
            }
        }

        // semantic_edges
        for se in &outcome.semantic_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![se.edge_id, se.file_path, se.source_symbol, se.source_symbol_uid, se.target_symbol, se.target_symbol_uid, se.relation_kind.as_str(), se.line, se.confidence, se.parser_tier.as_str()],
            )?;
        }

        // data_flow_edges
        for dfe in &outcome.data_flow_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier,env_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![dfe.edge_id, dfe.file_path, dfe.source_symbol_uid, dfe.target_symbol_uid, dfe.flow_kind, dfe.line, dfe.confidence, dfe.parser_tier.as_str(), dfe.env_key],
            )?;
        }

        // dispatch_sites
        for ds in &outcome.dispatch_sites {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
            )?;
        }

        Ok(())
    }

    /// Insert a single route node into the given connection.
    pub fn insert_route_node_into(conn: &Connection, r: &RouteNodeRecord) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT OR REPLACE INTO routes(route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![r.route_id, r.file_path, r.route_path, r.method, r.handler_symbol_uid, r.handler_name, r.framework, r.line, r.end_line, r.normalized_path, r.confidence, r.parser_tier.as_str()],
        )?;
        Ok(())
    }

    /// Set a metadata key=value on the given connection.
    pub fn set_metadata_on(conn: &Connection, key: &str, value: &str) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
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
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key, value])
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Stats ────────────────────────────────────────────────────

    pub(crate) fn stats(&self, project_path: &Path) -> CcResult<ProjectStats> {
        let conn = self.read_conn()?;
        let count = |table: &str| -> usize {
            conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap_or(0)
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

#[allow(dead_code)]
pub(crate) fn is_actionable_reference_name(name: &str) -> bool {
    if name.len() < 2 || name == "_" {
        return false;
    }
    const BUILTINS: &[&str] = &[
        "Ok",
        "Err",
        "Some",
        "None",
        "Result",
        "Option",
        "String",
        "Vec",
        "HashMap",
        "HashSet",
        "Self",
        "self",
        "super",
        "crate",
        "true",
        "false",
        "null",
        "undefined",
        "console",
        "require",
        "module",
        "exports",
        "Promise",
        "Object",
        "Array",
        "Number",
        "Boolean",
        "str",
        "int",
        "float",
        "bool",
        "dict",
        "list",
        "set",
        "tuple",
    ];
    !BUILTINS.contains(&name)
}

/// Lightweight route edge row for frontier expansion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteEdgeLite {
    pub edge_id: String,
    pub file_path: String,
    pub route_path: String,
    pub handler_name: Option<String>,
    pub method: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub handler_symbol_uid: Option<String>,
    pub framework: Option<String>,
    pub confidence: f64,
}

/// Lightweight route node row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteNodeLite {
    pub route_id: String,
    pub file_path: String,
    pub route_path: String,
    pub method: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub handler_name: Option<String>,
    pub framework: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub confidence: f64,
    /// Normalized route path for matching against HTTP call edges.
    /// Only populated by `all_route_nodes_lite`; other queries leave it as None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_path: Option<String>,
}

/// Lightweight co-change edge row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CoChangeLite {
    pub edge_id: String,
    pub file_a: String,
    pub file_b: String,
    pub co_change_count: u32,
    pub total_commits_a: u32,
    pub total_commits_b: u32,
    pub confidence: f64,
}

/// Lightweight HTTP call edge row for frontier expansion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HttpCallEdgeLite {
    pub edge_id: String,
    pub file_path: String,
    pub caller_symbol_uid: Option<String>,
    pub url_or_path: String,
    pub normalized_path: Option<String>,
    pub method: Option<String>,
    pub call_kind: String,
    pub line: u32,
    pub confidence: f64,
}

/// Lightweight symbol row for graph projections (impact seeds, reverse
/// callers): just the identity/location fields plus community membership.
#[derive(Debug, Clone)]
pub struct SymbolLiteRow {
    pub symbol_uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub community_id: Option<u32>,
}

/// Raw symbol row from the dead-code scan; UID may be empty for symbols
/// without a stable identity (callers filter those out).
#[derive(Debug, Clone)]
pub struct DeadCodeSymbolRow {
    pub name: String,
    pub symbol_uid: String,
    pub file_path: String,
    pub kind: String,
}

/// One resolved import edge from a specific file: target path plus the
/// original import string (cycle witness reporting).
#[derive(Debug, Clone)]
pub struct ImportWitnessRow {
    pub resolved_path: String,
    pub import_string: Option<String>,
}

/// Infra nodes, routes, and connecting edges matched for a service/route
/// query. Rows keep the JSON projection shape used by the MCP handlers.
#[derive(Debug, Clone)]
pub struct ServiceBindingRows {
    pub matched_infra_nodes: Vec<serde_json::Value>,
    pub matched_routes: Vec<serde_json::Value>,
    pub related_edges: Vec<serde_json::Value>,
}

/// Aggregated provenance counters over `call_edges`, grouped by
/// dispatch/resolution/synthesis dimensions. Each failed sub-query degrades
/// to an empty breakdown (matching the previous best-effort behavior).
#[derive(Debug, Clone, Default)]
pub struct CallEdgeProvenanceCounts {
    pub by_dispatch_kind: Vec<(Option<String>, i64)>,
    pub synthesized_total: i64,
    pub by_synthesized_by: Vec<(Option<String>, i64)>,
    pub by_resolution_kind: Vec<(Option<String>, i64)>,
}

/// 增量重解析场景的文件边数据载体。
/// 包含重新 resolve 所需的所有边类型，不含 chunk / literal 等无需重解析的数据。
pub struct FileEdgesForReresolve {
    pub symbols: Vec<cc_model::SymbolRecord>,
    pub imports: Vec<cc_model::ImportRecord>,
    pub call_edges: Vec<cc_model::CallEdgeRecord>,
    pub symbol_refs: Vec<cc_model::SymbolRefRecord>,
    pub semantic_edges: Vec<cc_model::SemanticEdgeRecord>,
    pub dispatch_sites: Vec<cc_model::DispatchSiteRecord>,
    pub route_edges: Vec<cc_model::edge::RouteEdgeRecord>,
}

// Read-only facet delegates (see `IndexDb::reads()`).
impl ReadOps<'_> {
    /// Read the persisted epoch vector. Missing keys (old databases) read as 0.
    pub fn generation(&self) -> CcResult<IndexGeneration> {
        self.0.generation()
    }

    /// Get a read connection from the pool.
    pub fn read_conn(&self) -> CcResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.0.read_conn()
    }

    pub fn get_file_state(&self) -> CcResult<HashMap<String, FileState>> {
        self.0.get_file_state()
    }

    pub fn get_metadata(&self, key: &str) -> CcResult<Option<String>> {
        self.0.get_metadata(key)
    }

    pub fn stats(&self, project_path: &Path) -> CcResult<ProjectStats> {
        self.0.stats(project_path)
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
    pub fn write_incremental_batch(
        &self,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
        dirty_units: &[FileWriteUnit],
        route_nodes: &[cc_model::edge::RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        precompressed: &PrecompressedChunks,
    ) -> CcResult<()> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_creates_schema() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let conn = db.read_conn().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"chunks".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"call_edges".to_string()));
    }

    #[test]
    fn metadata_round_trip() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        db.set_metadata("version", "1.0").unwrap();
        assert_eq!(db.get_metadata("version").unwrap(), Some("1.0".to_string()));
        assert_eq!(db.get_metadata("nonexistent").unwrap(), None);
    }

    fn file_unit(rel_path: &str) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: rel_path.to_string(),
            language: Language::Rust,
            content_hash: format!("hash-{rel_path}"),
            mtime: 1.0,
            size: 1,
            outcome: ParseOutcome::default(),
        }
    }

    fn chunk(chunk_index: u32, text: &str) -> cc_model::ChunkRecord {
        cc_model::ChunkRecord {
            chunk_id: format!("chunk:{chunk_index}"),
            file_path: "src/c.rs".to_string(),
            language: Language::Rust,
            chunk_index,
            start_line: 1,
            end_line: 2,
            breadcrumb: "root".to_string(),
            text: text.to_string(),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 4,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        }
    }

    /// prepare 阶段预压缩的 side-car 与事务内回退压缩必须产生逐字节一致的
    /// chunks 行（text blob + text_encoding），且读路径还原出原始文本。
    #[test]
    fn precompressed_chunk_payloads_match_in_transaction_compression() {
        let tiny = "tiny";
        let big = "fn handler() { compute_all_the_things(); }\n".repeat(40);
        let mut unit = file_unit("src/c.rs");
        unit.outcome.chunks = vec![chunk(0, tiny), chunk(1, &big)];

        // Path A: no side-car — compression falls back inside the transaction.
        let tmp_a = TempDir::new().unwrap();
        let db_a = IndexDb::open(&tmp_a.path().join("a.db")).unwrap().0;
        db_a.replace_files_batch(std::slice::from_ref(&unit))
            .unwrap();

        // Path B: side-car pre-compressed with the shared policy helper.
        let tmp_b = TempDir::new().unwrap();
        let db_b = IndexDb::open(&tmp_b.path().join("b.db")).unwrap().0;
        let blobs: Vec<Option<Vec<u8>>> = unit
            .outcome
            .chunks
            .iter()
            .map(|c| compress_chunk_text(&c.text))
            .collect();
        let mut precompressed = PrecompressedChunks::new();
        precompressed.insert("src/c.rs".to_string(), blobs);
        db_b.write_incremental_batch(&[], std::slice::from_ref(&unit), &[], &[], &[], &precompressed)
            .unwrap();

        let chunk_rows = |db: &IndexDb| -> Vec<(i64, rusqlite::types::Value, String)> {
            let conn = db.read_conn().unwrap();
            let mut stmt = conn
                .prepare("SELECT chunk_index, text, text_encoding FROM chunks ORDER BY chunk_index")
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, rusqlite::types::Value>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            rows
        };

        let rows_a = chunk_rows(&db_a);
        let rows_b = chunk_rows(&db_b);
        assert_eq!(rows_a, rows_b, "on-disk chunk rows must be byte-identical");
        assert_eq!(rows_a[0].2, "plain", "small chunk stays plain");
        assert_eq!(rows_a[1].2, "zstd", "large compressible chunk is zstd");

        // Read path decodes the precompressed payload back to the original.
        let conn = db_b.read_conn().unwrap();
        let restored: String = conn
            .query_row(
                "SELECT text, text_encoding FROM chunks WHERE chunk_index = 1",
                [],
                |row| read_chunk_text_with_encoding(row, 0, 1),
            )
            .unwrap();
        assert_eq!(restored, big, "decompressed chunk text round-trips");
    }

    /// 批量 FTS 删除（IN 分块）必须覆盖批内所有路径：替换后 chunks_fts /
    /// files_fts / literal_fts 无陈旧行，移除后与基表同步清空。文件数刻意
    /// 超过 IN_BATCH_SIZE，验证分块边界两侧都被删除。
    #[test]
    fn batched_fts_deletes_cover_all_paths_across_chunk_boundary() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("fts.db")).unwrap().0;

        let total = IN_BATCH_SIZE + 7;
        let units: Vec<FileWriteUnit> = (0..total)
            .map(|i| {
                let path = format!("src/f{i}.rs");
                let mut unit = file_unit(&path);
                let mut c = chunk(0, &format!("fn body_{i}() {{}}"));
                c.chunk_id = format!("chunk:{path}");
                c.file_path = path.clone();
                unit.outcome.chunks = vec![c];
                unit.outcome.literal_index = vec![cc_model::LiteralRecord {
                    literal_id: format!("lit:{path}"),
                    file_path: path,
                    literal: format!("marker {i}"),
                    literal_kind: "string".to_string(),
                    line: 1,
                    container: None,
                    confidence: 0.8,
                    enclosing_symbol_uid: None,
                    key_path: None,
                }];
                unit
            })
            .collect();
        db.replace_files_batch(&units).unwrap();

        let count = |table: &str| -> i64 {
            db.read_conn()
                .unwrap()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count("chunks_fts"), total as i64);

        // Re-replace every file through the incremental batch path: exactly
        // one FTS row per file must remain (no stale duplicates).
        db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();
        assert_eq!(count("chunks_fts"), total as i64);
        assert_eq!(count("files_fts"), total as i64);
        assert_eq!(count("literal_fts"), total as i64);

        // Remove every file: FTS mirrors empty out together with base tables.
        let paths: Vec<String> = units.iter().map(|u| u.rel_path.clone()).collect();
        db.remove_files_batch(&paths).unwrap();
        for table in ["chunks_fts", "files_fts", "literal_fts", "files"] {
            assert_eq!(count(table), 0, "{table} should be empty after removal");
        }
    }

    /// 原地替换（同路径 delete+insert）不得级联删除 test_edges：边是纯路径
    /// 派生的，内容更新不会改变边集；批内 to_remove 与 remove_files_batch
    /// 的级联删除必须保留。
    #[test]
    fn replace_in_place_keeps_test_edges_removal_cascades() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("te.db")).unwrap().0;

        let code = file_unit("src/foo.rs");
        let mut test = file_unit("tests/foo_test.rs");
        test.outcome.is_test_file = true;
        let units = vec![code, test];
        db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();
        db.rebuild_test_edges_for_files(&[
            "src/foo.rs".to_string(),
            "tests/foo_test.rs".to_string(),
        ])
        .unwrap();

        let edge_count = |db: &IndexDb| -> i64 {
            db.read_conn()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM test_edges", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(edge_count(&db), 1, "fixture must produce one test edge");

        // Content-only update through both replace paths: the edge survives.
        db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();
        assert_eq!(
            edge_count(&db),
            1,
            "in-place incremental replace must keep test_edges"
        );
        db.replace_files_batch(&units).unwrap();
        assert_eq!(
            edge_count(&db),
            1,
            "replace_files_batch must keep test_edges for unchanged paths"
        );

        // Removal still cascades.
        db.write_incremental_batch(
            &["tests/foo_test.rs".to_string()],
            &[],
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
        )
        .unwrap();
        assert_eq!(edge_count(&db), 0, "removing a path must drop its edges");
    }

    /// schema v5 不变量：app-maintained FTS 表（chunks_fts / files_fts /
    /// literal_fts）的 rowid 必须与基表行 rowid 对齐且内容一致——索引化
    /// 删除（rowid IN 子查询）依赖该对齐。覆盖首次插入与增量替换（基表
    /// rowid 变化后重新对齐），以及重复 literal_id 不产生 FTS 孤儿行。
    #[test]
    fn app_maintained_fts_rowids_align_with_base_tables() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("align.db")).unwrap().0;

        let literal = |path: &str, suffix: &str| cc_model::LiteralRecord {
            literal_id: format!("lit:{path}:{suffix}"),
            file_path: path.to_string(),
            literal: format!("marker {suffix}"),
            literal_kind: "string".to_string(),
            line: 1,
            container: None,
            confidence: 0.8,
            enclosing_symbol_uid: None,
            key_path: None,
        };
        let units: Vec<FileWriteUnit> = (0..3)
            .map(|i| {
                let path = format!("src/a{i}.rs");
                let mut unit = file_unit(&path);
                unit.outcome.chunks = (0..2)
                    .map(|ci| {
                        let mut c = chunk(ci, &format!("fn body_{i}_{ci}() {{}}"));
                        c.chunk_id = format!("chunk:{path}:{ci}");
                        c.file_path = path.clone();
                        c
                    })
                    .collect();
                // 同一 literal_id 刻意重复：OR IGNORE 必须跳过重复项的
                // FTS 写入，而不是留下指向已死 rowid 的孤儿行。
                unit.outcome.literal_index = vec![literal(&path, "x"), literal(&path, "x")];
                unit
            })
            .collect();
        db.replace_files_batch(&units).unwrap();
        // 增量替换：基表行获得新 rowid，FTS 必须随之重新对齐。
        db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();

        let conn = db.read_conn().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        for (fts, base, id_col) in [
            ("chunks_fts", "chunks", "chunk_id"),
            ("files_fts", "files", "file_path"),
            ("literal_fts", "literal_index", "literal_id"),
        ] {
            assert_eq!(
                count(&format!("SELECT COUNT(*) FROM {fts}")),
                count(&format!("SELECT COUNT(*) FROM {base}")),
                "{fts} must mirror {base} 1:1 (no stale/orphan rows)"
            );
            assert_eq!(
                count(&format!(
                    "SELECT COUNT(*) FROM {fts} f JOIN {base} b ON b.rowid = f.rowid \
                     WHERE b.{id_col} = f.{id_col}"
                )),
                count(&format!("SELECT COUNT(*) FROM {base}")),
                "every {fts} rowid must point at its own {base} row"
            );
        }
        // 对齐删除端到端：按 file_path 删除后 FTS 不留任何行。
        db.remove_files_batch(&["src/a0.rs".to_string(), "src/a1.rs".to_string()])
            .unwrap();
        assert_eq!(count("SELECT COUNT(*) FROM chunks_fts"), 2);
        assert_eq!(count("SELECT COUNT(*) FROM files_fts"), 1);
        assert_eq!(count("SELECT COUNT(*) FROM literal_fts"), 1);
    }

    #[test]
    fn route_edge_resolution_provenance_round_trips() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("routes.db")).unwrap().0;

        let mut unit = file_unit("src/routes.ts");
        unit.outcome
            .route_edges
            .push(cc_model::edge::RouteEdgeRecord {
                edge_id: "route:1".to_string(),
                file_path: "src/routes.ts".to_string(),
                route_path: "/users".to_string(),
                handler_name: Some("getUsers".to_string()),
                method: Some("GET".to_string()),
                line: 5,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: Some("id:getUsers".to_string()),
                handler_symbol_uid: Some("uid:getUsers".to_string()),
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("express".to_string()),
                route_kind: None,
                confidence: 0.8,
                parser_tier: cc_model::ParserTier::TreeSitter,
                resolution_strategy: Some("route_ladder:global_unique".to_string()),
                resolution_confidence: Some(0.75),
            });
        db.replace_files_batch(&[unit]).unwrap();

        let edges = db
            .reads()
            .load_file_edges_for_reresolve("src/routes.ts")
            .unwrap();
        assert_eq!(edges.route_edges.len(), 1);
        let route = &edges.route_edges[0];
        assert_eq!(
            route.resolution_strategy.as_deref(),
            Some("route_ladder:global_unique")
        );
        assert_eq!(route.resolution_confidence, Some(0.75));
    }

    #[test]
    fn generation_starts_at_zero_and_index_writes_bump_index_epoch() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("gen.db")).unwrap().0;

        // Old databases (or fresh ones) without the epoch keys read as 0/0.
        assert_eq!(db.generation().unwrap(), IndexGeneration::default());

        db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
        let after_write = db.generation().unwrap();
        assert_eq!(after_write.index_epoch, 1);
        assert_eq!(after_write.evidence_epoch, 0);

        db.write_incremental_batch(
            &["src/a.rs".to_string()],
            &[],
            &[],
            &[],
            &[],
            &PrecompressedChunks::new(),
        )
        .unwrap();
        let after_batch = db.generation().unwrap();
        assert!(after_batch.index_epoch > after_write.index_epoch);
        assert_eq!(after_batch.evidence_epoch, 0);

        // An empty incremental batch writes nothing and must not bump.
        db.write_incremental_batch(&[], &[], &[], &[], &[], &PrecompressedChunks::new())
            .unwrap();
        assert_eq!(db.generation().unwrap(), after_batch);
    }

    #[test]
    fn full_rebuild_advances_both_epochs_past_previous_values() {
        let tmp = TempDir::new().unwrap();
        // Production naming: rebuild_with_temp_db derives WAL/tmp paths from
        // the `.sqlite3` extension, so the test must use it too.
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;

        db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
        db.upsert_runtime_evidence(
            "ev1",
            "svc",
            Some("GET"),
            "/x",
            None,
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
        let before = db.generation().unwrap();
        assert!(before.index_epoch >= 1 && before.evidence_epoch >= 1);

        db.rebuild_with_temp_db(|_conn| Ok(())).unwrap();
        let after = db.generation().unwrap();
        assert!(after.index_epoch > before.index_epoch);
        assert!(after.evidence_epoch > before.evidence_epoch);
    }

    #[test]
    fn rebuild_generation_exceeds_writes_committed_during_rebuild() {
        use std::sync::mpsc;
        use std::sync::Arc;

        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0);
        db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();

        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let rebuild_db = Arc::clone(&db);
        let rebuild = std::thread::spawn(move || {
            rebuild_db.rebuild_with_temp_db(move |_conn| {
                started_tx.send(()).unwrap();
                // Park mid-rebuild so the main thread can commit writes that
                // advance the live epochs past the floor snapshot.
                go_rx.recv().unwrap();
                Ok(())
            })
        });

        started_rx.recv().unwrap();
        db.replace_files_batch(&[file_unit("src/b.rs")]).unwrap();
        db.upsert_runtime_evidence(
            "ev-mid",
            "svc",
            Some("GET"),
            "/y",
            None,
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
        let live_during_rebuild = db.generation().unwrap();
        go_tx.send(()).unwrap();
        rebuild.join().unwrap().unwrap();

        // The swapped-in database must exceed the epochs observed for the
        // concurrent writes, not just the floor taken when the rebuild began.
        let after = db.generation().unwrap();
        assert!(after.index_epoch > live_during_rebuild.index_epoch);
        assert!(after.evidence_epoch > live_during_rebuild.evidence_epoch);
    }

    #[test]
    fn schema_mismatch_rebuild_advances_generation_past_old_values() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("index.sqlite3");
        let old_generation;
        {
            let db = IndexDb::open(&path).unwrap().0;
            db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
            db.upsert_runtime_evidence(
                "ev1",
                "svc",
                Some("GET"),
                "/x",
                None,
                "2024-01-01T00:00:00Z",
            )
            .unwrap();
            old_generation = db.generation().unwrap();
            assert!(old_generation.index_epoch >= 1 && old_generation.evidence_epoch >= 1);
            // Force a schema mismatch for the next open.
            let conn = db.write_conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 1u32).unwrap();
        }

        let (db, status) = IndexDb::open(&path).unwrap();
        assert!(matches!(status, SchemaStatus::Initialized));
        let rebuilt = db.generation().unwrap();
        assert!(
            rebuilt.index_epoch > old_generation.index_epoch,
            "mismatch rebuild must not roll index_epoch back ({} <= {})",
            rebuilt.index_epoch,
            old_generation.index_epoch
        );
        assert!(rebuilt.evidence_epoch > old_generation.evidence_epoch);
    }

    // Stress repro for the pool-drop vs file-delete race fixed by in-place
    // schema reset; run with --ignored when touching the mismatch-rebuild path.
    #[test]
    #[ignore]
    fn mismatch_rebuild_stress_loop() {
        for iteration in 0..300 {
            let tmp = TempDir::new().unwrap();
            let path = tmp.path().join("index.sqlite3");
            {
                let db = IndexDb::open(&path).unwrap().0;
                db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
                let _ = db.generation().unwrap();
                let conn = db.write_conn.lock().unwrap();
                conn.pragma_update(None, "user_version", 1u32).unwrap();
            }
            let (db, _status) =
                IndexDb::open(&path).unwrap_or_else(|e| panic!("iter {iteration}: {e:?}"));
            let _ = db.generation().unwrap();
        }
    }

    #[test]
    fn empty_file_state() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        assert!(db.get_file_state().unwrap().is_empty());
    }

    #[test]
    fn chunk_text_encoding_reads_plain_and_zstd() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE chunks (chunk_id TEXT PRIMARY KEY, text TEXT NOT NULL, text_encoding TEXT NOT NULL);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks(chunk_id, text, text_encoding) VALUES('plain', ?1, 'plain')",
            ["hello plain"],
        )
        .unwrap();
        let compressed = zstd::encode_all(std::io::Cursor::new("hello compressed"), 3).unwrap();
        conn.execute(
            "INSERT INTO chunks(chunk_id, text, text_encoding) VALUES('zstd', ?1, 'zstd')",
            rusqlite::params![compressed],
        )
        .unwrap();

        let plain = conn
            .query_row(
                "SELECT text, text_encoding FROM chunks WHERE chunk_id='plain'",
                [],
                |row| read_chunk_text_with_encoding(row, 0, 1),
            )
            .unwrap();
        let zstd = conn
            .query_row(
                "SELECT text, text_encoding FROM chunks WHERE chunk_id='zstd'",
                [],
                |row| read_chunk_text_with_encoding(row, 0, 1),
            )
            .unwrap();

        assert_eq!(plain, "hello plain");
        assert_eq!(zstd, "hello compressed");
    }

    #[test]
    fn resolver_seed_symbols_excludes_requested_files() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("resolver_seed.db"))
            .unwrap()
            .0;

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
                 VALUES('src/lib.rs','Rust','h1',1.0,1,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
                 VALUES('src/main.rs','Rust','h2',1.0,1,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params!["sym_keep", "src/lib.rs", "helper", "function", "crate.lib.helper", "helper", "uid_keep"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params!["sym_skip", "src/main.rs", "main", "function", "crate.main.main", "main", "uid_skip"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let rows = db
            .resolver_seed_symbols_excluding(&["src/main.rs".to_string()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_path, "src/lib.rs");
        assert_eq!(rows[0].symbol_uid.as_deref(), Some("uid_keep"));
    }

    /// End-to-end test: HTTP call edges → route nodes cross-service chain.
    ///
    /// Verifies that:
    /// 1. http_calls_by_caller_uid returns the correct outbound call
    /// 2. route_nodes_by_normalized_path_and_method resolves the handler
    /// 3. http_callers_by_normalized_path_and_method returns the reverse lookup
    #[test]
    fn http_call_cross_service_chain() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("http_chain.db")).unwrap().0;

        // Insert test data via raw SQL
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();

            // Insert prerequisite files rows for FK constraints
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES('src/client.ts', 'typescript', 'hash1', 1000, 100, '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES('src/server/users.ts', 'typescript', 'hash2', 1000, 200, '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();

            // Insert an outbound HTTP call edge: caller_fn_uid calls GET /api/users
            tx.execute(
                "INSERT INTO http_call_edges(edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier)
                 VALUES('hce_1', 'src/client.ts', 'caller_fn_uid', '/api/users', '/api/users', 'GET', 'http', 42, 0.9, 'tree_sitter')",
                [],
            ).unwrap();

            // Insert a route node: GET /api/users → handler_fn_uid
            tx.execute(
                "INSERT INTO routes(edge_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, normalized_path, confidence, parser_tier, route_id)
                 VALUES('rn_1', 'src/server/users.ts', '/api/users', 'GET', 'handler_fn_uid', 'getUsers', 'express', 10, 25, '/api/users', 0.85, 'tree_sitter', 'rn_1')",
                [],
            ).unwrap();

            tx.commit().unwrap();
        }

        // 1. Forward: caller_fn_uid → outbound HTTP calls
        let calls = db.http_calls_by_caller_uid("caller_fn_uid", 10).unwrap();
        assert_eq!(calls.len(), 1, "should find 1 outbound HTTP call");
        assert_eq!(calls[0].normalized_path.as_deref(), Some("/api/users"));
        assert_eq!(calls[0].method.as_deref(), Some("GET"));
        assert_eq!(calls[0].caller_symbol_uid.as_deref(), Some("caller_fn_uid"));

        // 2. Resolve: normalized path → route handler
        let routes = db
            .route_nodes_by_normalized_path_and_method("/api/users", Some("GET"), 10)
            .unwrap();
        assert_eq!(routes.len(), 1, "should find 1 matching route node");
        assert_eq!(
            routes[0].handler_symbol_uid.as_deref(),
            Some("handler_fn_uid")
        );
        assert_eq!(routes[0].handler_name.as_deref(), Some("getUsers"));
        assert_eq!(routes[0].file_path, "src/server/users.ts");

        // 3. Reverse: who calls /api/users via HTTP?
        let callers = db
            .http_callers_by_normalized_path_and_method("/api/users", Some("GET"), 10)
            .unwrap();
        assert_eq!(callers.len(), 1, "should find 1 HTTP caller");
        assert_eq!(
            callers[0].caller_symbol_uid.as_deref(),
            Some("caller_fn_uid")
        );
        assert_eq!(callers[0].file_path, "src/client.ts");

        // 4. Negative case: non-existent path returns empty
        let empty = db.http_calls_by_caller_uid("nonexistent_uid", 10).unwrap();
        assert!(empty.is_empty(), "non-existent caller should return empty");

        let empty_routes = db
            .route_nodes_by_normalized_path_and_method("/api/nonexistent", Some("GET"), 10)
            .unwrap();
        assert!(
            empty_routes.is_empty(),
            "non-existent route should return empty"
        );
    }

    // --- Security: SQL injection regression tests ---

    #[test]
    fn test_find_symbol_safe_with_injection_string() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("injection_test.db"))
            .unwrap()
            .0;
        // Query with a classic SQL injection payload — should return empty, not panic or corrupt.
        let result = db.find_symbol("'; DROP TABLE symbols; --", true, 10);
        assert!(result.is_ok(), "injection string should not cause error");
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_find_symbol_safe_with_union_injection() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("injection_union.db"))
            .unwrap()
            .0;
        let result = db.find_symbol("' UNION SELECT * FROM sqlite_master --", false, 10);
        assert!(result.is_ok(), "UNION injection should not cause error");
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_find_symbol_safe_with_null_byte() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("injection_null.db"))
            .unwrap()
            .0;
        let result = db.find_symbol("test\0evil", true, 10);
        assert!(result.is_ok(), "null byte should not cause error");
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_find_symbol_safe_with_unicode_injection() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("injection_unicode.db"))
            .unwrap()
            .0;
        let result = db.find_symbol("name\u{200B}; DROP TABLE symbols", true, 10);
        assert!(result.is_ok(), "unicode injection should not cause error");
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_metadata_safe_with_injection_key() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("injection_meta.db"))
            .unwrap()
            .0;
        // Setting metadata with injection key — should work safely via parameterized queries.
        let set_result = db.set_metadata("'; DROP TABLE metadata; --", "value");
        assert!(
            set_result.is_ok(),
            "injection in metadata key should be safe"
        );
        let get_result = db.get_metadata("'; DROP TABLE metadata; --");
        assert!(get_result.is_ok());
        assert_eq!(get_result.unwrap(), Some("value".to_string()));
        // Confirm metadata table still exists by doing another operation.
        let get_normal = db.get_metadata("normal_key");
        assert!(get_normal.is_ok(), "metadata table should still exist");
    }
}
