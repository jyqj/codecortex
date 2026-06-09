//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.

use std::collections::HashMap;
use std::sync::Arc;

use cc_db::fts::sanitize_fts_query;
use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
use cc_model::config::{ProjectStats, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcError, CcResult};

pub use crate::plan::is_project_doc;
use crate::plan::{parse_language_name, CandidateChunk, SearchPlan};
use crate::rrf::rrf_accumulate;

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) config: SearchConfig,
    /// Last index generation this engine has accepted for cache state.
    /// There is no result cache today; this seam makes future query caches
    /// invalidate from the server's CodeIndex generation without changing
    /// SearchEngine callers again.
    cache_generation: u64,
}

impl SearchEngine {
    pub fn new(db: Arc<IndexDb>, config: &cc_model::ProjectConfig) -> Self {
        Self {
            db,
            config: config.search.clone(),
            cache_generation: 0,
        }
    }

    pub fn invalidate_cache(&mut self, generation: u64) {
        self.cache_generation = generation;
    }

    pub fn cache_generation(&self) -> u64 {
        self.cache_generation
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.stats(std::path::Path::new(""))
    }

    /// Core search — FTS5 + grep with RRF fusion and reranking.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Vec<SearchHit>> {
        let conn = self.db.read_conn()?;
        let plan = SearchPlan::build(&self.db, &self.config, request)?;
        let limits = plan.limits();

        // Lexical search (FTS5)
        let lexical_hits =
            self.lexical_search(&conn, plan.lexical_query(), limits.lexical, &plan)?;

        // Grep search
        let grep_hits = if plan.request().include_grep {
            self.grep_search(&conn, plan.grep_query(), limits.grep, &plan)?
        } else {
            Vec::new()
        };

        // RRF fusion
        let mut fused: HashMap<String, f64> = HashMap::new();
        let k = self.config.rrf_k;
        rrf_accumulate(
            &mut fused,
            &lexical_hits
                .iter()
                .map(|h| h.0.as_str())
                .collect::<Vec<_>>(),
            self.config.lexical_weight,
            k,
        );
        rrf_accumulate(
            &mut fused,
            &grep_hits.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
            self.config.grep_weight,
            k,
        );

        let mut candidates: Vec<(String, f64)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limits.rerank_window);

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let lane_ranks = plan.lane_ranks(&lexical_hits, &grep_hits);

        // ── Batch-fetch all candidate chunks in one query ─────
        let mut chunk_map: HashMap<String, CandidateChunk> = {
            let placeholders = (1..=candidates.len())
                .map(|i| format!("?{}", i))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                 symbol_name, symbol_kind, text, text_encoding \
                 FROM chunks WHERE chunk_id IN ({})",
                placeholders,
            );
            let chunk_ids_refs: Vec<&str> =
                candidates.iter().map(|(cid, _)| cid.as_str()).collect();
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk_ids_refs.iter()), |row| {
                    CandidateChunk::from_row(row)
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let mut map = HashMap::with_capacity(candidates.len());
            for data in rows.flatten() {
                map.insert(data.chunk_id.clone(), data);
            }
            map
        };

        let mut results = Vec::new();
        for (chunk_id, fused_score) in &candidates {
            let Some(chunk) = chunk_map.remove(chunk_id) else {
                continue;
            };
            if let Some(hit) = plan.hit_from_chunk(chunk, *fused_score, &lane_ranks) {
                results.push(hit);
            }
        }

        plan.finalize_results(&mut results);
        Ok(results)
    }

    /// Lexical search via FTS5.
    fn lexical_search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
        plan: &SearchPlan,
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
            if !plan.passes_filters(&file_path, language) {
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
        plan: &SearchPlan,
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

        let (sql, params) = plan.grep_scope_sql();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    read_chunk_text_with_encoding(row, 3, 4)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut matches = Vec::new();
        for row in rows {
            let (cid, file_path, language_name, text) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            // File-level filtering: path_prefix, languages, file_paths
            if !plan.passes_filters(&file_path, language) {
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
