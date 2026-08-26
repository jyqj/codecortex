//! Framework-detection read model: the per-file signal queries and
//! repo-level aggregates consumed by cc-index's `framework_registry`.
//!
//! The per-file half is a **scan session** ([`FrameworkScanSession`]) rather
//! than plain `ReadOps` methods: the registry loops the same 4 signal
//! queries over every indexed file (50k+ on large repos), so the session
//! holds ONE pooled connection for its lifetime and runs every query through
//! `prepare_cached` — statements are compiled once per session and reused
//! across the whole loop, exactly the profile the registry used to get from
//! a raw `read_conn()` checkout. Naive per-file typed calls would re-acquire
//! a connection and recompile 4 statements per file, a measured regression
//! on the `fw_detect_all_files` build step.
//!
//! Per-file signal methods degrade to empty on error (missing table, decode
//! failure): detection is best-effort by contract — a failed signal scan
//! means "no evidence", never a failed build.

use std::collections::HashMap;

use cc_model::CcResult;
use r2d2_sqlite::SqliteConnectionManager;

use crate::index_db::{read_chunk_text_with_encoding, IndexDb, ReadOps};
use crate::sql_util::db_err;

/// Per-file framework-signal scan session over one pooled read connection.
///
/// Obtained via [`IndexDb::framework_scan`]; hold it across a multi-file
/// detection loop so `prepare_cached` statements survive between files.
pub struct FrameworkScanSession {
    conn: r2d2::PooledConnection<SqliteConnectionManager>,
}

impl IndexDb {
    /// Check out a pooled read connection wrapped as a framework-signal scan
    /// session. See [`FrameworkScanSession`].
    pub fn framework_scan(&self) -> CcResult<FrameworkScanSession> {
        Ok(FrameworkScanSession {
            conn: self.read_conn()?,
        })
    }
}

impl FrameworkScanSession {
    /// Declared import strings of one file (`imports` table).
    pub fn file_import_strings(&self, file_path: &str) -> Vec<String> {
        self.conn
            .prepare_cached("SELECT import_string FROM imports WHERE file_path = ?1")
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    }

    /// Decoded text of the file's first `max_chunks` chunks, joined with a
    /// space — the CommonJS `require()` fallback's scan window.
    pub fn file_head_chunk_text(&self, file_path: &str, max_chunks: usize) -> String {
        self.conn
            .prepare_cached(
                "SELECT text, text_encoding FROM chunks WHERE file_path = ?1 ORDER BY chunk_index LIMIT ?2",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map(rusqlite::params![file_path, max_chunks as i64], |row| {
                    read_chunk_text_with_encoding(row, 0, 1)
                })
                .ok()
                .map(|rows| {
                    rows.filter_map(|r| r.ok())
                        .collect::<Vec<String>>()
                        .join(" ")
                })
            })
            .unwrap_or_default()
    }

    /// Distinct non-empty `framework` values of the file's route edges.
    pub fn file_route_frameworks(&self, file_path: &str) -> Vec<String> {
        self.conn
            .prepare_cached(
                "SELECT DISTINCT framework FROM routes WHERE file_path = ?1 AND framework IS NOT NULL AND framework != ''",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    }

    /// Non-NULL `framework_role` values of the file's symbols.
    pub fn file_symbol_roles(&self, file_path: &str) -> Vec<String> {
        self.conn
            .prepare_cached(
                "SELECT framework_role FROM symbols WHERE file_path = ?1 AND framework_role IS NOT NULL",
            )
            .ok()
            .and_then(|mut stmt| {
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
                    .ok()
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
            })
            .unwrap_or_default()
    }

    /// Whether the file is present in the index (existence probe used by
    /// incremental detection to skip removed files).
    pub fn file_is_indexed(&self, file_path: &str) -> bool {
        self.conn
            .prepare_cached("SELECT 1 FROM files WHERE file_path = ?1")
            .ok()
            .map(|mut stmt| {
                stmt.query_row(rusqlite::params![file_path], |_| Ok(()))
                    .is_ok()
            })
            .unwrap_or(false)
    }
}

/// `(framework_key, file_count, max_confidence)` aggregate over persisted
/// file-scope framework detections.
pub type FileFrameworkAggregate = (String, i64, f64);

impl IndexDb {
    /// Aggregate persisted file-scope detections per framework key
    /// (`COUNT(*)`, `MAX(confidence)`), the repo-level scoring input.
    pub(crate) fn file_framework_aggregates(&self) -> CcResult<Vec<FileFrameworkAggregate>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT framework_key, COUNT(*) as cnt, MAX(confidence) as max_conf \
                 FROM frameworks WHERE scope='file' GROUP BY framework_key",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Distinct non-empty `framework` values across all route edges (the
    /// repo-level route-framework signal).
    pub(crate) fn distinct_route_frameworks(&self) -> CcResult<Vec<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT framework FROM routes WHERE framework IS NOT NULL AND framework != ''",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// `{file_path: [(framework_key, confidence), ...]}` for a set of files,
    /// per-file lists ordered by confidence descending.
    ///
    /// Batched by `IN` group so arbitrarily large file sets stay under
    /// SQLite's bound-variable limit; a file's rows always land in a single
    /// batch, so per-file ordering is unaffected.
    pub(crate) fn frameworks_for_files(
        &self,
        file_paths: &[&str],
    ) -> CcResult<HashMap<String, Vec<(String, f64)>>> {
        let mut result: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        if file_paths.is_empty() {
            return Ok(result);
        }
        let conn = self.read_conn()?;
        for batch in file_paths.chunks(crate::sql_util::IN_BATCH_SIZE) {
            let placeholders: String = batch.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT scope_id, framework_key, confidence FROM frameworks \
                 WHERE scope='file' AND scope_id IN ({}) ORDER BY confidence DESC",
                placeholders
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, f64>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (fp, fw_key, conf) = row.map_err(db_err)?;
                result.entry(fp).or_default().push((fw_key, conf));
            }
        }
        Ok(result)
    }
}

impl ReadOps<'_> {
    /// See [`IndexDb::file_framework_aggregates`].
    pub fn file_framework_aggregates(&self) -> CcResult<Vec<FileFrameworkAggregate>> {
        self.0.file_framework_aggregates()
    }

    /// See [`IndexDb::distinct_route_frameworks`].
    pub fn distinct_route_frameworks(&self) -> CcResult<Vec<String>> {
        self.0.distinct_route_frameworks()
    }

    /// See [`IndexDb::frameworks_for_files`].
    pub fn frameworks_for_files(
        &self,
        file_paths: &[&str],
    ) -> CcResult<HashMap<String, Vec<(String, f64)>>> {
        self.0.frameworks_for_files(file_paths)
    }
}
