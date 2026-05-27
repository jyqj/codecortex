//! Graph navigation, call-graph traversal, symbol metadata, and auxiliary
//! queries for SearchEngine.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use cc_db::index_db::{
    read_chunk_text, CallEdgeLite, DiagnosticLite, LiteralLite, NeighborChunkRow, RouteEdgeLite,
    SymbolRefLite, SymbolRow,
};
use cc_model::search::SearchHit;
use cc_model::{CcError, CcResult};

use crate::engine::{parse_language_name, SearchEngine};

impl SearchEngine {
    // ══════════════════════════════════════════════════════════════
    // Graph navigation
    // ══════════════════════════════════════════════════════════════

    /// Find files connected via imports (both directions), scored by edge count.
    pub fn graph_neighbor_files(&self, file_path: &str, limit: usize) -> Vec<(String, f64)> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut scores: HashMap<String, f64> = HashMap::new();

        // Forward imports: files this file imports
        if let Ok(mut stmt) = conn.prepare(
            "SELECT resolved_path FROM imports \
             WHERE file_path = ?1 AND resolved_path IS NOT NULL LIMIT 12",
        ) {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    *scores.entry(row).or_insert(0.0) += 1.0;
                }
            }
        }

        // Reverse imports: files that import this file
        if let Ok(mut stmt) =
            conn.prepare("SELECT file_path FROM imports WHERE resolved_path = ?1 LIMIT 12")
        {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    *scores.entry(row).or_insert(0.0) += 0.95;
                }
            }
        }

        // Test edges
        if let Ok(mut stmt) =
            conn.prepare("SELECT test_file_path FROM test_edges WHERE code_file_path = ?1 LIMIT 8")
        {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    *scores.entry(row).or_insert(0.0) += 0.9;
                }
            }
        }
        if let Ok(mut stmt) =
            conn.prepare("SELECT code_file_path FROM test_edges WHERE test_file_path = ?1 LIMIT 8")
        {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    *scores.entry(row).or_insert(0.0) += 0.9;
                }
            }
        }

        let mut ranked: Vec<(String, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        ranked.truncate(limit);
        ranked
    }

    /// Callers of a symbol identified by name.
    pub fn caller_rows_by_name(&self, symbol_name: &str) -> Vec<CallEdgeLite> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(
            "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, \
             callee_symbol_uid, resolution_kind, parser_confidence, dispatch_kind, \
             synthesized_by, synthesis_key, registered_file, registered_line \
             FROM call_edges \
             WHERE lower(callee_symbol) = lower(?1) \
                OR lower(callee_symbol) LIKE lower(?2) \
             ORDER BY file_path, line LIMIT 20",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let short = symbol_name.rsplit('.').next().unwrap_or(symbol_name);
        let like_pattern = format!("%.{}", short);
        let rows = match stmt.query_map(rusqlite::params![symbol_name, like_pattern], |row| {
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
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Callers of a symbol identified by UID. Delegates to db.
    pub fn caller_rows_by_uid(&self, symbol_uid: &str) -> Vec<CallEdgeLite> {
        self.db
            .caller_rows_by_uid(symbol_uid, 20)
            .unwrap_or_default()
    }

    /// Callees of a symbol identified by name.
    pub fn callee_rows_by_name(&self, symbol_name: &str) -> Vec<CallEdgeLite> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(
            "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, \
             callee_symbol_uid, resolution_kind, parser_confidence, dispatch_kind, \
             synthesized_by, synthesis_key, registered_file, registered_line \
             FROM call_edges \
             WHERE lower(caller_symbol) = lower(?1) \
                OR lower(caller_symbol) LIKE lower(?2) \
             ORDER BY file_path, line LIMIT 20",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let short = symbol_name.rsplit('.').next().unwrap_or(symbol_name);
        let like_pattern = format!("%.{}", short);
        let rows = match stmt.query_map(rusqlite::params![symbol_name, like_pattern], |row| {
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
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Callees of a symbol identified by UID. Delegates to db.
    pub fn callee_rows_by_uid(&self, symbol_uid: &str) -> Vec<CallEdgeLite> {
        self.db
            .callee_rows_by_uid(symbol_uid, 20)
            .unwrap_or_default()
    }

    /// Route edge rows by route path. Delegates to db.
    pub fn route_rows_by_path(&self, route_path: &str) -> Vec<RouteEdgeLite> {
        self.db
            .route_rows_by_path(route_path, 20)
            .unwrap_or_default()
    }

    /// Route edge rows by handler symbol UID. Delegates to db.
    pub fn route_rows_by_handler(&self, handler_uid: &str) -> Vec<RouteEdgeLite> {
        self.db
            .route_rows_by_handler_uid(handler_uid, 20)
            .unwrap_or_default()
    }

    /// Query symbol_refs table, optionally filter by resolution_kind != "unresolved".
    pub fn symbol_reference_rows(
        &self,
        symbol_name: &str,
        include_unresolved: bool,
    ) -> CcResult<Vec<SymbolRefLite>> {
        let conn = self.db.read_conn()?;
        let short = symbol_name.rsplit('.').next().unwrap_or(symbol_name);
        let like_pattern = format!("%.{}", short);
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if include_unresolved {
            (
                "SELECT file_path, line, symbol_name, target_symbol_uid, resolution_kind, \
                 parser_confidence \
                 FROM symbol_refs \
                 WHERE lower(symbol_name) = lower(?1) \
                    OR lower(symbol_name) = lower(?2) \
                    OR lower(symbol_name) LIKE lower(?3) \
                 ORDER BY file_path, line LIMIT 20"
                    .into(),
                vec![
                    Box::new(symbol_name.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(short.to_string()),
                    Box::new(like_pattern),
                ],
            )
        } else {
            (
                "SELECT file_path, line, symbol_name, target_symbol_uid, resolution_kind, \
                 parser_confidence \
                 FROM symbol_refs \
                 WHERE (lower(symbol_name) = lower(?1) \
                    OR lower(symbol_name) = lower(?2) \
                    OR lower(symbol_name) LIKE lower(?3)) \
                   AND resolution_kind != 'unresolved' \
                 ORDER BY file_path, line LIMIT 20"
                    .into(),
                vec![
                    Box::new(symbol_name.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(short.to_string()),
                    Box::new(like_pattern),
                ],
            )
        };
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
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

    /// FTS on diagnostics. Delegates to db.
    pub fn diagnostic_rows(&self, query: &str) -> CcResult<Vec<DiagnosticLite>> {
        self.db.diagnostic_rows_by_message(query, 20)
    }

    /// FTS on literals. Delegates to db.
    pub fn literal_rows(&self, query: &str, kind: Option<&str>) -> CcResult<Vec<LiteralLite>> {
        self.db.search_literals(query, kind, 20)
    }

    // ══════════════════════════════════════════════════════════════
    // Chunk navigation
    // ══════════════════════════════════════════════════════════════

    /// Get neighboring chunks for a given chunk_id. Delegates to db.
    pub fn neighbor_chunks(&self, chunk_id: &str, window: usize) -> Vec<NeighborChunkRow> {
        let (file_path, chunk_index) = match self.db.chunk_index_by_id(chunk_id) {
            Ok(Some(pair)) => pair,
            _ => return Vec::new(),
        };
        self.db
            .neighbor_chunks(&file_path, chunk_index, window)
            .unwrap_or_default()
    }

    /// Find test files by convention: test_X, X_test, X.spec, X.test.
    pub fn test_related_chunks(&self, file_path: &str) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let stem = Path::new(file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if stem.is_empty() {
            return Vec::new();
        }

        let mut stmt = match conn.prepare(
            "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
             symbol_name, symbol_kind, text \
             FROM chunks \
             WHERE file_path != ?1 AND ( \
                 lower(file_path) LIKE ?2 OR lower(file_path) LIKE ?3 \
                 OR lower(file_path) LIKE ?4 OR lower(file_path) LIKE ?5 \
             ) \
             ORDER BY CASE WHEN lower(file_path) LIKE ?2 THEN 0 ELSE 1 END, \
                      file_path, chunk_index \
             LIMIT 8",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let pat_test_x = format!("%test%{}%", stem);
        let pat_x_test = format!("%{}%test%", stem);
        let pat_spec = format!("%/{}.spec%", stem);
        let pat_dot_test = format!("%/{}.test%", stem);
        let rows = match stmt.query_map(
            rusqlite::params![file_path, pat_test_x, pat_x_test, pat_spec, pat_dot_test],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    read_chunk_text(row, 8)?,
                ))
            },
        ) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for row in rows.flatten() {
            let (cid, fp, lang, sl, el, bc, sn, sk, text) = row;
            let key = (fp.clone(), sl, el);
            if !seen.insert(key) {
                continue;
            }
            hits.push(SearchHit {
                chunk_id: cid,
                file_path: fp,
                language: parse_language_name(&lang),
                start_line: sl,
                end_line: el,
                breadcrumb: bc,
                symbol_name: sn,
                symbol_kind: sk.and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                text,
                fused_score: 1.0,
                vector_score: 0.0,
                lexical_score: 0.0,
                grep_score: 0.0,
                graph_score: 0.0,
                rerank_score: 1.0,
                reasons: vec!["test-related".into()],
                source: "index".into(),
                lane: Some("test".into()),
                metadata: serde_json::json!({}),
            });
        }
        hits
    }

    /// Return (imports, reverse_imports) — files this file imports and files that import it.
    pub fn import_graph(&self, file_path: &str) -> (Vec<String>, Vec<String>) {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return (Vec::new(), Vec::new()),
        };
        let mut imports = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT import_string, resolved_path FROM imports \
             WHERE file_path = ?1 ORDER BY import_string",
        ) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![file_path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            }) {
                for row in rows.flatten() {
                    imports.push(row.1.unwrap_or(row.0));
                }
            }
        }

        let mut reverse_imports = Vec::new();
        if let Ok(mut stmt) = conn
            .prepare("SELECT file_path FROM imports WHERE resolved_path = ?1 ORDER BY file_path")
        {
            if let Ok(rows) =
                stmt.query_map(rusqlite::params![file_path], |row| row.get::<_, String>(0))
            {
                for row in rows.flatten() {
                    reverse_imports.push(row);
                }
            }
        }

        (imports, reverse_imports)
    }

    // ══════════════════════════════════════════════════════════════
    // Metadata
    // ══════════════════════════════════════════════════════════════

    /// Symbols whose span contains the given line, ordered by span size (smallest first).
    pub fn symbols_covering(&self, file_path: &str, line: u32) -> Vec<SymbolRow> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut stmt = match conn.prepare(
            "SELECT symbol_id, symbol_uid, name, kind, file_path, container, \
             start_line, end_line, qname, signature \
             FROM symbols \
             WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
             ORDER BY (end_line - start_line) ASC \
             LIMIT 12",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map(rusqlite::params![file_path, line], |row| {
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
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Lookup a symbol by its stable UID.
    pub fn symbol_by_uid(&self, uid: &str) -> Option<SymbolRow> {
        let conn = self.db.read_conn().ok()?;
        conn.query_row(
            "SELECT symbol_id, symbol_uid, name, kind, file_path, container, \
             start_line, end_line, qname, signature \
             FROM symbols WHERE symbol_uid = ?1 LIMIT 1",
            rusqlite::params![uid],
            |row| {
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
            },
        )
        .ok()
    }

    /// List indexed files, optionally filtered by glob pattern or language.
    pub fn list_files(&self, pattern: Option<&str>, language: Option<&str>) -> Vec<String> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut clauses = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(pat) = pattern {
            clauses.push("file_path LIKE ?".to_string());
            params.push(Box::new(pat.replace('*', "%")));
        }
        if let Some(lang) = language {
            clauses.push("language = ?".to_string());
            params.push(Box::new(lang.to_string()));
        }
        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT file_path FROM files {} ORDER BY file_path LIMIT 1000",
            where_clause
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = match stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0)) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Force SQLite WAL checkpoint for reader sync.
    pub fn refresh(&self) -> CcResult<()> {
        let conn = self.db.read_conn()?;
        // End the current read-transaction so the next query picks up WAL writes.
        conn.execute_batch("BEGIN; END;")
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }
}
