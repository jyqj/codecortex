//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.
//!
//! `SearchEngine` is one type split across sibling files, each contributing
//! an `impl SearchEngine` block (cc-db `index_db_*.rs` style):
//! - this file — the struct, construction ([`SearchEngine::new`]), and the
//!   core [`SearchEngine::search`] / detailed planned-search orchestration;
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
use crate::lanes::{
    default_lanes, fuse_outcomes, fused_candidate_ordering, materialize_lane_outcomes, run_lanes,
    LaneContext,
};
pub use crate::plan::is_project_doc;
use crate::plan::{CandidateChunk, SearchPlan};

/// Detailed retrieval keeps bounded-scan state independent of whether any hit exists.
#[derive(Default)]
pub struct SearchResult {
    pub cost: cc_model::retrieval_cost::RetrievalCost,
    pub hits: Vec<SearchHit>,
    pub lanes: Vec<cc_model::retrieval::LaneOutcome>,
    pub grep: Option<cc_model::retrieval::GrepDiagnostics>,
    pub scope: Option<cc_model::context::SearchScopeExplain>,
}

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) config: SearchConfig,
    pub(crate) query_config: cc_model::query::QueryConfig,
    pub(crate) retired: Mutex<bool>,
    pub(crate) ranking: RankingConfig,
    pub(crate) repo_tier: Option<RepoSizeTier>,
    /// Index epoch last observed by this engine. The result caches are keyed
    /// on the epochs read from the DB at search time, so they never need
    /// explicit invalidation; this field only detects epoch changes so the
    /// epoch-keyed chunk text cache can be cleared eagerly for memory hygiene.
    pub(crate) last_seen_incarnation: Mutex<Option<[u8; 16]>>,
    pub(crate) last_seen_index_epoch: AtomicU64,
    /// Evidence epoch last observed by this engine (companion to
    /// `last_seen_index_epoch`): detects evidence-only bumps so the
    /// graph-aware result cache can be cleared eagerly for memory hygiene
    /// (correctness comes from the epoch pair in its key).
    pub(crate) last_seen_evidence_epoch: AtomicU64,
    /// LRU result cache keyed by `(index_epoch, query_hash)`.
    ///
    /// INVARIANT: the stored slice is FINAL — planned retrieval assigns all
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
    /// Hash of the retrieval policy, complete [`RankingConfig`] and [`SearchConfig`].  Computed ONCE at
    /// construction — `config` and `ranking` are cloned into the engine in
    /// [`Self::new`] and never mutated afterwards (cc-server rebuilds the
    /// engine on project/config change), so the fingerprint is immutable
    /// per instance.  Folded into the graph-aware cache key.
    pub(crate) ranking_fingerprint: u64,
    /// LRU cache keyed by captured `(index_epoch, chunk_id)`. A late old reader
    /// cannot repopulate current-generation text after invalidation.
    pub(crate) chunk_text_cache: Mutex<LruCache<(u64, String), Arc<str>>>,
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
            query_config: config.query.clone(),
            retired: Mutex::new(false),
            ranking: config.ranking.clone(),
            repo_tier,
            last_seen_incarnation: Mutex::new(None),
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

    pub fn check_live(&self) -> CcResult<()> {
        if *self.retired.lock().unwrap_or_else(|p| p.into_inner()) {
            Err(cc_model::CcError::QueryInvalidated)
        } else {
            Ok(())
        }
    }
    pub fn retire(&self) {
        let mut retired = self.retired.lock().unwrap_or_else(|p| p.into_inner());
        *retired = true;
        self.invalidate_cache();
    }
    pub(crate) fn controlled_request(&self, request: &SearchRequest) -> CcResult<SearchRequest> {
        self.check_live()?;
        self.query_config.validate()?;
        crate::query_policy::QueryPolicy::resolve(
            &self.query_config,
            request,
            request.semantic.is_some(),
        )?;
        let mut request = request.clone();
        let budget = std::time::Duration::from_millis(self.query_config.deadline_ms);
        request.control = Some(match &request.control {
            Some(c) => c.limit_total(budget),
            None => cc_model::query::QueryControl::new(budget)?,
        });
        request
            .control
            .as_ref()
            .expect("installed query control")
            .check()?;
        Ok(request)
    }
    pub(crate) fn publish_query<T>(
        &self,
        request: &SearchRequest,
        write: impl FnOnce() -> T,
    ) -> CcResult<T> {
        let retired = self.retired.lock().unwrap_or_else(|p| p.into_inner());
        if *retired {
            return Err(cc_model::CcError::QueryInvalidated);
        }
        match &request.control {
            Some(control) => control.publish(write),
            None => Ok(write()),
        }
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.reads().stats(std::path::Path::new(""))
    }

    /// Core search — FTS5 + grep with RRF fusion and reranking.
    ///
    /// INVARIANT: `rerank_score` on the returned hits is FINAL and the list
    /// uses exact-identity tier then score then stable ID. Callers must not re-score or re-sort; graph-aware
    /// reranking happens inside [`Self::search_with_graph_context`], never
    /// downstream.  The shared `Arc<[SearchHit]>` return type enforces this:
    /// a result-cache hit is an `Arc` clone of the stored slice, so mutating
    /// it would corrupt the cache.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Arc<[SearchHit]>> {
        let request = self.controlled_request(request)?;
        let request = &request;
        let qhash = self.retrieval_query_hash(request);
        let (generation, (results, cacheable)) = self.with_stable_generation(|generation| {
            let cache_key = generation.local_key(qhash);
            request.control.as_ref().unwrap().check()?;
            if let Ok(mut cache) = self.result_cache.lock() {
                if let Some(cached) = cache.get(&cache_key).filter(|_| request.semantic.is_none()) {
                    self.result_cache_hits.fetch_add(1, Ordering::Relaxed);
                    return Ok((Arc::clone(cached), false));
                }
            }
            self.result_cache_misses.fetch_add(1, Ordering::Relaxed);
            let retrieved = self.search_internal_detailed(request, true, generation.index_epoch)?;
            let cacheable = retrieved.grep.as_ref().is_none_or(|g| !g.prefilter_failed)
                && retrieved
                    .lanes
                    .iter()
                    .all(cc_model::retrieval::LaneOutcome::is_cacheable);
            let results: Arc<[SearchHit]> = retrieved.hits.into();
            Ok((results, cacheable))
        })?;
        // A write after the read fence is safe: the accepted result is stored
        // only under its own epoch, never the writer's newer generation.
        self.publish_query(request, || {
            if cacheable && request.semantic.is_none() {
                if let Ok(mut cache) = self.result_cache.lock() {
                    cache.put(generation.local_key(qhash), Arc::clone(&results));
                }
            }
        })?;
        Ok(results)
    }

    /// Fresh query with coverage diagnostics; the legacy `search` returns hits only.
    pub fn search_with_diagnostics(&self, request: &SearchRequest) -> CcResult<SearchResult> {
        let request = self.controlled_request(request)?;
        let request = &request;
        let (_, result) = self.with_stable_generation(|generation| {
            self.search_internal_detailed(request, true, generation.index_epoch)
        })?;
        self.publish_query(request, || result)
    }

    /// Shared retrieval path. The caller observes epochs before invoking this.
    /// `truncate_to_top_k=false` retains the wider rerank window for graph scoring.
    /// This function always recomputes; both public cached paths own their LRUs.
    pub(crate) fn search_internal_detailed(
        &self,
        request: &SearchRequest,
        truncate_to_top_k: bool,
        cache_epoch: u64,
    ) -> CcResult<SearchResult> {
        // No pooled read connection is held here: plan build (preselect),
        // each lane, and the batch fetch below all check out and release
        // their own, so a 1-connection read pool never sees nested checkouts.
        self.check_live()?;
        let plan = SearchPlan::build_configured(
            &self.db,
            &self.config,
            &self.ranking,
            &self.query_config,
            request,
            self.repo_tier,
        )?;
        self.search_planned_detailed(&plan, truncate_to_top_k, cache_epoch)
    }

    pub(crate) fn search_planned_detailed(
        &self,
        plan: &SearchPlan,
        truncate_to_top_k: bool,
        cache_epoch: u64,
    ) -> CcResult<SearchResult> {
        let limits = plan.limits();
        let mut cost = cc_model::retrieval_cost::RetrievalCost::default();
        let scope = Some(plan.scope_explain(&self.config));

        // Retrieval lanes from the central registry
        // (`lanes::default_lanes()`), executed in deterministic fusion
        // order so RRF tie-breaking stays stable.
        let lanes = default_lanes();
        let lane_context = LaneContext {
            plan,
            cache_epoch,
            db: &self.db,
            config: &self.config,
            chunk_text_cache: &self.chunk_text_cache,
        };
        let mut lane_outcomes = run_lanes(&lanes, &lane_context)?;
        crate::lanes::append_semantic_outcome(&mut lane_outcomes, &lane_context)?;
        plan.control().check()?;
        let public_lane_outcomes = materialize_lane_outcomes(&mut lane_outcomes, &lane_context)?;
        for lane in &lane_outcomes {
            cost.lane_candidates
                .insert(lane.lane_id.to_string(), lane.hits.len());
            cost.lexical_sql.merge(lane.lexical_work);
        }
        let grep = lane_outcomes
            .iter()
            .find_map(|outcome| outcome.grep.clone());

        // RRF fusion across all lane outcomes; each candidate keeps its
        // per-lane contribution breakdown for the hit's score trace.
        let fused = fuse_outcomes(&lane_outcomes, self.config.rrf_k)?;
        debug_assert!(fused
            .values()
            .all(|score| crate::fusion::replay_fused(score).is_ok()));
        cost.fused_candidates = fused.len();

        let mut candidates: Vec<(String, crate::lanes::FusedScore)> = fused.into_iter().collect();
        // Named fusion contract (`fusion::fused_candidate_ordering`):
        // exact-identity tier, fused total descending, chunk_id ascending.
        candidates.sort_by(fused_candidate_ordering);
        candidates.truncate(limits.rerank_window);
        cost.hydrate_requested = candidates.len();

        if candidates.is_empty() {
            return Ok(SearchResult {
                hits: Vec::new(),
                lanes: public_lane_outcomes,
                grep,
                scope,
                cost,
            });
        }

        plan.control().check()?;
        let lane_ranks = plan.lane_ranks(&lane_outcomes);
        let expected_identities: HashMap<_, _> = public_lane_outcomes
            .iter()
            .flat_map(|lane| &lane.candidates)
            .map(|candidate| (candidate.legacy_chunk_id.as_str(), candidate))
            .collect();

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
                        if let Some(text) = cache.get(&(cache_epoch, cid.clone())) {
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
                .chunk_rows_by_ids_with_work(&chunk_ids_refs, &cached_texts)?;
            cost.hydration = rows.work;
            let mut map = HashMap::with_capacity(candidates.len());
            for data in rows.rows {
                let expected =
                    expected_identities
                        .get(data.chunk_id.as_str())
                        .ok_or_else(|| {
                            cc_model::CcError::Search("unexpected hydrated candidate".into())
                        })?;
                crate::fusion::validate_hydrated_candidate(
                    expected,
                    data.document.as_ref(),
                    data.source_evidence.as_ref().map(|proof| proof.span),
                )?;

                // Also populate cache for chunks that weren't cached yet,
                // benefiting subsequent searches against the same codebase.
                if !cached_texts.contains_key(&data.chunk_id) {
                    plan.control().publish(|| {
                        if let Ok(mut cache) = self.chunk_text_cache.lock() {
                            cache.put(
                                (cache_epoch, data.chunk_id.clone()),
                                Arc::from(data.text.as_str()),
                            );
                        }
                    })?;
                }
                map.insert(data.chunk_id.clone(), CandidateChunk::from(data));
            }
            map
        };

        plan.control().check()?;
        let mut results = Vec::new();
        for (chunk_id, fused_score) in &candidates {
            let chunk = chunk_map.remove(chunk_id).ok_or_else(|| {
                cc_model::CcError::Search(
                    "candidate removed between retrieval and hydration; retry".into(),
                )
            })?;
            if let Some(hit) = plan.hit_from_chunk(chunk, fused_score, &lane_ranks) {
                results.push(hit);
            }
        }

        if truncate_to_top_k {
            plan.finalize_results(&mut results);
        } else {
            plan.finalize_results_with_limit(&mut results, limits.rerank_window);
        }

        plan.control().check()?;
        // Pipeline exit invariant (debug builds only): every traced hit's
        // bill must replay its final rerank_score.
        crate::score_trace::validate_trace_consistency(&results)?;
        crate::score_trace::debug_assert_trace_consistency(&results);

        Ok(SearchResult {
            hits: results,
            lanes: public_lane_outcomes,
            grep,
            scope,
            cost,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cc_db::index_db::IndexDb;
    use cc_model::config::{ProjectConfig, SearchConfig};
    use cc_model::{CallEdgeRecord, ChunkRecord, Language, ParseOutcome, ParserTier};
    use std::sync::Arc;

    use crate::engine_test_support::{
        chunk_write_unit, documented_write_unit, insert_chunk_file, insert_graph_file,
        scoped_test_engine,
    };

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

        // Ordered hints have rank-decay semantics; swapping pins changes ranking.
        let pinned_ab = SearchRequest {
            pinned_file_paths: Some(vec!["a.rs".into(), "b.rs".into()]),
            ..base.clone()
        };
        let pinned_ba = SearchRequest {
            pinned_file_paths: Some(vec!["b.rs".into(), "a.rs".into()]),
            ..base.clone()
        };
        assert_ne!(
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

    // ── Lane seam (RetrievalLane trait) ────────────────────────

    use crate::lanes::{GrepLane, RetrievalLane};
    fn build_plan(engine: &SearchEngine, request: &SearchRequest) -> SearchPlan {
        SearchPlan::build(&engine.db, &engine.config, &engine.ranking, request, None).unwrap()
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
            cache_epoch: 0,
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
            cache_epoch: 0,
        };
        let hits = GrepLane.run(&context).unwrap();
        assert_eq!(hits, vec![("chunk:src/svc.rs".to_string(), 1.0)]);
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
            cache_epoch: 0,
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
            cache_epoch: 0,
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
            source: None,
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
        let unit = documented_write_unit(
            "src/named.rs",
            Language::Rust,
            "fn alpha_handler() {}",
            ParseOutcome {
                summary: "fixture".to_string(),
                chunks: vec![chunk],
                parser_tier: ParserTier::TreeSitter,
                parser_confidence: 1.0,
                ..Default::default()
            },
        );
        let conn = crate::test_seed::seed_conn(&engine.db);
        IndexDb::insert_file_data(&conn, &unit).unwrap();
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
