//! Lane hit generators for SearchEngine.
//!
//! Each method queries a specific data lane (paths, symbols, literals, routes,
//! diagnostics, frameworks) and returns `SearchHit` results tagged with the lane.

use std::collections::HashSet;

use cc_db::index_db::read_chunk_text;
use cc_model::search::SearchHit;

use crate::engine::{parse_language_name, SearchEngine};

impl SearchEngine {
    /// Look up files by path, create SearchHit per chunk in those files.
    pub fn hits_for_paths(&self, paths: &[&str], top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &path in paths {
            let mut stmt = match conn.prepare(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                 symbol_name, symbol_kind, text \
                 FROM chunks WHERE file_path = ?1 ORDER BY chunk_index LIMIT 2",
            ) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rows = match stmt.query_map(rusqlite::params![path], |row| {
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
            }) {
                Ok(r) => r,
                Err(_) => continue,
            };
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
                    symbol_kind: sk
                        .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                    text,
                    fused_score: 1.2,
                    vector_score: 0.0,
                    lexical_score: 0.0,
                    grep_score: 0.0,
                    graph_score: 0.0,
                    rerank_score: 1.2,
                    reasons: vec!["path-exact".into()],
                    source: "index".into(),
                    lane: Some("path".into()),
                    metadata: serde_json::json!({}),
                });
                if hits.len() >= top_k {
                    return hits;
                }
            }
        }
        hits
    }

    /// Query symbols by name, return chunk-based hits.
    pub fn hits_for_symbols(&self, names: &[&str], top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &name in names {
            let symbols = match self.db.find_symbol(name, false, 6) {
                Ok(s) => s,
                Err(_) => continue,
            };
            for sym in &symbols {
                let row = conn.query_row(
                    "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                     symbol_name, symbol_kind, text \
                     FROM chunks \
                     WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                     ORDER BY ABS(start_line - ?2) ASC, chunk_index ASC LIMIT 1",
                    rusqlite::params![sym.file_path, sym.start_line],
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
                );
                let (cid, fp, lang, sl, el, bc, sn, sk, text) = match row {
                    Ok(r) => r,
                    Err(_) => continue,
                };
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
                    symbol_name: sn.or_else(|| Some(sym.name.clone())),
                    symbol_kind: sk
                        .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s))
                        .or_else(|| cc_model::symbol::SymbolKind::from_str_lenient(&sym.kind)),
                    text,
                    fused_score: 1.3,
                    vector_score: 0.0,
                    lexical_score: 0.0,
                    grep_score: 0.0,
                    graph_score: 0.0,
                    rerank_score: 1.3,
                    reasons: vec![format!("symbol-exact:{}", name)],
                    source: "index".into(),
                    lane: Some("symbol".into()),
                    metadata: serde_json::json!({
                        "symbol_id": sym.symbol_id,
                        "qname": sym.qname,
                    }),
                });
                if hits.len() >= top_k {
                    return hits;
                }
            }
        }
        hits
    }

    /// Query symbols by stable UID, return chunk-based hits.
    pub fn hits_for_symbol_uids(&self, uids: &[&str], top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &uid in uids {
            let sym = match self.symbol_by_uid(uid) {
                Some(s) => s,
                None => continue,
            };
            let row = conn.query_row(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                 symbol_name, symbol_kind, text \
                 FROM chunks \
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                 ORDER BY ABS(start_line - ?2) ASC, chunk_index ASC LIMIT 1",
                rusqlite::params![sym.file_path, sym.start_line],
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
                        row.get::<_, String>(8)?,
                    ))
                },
            );
            let (cid, fp, lang, sl, el, bc, sn, sk, text) = match row {
                Ok(r) => r,
                Err(_) => continue,
            };
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
                symbol_name: sn.or_else(|| Some(sym.name.clone())),
                symbol_kind: sk
                    .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s))
                    .or_else(|| cc_model::symbol::SymbolKind::from_str_lenient(&sym.kind)),
                text,
                fused_score: 1.35,
                vector_score: 0.0,
                lexical_score: 0.0,
                grep_score: 0.0,
                graph_score: 0.0,
                rerank_score: 1.35,
                reasons: vec![format!("symbol-uid:{}", uid)],
                source: "index".into(),
                lane: Some("symbol".into()),
                metadata: serde_json::json!({
                    "symbol_id": sym.symbol_id,
                    "symbol_uid": sym.symbol_uid,
                    "qname": sym.qname,
                }),
            });
            if hits.len() >= top_k {
                return hits;
            }
        }
        hits
    }

    /// Use db.search_literals() to find string/number literals, return chunk-based hits.
    pub fn hits_for_literals(
        &self,
        literals: &[&str],
        kind: Option<&str>,
        top_k: usize,
    ) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &lit in literals {
            let lit_rows = match self.db.search_literals(lit, kind, 6) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for lr in &lit_rows {
                // Find the chunk containing this literal's line
                let chunk = conn.query_row(
                    "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                     symbol_name, symbol_kind, text \
                     FROM chunks \
                     WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                     ORDER BY chunk_index ASC LIMIT 1",
                    rusqlite::params![lr.file_path, lr.line],
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
                            row.get::<_, String>(8)?,
                        ))
                    },
                );
                let (cid, fp, lang, sl, el, bc, sn, sk, text) = match chunk {
                    Ok(r) => r,
                    Err(_) => continue,
                };
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
                    symbol_kind: sk
                        .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                    text,
                    fused_score: 1.1,
                    vector_score: 0.0,
                    lexical_score: 0.0,
                    grep_score: 0.0,
                    graph_score: 0.0,
                    rerank_score: 1.1,
                    reasons: vec![format!("literal-lane:{}", lit)],
                    source: "index".into(),
                    lane: Some("literal".into()),
                    metadata: serde_json::json!({
                        "literal": lr.literal,
                        "literal_kind": lr.literal_kind,
                    }),
                });
                if hits.len() >= top_k {
                    return hits;
                }
                break; // one chunk per literal row
            }
        }
        hits
    }

    /// Query route_edges matching patterns, return chunk-based hits.
    pub fn hits_for_routes(&self, patterns: &[&str], top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &pattern in patterns {
            let route_rows = match self.db.route_rows_by_path(pattern, 4) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for rr in &route_rows {
                // Find the chunk containing this route's handler line
                let chunk = conn.query_row(
                    "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                     symbol_name, symbol_kind, text \
                     FROM chunks \
                     WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                     ORDER BY chunk_index ASC LIMIT 1",
                    rusqlite::params![rr.file_path, rr.line],
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
                            row.get::<_, String>(8)?,
                        ))
                    },
                );
                let (cid, fp, lang, sl, el, bc, sn, sk, text) = match chunk {
                    Ok(r) => r,
                    Err(_) => continue,
                };
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
                    symbol_kind: sk
                        .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                    text,
                    fused_score: 1.2,
                    vector_score: 0.0,
                    lexical_score: 0.0,
                    grep_score: 0.0,
                    graph_score: 0.0,
                    rerank_score: 1.2,
                    reasons: vec![format!("route-lane:{}", pattern)],
                    source: "index".into(),
                    lane: Some("route".into()),
                    metadata: serde_json::json!({}),
                });
                if hits.len() >= top_k {
                    return hits;
                }
            }
        }
        hits
    }

    /// Use db.diagnostic_rows_by_message() for FTS on diagnostics, return chunk-based hits.
    pub fn hits_for_diagnostics(&self, messages: &[&str], top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();
        for &msg in messages {
            let diag_rows = match self.db.diagnostic_rows_by_message(msg, 6) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for dr in &diag_rows {
                let chunk = conn.query_row(
                    "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                     symbol_name, symbol_kind, text \
                     FROM chunks \
                     WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                     ORDER BY chunk_index ASC LIMIT 1",
                    rusqlite::params![dr.file_path, dr.line],
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
                            row.get::<_, String>(8)?,
                        ))
                    },
                );
                let (cid, fp, lang, sl, el, bc, sn, sk, text) = match chunk {
                    Ok(r) => r,
                    Err(_) => continue,
                };
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
                    symbol_kind: sk
                        .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                    text,
                    fused_score: 1.15,
                    vector_score: 0.0,
                    lexical_score: 0.0,
                    grep_score: 0.0,
                    graph_score: 0.0,
                    rerank_score: 1.15,
                    reasons: vec![format!("diagnostic-lane:{}", msg)],
                    source: "index".into(),
                    lane: Some("diagnostic".into()),
                    metadata: serde_json::json!({
                        "diagnostic_message": dr.message,
                        "severity": dr.severity,
                    }),
                });
                if hits.len() >= top_k {
                    return hits;
                }
                break; // one chunk per diagnostic row
            }
        }
        hits
    }

    /// Query files associated with a framework, their routes, and key symbols.
    pub fn hits_for_framework(&self, framework_key: &str, top_k: usize) -> Vec<SearchHit> {
        let conn = match self.db.read_conn() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut hits = Vec::new();
        let mut seen: HashSet<(String, u32, u32)> = HashSet::new();

        // 1. Framework symbols (via framework_role column)
        if let Ok(mut stmt) = conn.prepare(
            "SELECT s.file_path, s.start_line, s.name, s.kind, s.symbol_id, s.qname \
             FROM symbols s \
             WHERE s.framework_role LIKE ?1 \
             LIMIT 6",
        ) {
            let pattern = format!("%{}%", framework_key);
            if let Ok(rows) = stmt.query_map(rusqlite::params![pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            }) {
                for sym_row in rows.flatten() {
                    let (fp, sl, name, kind, symbol_id, qname) = sym_row;
                    let chunk = conn.query_row(
                        "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                         symbol_name, symbol_kind, text \
                         FROM chunks \
                         WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 \
                         ORDER BY ABS(start_line - ?2) ASC, chunk_index ASC LIMIT 1",
                        rusqlite::params![fp, sl],
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
                                row.get::<_, String>(8)?,
                            ))
                        },
                    );
                    if let Ok((cid, fp2, lang, sl2, el2, bc, sn, sk, text)) = chunk {
                        let key = (fp2.clone(), sl2, el2);
                        if seen.insert(key) {
                            hits.push(SearchHit {
                                chunk_id: cid,
                                file_path: fp2,
                                language: parse_language_name(&lang),
                                start_line: sl2,
                                end_line: el2,
                                breadcrumb: bc,
                                symbol_name: sn.or(Some(name)),
                                symbol_kind: sk
                                    .and_then(|s| {
                                        cc_model::symbol::SymbolKind::from_str_lenient(&s)
                                    })
                                    .or_else(|| {
                                        cc_model::symbol::SymbolKind::from_str_lenient(&kind)
                                    }),
                                text,
                                fused_score: 1.25,
                                vector_score: 0.0,
                                lexical_score: 0.0,
                                grep_score: 0.0,
                                graph_score: 0.0,
                                rerank_score: 1.25,
                                reasons: vec![
                                    format!("framework-lane:{}", framework_key),
                                    format!(
                                        "framework-symbol:{}",
                                        qname.as_deref().unwrap_or(&symbol_id)
                                    ),
                                ],
                                source: "index".into(),
                                lane: Some("framework".into()),
                                metadata: serde_json::json!({"framework_key": framework_key}),
                            });
                            if hits.len() >= top_k {
                                return hits;
                            }
                        }
                    }
                }
            }
        }

        // 2. Files associated via file_frameworks
        if let Ok(mut stmt) = conn.prepare(
            "SELECT file_path FROM file_frameworks WHERE framework_key = ?1 \
             ORDER BY confidence DESC LIMIT 6",
        ) {
            if let Ok(rows) = stmt.query_map(rusqlite::params![framework_key], |row| {
                row.get::<_, String>(0)
            }) {
                for fp_row in rows.flatten() {
                    let chunk = conn.query_row(
                        "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                         symbol_name, symbol_kind, text \
                         FROM chunks WHERE file_path = ?1 ORDER BY chunk_index LIMIT 1",
                        rusqlite::params![fp_row],
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
                                row.get::<_, String>(8)?,
                            ))
                        },
                    );
                    if let Ok((cid, fp, lang, sl, el, bc, sn, sk, text)) = chunk {
                        let key = (fp.clone(), sl, el);
                        if seen.insert(key) {
                            hits.push(SearchHit {
                                chunk_id: cid,
                                file_path: fp,
                                language: parse_language_name(&lang),
                                start_line: sl,
                                end_line: el,
                                breadcrumb: bc,
                                symbol_name: sn,
                                symbol_kind: sk.and_then(|s| {
                                    cc_model::symbol::SymbolKind::from_str_lenient(&s)
                                }),
                                text,
                                fused_score: 1.15,
                                vector_score: 0.0,
                                lexical_score: 0.0,
                                grep_score: 0.0,
                                graph_score: 0.0,
                                rerank_score: 1.15,
                                reasons: vec![format!("framework-lane:{}", framework_key)],
                                source: "index".into(),
                                lane: Some("framework".into()),
                                metadata: serde_json::json!({"framework_key": framework_key}),
                            });
                            if hits.len() >= top_k {
                                return hits;
                            }
                        }
                    }
                }
            }
        }

        hits
    }
}
