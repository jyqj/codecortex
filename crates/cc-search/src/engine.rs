//! SearchEngine — hybrid vector + lexical + grep retrieval with RRF fusion.
//!
//! Extended with multi-lane hit generators and graph/navigation queries.

use std::collections::HashMap;
use std::sync::Arc;

use cc_db::fts::{expand_query_text, sanitize_fts_query, tokenize_codeish};
use cc_db::index_db::{read_chunk_text, IndexDb};
use cc_model::config::{ProjectStats, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcError, CcResult, Language};

use crate::embeddings::{cosine_similarity, get_embedder, unpack_vector, Embedder};
use crate::rrf::rrf_accumulate;

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    embedder: Box<dyn Embedder>,
    pub(crate) config: SearchConfig,
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
}

pub(crate) fn parse_language_name(value: &str) -> Language {
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
            "README"
                | "DESIGN"
                | "ARCHITECTURE"
                | "CHANGELOG"
                | "CONTRIBUTING"
                | "LICENSE"
                | "ADR"
                | "DECISIONS"
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
        let db = Arc::new(IndexDb::open(&tmp.path().join("test_engine.db")).unwrap().0);
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
