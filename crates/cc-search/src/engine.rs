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

        let results: Arc<[SearchHit]> = self.search_internal(request, true)?.into();

        if let Ok(mut cache) = self.result_cache.lock() {
            cache.put(cache_key, Arc::clone(&results));
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
                // Deterministic tie-break on chunk id so equal fused scores (or
                // NaN, which compares Equal) produce a stable order across runs
                // and platforms instead of depending on HashMap iteration order.
                .then_with(|| a.0.cmp(&b.0))
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

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use cc_db::index_db::{FileWriteUnit, IndexDb};
    use cc_model::config::{ProjectConfig, SearchConfig};
    use cc_model::{CallEdgeRecord, ChunkRecord, Language, ParseOutcome, ParserTier};
    use std::sync::Arc;

    use crate::engine_test_support::{insert_chunk_file, insert_graph_file, scoped_test_engine};

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
