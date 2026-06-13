//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::index_db::{IndexDb, IndexGeneration};
use cc_model::config::{
    GraphEnrichLimits, ProjectStats, RankingConfig, RepoSizeTier, SearchConfig,
};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::CcResult;

use crate::enrich::{graph_enrich, GraphEnrichment};
use crate::lanes::{default_lanes, fuse_outcomes, run_lanes, LaneContext};
pub use crate::plan::is_project_doc;
use crate::plan::{CandidateChunk, SearchPlan};

/// Default LRU capacity for search results.
/// Override with `CODECORTEX_SEARCH_RESULT_CACHE_SIZE`.
///
/// Values are `Arc<[SearchHit]>`: a cache hit hands out a pointer clone
/// instead of deep-copying `top_k` hits, each of which carries full chunk
/// text in `SearchHit::text`.
const RESULT_CACHE_CAPACITY: usize = 32;

/// Default LRU capacity for decompressed chunk text.
/// Override with `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`.
///
/// Eliminates double-decompression on the retrieval (cache-miss) path:
/// `grep_search` scans all in-scope chunks (decompressing each), and
/// matching chunks are fetched again in the batch-fetch step.  Caching the
/// text avoids the second `zstd::decode_all`.  The cache-HIT path never
/// decompresses at all — it returns the `Arc`'d result list above.
const CHUNK_TEXT_CACHE_CAPACITY: usize = 512;

/// Default LRU capacity for graph-aware (post-enrichment) search results.
/// Override with `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`.
///
/// Separate slot count from `RESULT_CACHE_CAPACITY`: entries are heavier
/// (final hits plus enrichment context nodes) and this path is the default
/// agent entry point (`search_in_context_with`), so it gets its own knob.
const GRAPH_RESULT_CACHE_CAPACITY: usize = 32;

/// Read an LRU capacity from `var`, falling back to `default` when unset,
/// unparseable, or zero.
fn cache_capacity_from_env(var: &str, default: usize) -> NonZeroUsize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or_else(|| NonZeroUsize::new(default).unwrap())
}

/// Result cache map: `(index_epoch, query_hash)` → shared, immutable hits.
type ResultCache = LruCache<(u64, u64), Arc<[SearchHit]>>;

/// Graph-aware result cache map: `(index_epoch, evidence_epoch,
/// graph_query_hash)` → shared, immutable final `(hits, enrichment)` pair
/// from [`SearchEngine::search_with_graph_context`].
type GraphResultCache = LruCache<(u64, u64, u64), Arc<(Vec<SearchHit>, GraphEnrichment)>>;

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) config: SearchConfig,
    pub(crate) ranking: RankingConfig,
    pub(crate) repo_tier: Option<RepoSizeTier>,
    /// Index epoch last observed by this engine. The result caches are keyed
    /// on the epochs read from the DB at search time, so they never need
    /// explicit invalidation; this field only detects epoch changes so the
    /// chunk text cache (keyed by positional, non-content-addressed chunk
    /// ids) can be cleared eagerly.
    last_seen_index_epoch: AtomicU64,
    /// Evidence epoch last observed by this engine (companion to
    /// `last_seen_index_epoch`): detects evidence-only bumps so the
    /// graph-aware result cache can be cleared eagerly for memory hygiene
    /// (correctness comes from the epoch pair in its key).
    last_seen_evidence_epoch: AtomicU64,
    /// LRU result cache keyed by `(index_epoch, query_hash)`.
    ///
    /// INVARIANT: the stored slice is FINAL — `search_internal` assigns all
    /// scores, sorts, and truncates before the `put` in [`Self::search`], and
    /// nothing mutates hits afterwards (`search_with_graph_context`, which
    /// does mutate, never touches this cache — it has its own
    /// post-enrichment cache below).  A hit therefore returns `Arc::clone`
    /// of the shared slice with no per-hit deep copy.
    result_cache: Mutex<ResultCache>,
    /// LRU result cache for the graph-aware path, keyed by
    /// `(index_epoch, evidence_epoch, graph_query_hash)` — see
    /// [`Self::search_with_graph_context`].  Kept separate from
    /// `result_cache` because the stored values embed post-enrichment state
    /// (graph rerank + context nodes) that also depends on evidence-boosted
    /// edge confidence, hence the evidence_epoch in the key.
    graph_result_cache: Mutex<GraphResultCache>,
    /// Hash of the ranking-relevant config: the full [`RankingConfig`] plus
    /// [`SearchConfig`]'s `graph_weight`/`graph_top_k`.  Computed ONCE at
    /// construction — `config` and `ranking` are cloned into the engine in
    /// [`Self::new`] and never mutated afterwards (cc-server rebuilds the
    /// engine on project/config change), so the fingerprint is immutable
    /// per instance.  Folded into the graph-aware cache key.
    ranking_fingerprint: u64,
    /// LRU cache of decompressed chunk text keyed by `chunk_id`.
    chunk_text_cache: Mutex<LruCache<String, Arc<str>>>,
}

impl SearchEngine {
    pub fn new(
        db: Arc<IndexDb>,
        config: &cc_model::ProjectConfig,
        repo_tier: Option<RepoSizeTier>,
    ) -> Self {
        let initial_generation = db.reads().generation().unwrap_or_default();
        let ranking_fingerprint = Self::ranking_fingerprint(&config.search, &config.ranking);
        Self {
            db,
            config: config.search.clone(),
            ranking: config.ranking.clone(),
            repo_tier,
            last_seen_index_epoch: AtomicU64::new(initial_generation.index_epoch),
            last_seen_evidence_epoch: AtomicU64::new(initial_generation.evidence_epoch),
            result_cache: Mutex::new(LruCache::new(cache_capacity_from_env(
                "CODECORTEX_SEARCH_RESULT_CACHE_SIZE",
                RESULT_CACHE_CAPACITY,
            ))),
            graph_result_cache: Mutex::new(LruCache::new(cache_capacity_from_env(
                "CODECORTEX_GRAPH_SEARCH_CACHE_SIZE",
                GRAPH_RESULT_CACHE_CAPACITY,
            ))),
            ranking_fingerprint,
            chunk_text_cache: Mutex::new(LruCache::new(cache_capacity_from_env(
                "CODECORTEX_SEARCH_CHUNK_CACHE_SIZE",
                CHUNK_TEXT_CACHE_CAPACITY,
            ))),
        }
    }

    /// Drop all cached search state.
    ///
    /// NOT needed after index writes — cc-db bumps the persisted index epoch
    /// inside every write transaction and `search()` re-reads it per call.
    /// Kept only for defensive clearing (e.g. lock-poison recovery) and tests.
    pub fn invalidate_cache(&self) {
        if let Ok(mut cache) = self.result_cache.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.graph_result_cache.lock() {
            cache.clear();
        }
        if let Ok(mut cache) = self.chunk_text_cache.lock() {
            cache.clear();
        }
    }

    /// Observe the current `(index_epoch, evidence_epoch)` pair, clearing
    /// caches eagerly when either moved.  An index bump invalidates
    /// everything (the chunk text cache is keyed by positional chunk ids).
    /// An evidence bump (`boost_http_edge_confidence` ingestion) clears only
    /// the graph-aware result cache: its entries embed evidence-boosted edge
    /// confidence, while plain `search()` results and chunk text do not
    /// depend on evidence.  Both result caches stay correct regardless —
    /// their keys embed the epoch(s), so a bump simply misses; the clears
    /// here are memory hygiene.
    fn observe_epochs(&self) -> CcResult<IndexGeneration> {
        let generation = self.db.reads().generation()?;
        let prev_index = self
            .last_seen_index_epoch
            .swap(generation.index_epoch, Ordering::AcqRel);
        let prev_evidence = self
            .last_seen_evidence_epoch
            .swap(generation.evidence_epoch, Ordering::AcqRel);
        if prev_index != generation.index_epoch {
            self.invalidate_cache();
        } else if prev_evidence != generation.evidence_epoch {
            if let Ok(mut cache) = self.graph_result_cache.lock() {
                cache.clear();
            }
        }
        Ok(generation)
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.reads().stats(std::path::Path::new(""))
    }

    /// Compute a deterministic hash of `SearchRequest` key fields.
    fn query_hash(request: &SearchRequest) -> u64 {
        // Order-insensitive combine: hash each item independently and merge
        // with wrapping_add, so list order doesn't matter and we never pay a
        // clone + sort on the cache-hit path (large file lists are common).
        fn hash_unordered_opt_vec<T: std::hash::Hash>(
            value: &Option<Vec<T>>,
            hasher: &mut impl std::hash::Hasher,
        ) {
            use std::hash::Hasher as _;
            if let Some(items) = value {
                items.len().hash(hasher);
                let mut acc: u64 = 0;
                for item in items {
                    let mut item_hasher = std::collections::hash_map::DefaultHasher::new();
                    item.hash(&mut item_hasher);
                    acc = acc.wrapping_add(item_hasher.finish());
                }
                acc.hash(hasher);
            }
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        request.query.hash(&mut hasher);
        request.top_k.hash(&mut hasher);
        request.path_prefix.hash(&mut hasher);
        request.include_grep.hash(&mut hasher);
        request.file_preselect_limit.hash(&mut hasher);
        // Languages — order-insensitive like the path lists.
        let lang_names = request
            .languages
            .as_ref()
            .map(|langs| langs.iter().map(|l| l.as_str()).collect::<Vec<_>>());
        hash_unordered_opt_vec(&lang_names, &mut hasher);
        hash_unordered_opt_vec(&request.file_paths, &mut hasher);
        hash_unordered_opt_vec(&request.boost_file_paths, &mut hasher);
        // conversation_queries: 不排序，顺序影响 augmented_query_text() 拼接语义
        if let Some(ref cq) = request.conversation_queries {
            cq.hash(&mut hasher);
        }
        hash_unordered_opt_vec(&request.recent_file_paths, &mut hasher);
        hash_unordered_opt_vec(&request.pinned_file_paths, &mut hasher);
        hash_unordered_opt_vec(&request.overlay_file_paths, &mut hasher);
        hasher.finish()
    }

    /// Fingerprint of every config knob that shapes graph-aware results.
    /// The full [`RankingConfig`] is hashed via its serde serialization
    /// (struct field order is stable), so any future ranking knob is
    /// automatically covered without enumerating fields here; the two
    /// graph-lane params from [`SearchConfig`] are folded in explicitly.
    /// Called once from [`Self::new`] — see `ranking_fingerprint` field.
    fn ranking_fingerprint(config: &SearchConfig, ranking: &RankingConfig) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(ranking)
            .unwrap_or_default()
            .hash(&mut hasher);
        config.graph_weight.to_bits().hash(&mut hasher);
        config.graph_top_k.hash(&mut hasher);
        hasher.finish()
    }

    /// Cache hash for the graph-aware path: the base [`Self::query_hash`]
    /// combined with every post-search input that shapes the final
    /// `(hits, enrichment)` value — the enrichment limits, the token budget
    /// (it caps the enrichment's graph node budget), and the per-engine
    /// ranking fingerprint.
    fn graph_query_hash(
        &self,
        request: &SearchRequest,
        limits: &GraphEnrichLimits,
        token_budget: u32,
    ) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Self::query_hash(request).hash(&mut hasher);
        limits.max_resolve.hash(&mut hasher);
        limits.callers_per_sym.hash(&mut hasher);
        limits.callees_per_sym.hash(&mut hasher);
        limits.max_tests.hash(&mut hasher);
        limits.max_routes.hash(&mut hasher);
        limits.graph_budget_pct.hash(&mut hasher);
        token_budget.hash(&mut hasher);
        self.ranking_fingerprint.hash(&mut hasher);
        hasher.finish()
    }

    /// Core search — FTS5 + grep with RRF fusion and reranking.
    ///
    /// INVARIANT: `rerank_score` on the returned hits is FINAL and the list
    /// is sorted on it.  Callers must not re-score or re-sort; graph-aware
    /// reranking happens inside [`Self::search_with_graph_context`], never
    /// downstream.  The shared `Arc<[SearchHit]>` return type enforces this:
    /// a result-cache hit is an `Arc` clone of the stored slice, so mutating
    /// it would corrupt the cache.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Arc<[SearchHit]>> {
        // ── Cache lookup ─────────────────────────────────────────
        // The cache key embeds the persisted index epoch: any committed index
        // write bumps it, so stale entries can never be served. One metadata
        // SELECT per search; local SQLite reads are microseconds.
        let index_epoch = self.observe_epochs()?.index_epoch;
        let qhash = Self::query_hash(request);
        let cache_key = (index_epoch, qhash);
        if let Ok(mut cache) = self.result_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                tracing::debug!(
                    query = %request.query,
                    "search cache hit (index_epoch={}, hash={})",
                    index_epoch,
                    qhash,
                );
                return Ok(Arc::clone(cached));
            }
        }

        let results: Arc<[SearchHit]> = self.search_internal(request, true)?.into();

        if let Ok(mut cache) = self.result_cache.lock() {
            cache.put(cache_key, Arc::clone(&results));
        }
        Ok(results)
    }

    /// Search with graph-aware reranking, for context assembly.
    ///
    /// Runs the core search over a `rerank_window`-sized candidate list,
    /// computes a connectivity-based `graph_score` for the top
    /// `limits.max_resolve` hits, folds it into `rerank_score` using
    /// `ranking.graph_rerank_weight`, then performs the single, final sort
    /// and truncates to the request's `top_k`.
    ///
    /// INVARIANT: this is the only place the graph contribution is applied —
    /// `rerank_score` and hit order are final on return, and the returned
    /// [`GraphEnrichment`] carries the neighbor/test context nodes without
    /// any score state.
    ///
    /// Results ARE cached (LRU, own slot count via
    /// `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`, separate from [`Self::search`]'s
    /// cache).  The key covers both DB epochs (`index_epoch`,
    /// `evidence_epoch`), the request hash, the [`GraphEnrichLimits`], the
    /// token budget, and the per-engine ranking fingerprint — so a bump of
    /// either epoch simply misses.  evidence_epoch matters because runtime
    /// evidence ingestion (`boost_http_edge_confidence`) alters the edge
    /// confidence embedded in cached enrichment nodes without touching index
    /// content.  Degraded results (`graph_explain.read_errors` non-empty)
    /// are never cached: a transient DB failure must not be served from
    /// cache for the rest of the epoch pair.  A hit returns `Arc::clone` of
    /// the shared, immutable pair — callers must not mutate it.
    pub fn search_with_graph_context(
        &self,
        request: &SearchRequest,
        limits: &GraphEnrichLimits,
        token_budget: u32,
    ) -> CcResult<Arc<(Vec<SearchHit>, GraphEnrichment)>> {
        // ── Cache lookup ─────────────────────────────────────────
        let generation = self.observe_epochs()?;
        let qhash = self.graph_query_hash(request, limits, token_budget);
        let cache_key = (generation.index_epoch, generation.evidence_epoch, qhash);
        if let Ok(mut cache) = self.graph_result_cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                tracing::debug!(
                    query = %request.query,
                    "graph search cache hit (index_epoch={}, evidence_epoch={}, hash={})",
                    generation.index_epoch,
                    generation.evidence_epoch,
                    qhash,
                );
                return Ok(Arc::clone(cached));
            }
        }

        let mut hits = self.search_internal(request, false)?;

        let (scores, enrichment) = graph_enrich(&self.db, &hits, limits, token_budget);
        let weight = self.ranking.graph_rerank_weight;
        for hit in &mut hits {
            if let Some(&graph_score) = scores.get(&hit.chunk_id) {
                hit.graph_score = graph_score;
                // Atomically updates `rerank_score` and bills the matching
                // trace component, keeping `sum(score_trace) == rerank_score`.
                crate::score_trace::apply_traced_boost(
                    hit,
                    "boost:graph-rerank",
                    graph_score * weight,
                );
            }
        }

        // Single final sort + truncation: rerank_score is immutable after this.
        hits.sort_by(|a, b| {
            b.rerank_score
                .partial_cmp(&a.rerank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let top_k = if request.top_k == 0 {
            10
        } else {
            request.top_k
        };
        hits.truncate(top_k);
        crate::score_trace::debug_assert_trace_consistency(&hits);

        let result = Arc::new((hits, enrichment));
        // Degraded-not-cached: a transient DB read failure (recorded in
        // graph_explain.read_errors) produced partial graph context — keep
        // serving it for THIS call, but never from cache, so the next call
        // retries the reads instead of pinning the degradation to the epoch.
        if result.1.graph_explain.read_errors.is_empty() {
            if let Ok(mut cache) = self.graph_result_cache.lock() {
                cache.put(cache_key, Arc::clone(&result));
            }
        }
        Ok(result)
    }

    /// Shared implementation for `search` / `search_with_graph_context`.
    ///
    /// When `truncate_to_top_k` is true the result list is cut to `top_k`
    /// (standard behaviour).  When false, results are cut to `rerank_window`
    /// giving the graph-rerank step a wider candidate set.
    ///
    /// PRECONDITION: the caller has invoked [`Self::observe_epochs`] so the
    /// chunk text cache was cleared if the index epoch moved.  Result
    /// caching lives in [`Self::search`] / the graph-aware cache in
    /// [`Self::search_with_graph_context`]; this function always recomputes.
    fn search_internal(
        &self,
        request: &SearchRequest,
        truncate_to_top_k: bool,
    ) -> CcResult<Vec<SearchHit>> {
        // No pooled read connection is held here: plan build (preselect),
        // each lane, and the batch fetch below all check out and release
        // their own, so a 1-connection read pool never sees nested checkouts.
        let plan = SearchPlan::build(
            &self.db,
            &self.config,
            &self.ranking,
            request,
            self.repo_tier,
        )?;
        let limits = plan.limits();

        // Retrieval lanes from the central registry
        // (`lanes::default_lanes()`), executed in deterministic fusion
        // order so RRF tie-breaking stays stable.
        let lanes = default_lanes();
        let lane_context = LaneContext {
            plan: &plan,
            db: &self.db,
            config: &self.config,
            chunk_text_cache: &self.chunk_text_cache,
        };
        let lane_outcomes = run_lanes(&lanes, &lane_context)?;

        // RRF fusion across all lane outcomes; each candidate keeps its
        // per-lane contribution breakdown for the hit's score trace.
        let fused = fuse_outcomes(&lane_outcomes, self.config.rrf_k);

        let mut candidates: Vec<(String, crate::lanes::FusedScore)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| {
            b.1.total
                .partial_cmp(&a.1.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(limits.rerank_window);

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let lane_ranks = plan.lane_ranks(&lane_outcomes);

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

            let chunk_ids_refs: Vec<&str> =
                candidates.iter().map(|(cid, _)| cid.as_str()).collect();
            let rows = self
                .db
                .reads()
                .chunk_rows_by_ids(&chunk_ids_refs, &cached_texts)?;
            let mut map = HashMap::with_capacity(candidates.len());
            for data in rows {
                // Also populate cache for chunks that weren't cached yet,
                // benefiting subsequent searches against the same codebase.
                if !cached_texts.contains_key(&data.chunk_id) {
                    if let Ok(mut cache) = self.chunk_text_cache.lock() {
                        cache.put(data.chunk_id.clone(), Arc::from(data.text.as_str()));
                    }
                }
                map.insert(data.chunk_id.clone(), CandidateChunk::from(data));
            }
            map
        };

        let mut results = Vec::new();
        for (chunk_id, fused_score) in &candidates {
            let Some(chunk) = chunk_map.remove(chunk_id) else {
                continue;
            };
            if let Some(hit) = plan.hit_from_chunk(chunk, fused_score, &lane_ranks) {
                results.push(hit);
            }
        }

        if truncate_to_top_k {
            plan.finalize_results(&mut results);
        } else {
            plan.finalize_results_with_limit(&mut results, limits.rerank_window);
        }

        // Pipeline exit invariant (debug builds only): every traced hit's
        // bill must replay its final rerank_score.
        crate::score_trace::debug_assert_trace_consistency(&results);

        Ok(results)
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

        let conn = crate::test_seed::seed_conn(&engine.db);
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

    fn chunk_write_unit(file_path: &str, text: &str) -> FileWriteUnit {
        let chunk = ChunkRecord {
            chunk_id: format!("chunk:{}", file_path),
            file_path: file_path.to_string(),
            language: Language::Rust,
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
        FileWriteUnit {
            rel_path: file_path.to_string(),
            language: Language::Rust,
            content_hash: format!("hash-{file_path}-{}", text.len()),
            mtime: 0.0,
            size: text.len() as u64,
            outcome: ParseOutcome {
                summary: text.to_string(),
                chunks: vec![chunk],
                parser_tier: ParserTier::TreeSitter,
                parser_confidence: 1.0,
                ..Default::default()
            },
        }
    }

    #[test]
    fn index_write_invalidates_search_cache_without_manual_call() {
        let (engine, _tmp) = scoped_test_engine();

        // Committed write #1: chunk text contains alpha_one.
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = SearchRequest {
            query: "cached_marker".to_string(),
            top_k: 5,
            include_grep: true,
            ..Default::default()
        };
        let first = engine.search(&request).unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].text.contains("alpha_one"));

        // Committed write #2 replaces the SAME file (same chunk_id, new text).
        // No invalidate_cache() call: the epoch bump inside the cc-db write
        // transaction must make both the result cache and the chunk text
        // cache miss on the next search.
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_two() }",
            )])
            .unwrap();

        let second = engine.search(&request).unwrap();
        assert_eq!(second.len(), 1);
        assert!(
            second[0].text.contains("alpha_two"),
            "stale cached text served after index write: {}",
            second[0].text
        );
    }

    #[test]
    fn index_write_invalidates_chunk_text_cache_on_lexical_only_path() {
        // Lexical-only variant of the test above: with grep disabled, the
        // GrepLane never rescans chunk text, so the candidate batch-fetch is
        // the only consumer of chunk_text_cache — a missed clear would serve
        // the stale text here without grep masking it.
        let (engine, _tmp) = scoped_test_engine();

        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = SearchRequest {
            query: "cached_marker".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let first = engine.search(&request).unwrap();
        assert_eq!(first.len(), 1);
        assert!(first[0].text.contains("alpha_one"));

        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_two() }",
            )])
            .unwrap();

        let second = engine.search(&request).unwrap();
        assert_eq!(second.len(), 1);
        assert!(
            second[0].text.contains("alpha_two"),
            "stale chunk text served from batch-fetch cache after index write: {}",
            second[0].text
        );
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
    fn search_cache_hit_is_zero_copy_and_equivalent() {
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
        assert!(
            Arc::ptr_eq(&first, &second),
            "a cache hit must return the same shared allocation (zero-copy)"
        );

        // The shared slice must carry exactly what a fresh engine (separate
        // cache, guaranteed miss) recomputes from the same DB.
        let fresh_config = ProjectConfig {
            search: engine.config.clone(),
            ..Default::default()
        };
        let fresh_engine = SearchEngine::new(engine.db.clone(), &fresh_config, None);
        let recomputed = fresh_engine.search(&request).unwrap();
        assert!(
            !Arc::ptr_eq(&first, &recomputed),
            "fresh engine must recompute, not share the other engine's cache"
        );
        assert_eq!(first.len(), recomputed.len());
        for (cached, fresh) in first.iter().zip(recomputed.iter()) {
            assert_eq!(cached.chunk_id, fresh.chunk_id, "chunk_ids must match");
            assert_eq!(cached.text, fresh.text, "chunk text must match");
            assert!(
                (cached.rerank_score - fresh.rerank_score).abs() < 1e-12,
                "rerank_score must match: cached={} fresh={}",
                cached.rerank_score,
                fresh.rerank_score
            );
        }
    }

    #[test]
    fn search_completes_with_read_pool_size_one() {
        // Regression guard for nested pool checkouts: `search_internal` used
        // to hold a pooled read connection while plan build (preselect) and
        // the post-lane `chunk_rows_by_ids` batch fetch each checked out a
        // SECOND connection.  With a 1-connection read pool every such nested
        // checkout blocked for the full r2d2 connection timeout (~30s) and
        // then failed the search.  After the fix no connection is held across
        // `self.db.*` calls, so this must complete instantly.
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&tmp.path().join("index.sqlite3"), 1)
            .unwrap()
            .0;
        let config = ProjectConfig {
            search: SearchConfig {
                lexical_top_k: 3,
                grep_top_k: 3,
                rrf_k: 50,
                lexical_weight: 1.0,
                grep_weight: 1.0,
                rerank_window: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = SearchEngine::new(Arc::new(db), &config, None);
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

        let started = std::time::Instant::now();
        let hits = engine
            .search(&SearchRequest {
                query: "alpha".to_string(),
                top_k: 5,
                include_grep: true,
                ..Default::default()
            })
            .unwrap();
        assert!(
            !hits.is_empty(),
            "pool_size=1 search must produce candidates"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "pool_size=1 search must not stall on nested checkouts (took {:?})",
            started.elapsed()
        );
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

    // ── graph-aware result cache (search_with_graph_context) ──────────

    fn graph_cache_request() -> SearchRequest {
        SearchRequest {
            query: "cached_marker".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        }
    }

    #[test]
    fn graph_search_cache_hit_returns_shared_arc() {
        let (engine, _tmp) = scoped_test_engine();
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = graph_cache_request();
        let limits = RepoSizeTier::Small.graph_enrich_limits();
        let first = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        let second = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(!first.0.is_empty(), "fixture must produce hits");
        assert!(
            Arc::ptr_eq(&first, &second),
            "second identical request must be served from the graph-aware cache"
        );
    }

    #[test]
    fn graph_search_cache_misses_on_index_epoch_bump() {
        let (engine, _tmp) = scoped_test_engine();
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = graph_cache_request();
        let limits = RepoSizeTier::Small.graph_enrich_limits();
        let first = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(first.0[0].text.contains("alpha_one"));

        // Committed reindex of the same file bumps index_epoch inside the
        // cc-db write transaction — no manual invalidation call.
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_two() }",
            )])
            .unwrap();

        let second = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&first, &second),
            "index epoch bump must miss the graph-aware cache"
        );
        assert!(
            second.0[0].text.contains("alpha_two"),
            "stale enriched results served after index write: {}",
            second.0[0].text
        );
    }

    #[test]
    fn graph_search_cache_misses_on_evidence_epoch_bump() {
        let (engine, _tmp) = scoped_test_engine();
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = graph_cache_request();
        let limits = RepoSizeTier::Small.graph_enrich_limits();
        let first = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();

        // Runtime-evidence ingestion bumps evidence_epoch WITHOUT touching
        // index content; enrichment consumes evidence-boosted confidence, so
        // cached enriched results must not survive the bump.
        engine
            .db
            .writes()
            .boost_http_edge_confidence("missing-edge", 0.1)
            .unwrap();

        let second = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&first, &second),
            "evidence epoch bump must miss the graph-aware cache"
        );

        // The recomputed result is cached under the new epoch pair.
        let third = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            Arc::ptr_eq(&second, &third),
            "post-bump result must be cached under the new epoch pair"
        );
    }

    #[test]
    fn graph_search_cache_key_covers_budget_and_limits() {
        let (engine, _tmp) = scoped_test_engine();
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/cached.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();

        let request = graph_cache_request();
        let limits = RepoSizeTier::Small.graph_enrich_limits();
        let base = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();

        // Different token_budget → different key → miss (recompute).
        let other_budget = engine
            .search_with_graph_context(&request, &limits, 8000)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&base, &other_budget),
            "a different token_budget must not reuse the cached entry"
        );

        // Different GraphEnrichLimits → different key → miss.
        let mut other_limits = RepoSizeTier::Small.graph_enrich_limits();
        other_limits.callers_per_sym += 1;
        let other = engine
            .search_with_graph_context(&request, &other_limits, 4000)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&base, &other),
            "different GraphEnrichLimits must not reuse the cached entry"
        );

        // The original key is still cached, untouched by the misses above.
        let again = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            Arc::ptr_eq(&base, &again),
            "the original (budget, limits) entry must still be served"
        );
    }

    #[test]
    fn graph_search_degraded_result_is_not_cached() {
        let (engine, _tmp) = scoped_test_engine();
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn cached_marker() {}",
            "cached_marker",
            "uid:cached_marker",
            vec![],
        );
        // Drop call_edges so the enrichment's batched adjacency reads fail
        // (the graph lane swallows its own failures, so search still works).
        crate::test_seed::seed_conn(&engine.db)
            .execute("DROP TABLE call_edges", [])
            .unwrap();

        let request = graph_cache_request();
        let limits = RepoSizeTier::Small.graph_enrich_limits();
        let first = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            !first.1.graph_explain.read_errors.is_empty(),
            "fixture must produce a degraded enrichment"
        );
        assert_eq!(
            engine.graph_result_cache.lock().unwrap().len(),
            0,
            "degraded results must not be stored in the cache"
        );
        let second = engine
            .search_with_graph_context(&request, &limits, 4000)
            .unwrap();
        assert!(
            !Arc::ptr_eq(&first, &second),
            "degraded result must be recomputed, never served from cache"
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

        // GraphLane::search on an empty symbol table should return empty, not error
        let plan = SearchPlan::build(
            &engine.db,
            &engine.config,
            &engine.ranking,
            &SearchRequest {
                query: "alpha".to_string(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let graph_hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12);
        // Should succeed (possibly empty — no symbols indexed yet)
        assert!(graph_hits.is_ok());
    }

    // ── Lane seam (RetrievalLane trait) ────────────────────────

    use crate::lanes::{
        fuse_outcomes, run_lanes, FusedScore, GraphLane, GrepLane, LaneContext, LaneOutcome,
        LexicalLane, RetrievalLane, ScoreSlot, LANE_GRAPH, LANE_GREP, LANE_LEXICAL,
    };
    use cc_model::{CallEdgeRecord, SymbolRecord};

    fn build_plan(engine: &SearchEngine, request: &SearchRequest) -> SearchPlan {
        SearchPlan::build(&engine.db, &engine.config, &engine.ranking, request, None).unwrap()
    }

    /// Insert a single-chunk file together with one symbol (lines 1-1) and
    /// optional call edges, so the graph lane has seeds and hops to follow.
    fn insert_graph_file(
        engine: &SearchEngine,
        file_path: &str,
        text: &str,
        symbol_name: &str,
        symbol_uid: &str,
        call_edges: Vec<CallEdgeRecord>,
    ) {
        let chunk = ChunkRecord {
            chunk_id: format!("chunk:{}", file_path),
            file_path: file_path.to_string(),
            language: Language::Rust,
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
        let symbol = SymbolRecord {
            symbol_id: format!("sym:{file_path}:{symbol_name}"),
            file_path: file_path.to_string(),
            name: symbol_name.to_string(),
            kind: cc_model::SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let mut outcome = ParseOutcome {
            summary: text.to_string(),
            chunks: vec![chunk],
            symbols: vec![symbol],
            call_edges,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        };
        outcome.is_test_file = false;

        let conn = crate::test_seed::seed_conn(&engine.db);
        IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: file_path.to_string(),
                language: Language::Rust,
                content_hash: format!("hash-{file_path}"),
                mtime: 0.0,
                size: text.len() as u64,
                outcome,
            },
        )
        .unwrap();
    }

    #[test]
    fn lexical_lane_adapter_matches_inline_ranking() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "alphatoken appears here",
        );

        let request = SearchRequest {
            query: "alphatoken".to_string(),
            top_k: 5,
            include_grep: false,
            file_paths: Some(vec!["src/alpha.rs".to_string()]),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let lane = LexicalLane;
        assert_eq!(lane.lane_id(), LANE_LEXICAL);
        assert!(lane.is_enabled(&context), "lexical lane always runs");
        assert_eq!(
            lane.weight(&engine.config),
            engine.config.lexical_weight,
            "lexical lane weight comes from lexical_weight"
        );

        let hits = lane.run(&context).unwrap();
        assert_eq!(hits, vec![("chunk:src/alpha.rs".to_string(), 1.0)]);
    }

    #[test]
    fn grep_lane_adapter_ranks_matches_and_caches_only_hits() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/g.rs",
            Language::Rust,
            "the needle is right here",
        );
        insert_chunk_file(&engine, "src/other.rs", Language::Rust, "nothing relevant");

        let request = SearchRequest {
            query: "needle".to_string(),
            top_k: 5,
            include_grep: true,
            file_paths: Some(vec!["src/g.rs".to_string(), "src/other.rs".to_string()]),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let lane = GrepLane;
        assert_eq!(lane.lane_id(), LANE_GREP);
        assert!(lane.is_enabled(&context), "include_grep=true enables grep");
        assert_eq!(lane.weight(&engine.config), engine.config.grep_weight);

        let hits = lane.run(&context).unwrap();
        assert_eq!(hits, vec![("chunk:src/g.rs".to_string(), 1.0)]);

        // Side effect: only *matched* chunks land in the engine's chunk
        // text cache.  Scan-only rows stay out so a cold scan over a large
        // scope can't rotate the LRU and evict hot entries.
        let mut cache = engine.chunk_text_cache.lock().unwrap();
        assert!(cache.get("chunk:src/g.rs").is_some());
        assert!(
            cache.get("chunk:src/other.rs").is_none(),
            "non-matching scanned chunk must not enter the text cache"
        );
    }

    #[test]
    fn grep_lane_scan_budget_truncates_recency_first_and_deterministically() {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig {
            search: SearchConfig {
                lexical_top_k: 3,
                grep_top_k: 10,
                grep_scan_cap: 2,
                lexical_weight: 1.0,
                grep_weight: 0.8,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = SearchEngine::new(Arc::new(db), &config, None);

        // Four matching files, inserted oldest → newest.  With the scan
        // budget at 2, the unscoped recency-ordered scan only reaches the
        // two most recently indexed files.
        for name in ["a", "b", "c", "d"] {
            insert_chunk_file(
                &engine,
                &format!("src/{name}.rs"),
                Language::Rust,
                "the scanneedle is here",
            );
        }

        // file_preselect_limit=0 empties preselect, so no file scope is
        // materialized and grep takes the unscoped (budgeted) path.
        let request = SearchRequest {
            query: "scanneedle".to_string(),
            top_k: 10,
            include_grep: true,
            file_preselect_limit: Some(0),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let first = GrepLane.run(&context).unwrap();
        assert_eq!(
            first,
            vec![
                ("chunk:src/d.rs".to_string(), 1.0),
                ("chunk:src/c.rs".to_string(), 0.5),
            ],
            "budget of 2 must cover exactly the two most recently indexed files"
        );

        // Determinism: same index + same config + same query → same result.
        let second = GrepLane.run(&context).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn grep_lane_disabled_when_request_excludes_grep() {
        let (engine, _tmp) = scoped_test_engine();
        let request = SearchRequest {
            query: "needle".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };
        assert!(!GrepLane.is_enabled(&context));
    }

    #[test]
    fn graph_lane_adapter_ranks_seed_above_one_hop_neighbor() {
        let (engine, _tmp) = scoped_test_engine();
        let process_to_helper = CallEdgeRecord {
            edge_id: "edge:process->helper".to_string(),
            file_path: "src/a.rs".to_string(),
            caller_symbol: Some("process".to_string()),
            callee_symbol: "helper".to_string(),
            line: 1,
            caller_symbol_uid: Some("uid:process".to_string()),
            callee_symbol_uid: Some("uid:helper".to_string()),
            ..Default::default()
        };
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn process() { helper() }",
            "process",
            "uid:process",
            vec![process_to_helper],
        );
        insert_graph_file(
            &engine,
            "src/b.rs",
            "fn helper() {}",
            "helper",
            "uid:helper",
            vec![],
        );

        let request = SearchRequest {
            query: "process".to_string(),
            top_k: 5,
            include_grep: false,
            file_paths: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let lane = GraphLane;
        assert_eq!(lane.lane_id(), LANE_GRAPH);
        assert_eq!(lane.weight(&engine.config), engine.config.graph_weight);

        let hits = lane.run(&context).unwrap();
        assert_eq!(
            hits,
            vec![
                ("chunk:src/a.rs".to_string(), 1.0),
                ("chunk:src/b.rs".to_string(), 0.5),
            ],
            "seed symbol chunk first, 1-hop callee chunk at half score"
        );
    }

    #[test]
    fn graph_lane_respects_languages_filter_by_file_extension() {
        let (engine, _tmp) = scoped_test_engine();
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn process() {}",
            "process",
            "uid:process",
            vec![],
        );

        let request = SearchRequest {
            query: "process".to_string(),
            top_k: 5,
            include_grep: false,
            languages: Some(vec![Language::Rust]),
            file_paths: Some(vec!["src/a.rs".to_string()]),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);

        // Symbols live in a .rs file, the filter asks for Rust — the graph
        // lane must keep the seed hit instead of misclassifying the file as
        // Language::Unknown (regression: a file *path* was passed where a
        // language *name* was expected).
        let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
        assert_eq!(
            hits,
            vec![("chunk:src/a.rs".to_string(), 1.0)],
            "languages=[Rust] must not drop the Rust seed symbol's chunk"
        );
    }

    #[test]
    fn graph_lane_disabled_when_weight_is_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig {
            search: SearchConfig {
                graph_weight: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = SearchEngine::new(Arc::new(db), &config, None);
        let request = SearchRequest {
            query: "anything".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };
        assert!(
            !GraphLane.is_enabled(&context),
            "graph lane must short-circuit before any work when weight is 0"
        );
    }

    #[test]
    fn graph_lane_expands_caller_direction_at_decay() {
        // Mirror of the callee-direction golden test: the query matches the
        // CALLEE, and the caller must be pulled in at graph_neighbor_decay.
        let (engine, _tmp) = scoped_test_engine();
        let process_to_helper = CallEdgeRecord {
            edge_id: "edge:process->helper".to_string(),
            file_path: "src/a.rs".to_string(),
            caller_symbol: Some("process".to_string()),
            callee_symbol: "helper".to_string(),
            line: 1,
            caller_symbol_uid: Some("uid:process".to_string()),
            callee_symbol_uid: Some("uid:helper".to_string()),
            ..Default::default()
        };
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn process() { helper() }",
            "process",
            "uid:process",
            vec![process_to_helper],
        );
        insert_graph_file(
            &engine,
            "src/b.rs",
            "fn helper() {}",
            "helper",
            "uid:helper",
            vec![],
        );

        let request = SearchRequest {
            query: "helper".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
        assert_eq!(
            hits,
            vec![
                ("chunk:src/b.rs".to_string(), 1.0),
                ("chunk:src/a.rs".to_string(), 0.5),
            ],
            "seed (callee) chunk first, 1-hop caller chunk at decay score"
        );
    }

    #[test]
    fn graph_lane_scores_fuzzy_seed_below_exact_seed() {
        // Exact name match seeds at graph_seed_exact_score (1.0); a substring
        // match seeds at graph_seed_fuzzy_score (0.5).
        let (engine, _tmp) = scoped_test_engine();
        insert_graph_file(
            &engine,
            "src/exact.rs",
            "fn process() {}",
            "process",
            "uid:process",
            vec![],
        );
        insert_graph_file(
            &engine,
            "src/fuzzy.rs",
            "fn process_batch() {}",
            "process_batch",
            "uid:process_batch",
            vec![],
        );

        let request = SearchRequest {
            query: "process".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
        assert_eq!(
            hits,
            vec![
                ("chunk:src/exact.rs".to_string(), 1.0),
                ("chunk:src/fuzzy.rs".to_string(), 0.5),
            ],
            "exact-name seed must outrank substring seed"
        );
    }

    #[test]
    fn graph_lane_seeds_short_tokens_by_exact_name() {
        // Tokens under 3 chars cannot use the trigram table; they seed via
        // exact-name equality (both common casings) instead of being dropped.
        let (engine, _tmp) = scoped_test_engine();
        insert_graph_file(&engine, "src/ok.rs", "fn ok() {}", "ok", "uid:ok", vec![]);

        let request = SearchRequest {
            query: "ok".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
        assert_eq!(
            hits,
            vec![("chunk:src/ok.rs".to_string(), 1.0)],
            "a 2-char exact symbol name must still seed the graph lane"
        );
    }

    #[test]
    fn graph_lane_maps_symbol_to_smallest_containing_chunk() {
        // A file with a wide chunk and a narrow chunk that both contain the
        // symbol span: the lane must pick the narrowest container.
        let (engine, _tmp) = scoped_test_engine();
        let make_chunk = |chunk_id: &str, index: i64, start: u32, end: u32| ChunkRecord {
            chunk_id: chunk_id.to_string(),
            file_path: "src/wide.rs".to_string(),
            language: Language::Rust,
            chunk_index: index as u32,
            start_line: start,
            end_line: end,
            breadcrumb: "root".to_string(),
            text: "fn narrow_fn() {}".to_string(),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        };
        let symbol = SymbolRecord {
            symbol_id: "sym:src/wide.rs:narrow_fn".to_string(),
            file_path: "src/wide.rs".to_string(),
            name: "narrow_fn".to_string(),
            kind: cc_model::SymbolKind::Function,
            container: None,
            start_line: 2,
            end_line: 3,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some("uid:narrow_fn".to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let outcome = ParseOutcome {
            summary: "fixture".to_string(),
            chunks: vec![
                make_chunk("chunk:wide", 0, 1, 50),
                make_chunk("chunk:narrow", 1, 1, 5),
            ],
            symbols: vec![symbol],
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        };
        let conn = crate::test_seed::seed_conn(&engine.db);
        IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: "src/wide.rs".to_string(),
                language: Language::Rust,
                content_hash: "hash-wide".to_string(),
                mtime: 0.0,
                size: 10,
                outcome,
            },
        )
        .unwrap();
        drop(conn);

        let request = SearchRequest {
            query: "narrow_fn".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
        assert_eq!(
            hits,
            vec![("chunk:narrow".to_string(), 1.0)],
            "the smallest chunk containing the symbol span must win"
        );
    }

    /// Synthetic lane for exercising the generic lane loop.
    struct FakeLane {
        id: &'static str,
        enabled: bool,
        lane_weight: f64,
        annotates: bool,
        hits: Vec<(String, f64)>,
        ran: std::cell::Cell<bool>,
    }

    impl RetrievalLane for FakeLane {
        fn lane_id(&self) -> &'static str {
            self.id
        }
        fn weight(&self, _config: &SearchConfig) -> f64 {
            self.lane_weight
        }
        fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
            self.enabled
        }
        fn annotates_hits(&self) -> bool {
            self.annotates
        }
        fn run(&self, _context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
            self.ran.set(true);
            Ok(self.hits.clone())
        }
    }

    fn fake_candidate_chunk() -> CandidateChunk {
        CandidateChunk {
            chunk_id: "chunk:src/x.rs".to_string(),
            file_path: "src/x.rs".to_string(),
            language_name: "rust".to_string(),
            start_line: 1,
            end_line: 2,
            breadcrumb: "root".to_string(),
            symbol_name: None,
            symbol_kind: None,
            text: "fn x() {}".to_string(),
        }
    }

    #[test]
    fn new_lane_opting_into_annotation_gets_generic_hit_reasons() {
        // Extensibility guarantee: a brand-new lane that opts into per-hit
        // annotation (`annotates_hits() == true`) must surface `{lane_id}@{rank}`
        // reasons WITHOUT any edits to plan.rs lane-id whitelists.
        let (engine, _tmp) = scoped_test_engine();
        let request = SearchRequest {
            query: "anything".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let fourth = FakeLane {
            id: "fourth",
            enabled: true,
            lane_weight: 1.0,
            annotates: true,
            hits: vec![("chunk:src/x.rs".to_string(), 1.0)],
            ran: std::cell::Cell::new(false),
        };
        let fifth = FakeLane {
            id: "fifth",
            enabled: true,
            lane_weight: 1.0,
            annotates: true,
            hits: vec![
                ("chunk:other".to_string(), 1.0),
                ("chunk:src/x.rs".to_string(), 0.5),
            ],
            ran: std::cell::Cell::new(false),
        };

        let lanes: [&dyn RetrievalLane; 2] = [&fourth, &fifth];
        let outcomes = run_lanes(&lanes, &context).unwrap();
        let lane_ranks = plan.lane_ranks(&outcomes);

        let hit = plan
            .hit_from_chunk(
                fake_candidate_chunk(),
                &FusedScore {
                    total: 0.5,
                    by_lane: vec![],
                },
                &lane_ranks,
            )
            .unwrap();

        assert_eq!(
            hit.reasons,
            vec!["fourth@1".to_string(), "fifth@2".to_string()],
            "annotating lanes must contribute {{lane_id}}@{{rank}} reasons in lane order"
        );
        // Lanes without a declared ScoreSlot have no dedicated score field
        // in SearchHit (cc-model fields are fixed); built-ins stay 0.0.
        assert_eq!(hit.lexical_score, 0.0);
        assert_eq!(hit.grep_score, 0.0);
        assert_eq!(hit.graph_score, 0.0);
    }

    #[test]
    fn new_lane_declaring_score_slot_projects_without_plan_edits() {
        // Extensibility guarantee: a brand-new lane that declares an
        // existing ScoreSlot gets its rank-derived score projected into the
        // matching SearchHit field purely via trait impl + registration —
        // no lane-id match arm anywhere in plan.rs or engine.rs.
        struct SlottedLane;
        impl RetrievalLane for SlottedLane {
            fn lane_id(&self) -> &'static str {
                "semantic"
            }
            fn weight(&self, _config: &SearchConfig) -> f64 {
                1.0
            }
            fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
                true
            }
            fn annotates_hits(&self) -> bool {
                true
            }
            fn score_slot(&self) -> Option<ScoreSlot> {
                Some(ScoreSlot::Graph)
            }
            fn run(&self, _context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
                Ok(vec![
                    ("chunk:other".to_string(), 1.0),
                    ("chunk:src/x.rs".to_string(), 0.5),
                ])
            }
        }

        let (engine, _tmp) = scoped_test_engine();
        let request = SearchRequest {
            query: "anything".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let slotted = SlottedLane;
        let lanes: [&dyn RetrievalLane; 1] = [&slotted];
        let outcomes = run_lanes(&lanes, &context).unwrap();
        let lane_ranks = plan.lane_ranks(&outcomes);

        let hit = plan
            .hit_from_chunk(
                fake_candidate_chunk(),
                &FusedScore {
                    total: 0.5,
                    by_lane: vec![],
                },
                &lane_ranks,
            )
            .unwrap();

        assert_eq!(hit.reasons, vec!["semantic@2".to_string()]);
        assert_eq!(
            hit.graph_score, 0.5,
            "declared slot must receive the lane's rank-derived score (1/rank)"
        );
        assert_eq!(hit.lexical_score, 0.0);
        assert_eq!(hit.grep_score, 0.0);
    }

    #[test]
    fn default_lanes_registry_keeps_fusion_order() {
        // The registry is the single registration point; its order is the
        // deterministic RRF fusion order.
        let lanes = crate::lanes::default_lanes();
        let ids: Vec<&str> = lanes.iter().map(|lane| lane.lane_id()).collect();
        assert_eq!(ids, vec![LANE_LEXICAL, LANE_GREP, LANE_GRAPH]);
    }

    #[test]
    fn lane_opting_out_of_annotation_stays_fusion_only() {
        // A lane with annotates_hits() == false (like the graph lane) must
        // still feed RRF fusion but contribute no per-hit reason.
        let (engine, _tmp) = scoped_test_engine();
        let request = SearchRequest {
            query: "anything".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let silent = FakeLane {
            id: "silent",
            enabled: true,
            lane_weight: 1.0,
            annotates: false,
            hits: vec![("chunk:src/x.rs".to_string(), 1.0)],
            ran: std::cell::Cell::new(false),
        };

        let lanes: [&dyn RetrievalLane; 1] = [&silent];
        let outcomes = run_lanes(&lanes, &context).unwrap();

        let fused = fuse_outcomes(&outcomes, 50);
        assert!(
            fused.contains_key("chunk:src/x.rs"),
            "opted-out lane must still contribute to RRF fusion"
        );

        let lane_ranks = plan.lane_ranks(&outcomes);
        let hit = plan
            .hit_from_chunk(
                fake_candidate_chunk(),
                &fused["chunk:src/x.rs"],
                &lane_ranks,
            )
            .unwrap();
        assert!(
            hit.reasons.is_empty(),
            "fusion-only lane must produce no per-hit reasons, got {:?}",
            hit.reasons
        );
    }

    #[test]
    fn run_lanes_iterates_collection_and_skips_disabled_lane_before_work() {
        let (engine, _tmp) = scoped_test_engine();
        let request = SearchRequest {
            query: "anything".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let context = LaneContext {
            plan: &plan,
            db: &engine.db,
            config: &engine.config,
            chunk_text_cache: &engine.chunk_text_cache,
        };

        let active = FakeLane {
            id: "fake-active",
            enabled: true,
            lane_weight: 1.0,
            annotates: false,
            hits: vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
            ran: std::cell::Cell::new(false),
        };
        let disabled = FakeLane {
            id: "fake-disabled",
            enabled: false,
            lane_weight: 0.0,
            annotates: false,
            hits: vec![("z".to_string(), 1.0)],
            ran: std::cell::Cell::new(false),
        };

        let lanes: [&dyn RetrievalLane; 2] = [&active, &disabled];
        let outcomes = run_lanes(&lanes, &context).unwrap();

        assert!(active.ran.get(), "enabled lane must run");
        assert!(
            !disabled.ran.get(),
            "disabled lane must be skipped before work"
        );
        assert_eq!(
            outcomes.iter().map(|o| o.lane_id).collect::<Vec<_>>(),
            vec!["fake-active", "fake-disabled"],
            "outcome order must follow lane collection order"
        );
        assert_eq!(outcomes[0].hits.len(), 2);
        assert!(
            outcomes[1].hits.is_empty(),
            "skipped lane contributes nothing"
        );
    }

    #[test]
    fn fuse_outcomes_accumulates_rrf_generically() {
        let outcomes = vec![
            LaneOutcome {
                lane_id: "fake-a",
                weight: 1.0,
                annotates_hits: false,
                score_slot: None,
                hits: vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
            },
            LaneOutcome {
                lane_id: "fake-b",
                weight: 0.5,
                annotates_hits: false,
                score_slot: None,
                hits: vec![("y".to_string(), 1.0)],
            },
        ];
        let fused = fuse_outcomes(&outcomes, 50);

        // score(d) = sum over lanes of weight / (k + rank)
        assert!((fused["x"].total - 1.0 / 51.0).abs() < 1e-12);
        assert!((fused["y"].total - (1.0 / 52.0 + 0.5 / 51.0)).abs() < 1e-12);

        // Per-lane breakdown is preserved in lane-accumulation order and
        // sums (left-to-right) to the fused total bit-for-bit.
        assert_eq!(fused["x"].by_lane, vec![("fake-a", 1.0 / 51.0)]);
        assert_eq!(
            fused["y"].by_lane,
            vec![("fake-a", 1.0 / 52.0), ("fake-b", 0.5 / 51.0)]
        );
        for fused_score in fused.values() {
            let component_sum: f64 = fused_score.by_lane.iter().map(|(_, v)| v).sum();
            assert_eq!(component_sum, fused_score.total);
        }
    }

    // ── score trace invariant: sum(components) == rerank_score ─────────
    //
    // Property exercised over hand-written scenarios: for every hit the
    // search engine returns, the score_trace bill must replay the final
    // rerank_score exactly (1e-9 tolerance), so a hit's ranking is fully
    // auditable from its trace alone.

    fn assert_trace_replays_rerank(hits: &[SearchHit]) {
        assert!(!hits.is_empty(), "scenario must produce at least one hit");
        for hit in hits {
            assert!(
                !hit.score_trace.is_empty(),
                "every hit must carry a score trace, got none for {}",
                hit.chunk_id
            );
            let component_sum: f64 = hit.score_trace.iter().map(|(_, amount)| amount).sum();
            assert!(
                (component_sum - hit.rerank_score).abs() < 1e-9,
                "score_trace must sum to rerank_score for {}: sum={} rerank={} trace={:?}",
                hit.chunk_id,
                component_sum,
                hit.rerank_score,
                hit.score_trace
            );
        }
    }

    fn trace_components(hit: &SearchHit) -> Vec<&str> {
        hit.score_trace
            .iter()
            .map(|(component, _)| component.as_str())
            .collect()
    }

    #[test]
    fn score_trace_replays_rerank_for_pure_lexical_hit() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "fn alpha_handler() { process() }",
        );

        let hits = engine
            .search(&SearchRequest {
                query: "alpha_handler".to_string(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            })
            .unwrap();

        assert_trace_replays_rerank(&hits);
        let components = trace_components(&hits[0]);
        assert!(
            components.contains(&"rrf:lexical"),
            "lexical hit must bill its lexical RRF component, got {components:?}"
        );
        // Query tokens appear in the chunk text, so the overlap term fires.
        assert!(
            components.contains(&"overlap"),
            "token overlap must be billed, got {components:?}"
        );
    }

    #[test]
    fn score_trace_replays_rerank_for_grep_and_graph_mix() {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig {
            search: SearchConfig {
                lexical_top_k: 5,
                grep_top_k: 5,
                rrf_k: 50,
                lexical_weight: 1.0,
                grep_weight: 0.7,
                rerank_window: 5,
                graph_weight: 0.6,
                graph_top_k: 12,
                ..Default::default()
            },
            ..Default::default()
        };
        let engine = SearchEngine::new(Arc::new(db), &config, None);
        let process_to_helper = CallEdgeRecord {
            edge_id: "edge:process->helper".to_string(),
            file_path: "src/a.rs".to_string(),
            caller_symbol: Some("process".to_string()),
            callee_symbol: "helper".to_string(),
            line: 1,
            caller_symbol_uid: Some("uid:process".to_string()),
            callee_symbol_uid: Some("uid:helper".to_string()),
            ..Default::default()
        };
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn process() { helper() }",
            "process",
            "uid:process",
            vec![process_to_helper],
        );
        insert_graph_file(
            &engine,
            "src/b.rs",
            "fn helper() {}",
            "helper",
            "uid:helper",
            vec![],
        );

        let hits = engine
            .search(&SearchRequest {
                query: "process".to_string(),
                top_k: 5,
                include_grep: true,
                ..Default::default()
            })
            .unwrap();

        assert_trace_replays_rerank(&hits);
        let seed = hits
            .iter()
            .find(|hit| hit.file_path == "src/a.rs")
            .expect("seed file must be a hit");
        let components = trace_components(seed);
        assert!(
            components.contains(&"rrf:lexical"),
            "expected lexical RRF component, got {components:?}"
        );
        assert!(
            components.contains(&"rrf:grep"),
            "expected grep RRF component, got {components:?}"
        );
        assert!(
            components.contains(&"rrf:graph"),
            "expected graph RRF component, got {components:?}"
        );
    }

    #[test]
    fn score_trace_replays_rerank_with_preselect_and_multiple_boosts() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "fn alpha_handler() { process() }",
        );
        insert_chunk_file(
            &engine,
            "docs/alpha.md",
            Language::Markdown,
            "alpha_handler design notes",
        );

        let hits = engine
            .search(&SearchRequest {
                query: "alpha_handler".to_string(),
                top_k: 5,
                include_grep: false,
                boost_file_paths: Some(vec!["src/alpha.rs".to_string()]),
                recent_file_paths: Some(vec!["src/alpha.rs".to_string()]),
                pinned_file_paths: Some(vec!["src/alpha.rs".to_string()]),
                overlay_file_paths: Some(vec!["src/alpha.rs".to_string()]),
                ..Default::default()
            })
            .unwrap();

        assert_trace_replays_rerank(&hits);
        let boosted = hits
            .iter()
            .find(|hit| hit.file_path == "src/alpha.rs")
            .expect("boosted file must be a hit");
        let components = trace_components(boosted);
        for expected in [
            "boost:working-set-boost",
            "boost:recent-file",
            "boost:pinned-context",
            "boost:overlay-neighbor",
            "boost:stage-a",
        ] {
            assert!(
                components.contains(&expected),
                "expected {expected} in trace, got {components:?}"
            );
        }
        // The doc file collects its own doc-file boost.
        if let Some(doc_hit) = hits.iter().find(|hit| hit.file_path == "docs/alpha.md") {
            assert!(
                trace_components(doc_hit).contains(&"boost:doc-file"),
                "doc hit must bill boost:doc-file, got {:?}",
                doc_hit.score_trace
            );
        }
    }

    #[test]
    fn score_trace_replays_rerank_with_dsl_name_bonus() {
        let (engine, _tmp) = scoped_test_engine();
        // Chunk with a symbol_name so both symbol-exact and dsl-name fire.
        let chunk = ChunkRecord {
            chunk_id: "chunk:src/named.rs".to_string(),
            file_path: "src/named.rs".to_string(),
            language: Language::Rust,
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            breadcrumb: "root".to_string(),
            text: "fn alpha_handler() {}".to_string(),
            symbol_name: Some("alpha_handler".to_string()),
            symbol_kind: Some(cc_model::SymbolKind::Function),
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        };
        let conn = crate::test_seed::seed_conn(&engine.db);
        IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: "src/named.rs".to_string(),
                language: Language::Rust,
                content_hash: "hash-named".to_string(),
                mtime: 0.0,
                size: 10,
                outcome: ParseOutcome {
                    summary: "fixture".to_string(),
                    chunks: vec![chunk],
                    parser_tier: ParserTier::TreeSitter,
                    parser_confidence: 1.0,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        drop(conn);

        let hits = engine
            .search(&SearchRequest {
                query: "name:alpha_handler alpha_handler".to_string(),
                top_k: 5,
                include_grep: false,
                ..Default::default()
            })
            .unwrap();

        assert_trace_replays_rerank(&hits);
        let components = trace_components(&hits[0]);
        assert!(
            components.contains(&"boost:dsl-name"),
            "dsl-name bonus applied after hit construction must be billed, got {components:?}"
        );
        assert!(
            components.contains(&"boost:symbol-exact"),
            "expected symbol-exact boost, got {components:?}"
        );
    }

    #[test]
    fn score_trace_replays_rerank_after_graph_rerank() {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
        let config = ProjectConfig::default();
        let engine = SearchEngine::new(Arc::new(db), &config, None);
        let process_to_helper = CallEdgeRecord {
            edge_id: "edge:process->helper".to_string(),
            file_path: "src/a.rs".to_string(),
            caller_symbol: Some("process".to_string()),
            callee_symbol: "helper".to_string(),
            line: 1,
            caller_symbol_uid: Some("uid:process".to_string()),
            callee_symbol_uid: Some("uid:helper".to_string()),
            ..Default::default()
        };
        insert_graph_file(
            &engine,
            "src/a.rs",
            "fn process() { helper() }",
            "process",
            "uid:process",
            vec![process_to_helper],
        );
        insert_graph_file(
            &engine,
            "src/b.rs",
            "fn helper() {}",
            "helper",
            "uid:helper",
            vec![],
        );

        let outcome = engine
            .search_with_graph_context(
                &SearchRequest {
                    query: "process".to_string(),
                    top_k: 5,
                    include_grep: true,
                    ..Default::default()
                },
                &RepoSizeTier::Small.graph_enrich_limits(),
                4000,
            )
            .unwrap();
        let hits = &outcome.0;

        assert_trace_replays_rerank(hits);
        // At least one hit must have received (and billed) the post-search
        // graph-rerank contribution.
        assert!(
            hits.iter()
                .any(|hit| trace_components(hit).contains(&"boost:graph-rerank")),
            "graph rerank contribution must appear in some hit's trace: {:?}",
            hits.iter().map(trace_components).collect::<Vec<_>>()
        );
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
                ..Default::default()
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
