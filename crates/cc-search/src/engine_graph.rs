//! Graph-aware search path — [`SearchEngine::search_with_graph_context`]:
//! core search over a widened candidate window, connectivity-based graph
//! rerank, and the epoch-pair-keyed graph result cache.
//!
//! `impl SearchEngine` continuation of [`crate::engine`] (cc-db
//! `index_db_*.rs` style); the method body is unchanged.  The enrichment
//! itself (neighbor/test/route context nodes) lives in [`crate::enrich`];
//! this file owns how the engine drives it and caches the final pair.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use cc_model::config::GraphEnrichLimits;
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::CcResult;

use crate::engine::SearchEngine;
use crate::enrich::{graph_enrich_scoped, GraphEnrichment};

/// Connectivity is a structural prior, not a second source of query relevance.
/// Rank-derived lexical/grep support bounds it; exact identities retain full support.
fn direct_support(lexical: f64, grep: f64, exact: bool) -> f64 {
    if exact {
        return 1.0;
    }
    let finite = |s: f64| {
        if s.is_finite() {
            s.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    finite(lexical).max(finite(grep))
}
fn hit_support(hit: &SearchHit) -> f64 {
    direct_support(
        hit.lexical_score,
        hit.grep_score,
        hit.reasons.iter().any(|s| s == "exact-target"),
    )
}
fn compare_graph_seeds(a: &SearchHit, b: &SearchHit) -> std::cmp::Ordering {
    let exact = |h: &SearchHit| h.reasons.iter().any(|s| s == "exact-target");
    exact(b)
        .cmp(&exact(a))
        .then_with(|| hit_support(b).total_cmp(&hit_support(a)))
        .then_with(|| b.fused_score.total_cmp(&a.fused_score))
        .then_with(|| a.chunk_id.cmp(&b.chunk_id))
}

impl SearchEngine {
    /// Search with graph-aware reranking, for context assembly.
    ///
    /// Runs the core search over a `rerank_window`-sized candidate list,
    /// computes connectivity for at most `limits.max_resolve` directly-supported
    /// seeds, then gates its contribution by reciprocal lexical/grep rank and
    /// `ranking.graph_rerank_weight`, before the single, final sort
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
        self.search_graph_window(request, limits, token_budget, true)
    }

    /// The context selector consumes the existing bounded rerank window,
    /// without changing lane budgets or recomputing relevance scores.
    pub fn search_context_candidates(
        &self,
        request: &SearchRequest,
        limits: &GraphEnrichLimits,
        token_budget: u32,
    ) -> CcResult<Arc<(Vec<SearchHit>, GraphEnrichment)>> {
        self.search_graph_window(request, limits, token_budget, false)
    }

    fn search_graph_window(
        &self,
        request: &SearchRequest,
        limits: &GraphEnrichLimits,
        token_budget: u32,
        top_k_only: bool,
    ) -> CcResult<Arc<(Vec<SearchHit>, GraphEnrichment)>> {
        let request = self.controlled_request(request)?;
        let request = &request;
        let qhash = self.graph_query_hash(request, limits, token_budget);
        let (generation, (result, cacheable)) = self.with_stable_generation(|generation| {
            let cache_key = (generation.graph_key(qhash), top_k_only);
            if let Ok(mut cache) = self.graph_result_cache.lock() {
                if let Some(cached) = cache.get(&cache_key).filter(|_| request.semantic.is_none()) {
                    self.graph_cache_hits.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        query = %request.query,
                        "graph search cache hit (index_epoch={}, evidence_epoch={}, hash={})",
                        generation.index_epoch,
                        generation.evidence_epoch,
                        qhash,
                    );
                    return Ok((Arc::clone(cached), false));
                }
            }
            self.graph_cache_misses.fetch_add(1, Ordering::Relaxed);

            let plan = crate::plan::SearchPlan::build_configured(
                &self.db,
                &self.config,
                &self.ranking,
                &self.query_config,
                request,
                self.repo_tier,
            )?;
            let retrieval = self.search_planned_detailed(&plan, false, generation.index_epoch)?;
            let mut hits = retrieval.hits;
            // Choose graph seeds before applying structural/file priors again. The
            // previous rerank-prefix selection could reinforce incidental hub matches.
            hits.sort_by(compare_graph_seeds);
            let supported = hits.iter().take_while(|h| hit_support(h) > 0.0).count();
            let (scores, mut enrichment) = graph_enrich_scoped(
                &self.db,
                &hits[..supported],
                limits,
                token_budget,
                plan.hard_scope(),
            );
            enrichment.lane_outcomes = retrieval.lanes;
            enrichment.grep_diagnostics = retrieval.grep;
            enrichment.scope_explain = retrieval.scope;
            enrichment.retrieval_cost = retrieval.cost;
            let weight = self.ranking.graph_rerank_weight;
            for hit in &mut hits {
                if let Some(&graph_score) = scores.get(&hit.chunk_id) {
                    hit.graph_score = graph_score;
                    // Atomically updates `rerank_score` and bills the matching
                    // trace component, keeping `sum(score_trace) == rerank_score`.
                    crate::score_trace::apply_traced_boost(
                        hit,
                        "boost:graph-rerank",
                        graph_score * weight * hit_support(hit),
                    );
                }
            }

            // Single final sort + truncation: rerank_score is immutable after this.
            hits.sort_by(crate::plan::compare_hits);
            hits.truncate(if top_k_only {
                plan.limits().top_k
            } else {
                plan.limits().rerank_window
            });
            crate::score_trace::validate_trace_consistency(&hits)?;
            crate::score_trace::debug_assert_trace_consistency(&hits);

            plan.control().check()?;
            let result = Arc::new((hits, enrichment));
            // Degraded-not-cached: a transient DB read failure (recorded in
            // graph_explain.read_errors) produced partial graph context — keep
            // serving it for THIS call, but never from cache, so the next call
            // retries the reads instead of pinning the degradation to the epoch.
            let cacheable = result.1.graph_explain.read_errors.is_empty()
                && result
                    .1
                    .lane_outcomes
                    .iter()
                    .all(cc_model::retrieval::LaneOutcome::is_cacheable)
                && result
                    .1
                    .grep_diagnostics
                    .as_ref()
                    .is_none_or(|g| !g.prefilter_failed);
            Ok((result, cacheable))
        })?;
        self.publish_query(request, || {
            if cacheable && request.semantic.is_none() {
                if let Ok(mut cache) = self.graph_result_cache.lock() {
                    cache.put(
                        (generation.graph_key(qhash), top_k_only),
                        Arc::clone(&result),
                    );
                }
            }
        })?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_graph_seeds, direct_support, hit_support};
    use std::sync::Arc;

    use cc_model::config::RepoSizeTier;
    use cc_model::search::SearchRequest;

    use crate::engine_test_support::{chunk_write_unit, insert_graph_file, scoped_test_engine};

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
    fn graph_prior_requires_direct_support_and_keeps_exact_identity() {
        assert_eq!(direct_support(0.0, 0.0, false), 0.0);
        assert_eq!(direct_support(0.1, 0.0, false), 0.1);
        assert_eq!(direct_support(0.0, 1.0, false), 1.0);
        assert_eq!(direct_support(f64::NAN, f64::INFINITY, false), 0.0);
        assert_eq!(direct_support(-1.0, 2.0, false), 1.0);
        assert_eq!(direct_support(0.0, 0.0, true), 1.0);
    }
    #[test]
    fn graph_seed_selection_cannot_be_bought_by_an_unrelated_file_prior() {
        let (engine, _tmp) = scoped_test_engine();
        engine
            .db
            .writes()
            .replace_files_batch(&[chunk_write_unit(
                "src/first.rs",
                "fn cached_marker() { alpha_one() }",
            )])
            .unwrap();
        let hits = engine.search(&graph_cache_request()).unwrap();
        let mut direct = hits[0].clone();
        direct.lexical_score = 1.0;
        direct.grep_score = 0.0;
        direct.reasons.clear();
        let mut hub = direct.clone();
        hub.chunk_id = "hub".into();
        hub.lexical_score = 0.1;
        hub.rerank_score = 1_000_000.0;
        hub.graph_score = 0.4;
        assert!(compare_graph_seeds(&direct, &hub).is_lt());
        hub.lexical_score = 0.0;
        assert_eq!(hit_support(&hub), 0.0);
        hub.reasons.push("exact-target".into());
        assert!(compare_graph_seeds(&hub, &direct).is_lt());
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
}
