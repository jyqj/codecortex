//! IndexDatabase — the index.sqlite3 connection manager.
//!
//! Read: pool of connections (one per query, no manual refresh needed).
//! Write: single Mutex<Connection> for exclusive writes.
//! FTS sync: application-layer, in the same transaction as base table writes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
}

impl IndexDb {
    /// Open (or create) the index database at the given path using the default
    /// read pool size.
    /// If the schema version doesn't match, the database file is deleted and recreated.
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
            },
            schema_status,
        ))
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
        let pragmas = "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;";

        let conn = Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute_batch(pragmas)
            .map_err(|e| CcError::Database(e.to_string()))?;

        match migrate_index_db(&conn)? {
            status @ (SchemaStatus::UpToDate | SchemaStatus::Initialized) => Ok((conn, status)),
            SchemaStatus::Mismatch { stored } => {
                tracing::warn!(
                    stored_version = stored,
                    "deleting index database for schema rebuild"
                );
                // Export persistent assets before destroying the database.
                let preserved = Self::export_persistent_assets(&conn);
                drop(conn);

                let _ = std::fs::remove_file(path);
                let wal = path.with_extension("sqlite3-wal");
                let shm = path.with_extension("sqlite3-shm");
                let _ = std::fs::remove_file(&wal);
                let _ = std::fs::remove_file(&shm);

                let conn = Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
                conn.execute_batch(pragmas)
                    .map_err(|e| CcError::Database(e.to_string()))?;
                migrate_index_db(&conn)?;

                // Re-import preserved assets into the fresh database.
                if let Ok(assets) = preserved {
                    Self::import_persistent_assets(&conn, &assets)?;
                }
                // After mismatch rebuild the DB is empty — report as Initialized
                // so callers know an index build is needed.
                Ok((conn, SchemaStatus::Initialized))
            }
        }
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

    /// Get a read connection from the pool.
    pub fn read_conn(&self) -> CcResult<r2d2::PooledConnection<SqliteConnectionManager>> {
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
    pub fn checkpoint_wal(&self) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| CcError::Database(format!("wal_checkpoint: {}", e)))?;
        Ok(())
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

    // ── Full rebuild with temp-db + atomic swap ─────────────────

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
    pub fn rebuild_with_temp_db<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("sqlite3.tmp");

        // Clean up any stale temp file from a previous crashed run.
        let _ = std::fs::remove_file(&tmp_path);
        let _ = std::fs::remove_file(tmp_path.with_extension("tmp-wal"));
        let _ = std::fs::remove_file(tmp_path.with_extension("tmp-shm"));

        tracing::info!(
            tmp = %tmp_path.display(),
            "full rebuild: creating temp database"
        );

        // 1. Open temp db and apply schema
        let tmp_conn = Connection::open(&tmp_path)
            .map_err(|e| CcError::Database(format!("open temp db: {}", e)))?;
        tmp_conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| CcError::Database(format!("temp db pragmas: {}", e)))?;

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
                let _ = std::fs::remove_file(&tmp_path);
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

        // 7. Acquire write lock, do atomic swap while lock is held
        tracing::info!("full rebuild: swapping temp database into place");
        {
            // Lock the write connection to prevent concurrent writes.
            // The rename MUST happen inside this scope so no writer can
            // slip in between lock-release and file replacement.
            let _write_guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;

            // Remove the old WAL/SHM files — the new file will create its own
            let wal = self.db_path.with_extension("sqlite3-wal");
            let shm = self.db_path.with_extension("sqlite3-shm");
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);

            // Atomic rename: temp → main (inside write lock)
            std::fs::rename(&tmp_path, &self.db_path).map_err(|e| {
                CcError::Database(format!(
                    "atomic rename {} → {}: {}",
                    tmp_path.display(),
                    self.db_path.display(),
                    e
                ))
            })?;

            // Clean up temp WAL/SHM if any
            let tmp_wal = tmp_path.with_extension("tmp-wal");
            let tmp_shm = tmp_path.with_extension("tmp-shm");
            let _ = std::fs::remove_file(&tmp_wal);
            let _ = std::fs::remove_file(&tmp_shm);
        }

        // 8. Reopen write connection
        let (new_write_conn, _status) = Self::open_and_ensure_schema(&self.db_path)?;
        {
            let mut guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            *guard = new_write_conn;
        }

        // 9. Rebuild the read pool using the configured/adaptive pool size.
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
            tracing::warn!(err = %e, "full rebuild: WAL checkpoint failed (non-fatal)");
        }

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
    /// Enable via `IndexingConfig::use_direct_writer == true`.
    pub fn rebuild_with_direct_writer<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("direct-tmp.sqlite3");

        // Clean up stale temp file from a previous crashed run.
        let _ = std::fs::remove_file(&tmp_path);
        let _ = std::fs::remove_file(tmp_path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(tmp_path.with_extension("sqlite3-shm"));

        tracing::info!(
            tmp = %tmp_path.display(),
            "direct writer: creating high-speed temp database"
        );

        crate::direct_writer::DirectWriter::write_db(&tmp_path, FULL_SCHEMA_SQL, |tx| {
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

        // Atomic swap — same logic as rebuild_with_temp_db
        {
            let _write_guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;

            // Remove old WAL/SHM files
            let wal = self.db_path.with_extension("sqlite3-wal");
            let shm = self.db_path.with_extension("sqlite3-shm");
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);

            // Atomic rename: temp -> main
            std::fs::rename(&tmp_path, &self.db_path).map_err(|e| {
                CcError::Database(format!(
                    "atomic rename {} -> {}: {}",
                    tmp_path.display(),
                    self.db_path.display(),
                    e
                ))
            })?;

            // Clean up temp WAL/SHM if any
            let tmp_wal = tmp_path.with_extension("sqlite3-wal");
            let tmp_shm = tmp_path.with_extension("sqlite3-shm");
            let _ = std::fs::remove_file(&tmp_wal);
            let _ = std::fs::remove_file(&tmp_shm);
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
            tracing::warn!(err = %e, "direct writer: WAL checkpoint failed (non-fatal)");
        }

        tracing::info!("direct writer: swap complete");
        Ok(())
    }

    // ── File state ───────────────────────────────────────────────

    pub fn get_file_state(&self) -> CcResult<HashMap<String, FileState>> {
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

    pub fn replace_files_batch(&self, files: &[FileWriteUnit]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for file in files {
            Self::delete_file_data(&tx, &file.rel_path)?;
            Self::insert_file_data(&tx, file)?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Update only the edge/resolution data for dirty (DirtyResolveOnly) files.
    /// Does NOT delete or modify: files row, chunks, FTS, route_nodes,
    /// http_call_edges, data_flow_edges, literals, file_frameworks,
    /// co_change_edges, test_edges.
    /// Only replaces: symbols, imports, call_edges, symbol_refs, semantic_edges,
    /// dispatch_sites, route_edges.
    pub fn replace_reresolved_edges_only(&self, units: &[FileWriteUnit]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for file in units {
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
                    &tx,
                    &format!("DELETE FROM {} WHERE file_path = ?1", table),
                    rusqlite::params![rel],
                )?;
            }

            // Re-insert symbols
            for s in &outcome.symbols {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                    rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
                )?;
            }

            // Re-insert imports
            for i in &outcome.imports {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![i.file_path, i.import_string, i.resolved_path, i.imported_name, i.alias, i.is_namespace as i32, i.is_default as i32, i.is_reexport as i32],
                )?;
            }

            // Re-insert symbol_refs
            for r in &outcome.symbol_refs {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
                )?;
            }

            // Re-insert call_edges
            for e in &outcome.call_edges {
                Self::execute_cached(
                    &tx,
                    "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                    rusqlite::params![e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol, e.line, e.start_col, e.end_line, e.end_col, e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id, e.caller_symbol_uid, e.callee_symbol_uid, e.dispatch_kind.as_str(), e.call_kind, e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy, e.receiver_expr, e.arg_count.map(|v| v as i32), e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32, e.parser_tier.as_str(), e.parser_confidence, e.synthesized_by, e.synthesis_key, e.registered_file, e.registered_line.map(|v| v as i32)],
                )?;
            }

            // Re-insert semantic_edges
            for se in &outcome.semantic_edges {
                Self::execute_cached(
                    &tx,
                    "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![se.edge_id, se.file_path, se.source_symbol, se.source_symbol_uid, se.target_symbol, se.target_symbol_uid, se.relation_kind.as_str(), se.line, se.confidence, se.parser_tier.as_str()],
                )?;
            }

            // Re-insert dispatch_sites
            for ds in &outcome.dispatch_sites {
                Self::execute_cached(
                    &tx,
                    "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
                )?;
            }

            // Re-insert route_edges
            for r in &outcome.route_edges {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                    rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str()],
                )?;
            }
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_files_batch(&self, paths: &[String]) -> CcResult<usize> {
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
        for path in paths {
            Self::delete_file_data(&tx, path)?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(paths.len())
    }

    pub(crate) fn delete_file_data(conn: &Connection, rel_path: &str) -> CcResult<()> {
        for fts_table in &["chunks_fts", "files_fts", "literal_fts"] {
            conn.execute(
                &format!("DELETE FROM {} WHERE file_path = ?1", fts_table),
                rusqlite::params![rel_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        conn.execute(
            "DELETE FROM test_edges WHERE test_file_path = ?1 OR code_file_path = ?1",
            rusqlite::params![rel_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        // Delete file-scoped frameworks
        conn.execute(
            "DELETE FROM frameworks WHERE scope='file' AND scope_id = ?1",
            rusqlite::params![rel_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        for table in &[
            "routes",
            "data_flow_edges",
            "http_call_edges",
            "semantic_edges",
            "dispatch_sites",
        ] {
            conn.execute(
                &format!("DELETE FROM {} WHERE file_path = ?1", table),
                rusqlite::params![rel_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        conn.execute(
            "DELETE FROM co_change_edges WHERE file_a = ?1 OR file_b = ?1",
            rusqlite::params![rel_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM files WHERE file_path = ?1",
            rusqlite::params![rel_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Insert a single file's data into the given connection.
    /// Accepts `&Connection` so it works with both `Transaction` (via Deref)
    /// and bare connections (e.g. inside `rebuild_with_temp_db`).
    pub fn insert_file_data(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
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

        // files + files_fts
        Self::execute_cached(
            conn,
            "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![file.rel_path, file.language.as_str(), file.content_hash, file.mtime, file.size as i64, outcome.summary, excerpt, outcome.parser_tier.as_str(), outcome.parser_confidence, outcome.is_test_file as i32, now],
        )?;
        Self::execute_cached(
            conn,
            "INSERT INTO files_fts(file_path,summary,content_excerpt) VALUES(?1,?2,?3)",
            rusqlite::params![file.rel_path, outcome.summary, excerpt],
        )?;

        // chunks + chunks_fts
        for c in &outcome.chunks {
            // Compress chunk text with zstd when it saves space
            let text_bytes = c.text.as_bytes();
            let use_compressed = if text_bytes.len() > 128 {
                match zstd::encode_all(std::io::Cursor::new(text_bytes), 3) {
                    Ok(compressed) if compressed.len() < text_bytes.len() => Some(compressed),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(ref blob) = use_compressed {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), blob.as_slice(), "zstd", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            } else {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), c.text, "plain", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            }
            // FTS always receives uncompressed text
            Self::execute_cached(
                conn,
                "INSERT INTO chunks_fts(chunk_id,file_path,breadcrumb,symbol_name,text) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![c.chunk_id, c.file_path, c.breadcrumb, c.symbol_name, c.text],
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
        for r in &outcome.symbol_refs {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
            )?;
        }

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
            Self::execute_cached(conn, "INSERT OR REPLACE INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str()],
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

        // literal_index + literal_fts
        for l in &outcome.literal_index {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence, l.enclosing_symbol_uid, l.key_path],
            )?;
            Self::execute_cached(conn, "INSERT INTO literal_fts(literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4)", rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind])?;
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

    pub fn get_metadata(&self, key: &str) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        Ok(conn
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                rusqlite::params![key],
                |r| r.get::<_, String>(0),
            )
            .ok())
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", rusqlite::params![key, value])
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Stats ────────────────────────────────────────────────────

    pub fn stats(&self, project_path: &Path) -> CcResult<ProjectStats> {
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
