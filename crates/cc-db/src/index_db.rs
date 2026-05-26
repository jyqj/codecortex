//! IndexDatabase — the index.sqlite3 connection manager.
//!
//! Read: pool of connections (one per query, no manual refresh needed).
//! Write: single Mutex<Connection> for exclusive writes.
//! FTS sync: application-layer, in the same transaction as base table writes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use tracing::warn;

use cc_model::config::ProjectStats;
use cc_model::edge::{HttpCallEdgeRecord, RouteNodeRecord};
use cc_model::parse::ParseOutcome;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcError, CcResult, Language, ParserTier};
use serde_json::Value;

use crate::index_migrate::{
    migrate_index_db, SchemaStatus, CURRENT_SCHEMA_VERSION, FULL_SCHEMA_SQL,
};

/// `(name, kind, file_path, fan_in, fan_out)` for hotspot symbol queries.
type HotspotRow = (String, String, String, usize, usize);

/// Read a chunk text column that may be stored as zstd-compressed BLOB or plain TEXT.
///
/// Tries String first (uncompressed); on failure reads as BLOB and attempts
/// zstd decompression, falling back to raw UTF-8 interpretation.
pub fn read_chunk_text(row: &rusqlite::Row, col_idx: usize) -> rusqlite::Result<String> {
    match row.get::<_, String>(col_idx) {
        Ok(s) => Ok(s),
        Err(_) => {
            let blob: Vec<u8> = row.get(col_idx)?;
            match zstd::decode_all(blob.as_slice()) {
                Ok(decompressed) => Ok(String::from_utf8(decompressed).unwrap_or_default()),
                Err(_) => Ok(String::from_utf8(blob).unwrap_or_default()),
            }
        }
    }
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct DataFlowEdgeLite {
    pub file_path: String,
    pub line: u32,
    pub flow_kind: String,
    pub source_symbol_uid: Option<String>,
    pub target_symbol_uid: Option<String>,
    pub confidence: f64,
}

/// The index database handle.
pub struct IndexDb {
    db_path: PathBuf,
    pool: RwLock<Pool<SqliteConnectionManager>>,
    write_conn: Mutex<Connection>,
}

impl IndexDb {
    /// Open (or create) the index database at the given path.
    /// If the schema version doesn't match, the database file is deleted and recreated.
    pub fn open(path: &Path) -> CcResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let write_conn = Self::open_and_ensure_schema(path)?;

        let manager = SqliteConnectionManager::file(path)
            .with_init(|conn| {
                conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
                Ok(())
            });
        let pool = Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| CcError::Database(e.to_string()))?;

        Ok(Self {
            db_path: path.to_path_buf(),
            pool: RwLock::new(pool),
            write_conn: Mutex::new(write_conn),
        })
    }

    /// Open the database, check schema version, and rebuild if mismatched.
    fn open_and_ensure_schema(path: &Path) -> CcResult<Connection> {
        let pragmas = "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;";

        let conn = Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute_batch(pragmas)
            .map_err(|e| CcError::Database(e.to_string()))?;

        match migrate_index_db(&conn)? {
            SchemaStatus::UpToDate | SchemaStatus::Initialized => Ok(conn),
            SchemaStatus::Mismatch { stored } => {
                tracing::warn!(
                    stored_version = stored,
                    "deleting index database for schema rebuild"
                );
                // Close the connection before deleting the file.
                drop(conn);
                // Remove the main db file and WAL/SHM sidecars.
                let _ = std::fs::remove_file(path);
                let wal = path.with_extension("sqlite3-wal");
                let shm = path.with_extension("sqlite3-shm");
                let _ = std::fs::remove_file(&wal);
                let _ = std::fs::remove_file(&shm);

                // Reopen and initialise fresh.
                let conn = Connection::open(path).map_err(|e| CcError::Database(e.to_string()))?;
                conn.execute_batch(pragmas)
                    .map_err(|e| CcError::Database(e.to_string()))?;
                migrate_index_db(&conn)?;
                Ok(conn)
            }
        }
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

    fn execute_cached<P: rusqlite::Params>(
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

    /// SQL statements to drop non-PK indexes (for bulk rebuild performance).
    /// FTS virtual tables are excluded — their indexes are managed internally.
    const DROP_INDEXES_SQL: &str = "\
        DROP INDEX IF EXISTS idx_chunks_file;
        DROP INDEX IF EXISTS idx_chunks_symbol;
        DROP INDEX IF EXISTS idx_symbols_name;
        DROP INDEX IF EXISTS idx_symbols_file;
        DROP INDEX IF EXISTS idx_symbols_qname;
        DROP INDEX IF EXISTS idx_symbols_uid;
        DROP INDEX IF EXISTS idx_imports_file;
        DROP INDEX IF EXISTS idx_imports_resolved;
        DROP INDEX IF EXISTS idx_refs_symbol;
        DROP INDEX IF EXISTS idx_refs_file;
        DROP INDEX IF EXISTS idx_refs_target;
        DROP INDEX IF EXISTS idx_refs_target_uid;
        DROP INDEX IF EXISTS idx_resolution_attempts_file;
        DROP INDEX IF EXISTS idx_resolution_attempts_name;
        DROP INDEX IF EXISTS idx_resolution_attempts_kind;
        DROP INDEX IF EXISTS idx_ce_caller;
        DROP INDEX IF EXISTS idx_ce_callee;
        DROP INDEX IF EXISTS idx_ce_file;
        DROP INDEX IF EXISTS idx_ce_caller_uid;
        DROP INDEX IF EXISTS idx_ce_callee_uid;
        DROP INDEX IF EXISTS idx_te_test;
        DROP INDEX IF EXISTS idx_te_code;
        DROP INDEX IF EXISTS idx_re_path;
        DROP INDEX IF EXISTS idx_re_handler;
        DROP INDEX IF EXISTS idx_re_file;
        DROP INDEX IF EXISTS idx_re_handler_uid;
        DROP INDEX IF EXISTS idx_diag_file;
        DROP INDEX IF EXISTS idx_literal_kind;
        DROP INDEX IF EXISTS idx_literal_file;
        DROP INDEX IF EXISTS idx_literal_symbol;
        DROP INDEX IF EXISTS idx_scopes_file;
        DROP INDEX IF EXISTS idx_scopes_parent;
        DROP INDEX IF EXISTS idx_scopes_owner;
        DROP INDEX IF EXISTS idx_rn_file;
        DROP INDEX IF EXISTS idx_rn_path;
        DROP INDEX IF EXISTS idx_rn_handler;
        DROP INDEX IF EXISTS idx_rn_norm_path;
        DROP INDEX IF EXISTS idx_dfe_file;
        DROP INDEX IF EXISTS idx_dfe_source;
        DROP INDEX IF EXISTS idx_dfe_target;
        DROP INDEX IF EXISTS idx_cce_file_a;
        DROP INDEX IF EXISTS idx_cce_file_b;
        DROP INDEX IF EXISTS idx_cce_confidence;
        DROP INDEX IF EXISTS idx_hce_file;
        DROP INDEX IF EXISTS idx_hce_caller;
        DROP INDEX IF EXISTS idx_hce_norm_path;
        DROP INDEX IF EXISTS idx_hce_kind;
        DROP INDEX IF EXISTS idx_infra_node_file;
        DROP INDEX IF EXISTS idx_infra_node_kind;
        DROP INDEX IF EXISTS idx_infra_node_name;
        DROP INDEX IF EXISTS idx_infra_edge_src;
        DROP INDEX IF EXISTS idx_infra_edge_dst;
        DROP INDEX IF EXISTS idx_infra_edge_kind;
        DROP INDEX IF EXISTS idx_se_file;
        DROP INDEX IF EXISTS idx_se_source;
        DROP INDEX IF EXISTS idx_se_target;
        DROP INDEX IF EXISTS idx_se_kind;
        DROP INDEX IF EXISTS idx_dispatch_sites_file;
        DROP INDEX IF EXISTS idx_dispatch_sites_kind_key;
    ";

    /// SQL statements to recreate non-PK indexes.
    /// Must match the index definitions in `index_v1.sql`.
    const CREATE_INDEXES_SQL: &str = "\
        CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path, chunk_index);
        CREATE INDEX IF NOT EXISTS idx_chunks_symbol ON chunks(symbol_name);
        CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
        CREATE INDEX IF NOT EXISTS idx_symbols_qname ON symbols(qname);
        CREATE INDEX IF NOT EXISTS idx_symbols_uid ON symbols(symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_path);
        CREATE INDEX IF NOT EXISTS idx_imports_resolved ON imports(resolved_path);
        CREATE INDEX IF NOT EXISTS idx_refs_symbol ON symbol_refs(symbol_name);
        CREATE INDEX IF NOT EXISTS idx_refs_file ON symbol_refs(file_path);
        CREATE INDEX IF NOT EXISTS idx_refs_target ON symbol_refs(target_file_path, target_symbol_id);
        CREATE INDEX IF NOT EXISTS idx_refs_target_uid ON symbol_refs(target_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_resolution_attempts_file ON resolution_attempts(file_path);
        CREATE INDEX IF NOT EXISTS idx_resolution_attempts_name ON resolution_attempts(reference_name);
        CREATE INDEX IF NOT EXISTS idx_resolution_attempts_kind ON resolution_attempts(reference_kind);
        CREATE INDEX IF NOT EXISTS idx_ce_caller ON call_edges(caller_symbol);
        CREATE INDEX IF NOT EXISTS idx_ce_callee ON call_edges(callee_symbol);
        CREATE INDEX IF NOT EXISTS idx_ce_file ON call_edges(file_path);
        CREATE INDEX IF NOT EXISTS idx_ce_caller_uid ON call_edges(caller_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_ce_callee_uid ON call_edges(callee_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_te_test ON test_edges(test_file_path);
        CREATE INDEX IF NOT EXISTS idx_te_code ON test_edges(code_file_path);
        CREATE INDEX IF NOT EXISTS idx_re_path ON route_edges(route_path);
        CREATE INDEX IF NOT EXISTS idx_re_handler ON route_edges(handler_name);
        CREATE INDEX IF NOT EXISTS idx_re_file ON route_edges(file_path);
        CREATE INDEX IF NOT EXISTS idx_re_handler_uid ON route_edges(handler_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_diag_file ON diagnostics(file_path);
        CREATE INDEX IF NOT EXISTS idx_literal_kind ON literal_index(literal_kind);
        CREATE INDEX IF NOT EXISTS idx_literal_file ON literal_index(file_path);
        CREATE INDEX IF NOT EXISTS idx_literal_symbol ON literal_index(enclosing_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_scopes_file ON scopes(file_path);
        CREATE INDEX IF NOT EXISTS idx_scopes_parent ON scopes(parent_scope_id);
        CREATE INDEX IF NOT EXISTS idx_scopes_owner ON scopes(owner_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_rn_file ON route_nodes(file_path);
        CREATE INDEX IF NOT EXISTS idx_rn_path ON route_nodes(route_path);
        CREATE INDEX IF NOT EXISTS idx_rn_handler ON route_nodes(handler_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_rn_norm_path ON route_nodes(normalized_path);
        CREATE INDEX IF NOT EXISTS idx_dfe_file ON data_flow_edges(file_path);
        CREATE INDEX IF NOT EXISTS idx_dfe_source ON data_flow_edges(source_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_dfe_target ON data_flow_edges(target_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_cce_file_a ON co_change_edges(file_a);
        CREATE INDEX IF NOT EXISTS idx_cce_file_b ON co_change_edges(file_b);
        CREATE INDEX IF NOT EXISTS idx_cce_confidence ON co_change_edges(confidence);
        CREATE INDEX IF NOT EXISTS idx_hce_file ON http_call_edges(file_path);
        CREATE INDEX IF NOT EXISTS idx_hce_caller ON http_call_edges(caller_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_hce_norm_path ON http_call_edges(normalized_path);
        CREATE INDEX IF NOT EXISTS idx_hce_kind ON http_call_edges(call_kind);
        CREATE INDEX IF NOT EXISTS idx_infra_node_file ON infra_nodes(file_path);
        CREATE INDEX IF NOT EXISTS idx_infra_node_kind ON infra_nodes(kind);
        CREATE INDEX IF NOT EXISTS idx_infra_node_name ON infra_nodes(name);
        CREATE INDEX IF NOT EXISTS idx_infra_edge_src ON infra_edges(source_node_id);
        CREATE INDEX IF NOT EXISTS idx_infra_edge_dst ON infra_edges(target_node_id);
        CREATE INDEX IF NOT EXISTS idx_infra_edge_kind ON infra_edges(kind);
        CREATE INDEX IF NOT EXISTS idx_se_file ON semantic_edges(file_path);
        CREATE INDEX IF NOT EXISTS idx_se_source ON semantic_edges(source_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_se_target ON semantic_edges(target_symbol_uid);
        CREATE INDEX IF NOT EXISTS idx_se_kind ON semantic_edges(relation_kind);
        CREATE INDEX IF NOT EXISTS idx_dispatch_sites_file ON dispatch_sites(file_path);
        CREATE INDEX IF NOT EXISTS idx_dispatch_sites_kind_key ON dispatch_sites(site_kind, key);
    ";

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

        // 3. Drop non-PK indexes for faster bulk insert
        tmp_conn
            .execute_batch(Self::DROP_INDEXES_SQL)
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

        // 5. Recreate indexes
        tracing::info!("full rebuild: recreating indexes");
        tmp_conn
            .execute_batch(Self::CREATE_INDEXES_SQL)
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
        let new_write_conn = Self::open_and_ensure_schema(&self.db_path)?;
        {
            let mut guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            *guard = new_write_conn;
        }

        // 9. Rebuild the read pool
        let manager = SqliteConnectionManager::file(&self.db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
            )?;
            Ok(())
        });
        let new_pool = Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| CcError::Database(e.to_string()))?;
        {
            let mut pool_guard = self
                .pool
                .write()
                .map_err(|e| CcError::Database(format!("write pool lock: {}", e)))?;
            *pool_guard = new_pool;
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
        let new_write_conn = Self::open_and_ensure_schema(&self.db_path)?;
        {
            let mut guard = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            *guard = new_write_conn;
        }

        // Rebuild the read pool
        let manager = SqliteConnectionManager::file(&self.db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
            )?;
            Ok(())
        });
        let new_pool = Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| CcError::Database(e.to_string()))?;
        {
            let mut pool_guard = self
                .pool
                .write()
                .map_err(|e| CcError::Database(format!("write pool lock: {}", e)))?;
            *pool_guard = new_pool;
        }

        tracing::info!("direct writer: swap complete");
        Ok(())
    }

    /// Full rebuild using `replace_files_batch` semantics but with bulk
    /// optimizations applied (pragmas + index drop/recreate).
    ///
    /// This is a simpler alternative to `rebuild_with_temp_db` that operates
    /// in-place on the existing database. Suitable when you don't need
    /// atomic swap semantics.
    pub fn replace_files_batch_bulk(&self, files: &[FileWriteUnit]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;

        tracing::info!(
            file_count = files.len(),
            "bulk rebuild: applying aggressive pragmas"
        );

        // Apply bulk pragmas
        Self::set_bulk_rebuild_pragmas(&conn)?;

        // Drop indexes for faster bulk insert
        conn.execute_batch(Self::DROP_INDEXES_SQL)
            .map_err(|e| CcError::Database(format!("drop indexes: {}", e)))?;

        // Write all data in a single transaction
        let write_result = (|| -> CcResult<()> {
            let tx = conn
                .transaction()
                .map_err(|e| CcError::Database(e.to_string()))?;
            for file in files {
                Self::delete_file_data(&tx, &file.rel_path)?;
                Self::insert_file_data(&tx, file)?;
            }
            tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
            Ok(())
        })();

        // Always recreate indexes and restore pragmas, even on error
        let idx_result = conn
            .execute_batch(Self::CREATE_INDEXES_SQL)
            .map_err(|e| CcError::Database(format!("recreate indexes: {}", e)));

        let pragma_result = Self::restore_normal_pragmas(&conn);

        tracing::info!("bulk rebuild: pragmas restored, indexes recreated");

        // Return the first error if any
        write_result?;
        idx_result?;
        pragma_result?;

        Ok(())
    }

    // ── File state ───────────────────────────────────────────────

    pub fn get_file_state(&self) -> CcResult<HashMap<String, (String, f64)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path, content_hash, mtime FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get::<_, String>(1)?, row.get::<_, f64>(2)?),
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
    /// http_call_edges, data_flow_edges, literals, scopes, file_frameworks,
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
                "route_edges",
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
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
                    rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.scope_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
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
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,scope_id,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.scope_id, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
                )?;
            }

            // Re-insert call_edges
            for e in &outcome.call_edges {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
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
                    "INSERT INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
                )?;
            }

            // Re-insert route_edges
            for r in &outcome.route_edges {
                Self::execute_cached(
                    &tx,
                    "INSERT INTO route_edges(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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

    fn delete_file_data(conn: &Connection, rel_path: &str) -> CcResult<()> {
        for fts_table in &["chunks_fts", "files_fts", "diagnostics_fts", "literal_fts"] {
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
        for table in &[
            "file_frameworks",
            "route_nodes",
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
            let emb: Option<Vec<u8>> = if c.embedding.is_empty() {
                None
            } else {
                Some(c.embedding.iter().flat_map(|f| f.to_le_bytes()).collect())
            };
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
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,embedding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), blob.as_slice(), emb, c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            } else {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,embedding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), c.text, emb, c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
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
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
                rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.scope_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
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
            Self::execute_cached(conn, "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,scope_id,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.scope_id, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
            )?;
        }

        // call_edges
        for e in &outcome.call_edges {
            Self::execute_cached(conn, "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
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
            Self::execute_cached(conn, "INSERT INTO route_edges(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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

        // diagnostics + diagnostics_fts
        for d in &outcome.diagnostics {
            Self::execute_cached(conn, "INSERT INTO diagnostics(diagnostic_id,file_path,severity,message,line,end_line,source,code,confidence,symbol_uid) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![d.diagnostic_id, d.file_path, d.severity, d.message, d.line, d.end_line, d.source, d.code, d.confidence, d.symbol_uid],
            )?;
            Self::execute_cached(
                conn,
                "INSERT INTO diagnostics_fts(diagnostic_id,file_path,message) VALUES(?1,?2,?3)",
                rusqlite::params![d.diagnostic_id, d.file_path, d.message],
            )?;
        }

        // literal_index + literal_fts
        for l in &outcome.literal_index {
            Self::execute_cached(conn, "INSERT INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence, l.enclosing_symbol_uid, l.key_path],
            )?;
            Self::execute_cached(conn, "INSERT INTO literal_fts(literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4)", rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind])?;
        }

        // scopes
        for sc in &outcome.scopes {
            let bj = serde_json::to_string(&sc.bindings).unwrap_or_else(|_| "[]".into());
            Self::execute_cached(conn, "INSERT INTO scopes(scope_id,file_path,kind,name,parent_scope_id,owner_symbol_uid,start_line,start_col,end_line,end_col,bindings_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![sc.scope_id, sc.file_path, sc.kind, sc.name, sc.parent_scope_id, sc.owner_symbol_uid, sc.start_line, sc.start_col, sc.end_line, sc.end_col, bj],
            )?;
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
                "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![dfe.edge_id, dfe.file_path, dfe.source_symbol_uid, dfe.target_symbol_uid, dfe.flow_kind, dfe.line, dfe.confidence, dfe.parser_tier.as_str()],
            )?;
        }

        // dispatch_sites
        for ds in &outcome.dispatch_sites {
            Self::execute_cached(
                conn,
                "INSERT INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
            )?;
        }

        Ok(())
    }

    /// Insert a single route node into the given connection.
    pub fn insert_route_node_into(conn: &Connection, r: &RouteNodeRecord) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT OR REPLACE INTO route_nodes(route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
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
            indexed_route_edges: count("route_edges"),
            indexed_literals: count("literal_index"),
            indexed_diagnostics: count("diagnostics"),
            last_indexed_at: self.get_metadata("last_indexed_at")?,
            index_version: self.get_metadata("index_version")?,
        })
    }

    pub fn symbols_covering(
        &self,
        file_path: &str,
        line: u32,
        limit: usize,
    ) -> CcResult<Vec<SymbolCoverRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, file_path, start_line, end_line
                 FROM symbols
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2
                 ORDER BY (end_line - start_line) ASC
                 LIMIT ?3",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path, line, limit as i64], |row| {
                Ok(SymbolCoverRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    file_path: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn caller_rows_by_uid(
        &self,
        callee_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE callee_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![callee_uid, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn callee_rows_by_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE caller_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn symbol_ref_rows_by_uid(
        &self,
        target_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<SymbolRefLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, symbol_name, target_symbol_uid, resolution_kind, resolution_confidence
                 FROM symbol_refs
                 WHERE target_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![target_uid, limit as i64], |row| {
                Ok(SymbolRefLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    symbol_name: row.get(2)?,
                    target_symbol_uid: row.get(3)?,
                    resolution_kind: row.get(4)?,
                    confidence: row.get(5)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Rebuild the unresolved-reference feedback table from current refs/calls.
    ///
    /// This intentionally derives from `symbol_refs` and `call_edges` instead of
    /// being parser-owned: the resolver is the source of truth for what failed,
    /// and this table is a compact backlog for improving resolver quality.
    pub fn rebuild_resolution_attempts(&self) -> CcResult<usize> {
        #[derive(Debug)]
        struct Seed {
            source_table: String,
            source_id: String,
            file_path: String,
            reference_name: String,
            reference_kind: String,
            line: u32,
            column_no: u32,
            container: Option<String>,
            resolution_strategy: String,
            parser_tier: String,
            parser_confidence: f64,
            language: Option<String>,
        }

        let read = self.read_conn()?;
        let mut seeds = Vec::new();

        {
            let mut stmt = read
                .prepare(
                    "SELECT sr.ref_id, sr.file_path, sr.symbol_name, sr.ref_kind, sr.line, sr.column_no, sr.container, sr.resolution_strategy, sr.parser_tier, sr.parser_confidence, f.language
                     FROM symbol_refs sr
                     LEFT JOIN files f ON f.file_path = sr.file_path
                     WHERE sr.resolution_kind = 'unresolved' OR sr.target_symbol_uid IS NULL
                     LIMIT 20000",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Seed {
                        source_table: "symbol_refs".to_string(),
                        source_id: row.get(0)?,
                        file_path: row.get(1)?,
                        reference_name: row.get(2)?,
                        reference_kind: row.get(3)?,
                        line: row.get(4)?,
                        column_no: row.get(5)?,
                        container: row.get(6)?,
                        resolution_strategy: row.get(7)?,
                        parser_tier: row.get(8)?,
                        parser_confidence: row.get(9)?,
                        language: row.get(10)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            seeds.extend(rows.filter_map(|r| r.ok()));
        }

        {
            let mut stmt = read
                .prepare(
                    "SELECT ce.edge_id, ce.file_path, ce.callee_symbol, 'call', ce.line, ce.start_col, ce.caller_symbol, ce.resolution_strategy, ce.parser_tier, ce.parser_confidence, f.language
                     FROM call_edges ce
                     LEFT JOIN files f ON f.file_path = ce.file_path
                     WHERE ce.resolution_kind = 'unresolved' OR ce.callee_symbol_uid IS NULL
                     LIMIT 20000",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Seed {
                        source_table: "call_edges".to_string(),
                        source_id: row.get(0)?,
                        file_path: row.get(1)?,
                        reference_name: row.get(2)?,
                        reference_kind: row.get(3)?,
                        line: row.get(4)?,
                        column_no: row.get(5)?,
                        container: row.get(6)?,
                        resolution_strategy: row.get(7)?,
                        parser_tier: row.get(8)?,
                        parser_confidence: row.get(9)?,
                        language: row.get(10)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            seeds.extend(rows.filter_map(|r| r.ok()));
        }

        let mut prepared = Vec::with_capacity(seeds.len());
        for seed in seeds {
            if !is_actionable_reference_name(&seed.reference_name) {
                continue;
            }
            let candidates = Self::candidate_json_for_reference(&read, &seed.reference_name)?;
            let candidate_count = candidates.as_array().map(|a| a.len()).unwrap_or(0);
            // Keep the table focused on actionable resolver gaps: unresolved
            // references with at least one plausible target candidate.  External
            // library calls and local temporary identifiers otherwise dominate
            // the backlog and make it less useful for LLM/code-index workflows.
            if candidate_count == 0 {
                continue;
            }
            let failure_reason = if candidate_count == 1 {
                "single_candidate_not_selected"
            } else {
                "ambiguous_candidates"
            };
            prepared.push((seed, candidates.to_string(), failure_reason.to_string()));
        }
        drop(read);

        let mut write = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = write
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM resolution_attempts", [])
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut count = 0usize;
        for (seed, candidates_json, failure_reason) in prepared {
            let attempt_id = format!("{}:{}", seed.source_table, seed.source_id);
            tx.execute(
                "INSERT OR REPLACE INTO resolution_attempts(attempt_id,source_table,source_id,file_path,reference_name,reference_kind,line,column_no,container,candidates_json,failure_reason,resolution_strategy,parser_tier,parser_confidence,language,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                rusqlite::params![
                    attempt_id,
                    seed.source_table,
                    seed.source_id,
                    seed.file_path,
                    seed.reference_name,
                    seed.reference_kind,
                    seed.line,
                    seed.column_no,
                    seed.container,
                    candidates_json,
                    failure_reason,
                    seed.resolution_strategy,
                    seed.parser_tier,
                    seed.parser_confidence,
                    seed.language,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            count += 1;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    fn candidate_json_for_reference(
        conn: &Connection,
        reference_name: &str,
    ) -> CcResult<serde_json::Value> {
        let suffix_like = format!("%.{}", reference_name);
        let mut stmt = conn
            .prepare(
                "SELECT name, qname, file_path, kind, symbol_uid, start_line, end_line,
                        CASE
                          WHEN qname = ?1 THEN 0
                          WHEN name = ?1 THEN 1
                          WHEN qname LIKE ?2 THEN 2
                          ELSE 4
                        END AS rank
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1 OR qname LIKE ?2
                 ORDER BY rank ASC, file_path ASC, start_line ASC
                 LIMIT 8",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![reference_name, suffix_like], |row| {
                Ok(serde_json::json!({
                    "name": row.get::<_, String>(0)?,
                    "qname": row.get::<_, Option<String>>(1)?,
                    "file_path": row.get::<_, String>(2)?,
                    "kind": row.get::<_, String>(3)?,
                    "symbol_uid": row.get::<_, Option<String>>(4)?,
                    "start_line": row.get::<_, u32>(5)?,
                    "end_line": row.get::<_, u32>(6)?,
                    "rank": row.get::<_, i64>(7)?,
                }))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut candidates = Vec::new();
        for row in rows {
            if let Ok(v) = row {
                candidates.push(v);
            }
        }
        Ok(serde_json::Value::Array(candidates))
    }

    pub fn list_resolution_attempts(
        &self,
        limit: usize,
        file_path: Option<&str>,
        kind: Option<&str>,
    ) -> CcResult<Vec<ResolutionAttemptRow>> {
        let conn = self.read_conn()?;
        let mut sql = "SELECT attempt_id, source_table, source_id, file_path, reference_name, reference_kind, line, column_no, container, candidates_json, failure_reason, resolution_strategy, parser_tier, parser_confidence, language FROM resolution_attempts".to_string();
        let mut where_parts = Vec::new();
        let mut params = Vec::new();
        if let Some(fp) = file_path {
            where_parts.push("file_path = ?".to_string());
            params.push(fp.to_string());
        }
        if let Some(k) = kind {
            where_parts.push("reference_kind = ?".to_string());
            params.push(k.to_string());
        }
        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }
        sql.push_str(" ORDER BY file_path ASC, line ASC LIMIT ?");
        params.push(limit.to_string());

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let candidates_json: String = row.get(9)?;
                Ok(ResolutionAttemptRow {
                    attempt_id: row.get(0)?,
                    source_table: row.get(1)?,
                    source_id: row.get(2)?,
                    file_path: row.get(3)?,
                    reference_name: row.get(4)?,
                    reference_kind: row.get(5)?,
                    line: row.get(6)?,
                    column_no: row.get(7)?,
                    container: row.get(8)?,
                    candidates: serde_json::from_str(&candidates_json)
                        .unwrap_or_else(|_| serde_json::json!([])),
                    failure_reason: row.get(10)?,
                    resolution_strategy: row.get(11)?,
                    parser_tier: row.get(12)?,
                    parser_confidence: row.get(13)?,
                    language: row.get(14)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn data_flow_edges_by_uid(
        &self,
        uid: &str,
        limit: usize,
    ) -> CcResult<Vec<DataFlowEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, flow_kind, source_symbol_uid, target_symbol_uid, confidence
                 FROM data_flow_edges
                 WHERE source_symbol_uid = ?1 OR target_symbol_uid = ?1
                 ORDER BY confidence DESC, line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![uid, limit as i64], |row| {
                Ok(DataFlowEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    flow_kind: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol_uid: row.get(4)?,
                    confidence: row.get(5)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Graph / framework post-processing ───────────────────────

    pub fn call_uid_edges(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT caller_symbol_uid, callee_symbol_uid
                 FROM call_edges
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn symbol_names_by_uid(&self) -> CcResult<HashMap<String, String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT symbol_uid, name FROM symbols WHERE symbol_uid IS NOT NULL")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut map = HashMap::new();
        for row in rows {
            let (uid, name) = row.map_err(|e| CcError::Database(e.to_string()))?;
            map.insert(uid, name);
        }
        Ok(map)
    }

    pub fn update_communities(
        &self,
        assignments: &HashMap<String, u32>,
        labels: &HashMap<u32, String>,
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;

        tx.execute("UPDATE symbols SET community_id = NULL", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM communities", [])
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut member_counts: HashMap<u32, usize> = HashMap::new();
        for (uid, community_id) in assignments {
            tx.execute(
                "UPDATE symbols SET community_id = ?1 WHERE symbol_uid = ?2",
                rusqlite::params![community_id, uid],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            *member_counts.entry(*community_id).or_insert(0) += 1;
        }

        for (community_id, label) in labels {
            let member_count = member_counts.get(community_id).copied().unwrap_or(0);
            tx.execute(
                "INSERT INTO communities(community_id,label,member_count,representative_file,top_symbols_json)
                 VALUES(?1,?2,?3,NULL,'[]')",
                rusqlite::params![community_id, label, member_count as i64],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }

        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn replace_repo_frameworks(&self, signals: &[RepoFrameworkRecord]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM repo_frameworks", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        for (framework_key, confidence, evidences) in signals {
            let signals_json = serde_json::to_string(evidences).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT INTO repo_frameworks(framework_key,confidence,signals_json,updated_at)
                 VALUES(?1,?2,?3,?4)",
                rusqlite::params![framework_key, confidence, signals_json, now],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn replace_file_frameworks(&self, by_file: &[FileFrameworkRecord]) -> CcResult<()> {
        if by_file.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for (file_path, signals) in by_file {
            tx.execute(
                "DELETE FROM file_frameworks WHERE file_path = ?1",
                rusqlite::params![file_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            for (framework_key, confidence, evidence) in signals {
                let signals_json =
                    serde_json::to_string(&vec![evidence.clone()]).unwrap_or_else(|_| "[]".into());
                tx.execute(
                    "INSERT INTO file_frameworks(file_path,framework_key,confidence,signals_json)
                     VALUES(?1,?2,?3,?4)",
                    rusqlite::params![file_path, framework_key, confidence, signals_json],
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Symbol queries ────────────────────────────────────────────

    pub fn find_symbol(&self, name: &str, exact: bool, top_k: usize) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        let (sql, param): (&str, String) = if exact {
            (
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE name = ?1 ORDER BY file_path LIMIT ?2",
                name.to_string(),
            )
        } else {
            (
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE name LIKE ?1 ORDER BY file_path LIMIT ?2",
                format!("%{}%", name),
            )
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![param, top_k as i64], |row| {
                Ok(SymbolRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    file_path: row.get(4)?,
                    container: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    qname: row.get(8)?,
                    signature: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn file_symbols(&self, file_path: &str) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE file_path = ?1 ORDER BY start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(SymbolRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    file_path: row.get(4)?,
                    container: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    qname: row.get(8)?,
                    signature: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_symbol_targets(&self) -> CcResult<Vec<SymbolTargetRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, qname, file_path
                 FROM symbols
                 ORDER BY file_path, start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SymbolTargetRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    qname: row.get(3)?,
                    file_path: row.get(4)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Load persisted symbols needed to seed incremental resolver context.
    ///
    /// The result intentionally uses [`SymbolRecord`] so the resolver and type
    /// catalog can reuse the same in-memory representation for both transient
    /// and persisted symbols.
    pub fn resolver_seed_symbols_excluding(
        &self,
        excluded_files: &[String],
    ) -> CcResult<Vec<SymbolRecord>> {
        let conn = self.read_conn()?;
        let sql = if excluded_files.is_empty() {
            "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements FROM symbols ORDER BY file_path,start_line".to_string()
        } else {
            let placeholders = excluded_files
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements FROM symbols WHERE file_path NOT IN ({}) ORDER BY file_path,start_line",
                placeholders
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let params: Vec<&dyn rusqlite::types::ToSql> = excluded_files
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                let kind: String = row.get(3)?;
                let parser_tier: String = row.get(11)?;
                let param_count: Option<i64> = row.get(23)?;
                Ok(SymbolRecord {
                    symbol_id: row.get(0)?,
                    file_path: row.get(1)?,
                    name: row.get(2)?,
                    kind: SymbolKind::from_str_lenient(&kind).unwrap_or(SymbolKind::Variable),
                    container: row.get(4)?,
                    start_line: row.get(5)?,
                    end_line: row.get(6)?,
                    start_col: row.get(7)?,
                    end_col: row.get(8)?,
                    signature: row.get(9)?,
                    doc: row.get(10)?,
                    parser_tier: parse_parser_tier(&parser_tier),
                    parser_confidence: row.get(12)?,
                    qname: row.get(13)?,
                    parent_symbol_id: row.get(14)?,
                    scope_id: row.get(15)?,
                    export_name: row.get(16)?,
                    is_default_export: row.get::<_, i64>(17)? != 0,
                    symbol_uid: row.get(18)?,
                    framework_role: row.get(19)?,
                    receiver_type: row.get(20)?,
                    param_types: row.get(21)?,
                    return_type: row.get(22)?,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(24)?,
                    implements: row.get(25)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── File listing ─────────────────────────────────────────────

    pub fn list_indexed_files(&self) -> CcResult<Vec<FileInfoRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, language, size, parser_tier, indexed_at FROM files ORDER BY file_path",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FileInfoRow {
                    file_path: row.get(0)?,
                    language: row.get(1)?,
                    size: row.get::<_, i64>(2)? as u64,
                    parser_tier: row.get(3)?,
                    indexed_at: row.get(4)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_file_paths(&self) -> CcResult<Vec<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path FROM files ORDER BY file_path")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Check whether a file path exists in the index.
    pub fn file_is_indexed(&self, file_path: &str) -> CcResult<bool> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT 1 FROM files WHERE file_path = ?1 LIMIT 1")
            .map_err(|e| CcError::Database(e.to_string()))?;
        stmt.exists(rusqlite::params![file_path])
            .map_err(|e| CcError::Database(e.to_string()))
    }

    // ── Community / framework listing ────────────────────────────

    pub fn list_communities(&self) -> CcResult<Vec<CommunityRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT community_id, label, member_count FROM communities ORDER BY community_id",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CommunityRow {
                    community_id: row.get(0)?,
                    label: row.get(1)?,
                    member_count: row.get(2)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_repo_frameworks(&self) -> CcResult<Vec<(String, f64)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT framework_key, confidence FROM repo_frameworks ORDER BY confidence DESC",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Language distribution: count of files per language.
    pub fn language_distribution(&self) -> CcResult<Vec<(String, usize)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT language, COUNT(*) as cnt FROM files GROUP BY language ORDER BY cnt DESC",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Hotspot symbols: highest fan-in + fan-out, limited to `limit` entries.
    pub fn hotspot_symbols(&self, limit: usize) -> CcResult<Vec<HotspotRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT s.name, s.kind, s.file_path,
                        (SELECT COUNT(*) FROM call_edges WHERE callee_symbol = s.name) as fan_in,
                        (SELECT COUNT(*) FROM call_edges WHERE caller_symbol = s.name) as fan_out
                 FROM symbols s
                 ORDER BY fan_in + fan_out DESC
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, usize>(3)?,
                    row.get::<_, usize>(4)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Count of route edges.
    pub fn route_count(&self) -> CcResult<usize> {
        let conn = self.read_conn()?;
        let count = conn
            .query_row("SELECT COUNT(*) FROM route_edges", [], |r| {
                r.get::<_, usize>(0)
            })
            .unwrap_or(0);
        Ok(count)
    }

    // ── Generic JSON query ───────────────────────────────────────

    pub fn query_json(&self, sql: &str, params: &[String]) -> CcResult<Vec<Value>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let column_count = stmt.column_count();
        let column_names: Vec<String> = (0..column_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let mut obj = serde_json::Map::new();
                for (i, name) in column_names.iter().enumerate() {
                    if let Ok(s) = row.get::<_, String>(i) {
                        obj.insert(name.clone(), Value::String(s));
                    } else if let Ok(n) = row.get::<_, i64>(i) {
                        obj.insert(name.clone(), serde_json::json!(n));
                    } else if let Ok(f) = row.get::<_, f64>(i) {
                        obj.insert(name.clone(), serde_json::json!(f));
                    } else {
                        obj.insert(name.clone(), Value::Null);
                    }
                }
                Ok(Value::Object(obj))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(out)
    }

    /// Find test files impacted by changes to the given code files.
    pub fn find_impacted_tests(&self, file_paths: &[String]) -> CcResult<Vec<String>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
        let placeholders: Vec<String> = (1..=file_paths.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT DISTINCT test_file_path FROM test_edges WHERE code_file_path IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut tests = Vec::new();
        for r in rows {
            tests.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(tests)
    }

    /// Get a summary of a file: language, size, symbols count, chunks count, framework roles.
    pub fn file_summary(&self, file_path: &str) -> CcResult<Value> {
        let conn = self.read_conn()?;
        let file_info = conn.query_row(
            "SELECT language, size, parser_tier, summary, is_test_file FROM files WHERE file_path=?1",
            rusqlite::params![file_path],
            |row| {
                let mut obj = serde_json::Map::new();
                obj.insert("file_path".into(), Value::String(file_path.to_string()));
                obj.insert("language".into(), Value::String(row.get::<_, String>(0)?));
                obj.insert("size".into(), serde_json::json!(row.get::<_, i64>(1)?));
                obj.insert("parser_tier".into(), Value::String(row.get::<_, String>(2)?));
                obj.insert("summary".into(), Value::String(row.get::<_, String>(3).unwrap_or_default()));
                obj.insert("is_test_file".into(), serde_json::json!(row.get::<_, i32>(4).unwrap_or(0) != 0));
                Ok(obj)
            },
        ).map_err(|e| CcError::Database(e.to_string()))?;

        let mut obj = file_info;

        let symbol_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE file_path=?1",
                rusqlite::params![file_path],
                |row| row.get(0),
            )
            .unwrap_or(0);
        obj.insert("symbols_count".into(), serde_json::json!(symbol_count));

        let chunk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path=?1",
                rusqlite::params![file_path],
                |row| row.get(0),
            )
            .unwrap_or(0);
        obj.insert("chunks_count".into(), serde_json::json!(chunk_count));

        // Framework roles from file_frameworks
        let mut fw_stmt = conn.prepare(
            "SELECT framework_key, confidence FROM file_frameworks WHERE file_path=?1 ORDER BY confidence DESC"
        ).map_err(|e| CcError::Database(e.to_string()))?;
        let fw_rows = fw_stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(serde_json::json!({
                    "framework": row.get::<_, String>(0)?,
                    "confidence": row.get::<_, f64>(1)?
                }))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let frameworks: Vec<Value> = fw_rows.filter_map(|r| r.ok()).collect();
        obj.insert("frameworks".into(), Value::Array(frameworks));

        Ok(Value::Object(obj))
    }

    // ── Route queries (frontier expansion) ──────────────────────

    /// Query route edges by route path pattern. Returns matching route edge rows.
    pub fn route_rows_by_path(
        &self,
        route_path: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, route_path, handler_name, method, line,
                        end_line, handler_symbol_uid, framework, confidence
                 FROM route_edges
                 WHERE route_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![route_path, limit as i64], |row| {
                Ok(RouteEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    end_line: row.get(6)?,
                    handler_symbol_uid: row.get(7)?,
                    framework: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query route edges by handler symbol UID.
    pub fn route_rows_by_handler_uid(
        &self,
        handler_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, route_path, handler_name, method, line,
                        end_line, handler_symbol_uid, framework, confidence
                 FROM route_edges
                 WHERE handler_symbol_uid = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![handler_uid, limit as i64], |row| {
                Ok(RouteEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    end_line: row.get(6)?,
                    handler_symbol_uid: row.get(7)?,
                    framework: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── HTTP call edge queries (frontier expansion) ────────────

    /// Query outbound HTTP calls made by a given caller symbol UID.
    pub fn http_calls_by_caller_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                        method, call_kind, line, confidence
                 FROM http_call_edges
                 WHERE caller_symbol_uid = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid, limit as i64], |row| {
                Ok(HttpCallEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query HTTP callers that target a given normalized route path.
    /// Used for reverse expansion: "who calls this route handler?"
    pub fn http_callers_by_normalized_path(
        &self,
        normalized_path: &str,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                        method, call_kind, line, confidence
                 FROM http_call_edges
                 WHERE normalized_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path, limit as i64], |row| {
                Ok(HttpCallEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query HTTP callers by normalized path with optional method filter.
    /// Falls back to path-only match when method is None or no exact match found.
    pub fn http_callers_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                            method, call_kind, line, confidence
                     FROM http_call_edges
                     WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)
                     ORDER BY confidence DESC
                     LIMIT ?3",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m, limit as i64], |row| {
                    Ok(HttpCallEdgeLite {
                        edge_id: row.get(0)?,
                        file_path: row.get(1)?,
                        caller_symbol_uid: row.get(2)?,
                        url_or_path: row.get(3)?,
                        normalized_path: row.get(4)?,
                        method: row.get(5)?,
                        call_kind: row.get(6)?,
                        line: row.get(7)?,
                        confidence: row.get(8)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<HttpCallEdgeLite> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        // Fallback: path-only match
        self.http_callers_by_normalized_path(normalized_path, limit)
    }

    /// Query route_nodes by normalized_path (exact match).
    /// Returns server-side route handlers that match a normalized path.
    pub fn route_nodes_by_normalized_path(
        &self,
        normalized_path: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteNodeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid,
                        handler_name, framework, line, end_line, confidence
                 FROM route_nodes
                 WHERE normalized_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path, limit as i64], |row| {
                Ok(RouteNodeLite {
                    route_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    method: row.get(3)?,
                    handler_symbol_uid: row.get(4)?,
                    handler_name: row.get(5)?,
                    framework: row.get(6)?,
                    line: row.get(7)?,
                    end_line: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query route_nodes by normalized_path with optional HTTP method filter.
    /// Falls back to path-only match when method is None or no exact match found.
    pub fn route_nodes_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<RouteNodeLite>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT route_id, file_path, route_path, method, handler_symbol_uid,
                            handler_name, framework, line, end_line, confidence
                     FROM route_nodes
                     WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)
                     ORDER BY confidence DESC
                     LIMIT ?3",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m, limit as i64], |row| {
                    Ok(RouteNodeLite {
                        route_id: row.get(0)?,
                        file_path: row.get(1)?,
                        route_path: row.get(2)?,
                        method: row.get(3)?,
                        handler_symbol_uid: row.get(4)?,
                        handler_name: row.get(5)?,
                        framework: row.get(6)?,
                        line: row.get(7)?,
                        end_line: row.get(8)?,
                        confidence: row.get(9)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<RouteNodeLite> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        // Fallback: path-only match
        self.route_nodes_by_normalized_path(normalized_path, limit)
    }

    // ── Diagnostic queries (frontier expansion) ─────────────────

    /// Search diagnostics by message text using FTS.
    pub fn diagnostic_rows_by_message(
        &self,
        query: &str,
        limit: usize,
    ) -> CcResult<Vec<DiagnosticLite>> {
        let conn = self.read_conn()?;
        // Escape FTS special characters and build a prefix query
        let fts_query = query
            .replace('"', "\"\"")
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(
                "SELECT d.diagnostic_id, d.file_path, d.severity, d.message, d.line,
                        d.end_line, d.source, d.code, d.confidence, d.symbol_uid
                 FROM diagnostics d
                 JOIN diagnostics_fts f ON d.diagnostic_id = f.diagnostic_id
                 WHERE diagnostics_fts MATCH ?1
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                Ok(DiagnosticLite {
                    diagnostic_id: row.get(0)?,
                    file_path: row.get(1)?,
                    severity: row.get(2)?,
                    message: row.get(3)?,
                    line: row.get(4)?,
                    end_line: row.get(5)?,
                    source: row.get(6)?,
                    code: row.get(7)?,
                    confidence: row.get(8)?,
                    symbol_uid: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Neighbor chunk queries (frontier expansion) ─────────────

    /// Get the chunk_index for a given chunk_id.
    pub fn chunk_index_by_id(&self, chunk_id: &str) -> CcResult<Option<(String, u32)>> {
        let conn = self.read_conn()?;
        conn.query_row(
            "SELECT file_path, chunk_index FROM chunks WHERE chunk_id = ?1",
            rusqlite::params![chunk_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CcError::Database(other.to_string())),
        })
    }

    /// Get neighboring chunks (by chunk_index +/- radius) in the same file.
    pub fn neighbor_chunks(
        &self,
        file_path: &str,
        chunk_index: u32,
        radius: usize,
    ) -> CcResult<Vec<NeighborChunkRow>> {
        let conn = self.read_conn()?;
        let lo = chunk_index.saturating_sub(radius as u32);
        let hi = chunk_index + radius as u32;
        let mut stmt = conn
            .prepare(
                "SELECT chunk_id, file_path, chunk_index, start_line, end_line, text, breadcrumb
                 FROM chunks
                 WHERE file_path = ?1 AND chunk_index >= ?2 AND chunk_index <= ?3 AND chunk_index != ?4
                 ORDER BY chunk_index
                 LIMIT ?5",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(
                rusqlite::params![file_path, lo, hi, chunk_index, (radius * 2) as i64],
                |row| {
                    Ok(NeighborChunkRow {
                        chunk_id: row.get(0)?,
                        file_path: row.get(1)?,
                        chunk_index: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        text: read_chunk_text(row, 5)?,
                        breadcrumb: row.get(6)?,
                    })
                },
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Community lookup (frontier expansion) ───────────────────

    /// Get the community_id for a symbol UID.
    pub fn community_id_for_uid(&self, symbol_uid: &str) -> CcResult<Option<u32>> {
        let conn = self.read_conn()?;
        conn.query_row(
            "SELECT community_id FROM symbols WHERE symbol_uid = ?1 AND community_id IS NOT NULL",
            rusqlite::params![symbol_uid],
            |row| row.get::<_, u32>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CcError::Database(other.to_string())),
        })
    }

    // ── symbol_id fallback queries (frontier expansion) ─────────

    /// Caller rows by callee symbol_id (fallback when no UID available).
    pub fn caller_rows_by_symbol_id(
        &self,
        callee_symbol_id: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE target_symbol_id = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![callee_symbol_id, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Callee rows by caller symbol_id (fallback when no UID available).
    pub fn callee_rows_by_symbol_id(
        &self,
        caller_symbol_id: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE caller_symbol_id = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_symbol_id, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Literal search / replace ─────────────────────────────────

    /// Replace all literal_index rows for given files in a single transaction.
    pub fn replace_literals(
        &self,
        by_file: &[(String, Vec<cc_model::diagnostic::LiteralRecord>)],
    ) -> CcResult<()> {
        if by_file.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for (file_path, literals) in by_file {
            tx.execute(
                "DELETE FROM literal_fts WHERE file_path = ?1",
                rusqlite::params![file_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            tx.execute(
                "DELETE FROM literal_index WHERE file_path = ?1",
                rusqlite::params![file_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            for l in literals {
                tx.execute(
                    "INSERT INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence, l.enclosing_symbol_uid, l.key_path],
                ).map_err(|e| CcError::Database(e.to_string()))?;
                tx.execute(
                    "INSERT INTO literal_fts(literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4)",
                    rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind],
                ).map_err(|e| CcError::Database(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Search literals via FTS. Optionally filter by literal_kind.
    pub fn search_literals(
        &self,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<LiteralLite>> {
        let conn = self.read_conn()?;
        let fts_query = query
            .replace('"', "\"\"")
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let rows = if let Some(kind_val) = kind {
            let mut stmt = conn
                .prepare(
                    "SELECT l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence
                     FROM literal_fts
                     JOIN literal_index l ON l.literal_id = literal_fts.literal_id
                     WHERE literal_fts MATCH ?1 AND l.literal_kind = ?2
                     ORDER BY bm25(literal_fts)
                     LIMIT ?3",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(
                    rusqlite::params![fts_query, kind_val, limit as i64],
                    |row| {
                        Ok(LiteralLite {
                            literal_id: row.get(0)?,
                            file_path: row.get(1)?,
                            literal: row.get(2)?,
                            literal_kind: row.get(3)?,
                            line: row.get(4)?,
                            container: row.get(5)?,
                            confidence: row.get(6)?,
                        })
                    },
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            rows.filter_map(|r| r.ok()).collect()
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence
                     FROM literal_fts
                     JOIN literal_index l ON l.literal_id = literal_fts.literal_id
                     WHERE literal_fts MATCH ?1
                     ORDER BY bm25(literal_fts)
                     LIMIT ?2",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                    Ok(LiteralLite {
                        literal_id: row.get(0)?,
                        file_path: row.get(1)?,
                        literal: row.get(2)?,
                        literal_kind: row.get(3)?,
                        line: row.get(4)?,
                        container: row.get(5)?,
                        confidence: row.get(6)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            rows.filter_map(|r| r.ok()).collect()
        };
        Ok(rows)
    }

    // ── Test edge rebuild ────────────────────────────────────────

    /// Incremental test-edge rebuild for a set of changed files.
    ///
    /// Mirrors the Python `rebuild_test_edges_for_files()` logic:
    /// 1. Delete existing edges involving any changed file.
    /// 2. Re-derive test↔code pairs by basename heuristics.
    pub fn rebuild_test_edges_for_files(&self, changed: &[String]) -> CcResult<()> {
        if changed.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;

        // 1. Remove edges involving changed files
        for fp in changed {
            tx.execute(
                "DELETE FROM test_edges WHERE test_file_path = ?1 OR code_file_path = ?1",
                rusqlite::params![fp],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }

        // 2. Fetch all test / code file sets
        let all_tests: std::collections::HashSet<String> = {
            let mut stmt = tx
                .prepare("SELECT file_path FROM files WHERE is_test_file = 1")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected.into_iter().collect()
        };
        let all_code: std::collections::HashSet<String> = {
            let mut stmt = tx
                .prepare("SELECT file_path FROM files WHERE is_test_file = 0")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected.into_iter().collect()
        };

        let changed_set: std::collections::HashSet<&str> =
            changed.iter().map(|s| s.as_str()).collect();

        // 3. Build pairs to check
        let mut pairs: Vec<(String, String)> = Vec::new();

        // Changed test files → pair with all code files
        for tf in &all_tests {
            if changed_set.contains(tf.as_str()) {
                for cf in &all_code {
                    pairs.push((tf.clone(), cf.clone()));
                }
            }
        }
        // Changed code files → pair with all test files (skip already-covered tests)
        for cf in &all_code {
            if changed_set.contains(cf.as_str()) {
                for tf in &all_tests {
                    if !changed_set.contains(tf.as_str()) {
                        pairs.push((tf.clone(), cf.clone()));
                    }
                }
            }
        }

        // 4. Match pairs by basename heuristic
        for (test_file, code_file) in &pairs {
            let test_stem = Path::new(test_file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let code_stem = Path::new(code_file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            let base_clean = test_stem
                .strip_prefix("test_")
                .unwrap_or(test_stem)
                .strip_suffix("_test")
                .unwrap_or(test_stem);
            // Also handle .test / .spec naming convention
            let base_clean = base_clean
                .strip_suffix(".test")
                .or_else(|| base_clean.strip_suffix(".spec"))
                .unwrap_or(base_clean);

            let (confidence, reason) = if code_stem == base_clean {
                (0.9, "same-basename")
            } else if code_file.contains(base_clean) || test_file.contains(code_stem) {
                (0.7, "path-overlap")
            } else {
                continue;
            };

            let edge_id = format!("test:{}:{}", test_file, code_file);
            tx.execute(
                "INSERT OR REPLACE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![edge_id, test_file, code_file, reason, confidence],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }

        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Full rebuild of all test edges (delete all, re-derive from scratch).
    pub fn rebuild_test_edges(&self) -> CcResult<()> {
        // Collect all file paths, then delegate
        let all_files: Vec<String> = {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare("SELECT file_path FROM files")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected
        };
        // First purge all existing test edges
        {
            let conn = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            conn.execute("DELETE FROM test_edges", [])
                .map_err(|e| CcError::Database(e.to_string()))?;
        }
        self.rebuild_test_edges_for_files(&all_files)
    }

    // ── Route nodes ─────────────────────────────────────────────

    /// Batch-insert route nodes, replacing any existing rows.
    pub fn insert_route_nodes_batch(
        &self,
        routes: &[cc_model::edge::RouteNodeRecord],
    ) -> CcResult<()> {
        if routes.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for r in routes {
            tx.execute(
                "INSERT OR REPLACE INTO route_nodes(route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params![r.route_id, r.file_path, r.route_path, r.method, r.handler_symbol_uid, r.handler_name, r.framework, r.line, r.end_line, r.normalized_path, r.confidence, r.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// List all route nodes ordered by route_path.
    pub fn list_route_nodes(&self) -> CcResult<Vec<RouteNodeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence
                 FROM route_nodes
                 ORDER BY route_path",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RouteNodeLite {
                    route_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    method: row.get(3)?,
                    handler_symbol_uid: row.get(4)?,
                    handler_name: row.get(5)?,
                    framework: row.get(6)?,
                    line: row.get(7)?,
                    end_line: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Data-flow edges ─────────────────────────────────────────

    /// Batch-insert data-flow edges.
    pub fn insert_data_flow_edges_batch(
        &self,
        edges: &[cc_model::edge::DataFlowEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            tx.execute(
                "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![e.edge_id, e.file_path, e.source_symbol_uid, e.target_symbol_uid, e.flow_kind, e.line, e.confidence, e.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Semantic edges ───────────────────────────────────────────

    /// Batch-insert semantic edges (inheritance, implementation, decoration, etc.).
    pub fn insert_semantic_edges_batch(
        &self,
        edges: &[cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            tx.execute(
                "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![e.edge_id, e.file_path, e.source_symbol, e.source_symbol_uid, e.target_symbol, e.target_symbol_uid, e.relation_kind.as_str(), e.line, e.confidence, e.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Route edges (batch append) ─────────────────────────────

    /// Batch-insert additional route edges (from framework resolvers).
    pub fn insert_route_edges_batch(
        &self,
        edges: &[cc_model::edge::RouteEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for r in edges {
            tx.execute(
                "INSERT OR REPLACE INTO route_edges(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Remove semantic edges for a given file (used during incremental updates).
    pub fn remove_semantic_edges_by_file(&self, file_path: &str) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM semantic_edges WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Query semantic edges by optional source_uid, target_uid, and relation_kind.
    pub fn query_semantic_edges(
        &self,
        source_uid: Option<&str>,
        target_uid: Option<&str>,
        relation_kind: Option<&str>,
    ) -> CcResult<Vec<cc_model::edge::SemanticEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut sql = String::from(
            "SELECT edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier FROM semantic_edges WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(uid) = source_uid {
            params.push(Box::new(uid.to_string()));
            sql.push_str(&format!(" AND source_symbol_uid = ?{}", params.len()));
        }
        if let Some(uid) = target_uid {
            params.push(Box::new(uid.to_string()));
            sql.push_str(&format!(" AND target_symbol_uid = ?{}", params.len()));
        }
        if let Some(kind) = relation_kind {
            params.push(Box::new(kind.to_string()));
            sql.push_str(&format!(" AND relation_kind = ?{}", params.len()));
        }
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let relation_str: String = row.get(6)?;
                let tier_str: String = row.get(9)?;
                Ok(cc_model::edge::SemanticEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    source_symbol: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol: row.get(4)?,
                    target_symbol_uid: row.get(5)?,
                    relation_kind: match relation_str.as_str() {
                        "inherits" => cc_model::edge::SemanticRelation::Inherits,
                        "implements" => cc_model::edge::SemanticRelation::Implements,
                        "decorates" => cc_model::edge::SemanticRelation::Decorates,
                        "throws" => cc_model::edge::SemanticRelation::Throws,
                        "uses_type" => cc_model::edge::SemanticRelation::UsesType,
                        "defines" => cc_model::edge::SemanticRelation::Defines,
                        "defines_method" => cc_model::edge::SemanticRelation::DefinesMethod,
                        "contains_file" => cc_model::edge::SemanticRelation::ContainsFile,
                        "contains_module" => cc_model::edge::SemanticRelation::ContainsModule,
                        "renders_component" => cc_model::edge::SemanticRelation::RendersComponent,
                        other => {
                            warn!(kind = %other, "unknown semantic relation_kind in DB, mapping to Unknown");
                            cc_model::edge::SemanticRelation::Unknown
                        }
                    },
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: match tier_str.as_str() {
                        "generic" => ParserTier::Generic,
                        "heuristic" => ParserTier::Heuristic,
                        "tree_sitter" => ParserTier::TreeSitter,
                        "semantic" => ParserTier::Semantic,
                        "verified" => ParserTier::Verified,
                        _ => ParserTier::Generic,
                    },
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Co-change edges ─────────────────────────────────────────

    /// Batch-insert co-change edges (replaces all existing rows).
    pub fn insert_co_change_edges_batch(
        &self,
        edges: &[cc_model::edge::CoChangeEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM co_change_edges", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            tx.execute(
                "INSERT INTO co_change_edges(edge_id,file_a,file_b,co_change_count,total_commits_a,total_commits_b,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![e.edge_id, e.file_a, e.file_b, e.co_change_count, e.total_commits_a, e.total_commits_b, e.confidence],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Get co-change edges for a file, filtered by minimum confidence.
    pub fn get_co_changes_for_file(
        &self,
        file_path: &str,
        min_confidence: f64,
    ) -> CcResult<Vec<CoChangeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_a, file_b, co_change_count, total_commits_a, total_commits_b, confidence
                 FROM co_change_edges
                 WHERE (file_a = ?1 OR file_b = ?1) AND confidence >= ?2
                 ORDER BY confidence DESC",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path, min_confidence], |row| {
                Ok(CoChangeLite {
                    edge_id: row.get(0)?,
                    file_a: row.get(1)?,
                    file_b: row.get(2)?,
                    co_change_count: row.get(3)?,
                    total_commits_a: row.get(4)?,
                    total_commits_b: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get co-change neighbors for a file with an optional result cap.
    pub fn co_change_neighbors(
        &self,
        file_path: &str,
        min_confidence: f64,
        limit: usize,
    ) -> CcResult<Vec<CoChangeLite>> {
        let mut rows = self.get_co_changes_for_file(file_path, min_confidence)?;
        rows.truncate(limit);
        Ok(rows)
    }

    // ── HTTP call edge queries ──────────────────────────────────

    /// Query outbound HTTP calls made by a given caller symbol (full record).
    pub fn http_call_records_by_caller_uid(
        &self,
        caller_uid: &str,
    ) -> CcResult<Vec<HttpCallEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier, broker_type
                 FROM http_call_edges WHERE caller_symbol_uid = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid], |row| {
                Ok(HttpCallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: parse_parser_tier(row.get::<_, String>(9)?.as_str()),
                    broker_type: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query HTTP clients that call a route matching the given normalized path.
    pub fn http_callers_for_route_path(
        &self,
        normalized_path: &str,
    ) -> CcResult<Vec<HttpCallEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier, broker_type
                 FROM http_call_edges WHERE normalized_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path], |row| {
                Ok(HttpCallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: parse_parser_tier(row.get::<_, String>(9)?.as_str()),
                    broker_type: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query route nodes matching a normalized path.
    pub fn routes_by_normalized_path(
        &self,
        normalized_path: &str,
    ) -> CcResult<Vec<RouteNodeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence, parser_tier, normalized_path
                 FROM route_nodes WHERE normalized_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path], |row| {
                Ok(RouteNodeRecord {
                    route_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    method: row.get(3)?,
                    handler_symbol_uid: row.get(4)?,
                    handler_name: row.get(5)?,
                    framework: row.get(6)?,
                    line: row.get(7)?,
                    end_line: row.get(8)?,
                    confidence: row.get(9)?,
                    parser_tier: parse_parser_tier(row.get::<_, String>(10)?.as_str()),
                    normalized_path: row.get(11)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query route nodes matching a normalized path with optional method filter.
    /// Falls back to path-only match when method is None or no exact match found.
    pub fn routes_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
    ) -> CcResult<Vec<RouteNodeRecord>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence, parser_tier, normalized_path
                     FROM route_nodes WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m], |row| {
                    Ok(RouteNodeRecord {
                        route_id: row.get(0)?,
                        file_path: row.get(1)?,
                        route_path: row.get(2)?,
                        method: row.get(3)?,
                        handler_symbol_uid: row.get(4)?,
                        handler_name: row.get(5)?,
                        framework: row.get(6)?,
                        line: row.get(7)?,
                        end_line: row.get(8)?,
                        confidence: row.get(9)?,
                        parser_tier: parse_parser_tier(row.get::<_, String>(10)?.as_str()),
                        normalized_path: row.get(11)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<RouteNodeRecord> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        // Fallback: path-only match
        self.routes_by_normalized_path(normalized_path)
    }

    // ── Infrastructure graph ──────────────────────────────────────

    /// Replace all infra nodes and edges (upsert semantics).
    pub fn replace_infra_data(
        &self,
        nodes: &[cc_model::infra::InfraNode],
        edges: &[cc_model::infra::InfraEdge],
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for node in nodes {
            tx.execute(
                "INSERT OR REPLACE INTO infra_nodes (node_id, file_path, kind, name, namespace, line, end_line, properties, bound_symbol_uid, binding_confidence) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    node.node_id,
                    node.file_path,
                    node.kind.as_str(),
                    node.name,
                    node.namespace,
                    node.line,
                    node.end_line,
                    node.properties.to_string(),
                    node.bound_symbol_uid,
                    node.binding_confidence,
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        for edge in edges {
            tx.execute(
                "INSERT OR REPLACE INTO infra_edges (edge_id, source_node_id, target_node_id, kind, confidence, properties) VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    edge.edge_id,
                    edge.source_node_id,
                    edge.target_node_id,
                    edge.kind.as_str(),
                    edge.confidence,
                    edge.properties.to_string(),
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Delete all infra nodes (and referencing edges) for a given file.
    pub fn delete_infra_by_file(&self, file_path: &str) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM infra_edges WHERE source_node_id IN (SELECT node_id FROM infra_nodes WHERE file_path = ?1) OR target_node_id IN (SELECT node_id FROM infra_nodes WHERE file_path = ?1)",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM infra_nodes WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    // ── Dispatch sites ──────────────────────────────────────────

    /// Replace dispatch sites for a single file (delete old + insert new).
    pub fn replace_dispatch_sites(
        &self,
        file_path: &str,
        sites: &[cc_model::DispatchSiteRecord],
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM dispatch_sites WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        for ds in sites {
            Self::execute_cached(
                &tx,
                "INSERT INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
            )?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Load all dispatch sites from the database.
    pub fn load_all_dispatch_sites(&self) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let kind_str: String = row.get(6)?;
                Ok(cc_model::DispatchSiteRecord {
                    site_id: row.get(0)?,
                    file_path: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    enclosing_symbol_uid: row.get(4)?,
                    receiver_expr: row.get(5)?,
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Load dispatch sites filtered by site_kind and key.
    pub fn load_dispatch_sites_by_kind_key(
        &self,
        kind: &str,
        key: &str,
    ) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE site_kind = ?1 AND key = ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![kind, key], |row| {
                let kind_str: String = row.get(6)?;
                Ok(cc_model::DispatchSiteRecord {
                    site_id: row.get(0)?,
                    file_path: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    enclosing_symbol_uid: row.get(4)?,
                    receiver_expr: row.get(5)?,
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Delete all dispatch sites for a file.
    pub fn delete_dispatch_sites_for_file(&self, file_path: &str) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM dispatch_sites WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Load dispatch sites filtered by site_kind (all keys).
    pub fn load_dispatch_sites_by_kind(
        &self,
        kind: &str,
    ) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE site_kind = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![kind], |row| {
                let kind_str: String = row.get(6)?;
                Ok(cc_model::DispatchSiteRecord {
                    site_id: row.get(0)?,
                    file_path: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    enclosing_symbol_uid: row.get(4)?,
                    receiver_expr: row.get(5)?,
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Find symbols by exact name, filtering to function/class/component kinds.
    pub fn find_symbols_by_name_and_kinds(
        &self,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: String = kinds
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature \
             FROM symbols WHERE name = ?1 AND kind IN ({}) ORDER BY file_path",
            placeholders
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(name.to_string()));
        for kind in kinds {
            params.push(Box::new(kind.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(SymbolRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    file_path: row.get(4)?,
                    container: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    qname: row.get(8)?,
                    signature: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete synthetic semantic edges whose edge_id starts with a given prefix.
    pub fn delete_synthetic_semantic_edges(&self, edge_id_prefix: &str) -> CcResult<usize> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let pattern = format!("{}%", edge_id_prefix);
        let count = conn
            .execute(
                "DELETE FROM semantic_edges WHERE edge_id LIKE ?1",
                rusqlite::params![pattern],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    /// Find the symbol_uid of a method named `method_name` contained in the same class
    /// as the given symbol_uid. Used for finding `render` methods in class components.
    pub fn find_method_in_same_class(
        &self,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        // First find the container (class) of the given symbol
        let container: Option<String> = conn
            .query_row(
                "SELECT container FROM symbols WHERE symbol_uid = ?1",
                rusqlite::params![member_symbol_uid],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        let container = match container {
            Some(c) => c,
            None => return Ok(None),
        };
        // Find the file_path of the given symbol
        let file_path: Option<String> = conn
            .query_row(
                "SELECT file_path FROM symbols WHERE symbol_uid = ?1",
                rusqlite::params![member_symbol_uid],
                |row| row.get(0),
            )
            .ok();
        let file_path = match file_path {
            Some(fp) => fp,
            None => return Ok(None),
        };
        // Now find the method in the same class
        let result: Option<String> = conn
            .query_row(
                "SELECT symbol_uid FROM symbols WHERE file_path = ?1 AND container = ?2 AND name = ?3 AND kind = 'method' LIMIT 1",
                rusqlite::params![file_path, container, method_name],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        Ok(result)
    }

    /// Find all methods belonging to a given container (class/struct name),
    /// returning `(symbol_uid, name, file_path, start_line)`.
    pub fn find_methods_by_container(
        &self,
        container: &str,
    ) -> CcResult<Vec<(String, String, String, u32)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_uid, name, file_path, start_line \
                 FROM symbols WHERE container = ?1 AND kind = 'method' AND symbol_uid IS NOT NULL \
                 ORDER BY file_path, start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![container], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Find all classes that have methods matching any of the given name patterns.
    /// Returns `(container, file_path)` pairs (deduplicated).
    pub fn find_classes_with_method_names(
        &self,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        if method_names.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
        let placeholders: String = method_names
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT container, file_path \
             FROM symbols WHERE kind = 'method' AND container IS NOT NULL AND name IN ({}) \
             ORDER BY file_path",
            placeholders
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for name in method_names {
            params.push(Box::new(name.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Synthetic call edges ────────────────────────────────────

    /// Delete all synthetic call edges produced by a given synthesizer.
    pub fn delete_synthetic_call_edges(&self, synthesized_by: &str) -> CcResult<usize> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let count = conn
            .execute(
                "DELETE FROM call_edges WHERE synthesized_by = ?1",
                rusqlite::params![synthesized_by],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    /// Batch-insert synthetic call edges.
    pub fn insert_synthetic_call_edges(
        &self,
        edges: &[cc_model::CallEdgeRecord],
    ) -> CcResult<usize> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            Self::execute_cached(
                &tx,
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                rusqlite::params![
                    e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol,
                    e.line, e.start_col, e.end_line, e.end_col,
                    e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id,
                    e.caller_symbol_uid, e.callee_symbol_uid,
                    e.dispatch_kind.as_str(), e.call_kind,
                    e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy,
                    e.receiver_expr, e.arg_count.map(|v| v as i32),
                    e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32,
                    e.parser_tier.as_str(), e.parser_confidence,
                    e.synthesized_by, e.synthesis_key, e.registered_file,
                    e.registered_line.map(|v| v as i32)
                ],
            )?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(edges.len())
    }

    // ── Incremental dirty propagation ────────────────────────────

    /// 获取文件的导出符号指纹，用于增量时检测导出变化。
    /// 对该文件所有导出符号的 (symbol_uid, name, signature, export_name) 拼接做 blake3 hash。
    /// 无导出符号时返回 None。
    pub fn get_export_fingerprint(&self, file_path: &str) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_uid, name, COALESCE(signature, '') as sig, COALESCE(export_name, '') as exp
                 FROM symbols
                 WHERE file_path = ?1
                   AND (export_name IS NOT NULL OR is_default_export = 1)
                 ORDER BY symbol_uid",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut parts: Vec<String> = Vec::new();
        for row in rows {
            let (uid, name, sig, exp) = row.map_err(|e| CcError::Database(e.to_string()))?;
            parts.push(format!("{}|{}|{}|{}", uid, name, sig, exp));
        }

        if parts.is_empty() {
            return Ok(None);
        }

        let combined = parts.join("\n");
        let hash = blake3::hash(combined.as_bytes());
        Ok(Some(hash.to_hex().to_string()))
    }

    /// 查找所有导入了指定文件集合的文件路径（利用 idx_imports_resolved 索引）。
    /// 分批查询（每批最多 500），合并去重后返回。
    pub fn find_importers_of(&self, resolved_paths: &[String]) -> CcResult<Vec<String>> {
        if resolved_paths.is_empty() {
            return Ok(Vec::new());
        }
        const BATCH_SIZE: usize = 500;
        let conn = self.read_conn()?;
        let mut result: std::collections::HashSet<String> = std::collections::HashSet::new();

        for chunk in resolved_paths.chunks(BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            let sql = format!(
                "SELECT DISTINCT file_path FROM imports WHERE resolved_path IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?;
            for row in rows {
                result.insert(row.map_err(|e| CcError::Database(e.to_string()))?);
            }
        }

        Ok(result.into_iter().collect())
    }

    /// 从数据库加载文件的边数据，用于 DirtyResolveOnly 场景。
    /// 只加载需要重新 resolve 的字段：symbols, imports, call_edges, symbol_refs, semantic_edges。
    pub fn load_file_edges_for_reresolve(
        &self,
        file_path: &str,
    ) -> CcResult<FileEdgesForReresolve> {
        let conn = self.read_conn()?;

        // symbols
        let mut sym_stmt = conn
            .prepare(
                "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,\
                 signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,\
                 export_name,is_default_export,symbol_uid,framework_role,receiver_type,\
                 param_types,return_type,param_count,base_types,implements \
                 FROM symbols WHERE file_path = ?1 ORDER BY start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sym_rows = sym_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let kind: String = row.get(3)?;
                let parser_tier_str: String = row.get(11)?;
                let param_count: Option<i64> = row.get(23)?;
                Ok(cc_model::SymbolRecord {
                    symbol_id: row.get(0)?,
                    file_path: row.get(1)?,
                    name: row.get(2)?,
                    kind: cc_model::SymbolKind::from_str_lenient(&kind)
                        .unwrap_or(cc_model::SymbolKind::Variable),
                    container: row.get(4)?,
                    start_line: row.get(5)?,
                    end_line: row.get(6)?,
                    start_col: row.get(7)?,
                    end_col: row.get(8)?,
                    signature: row.get(9)?,
                    doc: row.get(10)?,
                    parser_tier: parse_parser_tier(&parser_tier_str),
                    parser_confidence: row.get(12)?,
                    qname: row.get(13)?,
                    parent_symbol_id: row.get(14)?,
                    scope_id: row.get(15)?,
                    export_name: row.get(16)?,
                    is_default_export: row.get::<_, i64>(17)? != 0,
                    symbol_uid: row.get(18)?,
                    framework_role: row.get(19)?,
                    receiver_type: row.get(20)?,
                    param_types: row.get(21)?,
                    return_type: row.get(22)?,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(24)?,
                    implements: row.get(25)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let symbols: Vec<cc_model::SymbolRecord> = sym_rows.filter_map(|r| r.ok()).collect();

        // imports
        let mut imp_stmt = conn
            .prepare(
                "SELECT file_path,import_string,resolved_path,imported_name,alias,\
                 is_namespace,is_default,is_reexport \
                 FROM imports WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let imp_rows = imp_stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(cc_model::ImportRecord {
                    file_path: row.get(0)?,
                    import_string: row.get(1)?,
                    resolved_path: row.get(2)?,
                    imported_name: row.get(3)?,
                    alias: row.get(4)?,
                    is_namespace: row.get::<_, i64>(5)? != 0,
                    is_default: row.get::<_, i64>(6)? != 0,
                    is_reexport: row.get::<_, i64>(7)? != 0,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let imports: Vec<cc_model::ImportRecord> = imp_rows.filter_map(|r| r.ok()).collect();

        // call_edges
        let mut ce_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,\
                 target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,\
                 caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,\
                 resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,\
                 parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line \
                 FROM call_edges WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let ce_rows = ce_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let dispatch_str: String = row.get(14)?;
                let resolution_str: String = row.get(16)?;
                let tier_str: String = row.get(24)?;
                let arg_count: Option<i32> = row.get(20)?;
                let registered_line: Option<i32> = row.get(29)?;
                Ok(cc_model::CallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    line: row.get(4)?,
                    start_col: row.get(5)?,
                    end_line: row.get(6)?,
                    end_col: row.get(7)?,
                    target_symbol_id: row.get(8)?,
                    target_file_path: row.get(9)?,
                    caller_symbol_id: row.get(10)?,
                    callee_ref_id: row.get(11)?,
                    caller_symbol_uid: row.get(12)?,
                    callee_symbol_uid: row.get(13)?,
                    dispatch_kind: match dispatch_str.as_str() {
                        "dynamic" => cc_model::DispatchKind::Dynamic,
                        "virtual_dispatch" => cc_model::DispatchKind::VirtualDispatch,
                        "optional_chain" => cc_model::DispatchKind::OptionalChain,
                        "constructor" => cc_model::DispatchKind::Constructor,
                        "event_emitter" => cc_model::DispatchKind::EventEmitter,
                        "callback_relay" => cc_model::DispatchKind::CallbackRelay,
                        "reactive_binding" => cc_model::DispatchKind::ReactiveBinding,
                        "field_observer" => cc_model::DispatchKind::FieldObserver,
                        _ => cc_model::DispatchKind::Direct,
                    },
                    call_kind: row.get(15)?,
                    resolution_kind: match resolution_str.as_str() {
                        "exact" => cc_model::ResolutionKind::Exact,
                        "qualified" => cc_model::ResolutionKind::Qualified,
                        "scope_resolved" => cc_model::ResolutionKind::ScopeResolved,
                        "heuristic" => cc_model::ResolutionKind::Heuristic,
                        _ => cc_model::ResolutionKind::Unresolved,
                    },
                    resolution_confidence: row.get(17)?,
                    resolution_strategy: row.get(18)?,
                    receiver_expr: row.get(19)?,
                    arg_count: arg_count.map(|v| v as u32),
                    is_optional_chain: row.get::<_, i64>(21)? != 0,
                    is_awaited: row.get::<_, i64>(22)? != 0,
                    is_constructor: row.get::<_, i64>(23)? != 0,
                    parser_tier: parse_parser_tier(&tier_str),
                    parser_confidence: row.get(25)?,
                    synthesized_by: row.get(26)?,
                    synthesis_key: row.get(27)?,
                    registered_file: row.get(28)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let call_edges: Vec<cc_model::CallEdgeRecord> = ce_rows.filter_map(|r| r.ok()).collect();

        // symbol_refs
        let mut sr_stmt = conn
            .prepare(
                "SELECT ref_id,file_path,symbol_name,container,ref_kind,line,column_no,\
                 target_symbol_id,target_file_path,target_symbol_uid,ref_name,scope_id,\
                 resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence \
                 FROM symbol_refs WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sr_rows = sr_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let resolution_str: String = row.get(12)?;
                let tier_str: String = row.get(17)?;
                Ok(cc_model::SymbolRefRecord {
                    ref_id: row.get(0)?,
                    file_path: row.get(1)?,
                    symbol_name: row.get(2)?,
                    container: row.get(3)?,
                    ref_kind: row.get(4)?,
                    line: row.get(5)?,
                    column: row.get(6)?,
                    target_symbol_id: row.get(7)?,
                    target_file_path: row.get(8)?,
                    target_symbol_uid: row.get(9)?,
                    ref_name: row.get(10)?,
                    scope_id: row.get(11)?,
                    resolution_kind: match resolution_str.as_str() {
                        "exact" => cc_model::ResolutionKind::Exact,
                        "qualified" => cc_model::ResolutionKind::Qualified,
                        "scope_resolved" => cc_model::ResolutionKind::ScopeResolved,
                        "heuristic" => cc_model::ResolutionKind::Heuristic,
                        _ => cc_model::ResolutionKind::Unresolved,
                    },
                    resolution_confidence: row.get(13)?,
                    resolution_strategy: row.get(14)?,
                    ref_end_line: row.get(15)?,
                    ref_end_col: row.get(16)?,
                    parser_tier: parse_parser_tier(&tier_str),
                    parser_confidence: row.get(18)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let symbol_refs: Vec<cc_model::SymbolRefRecord> = sr_rows.filter_map(|r| r.ok()).collect();

        // semantic_edges（只取需要重解析的类型）
        let mut se_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,\
                 target_symbol_uid,relation_kind,line,confidence,parser_tier \
                 FROM semantic_edges WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let se_rows = se_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let relation_str: String = row.get(6)?;
                let tier_str: String = row.get(9)?;
                Ok(cc_model::SemanticEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    source_symbol: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol: row.get(4)?,
                    target_symbol_uid: row.get(5)?,
                    relation_kind: match relation_str.as_str() {
                        "inherits" => cc_model::SemanticRelation::Inherits,
                        "implements" => cc_model::SemanticRelation::Implements,
                        "decorates" => cc_model::SemanticRelation::Decorates,
                        "throws" => cc_model::SemanticRelation::Throws,
                        "uses_type" => cc_model::SemanticRelation::UsesType,
                        "defines" => cc_model::SemanticRelation::Defines,
                        "defines_method" => cc_model::SemanticRelation::DefinesMethod,
                        "contains_file" => cc_model::SemanticRelation::ContainsFile,
                        "contains_module" => cc_model::SemanticRelation::ContainsModule,
                        "renders_component" => cc_model::SemanticRelation::RendersComponent,
                        other => {
                            warn!(kind = %other, "unknown semantic relation_kind in DB, mapping to Unknown");
                            cc_model::SemanticRelation::Unknown
                        }
                    },
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: parse_parser_tier(&tier_str),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let semantic_edges: Vec<cc_model::SemanticEdgeRecord> =
            se_rows.filter_map(|r| r.ok()).collect();

        // dispatch_sites
        let mut ds_stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let ds_rows = ds_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let kind_str: String = row.get(6)?;
                Ok(cc_model::DispatchSiteRecord {
                    site_id: row.get(0)?,
                    file_path: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    enclosing_symbol_uid: row.get(4)?,
                    receiver_expr: row.get(5)?,
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let dispatch_sites: Vec<cc_model::DispatchSiteRecord> =
            ds_rows.filter_map(|r| r.ok()).collect();

        // route_edges
        let mut re_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,\
                 handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,\
                 route_kind,confidence,parser_tier \
                 FROM route_edges WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let re_rows = re_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let tier_str: String = row.get(16)?;
                Ok(cc_model::edge::RouteEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    start_col: row.get(6)?,
                    end_line: row.get(7)?,
                    end_col: row.get(8)?,
                    handler_symbol_id: row.get(9)?,
                    handler_symbol_uid: row.get(10)?,
                    handler_expr: row.get(11)?,
                    router_symbol_uid: row.get(12)?,
                    framework: row.get(13)?,
                    route_kind: row.get(14)?,
                    confidence: row.get(15)?,
                    parser_tier: parse_parser_tier(&tier_str),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let route_edges: Vec<cc_model::edge::RouteEdgeRecord> =
            re_rows.filter_map(|r| r.ok()).collect();

        Ok(FileEdgesForReresolve {
            symbols,
            imports,
            call_edges,
            symbol_refs,
            semantic_edges,
            dispatch_sites,
            route_edges,
        })
    }

    // ── Architecture analysis queries ────────────────────────────

    /// 架构分析：语言分布（带百分比）
    pub fn architecture_languages(&self) -> CcResult<Vec<cc_model::architecture::LanguageStat>> {
        let dist = self.language_distribution()?;
        let total: usize = dist.iter().map(|(_, c)| c).sum();
        Ok(dist
            .into_iter()
            .take(15)
            .map(
                |(language, file_count)| cc_model::architecture::LanguageStat {
                    percentage: if total > 0 {
                        file_count as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                    language,
                    file_count,
                },
            )
            .collect())
    }

    /// 架构分析：包/模块（从文件路径第一级目录推导）
    pub fn architecture_packages(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::PackageInfo>> {
        use std::collections::HashMap;

        let conn = self.read_conn()?;

        // 统计每个包的文件数
        let mut file_stmt = conn
            .prepare("SELECT file_path FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let file_paths: Vec<String> = file_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut pkg_files: HashMap<String, usize> = HashMap::new();
        for fp in &file_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_files.entry(pkg).or_insert(0) += 1;
        }

        // 统计每个包的 symbol 数
        let mut sym_stmt = conn
            .prepare("SELECT file_path FROM symbols")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sym_paths: Vec<String> = sym_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut pkg_symbols: HashMap<String, usize> = HashMap::new();
        for fp in &sym_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_symbols.entry(pkg).or_insert(0) += 1;
        }

        // 从 call_edges 计算跨包 fan_in / fan_out
        // uid → file_path 映射
        let uid_rows = self.query_json(
            "SELECT symbol_uid, file_path FROM symbols WHERE symbol_uid IS NOT NULL",
            &[],
        )?;
        let mut uid_to_pkg: HashMap<String, String> = HashMap::new();
        for row in &uid_rows {
            let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
            let fp = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            if !uid.is_empty() {
                uid_to_pkg.insert(uid.to_string(), Self::extract_package_from_path(fp));
            }
        }

        let all_edges = self.call_uid_edges()?;
        let mut pkg_fan_in: HashMap<String, usize> = HashMap::new();
        let mut pkg_fan_out: HashMap<String, usize> = HashMap::new();
        for (caller_uid, callee_uid) in &all_edges {
            let from_pkg = uid_to_pkg.get(caller_uid.as_str());
            let to_pkg = uid_to_pkg.get(callee_uid.as_str());
            if let (Some(from), Some(to)) = (from_pkg, to_pkg) {
                if from != to {
                    *pkg_fan_out.entry(from.clone()).or_insert(0) += 1;
                    *pkg_fan_in.entry(to.clone()).or_insert(0) += 1;
                }
            }
        }

        let mut pkgs: Vec<cc_model::architecture::PackageInfo> = pkg_files
            .into_iter()
            .map(|(name, file_count)| cc_model::architecture::PackageInfo {
                symbol_count: *pkg_symbols.get(&name).unwrap_or(&0),
                fan_in: *pkg_fan_in.get(&name).unwrap_or(&0),
                fan_out: *pkg_fan_out.get(&name).unwrap_or(&0),
                name,
                file_count,
            })
            .collect();
        pkgs.sort_by(|a, b| b.file_count.cmp(&a.file_count));
        pkgs.truncate(limit);
        Ok(pkgs)
    }

    /// 从文件路径提取包名（跳过 src/lib/app/internal/pkg/cmd 等通用前缀）
    fn extract_package_from_path(file_path: &str) -> String {
        let skip = ["src", "lib", "app", "internal", "pkg", "cmd"];
        let parts: Vec<&str> = file_path.split('/').collect();
        for (idx, part) in parts.iter().enumerate() {
            if idx < parts.len() - 1 && !skip.contains(part) && !part.is_empty() {
                return (*part).to_string();
            }
        }
        parts
            .first()
            .filter(|p| !p.is_empty())
            .copied()
            .unwrap_or("root")
            .to_string()
    }

    /// 架构分析：入口点
    pub fn architecture_entry_points(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::EntryPointInfo>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name, file_path, kind, start_line FROM symbols
                 WHERE name IN ('main', '__main__', 'app', 'server', 'index', 'run', 'start')
                    OR framework_role LIKE '%entry%'
                    OR framework_role LIKE '%handler%'
                 ORDER BY start_line
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(2)?;
                // 从名称推断 entry point 类型
                let ep_kind = if name == "main" || name == "__main__" {
                    "main".to_string()
                } else if kind.contains("route") || name == "index" {
                    "route".to_string()
                } else if kind.contains("test") {
                    "test_suite".to_string()
                } else {
                    "handler".to_string()
                };
                Ok(cc_model::architecture::EntryPointInfo {
                    name,
                    file_path: row.get(1)?,
                    kind: ep_kind,
                    line: row.get::<_, u32>(3).unwrap_or(0),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 架构分析：HTTP 路由
    pub fn architecture_routes(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::RouteInfo>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(method, 'GET'), route_path, COALESCE(handler_name, ''), file_path
                 FROM route_edges
                 ORDER BY route_path
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::RouteInfo {
                    method: row.get(0)?,
                    path: row.get(1)?,
                    handler: row.get(2)?,
                    file_path: row.get(3)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 架构分析：高扇入热点（只统计 fan_in）
    pub fn architecture_hotspots(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::HotspotInfo>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT s.name, s.file_path, s.kind,
                        COUNT(ce.edge_id) as fan_in
                 FROM symbols s
                 JOIN call_edges ce ON ce.callee_symbol = s.name
                 GROUP BY s.name, s.file_path, s.kind
                 ORDER BY fan_in DESC
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::HotspotInfo {
                    name: row.get(0)?,
                    file_path: row.get(1)?,
                    kind: row.get(2)?,
                    fan_in: row.get::<_, usize>(3).unwrap_or(0),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 架构分析：跨包边界
    pub fn architecture_boundaries(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::BoundaryInfo>> {
        use std::collections::HashMap;

        // uid → package 映射
        let uid_rows = self.query_json(
            "SELECT symbol_uid, file_path FROM symbols WHERE symbol_uid IS NOT NULL",
            &[],
        )?;
        let mut uid_to_pkg: HashMap<String, String> = HashMap::new();
        for row in &uid_rows {
            let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
            let fp = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            if !uid.is_empty() {
                uid_to_pkg.insert(uid.to_string(), Self::extract_package_from_path(fp));
            }
        }

        let all_edges = self.call_uid_edges()?;
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for (caller_uid, callee_uid) in &all_edges {
            let from = uid_to_pkg.get(caller_uid.as_str());
            let to = uid_to_pkg.get(callee_uid.as_str());
            if let (Some(from_pkg), Some(to_pkg)) = (from, to) {
                if from_pkg != to_pkg {
                    *counts
                        .entry((from_pkg.clone(), to_pkg.clone()))
                        .or_insert(0) += 1;
                }
            }
        }

        let mut boundaries: Vec<cc_model::architecture::BoundaryInfo> = counts
            .into_iter()
            .map(|((source_package, target_package), call_count)| {
                cc_model::architecture::BoundaryInfo {
                    source_package,
                    target_package,
                    call_count,
                }
            })
            .collect();
        boundaries.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        boundaries.truncate(limit);
        Ok(boundaries)
    }

    /// 架构分析：社区
    pub fn architecture_communities(&self) -> CcResult<Vec<cc_model::architecture::CommunityInfo>> {
        let rows = self.list_communities()?;
        Ok(rows
            .into_iter()
            .map(|c| cc_model::architecture::CommunityInfo {
                id: c.community_id as i64,
                label: c.label,
                member_count: c.member_count as usize,
            })
            .collect())
    }

    /// 综合架构分析
    pub fn get_architecture_info(
        &self,
        aspects: &[&str],
        limit: usize,
    ) -> CcResult<cc_model::architecture::ArchitectureInfo> {
        let all = aspects.is_empty();
        Ok(cc_model::architecture::ArchitectureInfo {
            languages: if all || aspects.contains(&"languages") {
                self.architecture_languages()?
            } else {
                vec![]
            },
            packages: if all || aspects.contains(&"packages") {
                self.architecture_packages(limit)?
            } else {
                vec![]
            },
            entry_points: if all || aspects.contains(&"entry_points") {
                self.architecture_entry_points(limit)?
            } else {
                vec![]
            },
            routes: if all || aspects.contains(&"routes") {
                self.architecture_routes(limit)?
            } else {
                vec![]
            },
            hotspots: if all || aspects.contains(&"hotspots") {
                self.architecture_hotspots(limit)?
            } else {
                vec![]
            },
            boundaries: if all || aspects.contains(&"boundaries") {
                self.architecture_boundaries(limit)?
            } else {
                vec![]
            },
            communities: if all || aspects.contains(&"communities") {
                self.architecture_communities()?
            } else {
                vec![]
            },
        })
    }
}

/// Parse a parser_tier string back into the `ParserTier` enum.
fn parse_parser_tier(s: &str) -> ParserTier {
    match s {
        "generic" => ParserTier::Generic,
        "heuristic" => ParserTier::Heuristic,
        "tree_sitter" => ParserTier::TreeSitter,
        "semantic" => ParserTier::Semantic,
        "verified" => ParserTier::Verified,
        _ => ParserTier::Generic,
    }
}

fn is_actionable_reference_name(name: &str) -> bool {
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

/// Lightweight literal row for search results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LiteralLite {
    pub literal_id: String,
    pub file_path: String,
    pub literal: String,
    pub literal_kind: String,
    pub line: u32,
    pub container: Option<String>,
    pub confidence: f64,
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

/// Lightweight diagnostic row for frontier expansion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiagnosticLite {
    pub diagnostic_id: String,
    pub file_path: String,
    pub severity: String,
    pub message: String,
    pub line: u32,
    pub end_line: Option<u32>,
    pub source: String,
    pub code: Option<String>,
    pub confidence: f64,
    pub symbol_uid: Option<String>,
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

/// Neighbor chunk row for frontier expansion.
#[derive(Debug, Clone)]
pub struct NeighborChunkRow {
    pub chunk_id: String,
    pub file_path: String,
    pub chunk_index: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub breadcrumb: String,
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
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap();
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
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap();
        db.set_metadata("version", "1.0").unwrap();
        assert_eq!(db.get_metadata("version").unwrap(), Some("1.0".to_string()));
        assert_eq!(db.get_metadata("nonexistent").unwrap(), None);
    }

    #[test]
    fn empty_file_state() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap();
        assert!(db.get_file_state().unwrap().is_empty());
    }

    #[test]
    fn resolver_seed_symbols_excludes_requested_files() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("resolver_seed.db")).unwrap();

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
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params!["sym_keep", "src/lib.rs", "helper", "function", "crate.lib.helper", "helper", "uid_keep"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
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
        let db = IndexDb::open(&tmp.path().join("http_chain.db")).unwrap();

        // Insert test data via raw SQL
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();

            // Insert an outbound HTTP call edge: caller_fn_uid calls GET /api/users
            tx.execute(
                "INSERT INTO http_call_edges(edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier)
                 VALUES('hce_1', 'src/client.ts', 'caller_fn_uid', '/api/users', '/api/users', 'GET', 'http', 42, 0.9, 'tree_sitter')",
                [],
            ).unwrap();

            // Insert a route node: GET /api/users → handler_fn_uid
            tx.execute(
                "INSERT INTO route_nodes(route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, normalized_path, confidence, parser_tier)
                 VALUES('rn_1', 'src/server/users.ts', '/api/users', 'GET', 'handler_fn_uid', 'getUsers', 'express', 10, 25, '/api/users', 0.85, 'tree_sitter')",
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
}
