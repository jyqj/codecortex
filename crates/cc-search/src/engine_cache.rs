//! [`SearchEngine`] cache layer — the three LRUs (result, graph-aware
//! result, chunk text), their epoch-keyed invalidation, the query/config
//! cache keys, and hit/miss statistics ([`CacheStats`]).
//!
//! `impl SearchEngine` continuation of [`crate::engine`] (cc-db
//! `index_db_*.rs` style); method bodies are unchanged. [`CacheStats`] is
//! re-exported from [`crate::engine`], so the external path
//! (`cc_search::engine::CacheStats`) is unchanged.

use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use lru::LruCache;

use cc_db::index_db::IndexGeneration;
use cc_model::config::{GraphEnrichLimits, RankingConfig, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::CcResult;

use crate::engine::SearchEngine;
use crate::enrich::GraphEnrichment;

/// Default LRU capacity for search results.
/// Override with `CODECORTEX_SEARCH_RESULT_CACHE_SIZE`.
///
/// Values are `Arc<[SearchHit]>`: a cache hit hands out a pointer clone
/// instead of deep-copying `top_k` hits, each of which carries full chunk
/// text in `SearchHit::text`.
pub(crate) const RESULT_CACHE_CAPACITY: usize = 32;

/// Default LRU capacity for decompressed chunk text.
/// Override with `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`.
///
/// Eliminates double-decompression on the retrieval (cache-miss) path:
/// `grep_search` scans all in-scope chunks (decompressing each), and
/// matching chunks are fetched again in the batch-fetch step.  Caching the
/// text avoids the second `zstd::decode_all`.  The cache-HIT path never
/// decompresses at all — it returns the `Arc`'d result list above.
pub(crate) const CHUNK_TEXT_CACHE_CAPACITY: usize = 512;

/// Default LRU capacity for graph-aware (post-enrichment) search results.
/// Override with `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`.
///
/// Separate slot count from `RESULT_CACHE_CAPACITY`: entries are heavier
/// (final hits plus enrichment context nodes) and this path is the default
/// agent entry point (`search_in_context_with`), so it gets its own knob.
pub(crate) const GRAPH_RESULT_CACHE_CAPACITY: usize = 32;

/// Read an LRU capacity from `var`, falling back to `default` when unset,
/// unparseable, or zero.
pub(crate) fn cache_capacity_from_env(var: &str, default: usize) -> NonZeroUsize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or_else(|| NonZeroUsize::new(default).unwrap())
}

/// Result cache map: `(index_epoch, query_hash)` → shared, immutable hits.
pub(crate) type ResultCache = LruCache<(u64, u64), Arc<[SearchHit]>>;

/// Graph-aware result cache map: `(index_epoch, evidence_epoch,
/// graph_query_hash)` → shared, immutable final `(hits, enrichment)` pair
/// from [`SearchEngine::search_with_graph_context`].
pub(crate) type GraphResultCache =
    LruCache<(u64, u64, u64), Arc<(Vec<SearchHit>, GraphEnrichment)>>;

/// Snapshot of search result-cache hit/miss counters.
///
/// `result_*` covers [`SearchEngine::search`]; `graph_*` covers
/// [`SearchEngine::search_with_graph_context`]. Both LRUs are keyed by the
/// persisted epoch(s), so a hit means an identical query was served without
/// recomputation. The hit rate quantifies how often the warm cache path is
/// taken vs a cold recompute — the gap `bench` reports as warm/cold latency.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub result_hits: u64,
    pub result_misses: u64,
    pub graph_hits: u64,
    pub graph_misses: u64,
}

impl CacheStats {
    /// `result_hits / (result_hits + result_misses)`, or `0.0` when empty.
    pub fn result_hit_rate(&self) -> f64 {
        let total = self.result_hits + self.result_misses;
        if total == 0 {
            0.0
        } else {
            self.result_hits as f64 / total as f64
        }
    }
    /// `graph_hits / (graph_hits + graph_misses)`, or `0.0` when empty.
    pub fn graph_hit_rate(&self) -> f64 {
        let total = self.graph_hits + self.graph_misses;
        if total == 0 {
            0.0
        } else {
            self.graph_hits as f64 / total as f64
        }
    }
}

impl SearchEngine {
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

    /// Snapshot of result-cache hit/miss counters (Relaxed atomic loads).
    ///
    /// Counters are monotonic over the engine's lifetime — they accumulate
    /// cache hits/misses and are NOT reset by [`Self::invalidate_cache`]
    /// (only a fresh [`SearchEngine`] zeroes them). Use to quantify the
    /// warm/cold split behind per-tool warm vs cold latency.
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            result_hits: self.result_cache_hits.load(Ordering::Relaxed),
            result_misses: self.result_cache_misses.load(Ordering::Relaxed),
            graph_hits: self.graph_cache_hits.load(Ordering::Relaxed),
            graph_misses: self.graph_cache_misses.load(Ordering::Relaxed),
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
    pub(crate) fn observe_epochs(&self) -> CcResult<IndexGeneration> {
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

    /// Compute a deterministic hash of `SearchRequest` key fields.
    pub(crate) fn query_hash(request: &SearchRequest) -> u64 {
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
    pub(crate) fn ranking_fingerprint(config: &SearchConfig, ranking: &RankingConfig) -> u64 {
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
    pub(crate) fn graph_query_hash(
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cc_model::config::ProjectConfig;
    use cc_model::search::SearchRequest;
    use cc_model::Language;

    use crate::engine::SearchEngine;
    use crate::engine_test_support::{chunk_write_unit, insert_chunk_file, scoped_test_engine};

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
    fn cache_stats_counts_hits_and_misses() {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/alpha.rs",
            Language::Rust,
            "fn alpha_handler() { process() }",
        );

        let request = SearchRequest {
            query: "alpha".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        };

        // Fresh engine: counters start at zero.
        let before = engine.cache_stats();
        assert_eq!(before.result_hits, 0);
        assert_eq!(before.result_misses, 0);

        // First search computes and stores — a miss.
        let _ = engine.search(&request).unwrap();
        let after_first = engine.cache_stats();
        assert_eq!(after_first.result_hits, 0, "first search must miss");
        assert_eq!(after_first.result_misses, 1);

        // Second identical search serves from cache — a hit.
        let _ = engine.search(&request).unwrap();
        let after_second = engine.cache_stats();
        assert_eq!(
            after_second.result_hits, 1,
            "second identical search must hit"
        );
        assert_eq!(after_second.result_misses, 1);
        assert!(
            (after_second.result_hit_rate() - 0.5).abs() < f64::EPSILON,
            "hit rate must be 0.5 after one hit and one miss"
        );
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
}
