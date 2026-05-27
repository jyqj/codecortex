//! SearchEngine — hybrid vector + lexical + grep retrieval with RRF fusion.
//!
//! Extended with multi-lane hit generators and graph/navigation queries.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use cc_db::fts::{expand_query_text, sanitize_fts_query, tokenize_codeish};
use cc_db::index_db::{
    read_chunk_text, CallEdgeLite, DiagnosticLite, IndexDb, LiteralLite, NeighborChunkRow,
    RouteEdgeLite, SymbolRefLite, SymbolRow,
};
use cc_model::config::{ProjectStats, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcError, CcResult, Language};

use crate::embeddings::{cosine_similarity, get_embedder, unpack_vector, Embedder};
use crate::rrf::rrf_accumulate;

pub struct SearchEngine {
    db: Arc<IndexDb>,
    embedder: Box<dyn Embedder>,
    config: SearchConfig,
}

fn augmented_query_text(request: &SearchRequest) -> String {
    let mut parts = Vec::new();
    let primary = request.query.trim();
    if !primary.is_empty() {
        parts.push(primary.to_string());
    }
    if let Some(extra) = &request.conversation_queries {
        for query in extra.iter().rev().take(4) {
            let q = query.trim();
            if !q.is_empty() && !parts.iter().any(|existing| existing == q) {
                parts.push(q.to_string());
            }
        }
    }
    parts.join("\n")
}

impl SearchEngine {
    pub fn new(db: Arc<IndexDb>, config: &cc_model::ProjectConfig) -> Self {
        let embedder = get_embedder(&config.embeddings);
        Self {
            db,
            embedder,
            config: config.search.clone(),
        }
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.stats(std::path::Path::new(""))
    }

    /// Core search — hybrid vector + lexical + grep with RRF fusion and reranking.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Vec<SearchHit>> {
        let conn = self.db.read_conn()?;

        // ── DSL filter extraction ──────────────────────────────
        let parsed = crate::dsl::parse_search_dsl(&request.query);
        let mut request = request.clone();

        // Apply DSL path_filter as path_prefix (if not already set).
        if parsed.path_filter.is_some() && request.path_prefix.is_none() {
            request.path_prefix = parsed.path_filter.clone();
        }

        // Apply DSL lang_filter as languages constraint (if not already set).
        if let Some(ref lang_str) = parsed.lang_filter {
            if request.languages.is_none() {
                let lang = cc_model::Language::from_name(lang_str);
                if lang != cc_model::Language::Unknown {
                    request.languages = Some(vec![lang]);
                }
            }
        }

        // Replace query with cleaned text (filters stripped).
        if !parsed.text.is_empty() {
            request.query = parsed.text.clone();
        }
        // If only filters were given with no free text, keep original query for embedding.

        let query_text = augmented_query_text(&request);
        let expanded = expand_query_text(&query_text);
        let top_k = if request.top_k == 0 {
            10
        } else {
            request.top_k
        };

        // ── Stage A: preselect candidate files ──────────────────
        let preselect_limit = request
            .file_preselect_limit
            .unwrap_or_else(|| 60usize.max(top_k * 12));
        let preselect_result = crate::preselect::preselect_files(
            &self.db,
            &query_text,
            request.path_prefix.as_deref(),
            request.boost_file_paths.as_deref(),
            request.recent_file_paths.as_deref(),
            request.pinned_file_paths.as_deref(),
            request.overlay_file_paths.as_deref(),
            request.file_paths.as_deref(),
            preselect_limit,
        )?;
        if !preselect_result.files.is_empty() && request.file_paths.is_none() {
            request.file_paths = Some(preselect_result.files.clone());
        }
        let request = &request;

        // Vector search
        let qvec = self.embedder.embed(&expanded);
        let vector_hits = self.vector_search(&conn, &qvec, top_k * 4, request)?;

        // Lexical search (FTS5)
        let lexical_hits = self.lexical_search(&conn, &expanded, top_k * 4, request)?;

        // Grep search
        let grep_hits = if request.include_grep {
            self.grep_search(&conn, &request.query, top_k * 2, request)?
        } else {
            Vec::new()
        };

        // RRF fusion
        let mut fused: HashMap<String, f64> = HashMap::new();
        let k = self.config.rrf_k;
        rrf_accumulate(
            &mut fused,
            &vector_hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>(),
            self.config.vector_weight,
            k,
        );
        rrf_accumulate(
            &mut fused,
            &lexical_hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>(),
            self.config.lexical_weight,
            k,
        );
        rrf_accumulate(
            &mut fused,
            &grep_hits.iter().map(|h| h.0.clone()).collect::<Vec<_>>(),
            self.config.grep_weight,
            k,
        );

        // Get top candidates
        let mut candidates: Vec<(String, f64)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(self.config.rerank_window.max(top_k));

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch chunk data
        let query_tokens = tokenize_codeish(&query_text);

        let boost_set: std::collections::HashSet<&str> = request
            .boost_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let recent_set: std::collections::HashSet<&str> = request
            .recent_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let pinned_set: std::collections::HashSet<&str> = request
            .pinned_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let overlay_set: std::collections::HashSet<&str> = request
            .overlay_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let vector_rank: HashMap<&str, usize> = vector_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str(), i + 1))
            .collect();
        let lexical_rank: HashMap<&str, usize> = lexical_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str(), i + 1))
            .collect();
        let grep_rank: HashMap<&str, usize> = grep_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str(), i + 1))
            .collect();

        let mut results = Vec::new();
        // Fetch each chunk and build SearchHit
        for (chunk_id, fused_score) in &candidates {
            let row = conn.query_row(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, symbol_name, symbol_kind, text FROM chunks WHERE chunk_id = ?1",
                rusqlite::params![chunk_id],
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
            let row = match row {
                Ok(r) => r,
                Err(_) => continue,
            };

            let (cid, fp, lang, sl, el, bc, sn, sk, text) = row;
            let language = parse_language_name(&lang);
            if !passes_filters(&fp, language, request) {
                continue;
            }
            let path_text = format!("{} {} {}", fp, bc, sn.as_deref().unwrap_or(""));
            let overlap =
                crate::rrf::overlap_score(&query_tokens, &format!("{}\n{}", path_text, text));
            let mut rerank = fused_score + overlap * 0.35;

            let mut reasons = Vec::new();
            let vector_score = vector_rank
                .get(cid.as_str())
                .map(|rank| 1.0 / *rank as f64)
                .unwrap_or(0.0);
            let lexical_score = lexical_rank
                .get(cid.as_str())
                .map(|rank| 1.0 / *rank as f64)
                .unwrap_or(0.0);
            let grep_score = grep_rank
                .get(cid.as_str())
                .map(|rank| 1.0 / *rank as f64)
                .unwrap_or(0.0);

            // Reasons with rank info: "vector@3", "lexical@5"
            if let Some(&rank) = vector_rank.get(cid.as_str()) {
                reasons.push(format!("vector@{}", rank));
            }
            if let Some(&rank) = lexical_rank.get(cid.as_str()) {
                reasons.push(format!("lexical@{}", rank));
            }
            if let Some(&rank) = grep_rank.get(cid.as_str()) {
                reasons.push(format!("grep@{}", rank));
            }

            // Symbol exact match boost (+0.18)
            if let Some(ref sym_name) = sn {
                let sym_lower = sym_name.to_lowercase();
                if query_tokens.contains(&sym_lower) {
                    rerank += 0.18;
                    reasons.push("symbol-exact".into());
                }
            }

            // Path prefix boost (+0.05)
            if let Some(ref prefix) = request.path_prefix {
                if fp.starts_with(prefix.as_str()) {
                    rerank += 0.05;
                }
            }

            // Doc file boost (+0.08 for README/DESIGN/CHANGELOG/docs/)
            if is_project_doc(&fp) {
                rerank += 0.08;
                reasons.push("doc-file".into());
            }

            // Working-set / recent / pinned / overlay boosts
            if boost_set.contains(fp.as_str()) {
                rerank += 0.22;
                reasons.push("working-set-boost".into());
            }
            if recent_set.contains(fp.as_str()) {
                rerank += 0.12;
                reasons.push("recent-file".into());
            }
            if pinned_set.contains(fp.as_str()) {
                rerank += 0.20;
                reasons.push("pinned-context".into());
            }
            if overlay_set.contains(fp.as_str()) {
                rerank += 0.10;
                reasons.push("overlay-neighbor".into());
            }

            // Stage A file score contribution
            let stage_a_score = preselect_result.scores.get(&fp).copied().unwrap_or(0.0);
            if stage_a_score > 0.0 {
                rerank += (stage_a_score * 0.04).min(0.25);
                if let Some(file_reasons) = preselect_result.reasons.get(&fp) {
                    for r in file_reasons.iter().take(3) {
                        reasons.push(r.clone());
                    }
                }
            }

            // Deduplicate reasons preserving order
            let mut seen = std::collections::HashSet::new();
            reasons.retain(|r| seen.insert(r.clone()));

            // Build stage_a metadata
            let metadata = serde_json::json!({
                "stage_a_file_score": stage_a_score,
                "stage_a_files_considered": preselect_result.files.len(),
                "stage_a_file_reasons": preselect_result.reasons.get(&fp).cloned().unwrap_or_default(),
            });

            results.push(SearchHit {
                chunk_id: cid,
                file_path: fp,
                language,
                start_line: sl,
                end_line: el,
                breadcrumb: bc,
                symbol_name: sn,
                symbol_kind: sk.and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                text,
                fused_score: *fused_score,
                vector_score,
                lexical_score,
                grep_score,
                graph_score: 0.0,
                rerank_score: rerank,
                reasons,
                source: "index".into(),
                lane: None,
                metadata,
            });
        }

        // ── DSL post-filters: kind, name ────────────────────────
        if let Some(ref kind_filter) = parsed.kind_filter {
            results.retain(|hit| {
                match &hit.symbol_kind {
                    Some(sk) => crate::dsl::matches_kind(sk, kind_filter),
                    None => false, // no symbol_kind => filtered out
                }
            });
        }
        if let Some(ref name_filter) = parsed.name_filter {
            let nf_lower = name_filter.to_lowercase();
            // Boost hits whose symbol_name matches; filter out those that don't.
            for hit in &mut results {
                if let Some(ref sn) = hit.symbol_name {
                    if sn.to_lowercase().contains(&nf_lower) {
                        hit.rerank_score += 0.25;
                        hit.reasons.push(format!("dsl-name:{}", name_filter));
                    }
                }
            }
            // Keep only hits that have a matching symbol name.
            results.retain(|hit| {
                hit.symbol_name
                    .as_ref()
                    .map(|sn| sn.to_lowercase().contains(&nf_lower))
                    .unwrap_or(false)
            });
        }

        results.sort_by(|a, b| {
            b.rerank_score
                .partial_cmp(&a.rerank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_k);
        Ok(results)
    }

    /// Batch query symbol in/out degrees from call_edges.
    ///
    /// Returns `{symbol_uid: (degree_in, degree_out)}`.
    pub fn batch_symbol_degrees(
        &self,
        symbol_uids: &[String],
    ) -> CcResult<HashMap<String, (u32, u32)>> {
        if symbol_uids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.db.read_conn()?;
        let mut result: HashMap<String, (u32, u32)> =
            symbol_uids.iter().map(|s| (s.clone(), (0, 0))).collect();

        let batch_size = 200;
        for batch in symbol_uids.chunks(batch_size) {
            let placeholders: String = batch.iter().map(|_| "?").collect::<Vec<_>>().join(",");

            // Incoming edges (callers -> this symbol)
            let sql_in = format!(
                "SELECT callee_symbol_uid, COUNT(*) AS cnt FROM call_edges \
                 WHERE callee_symbol_uid IN ({}) GROUP BY callee_symbol_uid",
                placeholders
            );
            let mut stmt_in = conn
                .prepare(&sql_in)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let params_in: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();
            let rows_in = stmt_in
                .query_map(params_in.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            for row in rows_in.flatten() {
                if let Some(entry) = result.get_mut(&row.0) {
                    entry.0 = row.1;
                }
            }

            // Outgoing edges (this symbol -> callees)
            let sql_out = format!(
                "SELECT caller_symbol_uid, COUNT(*) AS cnt FROM call_edges \
                 WHERE caller_symbol_uid IN ({}) GROUP BY caller_symbol_uid",
                placeholders
            );
            let mut stmt_out = conn
                .prepare(&sql_out)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let params_out: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();
            let rows_out = stmt_out
                .query_map(params_out.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            for row in rows_out.flatten() {
                if let Some(entry) = result.get_mut(&row.0) {
                    entry.1 = row.1;
                }
            }
        }

        Ok(result)
    }

    /// Generate compact metadata for a search hit (for summary mode).
    ///
    /// Includes symbol_uid, signature, community_id, degree_in, degree_out,
    /// connected symbols (top 3).
    pub fn compact_hit_metadata(&self, hit: &SearchHit) -> CcResult<serde_json::Value> {
        let conn = self.db.read_conn()?;
        let mut meta = serde_json::Map::new();

        // Find the symbol for this hit
        let symbol_row: Option<(String, Option<String>, Option<i64>)> =
            if let Some(ref sname) = hit.symbol_name {
                conn.query_row(
                    "SELECT symbol_uid, signature, community_id FROM symbols \
                     WHERE file_path = ?1 AND name = ?2 LIMIT 1",
                    rusqlite::params![hit.file_path, sname],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .ok()
            } else {
                conn.query_row(
                    "SELECT symbol_uid, signature, community_id FROM symbols \
                     WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2 LIMIT 1",
                    rusqlite::params![hit.file_path, hit.start_line],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )
                .ok()
            };

        if let Some((symbol_uid, signature, community_id)) = symbol_row {
            meta.insert(
                "symbol_uid".into(),
                serde_json::Value::String(symbol_uid.clone()),
            );
            if let Some(sig) = signature {
                meta.insert("signature".into(), serde_json::Value::String(sig));
            }
            if let Some(cid) = community_id {
                meta.insert(
                    "community_id".into(),
                    serde_json::Value::Number(serde_json::Number::from(cid)),
                );
            }

            // Degrees
            let degrees = self.batch_symbol_degrees(std::slice::from_ref(&symbol_uid))?;
            if let Some(&(d_in, d_out)) = degrees.get(&symbol_uid) {
                meta.insert(
                    "degree_in".into(),
                    serde_json::Value::Number(serde_json::Number::from(d_in)),
                );
                meta.insert(
                    "degree_out".into(),
                    serde_json::Value::Number(serde_json::Number::from(d_out)),
                );
            }

            // Connected symbols (top 3 callers + top 3 callees, deduped to 5)
            let mut connected: Vec<String> = Vec::new();
            // Callers
            if let Ok(mut stmt) = conn.prepare(
                "SELECT s.name FROM call_edges ce JOIN symbols s ON s.symbol_uid = ce.caller_symbol_uid \
                 WHERE ce.callee_symbol_uid = ?1 LIMIT 3",
            ) {
                if let Ok(rows) =
                    stmt.query_map(rusqlite::params![symbol_uid], |row| row.get::<_, String>(0))
                {
                    for row in rows.flatten() {
                        connected.push(row);
                    }
                }
            }
            // Callees
            if let Ok(mut stmt) = conn.prepare(
                "SELECT s.name FROM call_edges ce JOIN symbols s ON s.symbol_uid = ce.callee_symbol_uid \
                 WHERE ce.caller_symbol_uid = ?1 LIMIT 3",
            ) {
                if let Ok(rows) =
                    stmt.query_map(rusqlite::params![symbol_uid], |row| row.get::<_, String>(0))
                {
                    for row in rows.flatten() {
                        connected.push(row);
                    }
                }
            }
            connected.truncate(5);
            meta.insert(
                "connected_symbols".into(),
                serde_json::Value::Array(
                    connected
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }

        Ok(serde_json::Value::Object(meta))
    }

    /// Vector search: compute cosine similarity for all chunks.
    fn vector_search(
        &self,
        conn: &rusqlite::Connection,
        qvec: &[f32],
        limit: usize,
        request: &SearchRequest,
    ) -> CcResult<Vec<(String, f64)>> {
        let mut scored: Vec<(String, f64)> = Vec::new();

        const FILE_BATCH_SIZE: usize = 256;
        if let Some(file_paths) = request.file_paths.as_ref().filter(|v| !v.is_empty()) {
            for batch in file_paths.chunks(FILE_BATCH_SIZE) {
                let placeholders = batch.iter().map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT chunk_id, file_path, language, embedding FROM chunks WHERE embedding IS NOT NULL AND file_path IN ({})",
                    placeholders
                );
                let mut stmt = conn
                    .prepare(&sql)
                    .map_err(|e| CcError::Database(e.to_string()))?;
                let params = rusqlite::params_from_iter(batch.iter());
                let rows = stmt
                    .query_map(params, |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Vec<u8>>(3)?,
                        ))
                    })
                    .map_err(|e| CcError::Database(e.to_string()))?;

                for row in rows {
                    let (cid, file_path, language_name, blob) =
                        row.map_err(|e| CcError::Database(e.to_string()))?;
                    let language = parse_language_name(&language_name);
                    if !passes_filters(&file_path, language, request) {
                        continue;
                    }
                    let embedding = unpack_vector(&blob);
                    let sim = cosine_similarity(qvec, &embedding);
                    if sim > 0.0 {
                        scored.push((cid, sim));
                    }
                }
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT chunk_id, file_path, language, embedding FROM chunks WHERE embedding IS NOT NULL",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;

            for row in rows {
                let (cid, file_path, language_name, blob) =
                    row.map_err(|e| CcError::Database(e.to_string()))?;
                let language = parse_language_name(&language_name);
                if !passes_filters(&file_path, language, request) {
                    continue;
                }
                let embedding = unpack_vector(&blob);
                let sim = cosine_similarity(qvec, &embedding);
                if sim > 0.0 {
                    scored.push((cid, sim));
                }
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }

    /// Lexical search via FTS5.
    fn lexical_search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
        request: &SearchRequest,
    ) -> CcResult<Vec<(String, f64)>> {
        let fts_q = sanitize_fts_query(query);
        if fts_q == r#""""# {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT chunks_fts.chunk_id, chunks.file_path, chunks.language, bm25(chunks_fts, 1.0, 1.0, 2.0) AS score
             FROM chunks_fts
             JOIN chunks ON chunks.chunk_id = chunks_fts.chunk_id
             WHERE chunks_fts MATCH ?1
             ORDER BY score LIMIT ?2"
        ).map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![fts_q, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            let (cid, file_path, language_name, _score) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            if !passes_filters(&file_path, language, request) {
                continue;
            }
            results.push(cid);
        }
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, 1.0 / (i + 1) as f64))
            .collect())
    }

    /// Grep search — regex match on chunk text with file-level filtering.
    fn grep_search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
        request: &SearchRequest,
    ) -> CcResult<Vec<(String, f64)>> {
        // Build a simple case-insensitive regex from the query
        let escaped = regex::escape(query);
        let re = match regex::RegexBuilder::new(&escaped)
            .case_insensitive(true)
            .build()
        {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let mut stmt = conn
            .prepare("SELECT chunk_id, file_path, language, text FROM chunks")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    read_chunk_text(row, 3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut matches = Vec::new();
        for row in rows {
            let (cid, file_path, language_name, text) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            // File-level filtering: path_prefix, languages, file_paths
            if !passes_filters(&file_path, language, request) {
                continue;
            }
            if re.is_match(&text) {
                matches.push(cid);
                if matches.len() >= limit {
                    break;
                }
            }
        }
        Ok(matches
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, 1.0 / (i + 1) as f64))
            .collect())
    }

    // ══════════════════════════════════════════════════════════════
    // Lane hit generators
    // ══════════════════════════════════════════════════════════════

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

fn parse_language_name(value: &str) -> Language {
    Language::from_name(value)
}

/// Return true if the file path looks like a project documentation file.
///
/// Public so other crates can reuse the heuristic (e.g. for role tagging).
///
/// Matches: README.md, DESIGN.md, CHANGELOG.md, CONTRIBUTING.md, docs/*.md,
/// and similar top-level or docs-directory markdown files commonly used for
/// project documentation.
pub fn is_project_doc(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    if !lower.ends_with(".md") {
        return false;
    }
    // Top-level doc files (no directory separator or single-level path)
    let segments: Vec<&str> = file_path.split('/').collect();
    if segments.len() <= 2 {
        let name = segments.last().unwrap_or(&"").to_uppercase();
        if matches!(
            name.trim_end_matches(".MD").trim_end_matches(".md"),
            "README" | "DESIGN" | "ARCHITECTURE" | "CHANGELOG"
                | "CONTRIBUTING" | "LICENSE" | "ADR" | "DECISIONS"
        ) {
            return true;
        }
    }
    // Files under docs/ or doc/ directory
    if lower.starts_with("docs/") || lower.starts_with("doc/") {
        return true;
    }
    // ADR directory pattern
    if lower.contains("/adr/") || lower.contains("/adrs/") {
        return true;
    }
    false
}

fn passes_filters(file_path: &str, language: Language, request: &SearchRequest) -> bool {
    if let Some(prefix) = &request.path_prefix {
        if !file_path.starts_with(prefix) {
            return false;
        }
    }
    if let Some(languages) = &request.languages {
        if !languages.contains(&language) {
            return false;
        }
    }
    if let Some(files) = &request.file_paths {
        if !files.iter().any(|file| file == file_path) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Helper: create an IndexDb with some seed data for testing.
    fn make_test_db() -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("test_engine.db")).unwrap());
        let conn = db.read_conn().unwrap();

        // Insert files
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, \
                 content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at) \
             VALUES('src/main.rs', 'Rust', 'hash1', 1.0, 100, '', '', 'full', 1.0, 0, '2024-01-01');
             INSERT INTO files(file_path, language, content_hash, mtime, size, summary, \
                 content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at) \
             VALUES('tests/test_main.rs', 'Rust', 'hash2', 1.0, 80, '', '', 'full', 1.0, 1, '2024-01-01');
             INSERT INTO files(file_path, language, content_hash, mtime, size, summary, \
                 content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at) \
             VALUES('src/lib.rs', 'Rust', 'hash3', 1.0, 200, '', '', 'full', 1.0, 0, '2024-01-01');",
        )
        .unwrap();

        // Insert chunks
        conn.execute_batch(
            "INSERT INTO chunks(chunk_id, file_path, language, chunk_index, start_line, end_line, \
                 breadcrumb, symbol_name, symbol_kind, text, token_estimate, parser_tier, parser_confidence) \
             VALUES('c1', 'src/main.rs', 'Rust', 0, 1, 20, 'main', 'main', 'function', \
                    'fn main() { println!(\"hello\"); }', 10, 'full', 1.0);
             INSERT INTO chunks(chunk_id, file_path, language, chunk_index, start_line, end_line, \
                 breadcrumb, symbol_name, symbol_kind, text, token_estimate, parser_tier, parser_confidence) \
             VALUES('c2', 'tests/test_main.rs', 'Rust', 0, 1, 10, 'test', 'test_main', 'function', \
                    '#[test] fn test_main() { assert!(true); }', 8, 'full', 1.0);
             INSERT INTO chunks(chunk_id, file_path, language, chunk_index, start_line, end_line, \
                 breadcrumb, symbol_name, symbol_kind, text, token_estimate, parser_tier, parser_confidence) \
             VALUES('c3', 'src/lib.rs', 'Rust', 0, 1, 30, 'lib', 'process', 'function', \
                    'pub fn process() -> Result<()> { Ok(()) }', 12, 'full', 1.0);",
        )
        .unwrap();
        conn.execute(
            "UPDATE chunks SET embedding = ?1 WHERE chunk_id = 'c1'",
            rusqlite::params![crate::embeddings::pack_vector(&[1.0, 0.0, 0.0, 0.0])],
        )
        .unwrap();
        conn.execute(
            "UPDATE chunks SET embedding = ?1 WHERE chunk_id = 'c2'",
            rusqlite::params![crate::embeddings::pack_vector(&[0.0, 1.0, 0.0, 0.0])],
        )
        .unwrap();
        conn.execute(
            "UPDATE chunks SET embedding = ?1 WHERE chunk_id = 'c3'",
            rusqlite::params![crate::embeddings::pack_vector(&[0.0, 0.0, 1.0, 0.0])],
        )
        .unwrap();

        // Insert imports for import_graph test
        conn.execute_batch(
            "INSERT INTO imports(file_path, import_string, resolved_path, imported_name, alias, \
                 is_namespace, is_default, is_reexport) \
             VALUES('src/main.rs', 'crate::lib', 'src/lib.rs', 'process', NULL, 0, 0, 0);
             INSERT INTO imports(file_path, import_string, resolved_path, imported_name, alias, \
                 is_namespace, is_default, is_reexport) \
             VALUES('tests/test_main.rs', 'crate::main', 'src/main.rs', 'main', NULL, 0, 0, 0);",
        )
        .unwrap();

        (tmp, db)
    }

    #[test]
    fn test_hits_for_paths_creates_hits_with_correct_lane() {
        let (_tmp, db) = make_test_db();
        let config = cc_model::ProjectConfig::default();
        let engine = SearchEngine::new(db, &config);

        let hits = engine.hits_for_paths(&["src/main.rs", "src/lib.rs"], 10);
        assert!(!hits.is_empty(), "should return hits for known paths");
        for hit in &hits {
            assert_eq!(hit.lane.as_deref(), Some("path"), "lane should be 'path'");
            assert!(
                hit.reasons.iter().any(|r| r == "path-exact"),
                "reasons should contain 'path-exact'"
            );
            assert_eq!(hit.source, "index");
        }
        // Verify file_paths are correct
        let file_paths: Vec<&str> = hits.iter().map(|h| h.file_path.as_str()).collect();
        assert!(
            file_paths.contains(&"src/main.rs") || file_paths.contains(&"src/lib.rs"),
            "should return chunks from requested files"
        );
    }

    #[test]
    fn test_test_related_chunks_finds_test_files() {
        let (_tmp, db) = make_test_db();
        let config = cc_model::ProjectConfig::default();
        let engine = SearchEngine::new(db, &config);

        let hits = engine.test_related_chunks("src/main.rs");
        assert!(
            !hits.is_empty(),
            "should find test_main.rs as related test file"
        );
        assert!(
            hits.iter().any(|h| h.file_path.contains("test_main")),
            "should include test_main.rs"
        );
        for hit in &hits {
            assert_eq!(hit.lane.as_deref(), Some("test"), "lane should be 'test'");
        }
    }

    #[test]
    fn test_import_graph_returns_both_directions() {
        let (_tmp, db) = make_test_db();
        let config = cc_model::ProjectConfig::default();
        let engine = SearchEngine::new(db, &config);

        let (imports, reverse_imports) = engine.import_graph("src/main.rs");
        // src/main.rs imports src/lib.rs
        assert!(
            imports.iter().any(|p| p == "src/lib.rs"),
            "should list src/lib.rs as an import, got: {:?}",
            imports
        );
        // tests/test_main.rs imports src/main.rs, so src/main.rs has a reverse import
        assert!(
            reverse_imports.iter().any(|p| p == "tests/test_main.rs"),
            "should list tests/test_main.rs as reverse import, got: {:?}",
            reverse_imports
        );
    }

    #[test]
    fn test_vector_search_respects_candidate_file_scope() {
        let (_tmp, db) = make_test_db();
        let config = cc_model::ProjectConfig::default();
        let engine = SearchEngine::new(db, &config);

        let qvec = vec![0.0, 0.0, 1.0, 0.0];
        let request = SearchRequest {
            query: "main".into(),
            file_paths: Some(vec!["src/lib.rs".into()]),
            ..Default::default()
        };

        let conn = engine.db.read_conn().unwrap();
        let hits = engine.vector_search(&conn, &qvec, 10, &request).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "c3");
    }

    #[test]
    fn test_is_project_doc_top_level_files() {
        assert!(is_project_doc("README.md"));
        assert!(is_project_doc("DESIGN.md"));
        assert!(is_project_doc("CHANGELOG.md"));
        assert!(is_project_doc("CONTRIBUTING.md"));
        assert!(is_project_doc("ARCHITECTURE.md"));
        // Case-insensitive file name matching
        assert!(is_project_doc("readme.md"));
        assert!(is_project_doc("Readme.md"));
    }

    #[test]
    fn test_is_project_doc_docs_directory() {
        assert!(is_project_doc("docs/getting-started.md"));
        assert!(is_project_doc("docs/adr/0001-use-sqlite.md"));
        assert!(is_project_doc("doc/api.md"));
    }

    #[test]
    fn test_is_project_doc_adr_directory() {
        assert!(is_project_doc("architecture/adr/0002-rrf-fusion.md"));
        assert!(is_project_doc("decisions/adrs/0003-embedding.md"));
    }

    #[test]
    fn test_is_project_doc_non_doc_files() {
        assert!(!is_project_doc("src/main.rs"));
        assert!(!is_project_doc("src/lib.rs"));
        assert!(!is_project_doc("tests/test_main.rs"));
        // Non-doc markdown in src
        assert!(!is_project_doc("src/deep/nested/notes.md"));
        // Non-md files
        assert!(!is_project_doc("README.txt"));
    }
}
