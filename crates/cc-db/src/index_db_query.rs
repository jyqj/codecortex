//! IndexDb methods: symbol/file queries, search, JSON, listing.

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcResult, ParserTier};
use serde_json::Value;

use crate::index_db::{CommunityRow, FileInfoRow, IndexDb, ReadOps, SymbolRow, SymbolTargetRow};
use crate::sql_util::db_err;

/// `(name, kind, file_path, fan_in, fan_out)` for hotspot symbol queries.
impl IndexDb {
    pub(crate) fn find_symbol(
        &self,
        name: &str,
        exact: bool,
        top_k: usize,
    ) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        let (sql, param): (&str, String) = if exact {
            (
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE name = ?1 ORDER BY file_path LIMIT ?2",
                name.to_string(),
            )
        } else if name.len() >= 3 {
            // Trigram-accelerated substring match: symbols_fts is an FTS5 trigram
            // mirror of symbols(name), so a leading-wildcard LIKE is index-served
            // instead of forcing a full table scan. Join back for full columns.
            (
                "SELECT s.symbol_id, s.symbol_uid, s.name, s.kind, s.file_path, s.container, s.start_line, s.end_line, s.qname, s.signature
                 FROM symbols_fts f JOIN symbols s ON s.symbol_id = f.symbol_id
                 WHERE f.name LIKE ?1 ORDER BY s.file_path LIMIT ?2",
                format!("%{}%", name),
            )
        } else {
            // Patterns shorter than 3 chars cannot use trigram acceleration; fall
            // back to a bounded LIKE scan on symbols(name).
            (
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE name LIKE ?1 ORDER BY file_path LIMIT ?2",
                format!("%{}%", name),
            )
        };
        let mut stmt = conn.prepare(sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![param, top_k as i64],
                crate::rows::symbol_row,
            )
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn file_symbols(&self, file_path: &str) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        Self::file_symbols_on(&conn, file_path)
    }

    pub(crate) fn file_symbols_on(
        conn: &rusqlite::Connection,
        file_path: &str,
    ) -> CcResult<Vec<SymbolRow>> {
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature
                 FROM symbols WHERE file_path = ?1 ORDER BY start_line",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], crate::rows::symbol_row)
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn list_symbol_targets(&self) -> CcResult<Vec<SymbolTargetRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, qname, file_path
                 FROM symbols
                 ORDER BY file_path, start_line",
            )
            .map_err(db_err)?;
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
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Symbols seeding the resolver's [`SymbolCatalog`]/`TypeCatalog` on
    /// incremental builds (all persisted symbols outside the re-parsed and
    /// removed files).
    ///
    /// Column contract: the SELECT is trimmed to the fields those two
    /// consumers actually read — catalog entries (symbol_id, file_path,
    /// name, kind, container, qname, export_name, is_default_export,
    /// symbol_uid, start/end_line) plus the type-catalog inputs
    /// (receiver_type, param_count, base_types, implements). The remaining
    /// `SymbolRecord` fields (signature, doc, cols, parser tier/confidence,
    /// parent_symbol_id, framework_role, param/return types) are filled with
    /// defaults; a consumer that starts reading one of them must add the
    /// column back here AND extend `seed_symbol_cache::project_seed` plus
    /// the `symbols_seed` aggregate columns in `signature_agg` in the same
    /// change.
    ///
    /// The full snapshot is served from the cross-build cache on the handle
    /// when the persisted `symbols_seed` aggregate matches (see
    /// `crate::seed_symbol_cache`); databases without a stored aggregate
    /// baseline keep the historical direct load.
    pub(crate) fn resolver_seed_symbols_excluding(
        &self,
        excluded_files: &[String],
    ) -> CcResult<Vec<SymbolRecord>> {
        let conn = self.read_conn()?;
        // Token read and row load must observe ONE snapshot: in autocommit
        // mode each statement snapshots independently, so a write committing
        // between them could pin cache content that does not correspond to
        // the stored token (ABA). An explicit read transaction pins the
        // snapshot at the first SELECT — legal on the pool's query_only
        // connections (BEGIN + SELECT + COMMIT performs no writes; pinned by
        // `read_pool_supports_explicit_read_transaction`).
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        let token = match crate::signature_agg::load_on(&tx)? {
            Some(aggs) => aggs.symbols_seed,
            None => return Self::load_seed_rows_on(&tx, excluded_files),
        };
        if let Some(hit) = self.seed_cache_materialize(token, excluded_files) {
            return Ok(hit);
        }
        // Miss: load the full snapshot (exclusion applied in memory) so it
        // can seed the cache; the surrounding read transaction guarantees the
        // rows match `token`.
        let all = Self::load_seed_rows_on(&tx, &[])?;
        drop(tx); // read-only: rollback and commit are equivalent
        self.seed_cache_store(token, &all);
        if excluded_files.is_empty() {
            return Ok(all);
        }
        let excluded: std::collections::HashSet<&str> =
            excluded_files.iter().map(String::as_str).collect();
        Ok(all
            .into_iter()
            .filter(|s| !excluded.contains(s.file_path.as_str()))
            .collect())
    }

    /// Direct SQL load behind [`Self::resolver_seed_symbols_excluding`].
    pub(crate) fn load_seed_rows_on(
        conn: &rusqlite::Connection,
        excluded_files: &[String],
    ) -> CcResult<Vec<SymbolRecord>> {
        const SEED_COLUMNS: &str = "symbol_id,file_path,name,kind,container,start_line,end_line,\
             qname,export_name,is_default_export,symbol_uid,receiver_type,param_count,\
             base_types,implements";
        let sql = if excluded_files.is_empty() {
            format!("SELECT {SEED_COLUMNS} FROM symbols ORDER BY file_path,start_line")
        } else {
            let placeholders = excluded_files
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "SELECT {SEED_COLUMNS} FROM symbols WHERE file_path NOT IN ({}) ORDER BY file_path,start_line",
                placeholders
            )
        };

        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = excluded_files
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                let kind: String = row.get(3)?;
                let param_count: Option<i64> = row.get(12)?;
                Ok(SymbolRecord {
                    symbol_id: row.get(0)?,
                    file_path: row.get(1)?,
                    name: row.get(2)?,
                    kind: SymbolKind::from_str_lenient(&kind).unwrap_or(SymbolKind::Variable),
                    container: row.get(4)?,
                    start_line: row.get(5)?,
                    end_line: row.get(6)?,
                    start_col: 0,
                    end_col: 0,
                    signature: None,
                    doc: None,
                    parser_tier: ParserTier::Generic,
                    parser_confidence: 0.0,
                    qname: row.get(7)?,
                    parent_symbol_id: None,
                    scope_id: None,
                    export_name: row.get(8)?,
                    is_default_export: row.get::<_, i64>(9)? != 0,
                    symbol_uid: row.get(10)?,
                    framework_role: None,
                    receiver_type: row.get(11)?,
                    param_types: None,
                    return_type: None,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(13)?,
                    implements: row.get(14)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn list_indexed_files(&self) -> CcResult<Vec<FileInfoRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, language, size, parser_tier, indexed_at FROM files ORDER BY file_path",
            )
            .map_err(db_err)?;
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
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn list_file_paths(&self) -> CcResult<Vec<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path FROM files ORDER BY file_path")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn file_is_indexed(&self, file_path: &str) -> CcResult<bool> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT 1 FROM files WHERE file_path = ?1 LIMIT 1")
            .map_err(db_err)?;
        stmt.exists(rusqlite::params![file_path]).map_err(db_err)
    }

    pub(crate) fn list_communities(&self) -> CcResult<Vec<CommunityRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT community_id, label, member_count FROM communities ORDER BY community_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CommunityRow {
                    community_id: row.get(0)?,
                    label: row.get(1)?,
                    member_count: row.get(2)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn list_repo_frameworks(&self) -> CcResult<Vec<(String, f64)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT framework_key, confidence FROM frameworks WHERE scope='repo' ORDER BY confidence DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn language_distribution(&self) -> CcResult<Vec<(String, usize)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT language, COUNT(*) as cnt FROM files GROUP BY language ORDER BY cnt DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Total row count of `table`. The table name must come from a trusted
    /// static catalog (it is interpolated, not bound).
    pub(crate) fn count_table_rows(&self, table: &str) -> CcResult<i64> {
        let conn = self.read_conn()?;
        conn.query_row(
            &format!("SELECT COUNT(*) AS cnt FROM {}", table),
            [],
            |row| row.get(0),
        )
        .map_err(db_err)
    }

    pub(crate) fn query_json(&self, sql: &str, params: &[String]) -> CcResult<Vec<Value>> {
        let conn = self.read_conn()?;
        Self::query_json_on(&conn, sql, params)
    }

    pub(crate) fn query_json_on(
        conn: &rusqlite::Connection,
        sql: &str,
        params: &[String],
    ) -> CcResult<Vec<Value>> {
        let mut stmt = conn.prepare(sql).map_err(db_err)?;
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
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(db_err)?);
        }
        Ok(out)
    }

    pub(crate) fn find_impacted_tests(&self, file_paths: &[String]) -> CcResult<Vec<String>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
        let placeholders: Vec<String> = (1..=file_paths.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT DISTINCT test_file_path FROM test_edges WHERE code_file_path IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        let mut tests = Vec::new();
        for r in rows {
            tests.push(r.map_err(db_err)?);
        }
        Ok(tests)
    }

    pub(crate) fn file_summary(&self, file_path: &str) -> CcResult<Value> {
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
        ).map_err(db_err)?;

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

        let mut fw_stmt = conn.prepare(
            "SELECT framework_key, confidence FROM frameworks WHERE scope='file' AND scope_id=?1 ORDER BY confidence DESC"
        ).map_err(db_err)?;
        let fw_rows = fw_stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(serde_json::json!({
                    "framework": row.get::<_, String>(0)?,
                    "confidence": row.get::<_, f64>(1)?
                }))
            })
            .map_err(db_err)?;
        let frameworks: Vec<Value> = fw_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        obj.insert("frameworks".into(), Value::Array(frameworks));

        Ok(Value::Object(obj))
    }
}

// Read-only facet delegates (see `IndexDb::reads()`).
impl ReadOps<'_> {
    pub fn find_symbol(&self, name: &str, exact: bool, top_k: usize) -> CcResult<Vec<SymbolRow>> {
        self.0.find_symbol(name, exact, top_k)
    }

    pub fn file_symbols(&self, file_path: &str) -> CcResult<Vec<SymbolRow>> {
        self.0.file_symbols(file_path)
    }

    pub fn list_symbol_targets(&self) -> CcResult<Vec<SymbolTargetRow>> {
        self.0.list_symbol_targets()
    }

    pub fn resolver_seed_symbols_excluding(
        &self,
        excluded_files: &[String],
    ) -> CcResult<Vec<SymbolRecord>> {
        self.0.resolver_seed_symbols_excluding(excluded_files)
    }

    pub fn list_indexed_files(&self) -> CcResult<Vec<FileInfoRow>> {
        self.0.list_indexed_files()
    }

    pub fn list_file_paths(&self) -> CcResult<Vec<String>> {
        self.0.list_file_paths()
    }

    pub fn file_is_indexed(&self, file_path: &str) -> CcResult<bool> {
        self.0.file_is_indexed(file_path)
    }

    pub fn list_communities(&self) -> CcResult<Vec<CommunityRow>> {
        self.0.list_communities()
    }

    pub fn list_repo_frameworks(&self) -> CcResult<Vec<(String, f64)>> {
        self.0.list_repo_frameworks()
    }

    pub fn language_distribution(&self) -> CcResult<Vec<(String, usize)>> {
        self.0.language_distribution()
    }

    /// Total row count of `table`. The table name must come from a trusted
    pub fn count_table_rows(&self, table: &str) -> CcResult<i64> {
        self.0.count_table_rows(table)
    }

    pub fn query_json(&self, sql: &str, params: &[String]) -> CcResult<Vec<Value>> {
        self.0.query_json(sql, params)
    }

    pub fn find_impacted_tests(&self, file_paths: &[String]) -> CcResult<Vec<String>> {
        self.0.find_impacted_tests(file_paths)
    }

    pub fn file_summary(&self, file_path: &str) -> CcResult<Value> {
        self.0.file_summary(file_path)
    }
}

#[cfg(test)]
mod tests {
    use crate::index_db::IndexDb;
    use tempfile::TempDir;

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        (db, tmp)
    }

    /// Helper: insert a file row so foreign-key constraints are satisfied.
    fn insert_file(db: &IndexDb, file_path: &str) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
             VALUES(?1,'Rust','hash1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            rusqlite::params![file_path],
        )
        .unwrap();
    }

    #[test]
    fn count_table_rows_counts_and_errors_on_missing_table() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs");
        insert_file(&db, "src/b.rs");

        assert_eq!(db.count_table_rows("files").unwrap(), 2);
        assert_eq!(db.count_table_rows("chunks").unwrap(), 0);
        assert!(db.count_table_rows("no_such_table").is_err());
    }

    /// The seed-cache refill wraps its token read + row load in an explicit
    /// read transaction on a pooled query_only connection (see
    /// `resolver_seed_symbols_excluding`). Pin that SQLite accepts
    /// BEGIN + SELECT + COMMIT there — query_only rejects writes, not
    /// transactions.
    #[test]
    fn read_pool_supports_explicit_read_transaction() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs");

        let conn = db.read_conn().unwrap();
        let tx = conn
            .unchecked_transaction()
            .expect("BEGIN on a query_only connection");
        let count: i64 = tx
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        tx.commit().expect("COMMIT on a query_only connection");
    }

    #[test]
    fn test_find_symbol_exact_and_like() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/main.rs","process_data","Function","",1,10,0,0,"fn process_data()","","tree_sitter",0.8,"process_data","uid_process"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id2","src/main.rs","process_request","Function","",20,30,0,0,"fn process_request()","","tree_sitter",0.8,"process_request","uid_request"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id3","src/main.rs","handle_event","Function","",40,50,0,0,"fn handle_event()","","tree_sitter",0.8,"handle_event","uid_handle"],
            ).unwrap();
            tx.commit().unwrap();
        }

        // Exact match
        let exact = db.find_symbol("process_data", true, 10).unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].name, "process_data");

        // LIKE match: "process" should match both process_data and process_request
        let like = db.find_symbol("process", false, 10).unwrap();
        assert_eq!(like.len(), 2);
        let names: Vec<&str> = like.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"process_data"));
        assert!(names.contains(&"process_request"));

        // Exact match for non-existent returns empty
        let none = db.find_symbol("process", true, 10).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn test_file_symbols() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/lib.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/lib.rs","beta","Function","",20,30,0,0,"fn beta()","","tree_sitter",0.8,"beta","uid_beta"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id2","src/lib.rs","alpha","Function","",1,10,0,0,"fn alpha()","","tree_sitter",0.8,"alpha","uid_alpha"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id3","src/lib.rs","gamma","Function","",50,60,0,0,"fn gamma()","","tree_sitter",0.8,"gamma","uid_gamma"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let syms = db.file_symbols("src/lib.rs").unwrap();
        assert_eq!(syms.len(), 3);
        // Ordered by start_line
        assert_eq!(syms[0].name, "alpha");
        assert_eq!(syms[0].start_line, 1);
        assert_eq!(syms[1].name, "beta");
        assert_eq!(syms[1].start_line, 20);
        assert_eq!(syms[2].name, "gamma");
        assert_eq!(syms[2].start_line, 50);
    }

    #[test]
    fn test_query_json() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/main.rs","main","Function","",1,10,0,0,"fn main()","","tree_sitter",0.8,"main","uid_main"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let results = db
            .query_json(
                "SELECT name, start_line, parser_confidence FROM symbols WHERE symbol_id = ?1",
                &["id1".to_string()],
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        let row = &results[0];
        // String column
        assert_eq!(row["name"], "main");
        // Integer column: start_line = 1
        assert_eq!(row["start_line"], 1);
        // Float column: parser_confidence = 0.8
        let confidence = row["parser_confidence"].as_f64().unwrap();
        assert!((confidence - 0.8).abs() < 1e-9);
    }

    #[test]
    fn test_file_summary() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // Insert 2 symbols
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/main.rs","main","Function","",1,10,0,0,"fn main()","","tree_sitter",0.8,"main","uid_main"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id2","src/main.rs","helper","Function","",20,30,0,0,"fn helper()","","tree_sitter",0.8,"helper","uid_helper"],
            ).unwrap();
            // Insert 1 chunk
            tx.execute(
                "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,token_estimate,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                rusqlite::params!["ch1","src/main.rs","rust",0,1,10,"main","main","Function","fn main() {}",5,"tree_sitter",0.8],
            ).unwrap();
            tx.commit().unwrap();
        }

        let summary = db.file_summary("src/main.rs").unwrap();
        assert_eq!(summary["file_path"], "src/main.rs");
        assert_eq!(summary["language"], "Rust");
        assert_eq!(summary["symbols_count"], 2);
        assert_eq!(summary["chunks_count"], 1);
        assert_eq!(summary["parser_tier"], "tree_sitter");
    }
}
