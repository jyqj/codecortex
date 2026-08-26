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
use crate::enrich::{graph_enrich, GraphEnrichment};

impl SearchEngine {
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
                self.graph_cache_hits.fetch_add(1, Ordering::Relaxed);
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
        self.graph_cache_misses.fetch_add(1, Ordering::Relaxed);

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
}

#[cfg(test)]
mod tests {
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
