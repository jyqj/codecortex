//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.
//!
//! `SearchEngine` is one type split across sibling files, each contributing
//! an `impl SearchEngine` block (cc-db `index_db_*.rs` style):
//! - this file — the struct, construction ([`SearchEngine::new`]), and the
//!   core [`SearchEngine::search`] / `search_internal` orchestration;
//! - [`crate::engine_cache`] — the three LRUs (result / graph-aware result /
//!   chunk text), epoch observation, cache keys, and [`CacheStats`]
//!   (re-exported here so the `engine::CacheStats` path is unchanged);
//! - [`crate::engine_graph`] — the graph-aware search path
//!   ([`SearchEngine::search_with_graph_context`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::index_db::IndexDb;
use cc_model::config::{ProjectStats, RankingConfig, RepoSizeTier, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::CcResult;

pub use crate::engine_cache::CacheStats;
use crate::engine_cache::{
    cache_capacity_from_env, GraphResultCache, ResultCache, CHUNK_TEXT_CACHE_CAPACITY,
    GRAPH_RESULT_CACHE_CAPACITY, RESULT_CACHE_CAPACITY,
};
use crate::lanes::{default_lanes, fuse_outcomes, run_lanes, LaneContext};
pub use crate::plan::is_project_doc;
use crate::plan::{CandidateChunk, SearchPlan};

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
    pub(crate) last_seen_index_epoch: AtomicU64,
    /// Evidence epoch last observed by this engine (companion to
    /// `last_seen_index_epoch`): detects evidence-only bumps so the
    /// graph-aware result cache can be cleared eagerly for memory hygiene
    /// (correctness comes from the epoch pair in its key).
    pub(crate) last_seen_evidence_epoch: AtomicU64,
    /// LRU result cache keyed by `(index_epoch, query_hash)`.
    ///
    /// INVARIANT: the stored slice is FINAL — `search_internal` assigns all
    /// scores, sorts, and truncates before the `put` in [`Self::search`], and
    /// nothing mutates hits afterwards (`search_with_graph_context`, which
    /// does mutate, never touches this cache — it has its own
    /// post-enrichment cache below).  A hit therefore returns `Arc::clone`
    /// of the shared slice with no per-hit deep copy.
    pub(crate) result_cache: Mutex<ResultCache>,
    /// LRU result cache for the graph-aware path, keyed by
    /// `(index_epoch, evidence_epoch, graph_query_hash)` — see
    /// [`Self::search_with_graph_context`].  Kept separate from
    /// `result_cache` because the stored values embed post-enrichment state
    /// (graph rerank + context nodes) that also depends on evidence-boosted
    /// edge confidence, hence the evidence_epoch in the key.
    pub(crate) graph_result_cache: Mutex<GraphResultCache>,
    /// Hash of the ranking-relevant config: the full [`RankingConfig`] plus
    /// [`SearchConfig`]'s `graph_weight`/`graph_top_k`.  Computed ONCE at
    /// construction — `config` and `ranking` are cloned into the engine in
    /// [`Self::new`] and never mutated afterwards (cc-server rebuilds the
    /// engine on project/config change), so the fingerprint is immutable
    /// per instance.  Folded into the graph-aware cache key.
    pub(crate) ranking_fingerprint: u64,
    /// LRU cache of decompressed chunk text keyed by `chunk_id`.
    pub(crate) chunk_text_cache: Mutex<LruCache<String, Arc<str>>>,
    /// Hit/miss counters for `result_cache` and `graph_result_cache`,
    /// snapshot via [`Self::cache_stats`] (Relaxed atomic reads). Quantify
    /// the warm vs cold split that `bench` reports as per-tool warm/cold
    /// latency. Monotonic over the engine's lifetime; never reset.
    pub(crate) result_cache_hits: AtomicU64,
    pub(crate) result_cache_misses: AtomicU64,
    pub(crate) graph_cache_hits: AtomicU64,
    pub(crate) graph_cache_misses: AtomicU64,
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
            result_cache_hits: AtomicU64::new(0),
            result_cache_misses: AtomicU64::new(0),
            graph_cache_hits: AtomicU64::new(0),
            graph_cache_misses: AtomicU64::new(0),
        }
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.reads().stats(std::path::Path::new(""))
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
                self.result_cache_hits.fetch_add(1, Ordering::Relaxed);
                tracing::debug!(
                    query = %request.query,
                    "search cache hit (index_epoch={}, hash={})",
                    index_epoch,
                    qhash,
                );
                return Ok(Arc::clone(cached));
            }
        }
        self.result_cache_misses.fetch_add(1, Ordering::Relaxed);

        let (hits, diagnostics) = self.search_internal(request, true)?;
        let results: Arc<[SearchHit]> = hits.into();
        if diagnostics.cacheable() {
            if let Ok(mut cache) = self.result_cache.lock() {
                cache.put(cache_key, Arc::clone(&results));
            }
        }
        Ok(results)
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
    pub(crate) fn search_internal(
        &self,
        request: &SearchRequest,
        truncate_to_top_k: bool,
    ) -> CcResult<(Vec<SearchHit>, crate::diagnostics::RetrievalDiagnostics)> {
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
        let mut diagnostics = plan.diagnostics();
        let fused = fuse_outcomes(&lane_outcomes, self.config.rrf_k);

        let mut candidates: Vec<(String, crate::lanes::FusedScore)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| {
            b.1.total
                .partial_cmp(&a.1.total)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Deterministic tie-break on chunk id so equal fused scores (or
                // NaN, which compares Equal) produce a stable order across runs
                // and platforms instead of depending on HashMap iteration order.
                .then_with(|| a.0.cmp(&b.0))
        });
        diagnostics.candidate_union = candidates.len();
        if self.config.trace_candidates {
            // Explicit evaluation instrumentation; don't silently claim complete
            // stage recall when a configured candidate set exceeds the trace cap.
            diagnostics.trace_truncated = candidates.len() > 512;
            let ids: Vec<_> = candidates
                .iter()
                .take(512)
                .map(|(id, _)| id.as_str())
                .collect();
            let rows = self
                .db
                .retrieval()
                .chunk_rows_by_ids(&ids, &HashMap::new())?;
            let locations: HashMap<_, _> = rows
                .into_iter()
                .map(|r| {
                    (
                        r.chunk_id.clone(),
                        crate::diagnostics::CandidateLocation {
                            chunk_id: r.chunk_id,
                            file_path: r.file_path,
                            start_line: r.start_line,
                            end_line: r.end_line,
                            symbol_name: r.symbol_name,
                        },
                    )
                })
                .collect();
            diagnostics.stages.insert(
                "candidate_union".into(),
                ids.iter()
                    .filter_map(|id| locations.get(*id).cloned())
                    .collect(),
            );
            for lane in &lane_outcomes {
                diagnostics.stages.insert(
                    lane.lane_id.to_string(),
                    lane.hits
                        .iter()
                        .filter_map(|(id, _)| locations.get(id).cloned())
                        .collect(),
                );
            }
        }
        candidates.truncate(limits.rerank_window);
        diagnostics.rerank_candidates = candidates.len();
        if let Some(union) = diagnostics.stages.get("candidate_union") {
            let ids: std::collections::HashSet<_> =
                candidates.iter().map(|(id, _)| id.as_str()).collect();
            let locators = union
                .iter()
                .filter(|l| ids.contains(l.chunk_id.as_str()))
                .cloned()
                .collect();
            diagnostics.stages.insert("rerank_window".into(), locators);
        }

        if candidates.is_empty() {
            return Ok((Vec::new(), diagnostics));
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
                .retrieval()
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

        diagnostics.returned = results.len();
        if self.config.trace_candidates {
            diagnostics.stages.insert(
                "ranked".into(),
                results
                    .iter()
                    .map(|h| crate::diagnostics::CandidateLocation {
                        chunk_id: h.chunk_id.clone(),
                        file_path: h.file_path.clone(),
                        start_line: h.start_line,
                        end_line: h.end_line,
                        symbol_name: h.symbol_name.clone(),
                    })
                    .collect(),
            );
        }
        Ok((results, diagnostics))
    }

    /// Uncached diagnostic view of the same production pipeline. Errors and
    /// empty results retain diagnostics; no global last-query mutable state.
    pub fn search_diagnosed(
        &self,
        request: &SearchRequest,
    ) -> CcResult<(Vec<SearchHit>, crate::diagnostics::RetrievalDiagnostics)> {
        self.observe_epochs()?;
        self.search_internal(request, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cc_db::index_db::{FileWriteUnit, IndexDb};
    use cc_model::config::{ProjectConfig, SearchConfig};
    use cc_model::{CallEdgeRecord, ChunkRecord, Language, ParseOutcome, ParserTier, SymbolRecord};
    use std::sync::Arc;

    use crate::engine_test_support::{
        chunk_write_unit, insert_chunk_file, insert_graph_file, scoped_test_engine,
    };

    #[test]
    fn grep_hard_scope_rescues_mid_token_outside_working_set() {
        let (mut engine, _tmp) = scoped_test_engine();
        engine.config.grep_weight = 1.0;
        insert_chunk_file(
            &engine,
            "src/a.rs",
            Language::Rust,
            "fn a() { let s = \"leftNeedleRight\"; }",
        );
        insert_chunk_file(&engine, "src/z.rs", Language::Rust, "fn unrelated() {}");
        let req = SearchRequest {
            query: "Needle".into(),
            include_grep: true,
            top_k: 5,
            boost_file_paths: Some(vec!["src/z.rs".into()]),
            file_preselect_limit: Some(1),
            ..Default::default()
        };
        let (hits, diag) = engine.search_diagnosed(&req).unwrap();
        assert!(hits.iter().any(|h| h.file_path == "src/a.rs"));
        assert!(diag.lanes["grep"].returned > 0);
        let scoped = SearchRequest {
            file_paths: Some(vec!["src/z.rs".into()]),
            ..req
        };
        assert!(engine.search_diagnosed(&scoped).unwrap().0.is_empty());
    }

    #[test]
    fn empty_budget_limited_search_retains_diagnostics_and_is_not_cached() {
        let (mut engine, _tmp) = scoped_test_engine();
        engine.config.grep_scan_cap = 0;
        engine.config.grep_weight = 1.0;
        insert_chunk_file(&engine, "src/a.rs", Language::Rust, "fn a() {}");
        let req = SearchRequest {
            query: "absentliteral".into(),
            include_grep: true,
            top_k: 5,
            ..Default::default()
        };
        let (hits, diag) = engine.search_diagnosed(&req).unwrap();
        assert!(hits.is_empty());
        assert!(diag.lanes["grep"].work_limited);
        assert!(!diag.cacheable());
        let first = engine.search(&req).unwrap();
        let second = engine.search(&req).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn full_graph_ablation_skips_all_graph_reads_and_zero_weight_lanes() {
        let (mut engine, _tmp) = scoped_test_engine();
        engine.config.graph_features = false;
        engine.config.grep_weight = 0.0;
        insert_chunk_file(&engine, "src/a.rs", Language::Rust, "fn marker() {}");
        crate::test_seed::seed_conn(&engine.db)
            .execute("DROP TABLE call_edges", [])
            .unwrap();
        let req = SearchRequest {
            query: "marker".into(),
            include_grep: true,
            top_k: 5,
            ..Default::default()
        };
        let outcome = engine
            .search_with_graph_context(&req, &RepoSizeTier::Small.graph_enrich_limits(), 4000)
            .unwrap();
        assert_eq!(outcome.0.len(), 1);
        assert!(outcome.1.nodes.is_empty());
        assert!(outcome.1.graph_explain.read_errors.is_empty());
        assert!(!outcome.1.retrieval.lanes["graph"].enabled);
        assert!(!outcome.1.retrieval.lanes["grep"].enabled);
        assert!(outcome.1.retrieval.cacheable());
        assert!(outcome
            .0
            .iter()
            .all(|hit| !hit.score_trace.iter().any(|c| c.0.contains("graph"))));
    }

    #[test]
    fn stage_trace_exposes_candidate_identity_only_when_enabled() {
        let (mut engine, _tmp) = scoped_test_engine();
        insert_chunk_file(&engine, "src/a.rs", Language::Rust, "fn marker() {}");
        let req = SearchRequest {
            query: "marker".into(),
            top_k: 5,
            ..Default::default()
        };
        assert!(engine.search_diagnosed(&req).unwrap().1.stages.is_empty());
        engine.config.trace_candidates = true;
        let (_, diag) = engine.search_diagnosed(&req).unwrap();
        assert_eq!(diag.stages["candidate_union"][0].file_path, "src/a.rs");
        assert!(!diag.trace_truncated);
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
    fn build_plan(engine: &SearchEngine, request: &SearchRequest) -> SearchPlan {
        SearchPlan::build(&engine.db, &engine.config, &engine.ranking, request, None).unwrap()
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
        let (mut engine, _tmp) = scoped_test_engine();
        engine.config.grep_weight = 1.0;
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
        assert!(
            lane.is_enabled(&context),
            "include_grep=true and positive weight enable grep"
        );
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

    /// The FTS prefilter phrase mirrors unicode61 tokenization: alphanumeric
    /// runs, quoted as a phrase, with a trailing `*` when the literal ends
    /// mid-token. Punctuation-only literals yield no phrase (full scan only).
    #[test]
    fn grep_prefilter_phrase_tokenizes_like_unicode61() {
        use crate::lanes::grep_prefilter_phrase;
        assert_eq!(
            grep_prefilter_phrase("getUserById"),
            Some("\"getUserById\"*".to_string())
        );
        assert_eq!(
            grep_prefilter_phrase("get_user_by_id"),
            Some("\"get user by id\"*".to_string())
        );
        assert_eq!(
            grep_prefilter_phrase("read(&mut buf)"),
            Some("\"read mut buf\"".to_string()),
            "literal ending at a token boundary needs no prefix star"
        );
        assert_eq!(
            grep_prefilter_phrase("->"),
            None,
            "punctuation-only literal has no tokenizable content"
        );
        assert_eq!(
            grep_prefilter_phrase("a"),
            None,
            "single-character tokens alone are too noisy to prefilter"
        );
    }

    /// The unscoped grep scan must still find matches the FTS tokenizer
    /// cannot see (a mid-token substring like `UserById` inside
    /// `getUserById`): stage 1's prefilter misses them, stage 2's full scan
    /// covers them. Token-boundary matches keep working too.
    #[test]
    fn grep_lane_prefilter_keeps_midtoken_matches_via_full_scan() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/svc.rs",
            Language::Rust,
            "fn getUserById(id: u64) {}",
        );
        insert_chunk_file(&engine, "src/other.rs", Language::Rust, "nothing here");

        // Unscoped (empty preselect): the prefilter stage runs. The query is
        // a mid-token substring — FTS sees only the token `getuserbyid`, so
        // `\"userbyid\"*` matches nothing and stage 2 must find the hit.
        let request = SearchRequest {
            query: "UserById".to_string(),
            top_k: 5,
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
        let hits = GrepLane.run(&context).unwrap();
        assert_eq!(
            hits,
            vec![("chunk:src/svc.rs".to_string(), 1.0)],
            "mid-token substring must survive the prefilter via stage-2 full scan"
        );

        // Token-boundary query (prefilter-visible) finds the same chunk.
        let request = SearchRequest {
            query: "getUserById".to_string(),
            top_k: 5,
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
        let hits = GrepLane.run(&context).unwrap();
        assert_eq!(hits, vec![("chunk:src/svc.rs".to_string(), 1.0)]);
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
        ran: std::sync::atomic::AtomicBool,
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
            self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
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
            ran: std::sync::atomic::AtomicBool::new(false),
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
            ran: std::sync::atomic::AtomicBool::new(false),
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
            ran: std::sync::atomic::AtomicBool::new(false),
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
            ran: std::sync::atomic::AtomicBool::new(false),
        };
        let disabled = FakeLane {
            id: "fake-disabled",
            enabled: false,
            lane_weight: 0.0,
            annotates: false,
            hits: vec![("z".to_string(), 1.0)],
            ran: std::sync::atomic::AtomicBool::new(false),
        };

        let lanes: [&dyn RetrievalLane; 2] = [&active, &disabled];
        let outcomes = run_lanes(&lanes, &context).unwrap();

        assert!(
            active.ran.load(std::sync::atomic::Ordering::SeqCst),
            "enabled lane must run"
        );
        assert!(
            !disabled.ran.load(std::sync::atomic::Ordering::SeqCst),
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

    /// Parallel lane execution (3+ enabled lanes → scoped threads) must
    /// preserve outcome order (= slice order) and run every enabled lane —
    /// RRF fusion and tie-breaking depend on that order being deterministic.
    #[test]
    fn run_lanes_parallel_preserves_slice_order_and_runs_all() {
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

        let mk = |id: &'static str, hit: &str| FakeLane {
            id,
            enabled: true,
            lane_weight: 1.0,
            annotates: false,
            hits: vec![(hit.to_string(), 1.0)],
            ran: std::sync::atomic::AtomicBool::new(false),
        };
        let a = mk("lane-a", "hit-a");
        let b = mk("lane-b", "hit-b");
        let c = mk("lane-c", "hit-c");

        let lanes: [&dyn RetrievalLane; 3] = [&a, &b, &c];
        let outcomes = run_lanes(&lanes, &context).unwrap();

        for lane in [&a, &b, &c] {
            assert!(
                lane.ran.load(std::sync::atomic::Ordering::SeqCst),
                "every enabled lane must run"
            );
        }
        assert_eq!(
            outcomes.iter().map(|o| o.lane_id).collect::<Vec<_>>(),
            vec!["lane-a", "lane-b", "lane-c"],
            "outcome order must follow slice order regardless of completion order"
        );
        assert_eq!(outcomes[0].hits[0].0, "hit-a");
        assert_eq!(outcomes[1].hits[0].0, "hit-b");
        assert_eq!(outcomes[2].hits[0].0, "hit-c");
    }

    /// A failing lane aborts the whole search even when lanes run
    /// concurrently, and the error surfaced is the first failure in slice
    /// order (deterministic regardless of thread scheduling).
    #[test]
    fn run_lanes_parallel_propagates_first_error_in_slice_order() {
        struct FailLane {
            id: &'static str,
        }
        impl RetrievalLane for FailLane {
            fn lane_id(&self) -> &'static str {
                self.id
            }
            fn weight(&self, _config: &SearchConfig) -> f64 {
                1.0
            }
            fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
                true
            }
            fn annotates_hits(&self) -> bool {
                false
            }
            fn run(&self, _context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
                Err(cc_model::CcError::Search(format!("{} failed", self.id)))
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

        let ok = FakeLane {
            id: "lane-ok",
            enabled: true,
            lane_weight: 1.0,
            annotates: false,
            hits: vec![],
            ran: std::sync::atomic::AtomicBool::new(false),
        };
        let fail_b = FailLane { id: "lane-b" };
        let fail_c = FailLane { id: "lane-c" };

        let lanes: [&dyn RetrievalLane; 3] = [&ok, &fail_b, &fail_c];
        let err = match run_lanes(&lanes, &context) {
            Err(e) => e,
            Ok(_) => panic!("failing lane must abort the search"),
        };
        assert!(
            err.to_string().contains("lane-b failed"),
            "first failing lane in slice order must win, got: {err}"
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
}
