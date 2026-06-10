//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::fts::sanitize_fts_query;
use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
use cc_model::config::{ProjectStats, RepoSizeTier, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcError, CcResult};

pub use crate::plan::is_project_doc;
use crate::plan::{parse_language_name, CandidateChunk, SearchPlan};
use crate::rrf::rrf_accumulate;

/// Default LRU capacity for search results.
/// Override with `CODECORTEX_SEARCH_RESULT_CACHE_SIZE`.
const RESULT_CACHE_CAPACITY: usize = 32;

/// Default LRU capacity for decompressed chunk text.
/// Override with `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`.
///
/// Eliminates double-decompression: `grep_search` scans all in-scope chunks
/// (decompressing each), and matching chunks are fetched again in the
/// batch-fetch step.  Caching the text avoids the second `zstd::decode_all`.
const CHUNK_TEXT_CACHE_CAPACITY: usize = 512;

/// Read an LRU capacity from `var`, falling back to `default` when unset,
/// unparseable, or zero.
fn cache_capacity_from_env(var: &str, default: usize) -> NonZeroUsize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or_else(|| NonZeroUsize::new(default).unwrap())
}

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) config: SearchConfig,
    pub(crate) repo_tier: Option<RepoSizeTier>,
    /// Last index generation this engine has accepted for cache state.
    cache_generation: u64,
    /// LRU result cache keyed by `(cache_generation, query_hash)`.
    result_cache: Mutex<LruCache<(u64, u64), Vec<SearchHit>>>,
    /// LRU cache of decompressed chunk text keyed by `chunk_id`.
    chunk_text_cache: Mutex<LruCache<String, Arc<str>>>,
}

impl SearchEngine {
    pub fn new(
        db: Arc<IndexDb>,
        config: &cc_model::ProjectConfig,
        repo_tier: Option<RepoSizeTier>,
    ) -> Self {
        Self {
            db,
            config: config.search.clone(),
            repo_tier,
            cache_generation: 0,
            result_cache: Mutex::new(LruCache::new(cache_capacity_from_env(
                "CODECORTEX_SEARCH_RESULT_CACHE_SIZE",
                RESULT_CACHE_CAPACITY,
            ))),
            chunk_text_cache: Mutex::new(LruCache::new(cache_capacity_from_env(
                "CODECORTEX_SEARCH_CHUNK_CACHE_SIZE",
                CHUNK_TEXT_CACHE_CAPACITY,
            ))),
        }
    }

    pub fn invalidate_cache(&mut self, generation: u64) {
        self.cache_generation = generation;
        if let Ok(mut cache) = self.result_cache.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.chunk_text_cache.lock() {
            cache.clear();
        }
    }

    pub fn cache_generation(&self) -> u64 {
        self.cache_generation
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.stats(std::path::Path::new(""))
    }

    /// Compute a deterministic hash of `SearchRequest` key fields.
    fn query_hash(request: &SearchRequest) -> u64 {
        fn hash_sorted_opt_vec<T: Ord + std::hash::Hash + Clone>(
            value: &Option<Vec<T>>,
            hasher: &mut impl std::hash::Hasher,
        ) {
            if let Some(items) = value {
                let mut sorted = items.clone();
                sorted.sort_unstable();
                sorted.hash(hasher);
            }
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        request.query.hash(&mut hasher);
        request.top_k.hash(&mut hasher);
        request.path_prefix.hash(&mut hasher);
        request.include_grep.hash(&mut hasher);
        request.file_preselect_limit.hash(&mut hasher);
        // Languages — hash names in sorted order for stability.
        if let Some(ref langs) = request.languages {
            let mut names: Vec<&str> = langs.iter().map(|l| l.as_str()).collect();
            names.sort_unstable();
            names.hash(&mut hasher);
        }
        hash_sorted_opt_vec(&request.file_paths, &mut hasher);
        hash_sorted_opt_vec(&request.boost_file_paths, &mut hasher);
        // conversation_queries: 不排序，顺序影响 augmented_query_text() 拼接语义
        if let Some(ref cq) = request.conversation_queries {
            cq.hash(&mut hasher);
        }
        hash_sorted_opt_vec(&request.recent_file_paths, &mut hasher);
        hash_sorted_opt_vec(&request.pinned_file_paths, &mut hasher);
        hash_sorted_opt_vec(&request.overlay_file_paths, &mut hasher);
        hasher.finish()
    }

    /// Core search — FTS5 + grep with RRF fusion and reranking.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Vec<SearchHit>> {
        self.search_internal(request, true)
    }

    /// Like `search()` but returns up to `rerank_window` results instead of
    /// `top_k`.  Used by `search_in_context_with()` to give graph enrichment
    /// a larger candidate window before final truncation.
    ///
    /// Results are **not** cached (this path is typically called once per
    /// `search_in_context_with` invocation).
    pub fn search_extended(&self, request: &SearchRequest) -> CcResult<Vec<SearchHit>> {
        self.search_internal(request, false)
    }

    /// Shared implementation for `search` / `search_extended`.
    ///
    /// When `truncate_to_top_k` is true the result list is cut to `top_k`
    /// (standard behaviour).  When false, results are cut to `rerank_window`
    /// giving downstream graph enrichment a wider candidate set.
    fn search_internal(
        &self,
        request: &SearchRequest,
        truncate_to_top_k: bool,
    ) -> CcResult<Vec<SearchHit>> {
        // ── Cache lookup (only for the truncated path) ──────────
        let qhash = Self::query_hash(request);
        let cache_key = (self.cache_generation, qhash);
        if truncate_to_top_k {
            if let Ok(mut cache) = self.result_cache.lock() {
                if let Some(cached) = cache.get(&cache_key) {
                    tracing::debug!(
                        query = %request.query,
                        "search cache hit (generation={}, hash={})",
                        self.cache_generation,
                        qhash,
                    );
                    return Ok(cached.clone());
                }
            }
        }

        let conn = self.db.read_conn()?;
        let plan = SearchPlan::build(&self.db, &self.config, request, self.repo_tier)?;
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

        // Graph search lane
        let graph_hits = if self.config.graph_weight > 0.0 {
            self.graph_search(&conn, plan.query_tokens(), self.config.graph_top_k, &plan)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // RRF fusion (3-lane: lexical + grep + graph)
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
        if !graph_hits.is_empty() {
            rrf_accumulate(
                &mut fused,
                &graph_hits.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
                self.config.graph_weight,
                k,
            );
        }

        let mut candidates: Vec<(String, f64)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(limits.rerank_window);

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let lane_ranks = plan.lane_ranks(&lexical_hits, &grep_hits);

        // ── Batch-fetch all candidate chunks in one query ─────
        //
        // When grep was enabled, the chunk text cache already holds
        // decompressed text for many (often all) candidates.  We
        // reuse those cached values to avoid a second zstd decode.
        let mut chunk_map: HashMap<String, CandidateChunk> = {
            // Snapshot cached texts for the candidate set so we only hold
            // the mutex briefly.
            let cached_texts: HashMap<String, Arc<str>> = {
                let mut snapshot = HashMap::new();
                if let Ok(mut cache) = self.chunk_text_cache.lock() {
                    for (cid, _) in &candidates {
                        if let Some(text) = cache.get(cid) {
                            snapshot.insert(cid.clone(), Arc::clone(text));
                        }
                    }
                }
                snapshot
            };

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
                    let chunk_id: String = row.get(0)?;
                    if let Some(text) = cached_texts.get(&chunk_id) {
                        CandidateChunk::from_row_with_text(row, text.to_string())
                    } else {
                        CandidateChunk::from_row(row)
                    }
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let mut map = HashMap::with_capacity(candidates.len());
            for data in rows.flatten() {
                // Also populate cache for chunks that weren't cached yet,
                // benefiting subsequent searches against the same codebase.
                if !cached_texts.contains_key(&data.chunk_id) {
                    if let Ok(mut cache) = self.chunk_text_cache.lock() {
                        cache.put(data.chunk_id.clone(), Arc::from(data.text.as_str()));
                    }
                }
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

        if truncate_to_top_k {
            plan.finalize_results(&mut results);
        } else {
            plan.finalize_results_with_limit(&mut results, limits.rerank_window);
        }

        // ── Cache store (only for the truncated path) ───────────
        if truncate_to_top_k {
            if let Ok(mut cache) = self.result_cache.lock() {
                cache.put(cache_key, results.clone());
            }
        }

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
        let (sql, mut params) = plan.lexical_scope_sql(limit);
        params.insert(0, fts_q);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
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

            // Cache the decompressed text so batch-fetch can reuse it.
            let text_arc: Arc<str> = Arc::from(text.as_str());
            if let Ok(mut cache) = self.chunk_text_cache.lock() {
                cache.put(cid.clone(), Arc::clone(&text_arc));
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

    /// Graph retrieval lane: find chunks connected to query-matching symbols
    /// via the call graph (1-hop callers + callees).
    fn graph_search(
        &self,
        conn: &rusqlite::Connection,
        query_tokens: &[String],
        limit: usize,
        plan: &SearchPlan,
    ) -> CcResult<Vec<(String, f64)>> {
        // Step 1: Find seed symbols matching query tokens via symbols_fts (trigram LIKE)
        let seed_uids = self.find_seed_symbol_uids(conn, query_tokens)?;
        if seed_uids.is_empty() {
            return Ok(Vec::new());
        }

        // Step 2: 1-hop expansion via call_edges (callers + callees)
        let mut neighbor_uids: HashMap<String, f64> = HashMap::new();
        for (uid, seed_score) in &seed_uids {
            // Include the seed itself (distance 0)
            neighbor_uids
                .entry(uid.clone())
                .and_modify(|s| *s = s.max(*seed_score))
                .or_insert(*seed_score);

            // Callees of seed (distance 1)
            if let Ok(callees) = self.db.callee_rows_by_uid(uid, 10) {
                for edge in &callees {
                    if let Some(ref callee_uid) = edge.callee_symbol_uid {
                        let score = seed_score * 0.5;
                        neighbor_uids
                            .entry(callee_uid.clone())
                            .and_modify(|s| *s = s.max(score))
                            .or_insert(score);
                    }
                }
            }

            // Callers of seed (distance 1)
            if let Ok(callers) = self.db.caller_rows_by_uid(uid, 10) {
                for edge in &callers {
                    if let Some(ref caller_uid) = edge.caller_symbol_uid {
                        let score = seed_score * 0.5;
                        neighbor_uids
                            .entry(caller_uid.clone())
                            .and_modify(|s| *s = s.max(score))
                            .or_insert(score);
                    }
                }
            }
        }

        if neighbor_uids.is_empty() {
            return Ok(Vec::new());
        }

        // Step 3: Map symbol UIDs -> chunks, applying file filters
        let uid_list: Vec<String> = neighbor_uids.keys().cloned().collect();
        let sym_rows = self.db.symbol_rows_by_uids(&uid_list)?;

        let mut chunk_scores: Vec<(String, f64)> = Vec::new();
        let mut seen_chunks = std::collections::HashSet::new();

        for (uid, score) in &neighbor_uids {
            if let Some(sym) = sym_rows.get(uid) {
                // Check file filter
                if !plan.passes_filters(&sym.file_path, parse_language_name(&sym.file_path)) {
                    continue;
                }
                // Find chunk containing this symbol
                if let Ok(Some(cid)) =
                    self.find_chunk_for_symbol(conn, &sym.file_path, sym.start_line, sym.end_line)
                {
                    if seen_chunks.insert(cid.clone()) {
                        chunk_scores.push((cid, *score));
                    }
                }
            }
        }

        // Sort by score descending and limit
        chunk_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        chunk_scores.truncate(limit);
        Ok(chunk_scores)
    }

    /// Find seed symbols matching query tokens via the symbols_fts trigram table.
    ///
    /// Uses LIKE substring matching (symbols_fts is an FTS5 trigram table, not
    /// a standard BM25 table).
    fn find_seed_symbol_uids(
        &self,
        conn: &rusqlite::Connection,
        query_tokens: &[String],
    ) -> CcResult<Vec<(String, f64)>> {
        let mut results: HashMap<String, f64> = HashMap::new();

        for token in query_tokens.iter().take(5) {
            if token.len() < 3 {
                continue;
            }
            // Use trigram-accelerated LIKE via symbols_fts
            let like_pattern = format!("%{}%", token);
            let sql = "SELECT s.symbol_uid, s.name \
                       FROM symbols_fts f \
                       JOIN symbols s ON s.symbol_id = f.symbol_id \
                       WHERE f.name LIKE ?1 \
                       AND s.symbol_uid IS NOT NULL \
                       ORDER BY s.file_path \
                       LIMIT 10";
            let mut stmt = conn
                .prepare(sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![like_pattern], |row| {
                    let uid: String = row.get(0)?;
                    let name: String = row.get(1)?;
                    Ok((uid, name))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;

            for row in rows.flatten() {
                let (uid, name) = row;
                // Score: exact match > contains
                let relevance = if name.to_lowercase() == *token {
                    1.0
                } else {
                    0.5
                };
                results
                    .entry(uid)
                    .and_modify(|s| *s = s.max(relevance))
                    .or_insert(relevance);
            }
        }

        let mut sorted: Vec<(String, f64)> = results.into_iter().collect();
        sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(20); // max 20 seed symbols
        Ok(sorted)
    }

    /// Find the chunk_id containing a symbol at the given file/line range.
    fn find_chunk_for_symbol(
        &self,
        conn: &rusqlite::Connection,
        file_path: &str,
        start_line: u32,
        end_line: u32,
    ) -> CcResult<Option<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT chunk_id FROM chunks \
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?3 \
                 ORDER BY (end_line - start_line) ASC \
                 LIMIT 1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let result = stmt.query_row(rusqlite::params![file_path, start_line, end_line], |row| {
            row.get::<_, String>(0)
        });
        match result {
            Ok(cid) => Ok(Some(cid)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CcError::Database(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cc_db::index_db::{FileWriteUnit, IndexDb};
    use cc_model::config::{ProjectConfig, SearchConfig};
    use cc_model::{ChunkRecord, Language, ParseOutcome, ParserTier};
    use std::sync::Arc;

    fn scoped_test_engine() -> (SearchEngine, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig {
            search: SearchConfig {
                lexical_top_k: 3,
                grep_top_k: 3,
                rrf_k: 50,
                lexical_weight: 1.0,
                grep_weight: 0.0,
                rerank_window: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        (SearchEngine::new(Arc::new(db), &config, None), tmp)
    }

    fn insert_chunk_file(engine: &SearchEngine, file_path: &str, language: Language, text: &str) {
        let chunk = ChunkRecord {
            chunk_id: format!("chunk:{}", file_path),
            file_path: file_path.to_string(),
            language,
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            breadcrumb: "root".to_string(),
            text: text.to_string(),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        };
        let mut outcome = ParseOutcome {
            summary: text.to_string(),
            chunks: vec![chunk],
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        };
        outcome.is_test_file = false;

        let conn = engine.db.read_conn().unwrap();
        IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: file_path.to_string(),
                language,
                content_hash: format!("hash-{file_path}"),
                mtime: 0.0,
                size: text.len() as u64,
                outcome,
            },
        )
        .unwrap();
    }

    #[test]
    fn search_cache_returns_same_results() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "fn alpha_handler() { process() }",
        );
        insert_chunk_file(
            &engine,
            "src/beta.rs",
            Language::Rust,
            "fn beta_helper() { alpha_handler() }",
        );

        let request = SearchRequest {
            query: "alpha".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };

        let first = engine.search(&request).unwrap();
        let second = engine.search(&request).unwrap();

        assert!(!first.is_empty(), "should find at least one result");
        assert_eq!(first.len(), second.len(), "cached result length must match");
        for (a, b) in first.iter().zip(second.iter()) {
            assert_eq!(a.chunk_id, b.chunk_id, "chunk_ids must match");
            assert!(
                (a.fused_score - b.fused_score).abs() < f64::EPSILON,
                "fused_scores must match"
            );
        }
    }

    #[test]
    fn lexical_search_applies_path_prefix_before_limit() {
        let (engine, _tmp) = scoped_test_engine();
        let noisy_match = "scopeleak ".repeat(40);
        for i in 0..8 {
            insert_chunk_file(
                &engine,
                &format!("vendor/generated/out_{i}.rs"),
                Language::Rust,
                &noisy_match,
            );
        }
        insert_chunk_file(
            &engine,
            "src/in_scope/target.rs",
            Language::Rust,
            "scopeleak target implementation",
        );

        let hits = engine
            .search(&SearchRequest {
                query: "scopeleak".to_string(),
                top_k: 1,
                path_prefix: Some("src/in_scope/".to_string()),
                include_grep: false,
                file_preselect_limit: Some(20),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(
            hits.iter()
                .map(|hit| hit.file_path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/in_scope/target.rs"]
        );
    }

    #[test]
    fn cache_key_includes_context_fields() {
        let base = SearchRequest {
            query: "foo".into(),
            top_k: 10,
            ..Default::default()
        };

        let with_conv = SearchRequest {
            conversation_queries: Some(vec!["bar".into()]),
            ..base.clone()
        };
        assert_ne!(
            SearchEngine::query_hash(&base),
            SearchEngine::query_hash(&with_conv)
        );

        let with_pinned = SearchRequest {
            pinned_file_paths: Some(vec!["a.rs".into()]),
            ..base.clone()
        };
        assert_ne!(
            SearchEngine::query_hash(&base),
            SearchEngine::query_hash(&with_pinned)
        );

        let with_recent = SearchRequest {
            recent_file_paths: Some(vec!["b.rs".into()]),
            ..base.clone()
        };
        assert_ne!(
            SearchEngine::query_hash(&base),
            SearchEngine::query_hash(&with_recent)
        );

        let with_overlay = SearchRequest {
            overlay_file_paths: Some(vec!["c.rs".into()]),
            ..base.clone()
        };
        assert_ne!(
            SearchEngine::query_hash(&base),
            SearchEngine::query_hash(&with_overlay)
        );

        // 集合语义: pinned=[a,b] == pinned=[b,a]
        let pinned_ab = SearchRequest {
            pinned_file_paths: Some(vec!["a.rs".into(), "b.rs".into()]),
            ..base.clone()
        };
        let pinned_ba = SearchRequest {
            pinned_file_paths: Some(vec!["b.rs".into(), "a.rs".into()]),
            ..base.clone()
        };
        assert_eq!(
            SearchEngine::query_hash(&pinned_ab),
            SearchEngine::query_hash(&pinned_ba)
        );

        // 顺序敏感: conversation_queries=[a,b] != [b,a]
        let conv_ab = SearchRequest {
            conversation_queries: Some(vec!["a".into(), "b".into()]),
            ..base.clone()
        };
        let conv_ba = SearchRequest {
            conversation_queries: Some(vec!["b".into(), "a".into()]),
            ..base.clone()
        };
        assert_ne!(
            SearchEngine::query_hash(&conv_ab),
            SearchEngine::query_hash(&conv_ba)
        );
    }

    #[test]
    fn graph_search_returns_empty_for_no_symbols() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "fn alpha_handler() { process() }",
        );

        // graph_search on an empty symbol table should return empty, not error
        let conn = engine.db.read_conn().unwrap();
        let plan = SearchPlan::build(
            &engine.db,
            &engine.config,
            &SearchRequest {
                query: "alpha".to_string(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let graph_hits = engine.graph_search(&conn, plan.query_tokens(), 12, &plan);
        // Should succeed (possibly empty — no symbols indexed yet)
        assert!(graph_hits.is_ok());
    }

    #[test]
    fn graph_lane_does_not_break_existing_search() {
        // Ensure enabling graph_weight > 0 doesn't change results when no
        // symbols exist: search should still return lexical-only results.
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig {
            search: SearchConfig {
                lexical_top_k: 3,
                grep_top_k: 3,
                rrf_k: 50,
                lexical_weight: 1.0,
                grep_weight: 0.0,
                rerank_window: 3,
                graph_weight: 0.6,
                graph_top_k: 12,
            },
            ..Default::default()
        };
        let engine = SearchEngine::new(Arc::new(db), &config, None);
        insert_chunk_file(
            &engine,
            "src/foo.rs",
            Language::Rust,
            "fn foo_handler() { do_work() }",
        );

        let request = SearchRequest {
            query: "foo".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let results = engine.search(&request).unwrap();
        assert!(!results.is_empty(), "lexical results should still work");
    }
}
