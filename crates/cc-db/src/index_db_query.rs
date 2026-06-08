//! IndexDb methods: symbol/file queries, search, JSON, listing.

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcError, CcResult};
use serde_json::Value;

use crate::index_db::{
    parse_parser_tier, CommunityRow, FileInfoRow, IndexDb, SymbolRow, SymbolTargetRow,
};

/// `(name, kind, file_path, fan_in, fan_out)` for hotspot symbol queries.
impl IndexDb {
    pub fn find_symbol(&self, name: &str, exact: bool, top_k: usize) -> CcResult<Vec<SymbolRow>> {
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

    pub fn resolver_seed_symbols_excluding(
        &self,
        excluded_files: &[String],
    ) -> CcResult<Vec<SymbolRecord>> {
        let conn = self.read_conn()?;
        let sql = if excluded_files.is_empty() {
            "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements FROM symbols ORDER BY file_path,start_line".to_string()
        } else {
            let placeholders = excluded_files
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements FROM symbols WHERE file_path NOT IN ({}) ORDER BY file_path,start_line",
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
                let param_count: Option<i64> = row.get(22)?;
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
                    scope_id: None,
                    export_name: row.get(15)?,
                    is_default_export: row.get::<_, i64>(16)? != 0,
                    symbol_uid: row.get(17)?,
                    framework_role: row.get(18)?,
                    receiver_type: row.get(19)?,
                    param_types: row.get(20)?,
                    return_type: row.get(21)?,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(23)?,
                    implements: row.get(24)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

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

    pub fn file_is_indexed(&self, file_path: &str) -> CcResult<bool> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT 1 FROM files WHERE file_path = ?1 LIMIT 1")
            .map_err(|e| CcError::Database(e.to_string()))?;
        stmt.exists(rusqlite::params![file_path])
            .map_err(|e| CcError::Database(e.to_string()))
    }

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
                "SELECT framework_key, confidence FROM frameworks WHERE scope='repo' ORDER BY confidence DESC",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

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

        let mut fw_stmt = conn.prepare(
            "SELECT framework_key, confidence FROM frameworks WHERE scope='file' AND scope_id=?1 ORDER BY confidence DESC"
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
