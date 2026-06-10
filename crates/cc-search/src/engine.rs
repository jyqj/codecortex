//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::index_db::IndexDb;
use cc_model::config::{ProjectStats, RepoSizeTier, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::CcResult;

use crate::lanes::{
    fuse_outcomes, run_lanes, GraphLane, GrepLane, LaneContext, LexicalLane, RetrievalLane,
};
pub use crate::plan::is_project_doc;
use crate::plan::{CandidateChunk, SearchPlan};

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

        // No pooled read connection is held here: plan build (preselect),
        // each lane, and the batch fetch below all check out and release
        // their own, so a 1-connection read pool never sees nested checkouts.
        let plan = SearchPlan::build(&self.db, &self.config, request, self.repo_tier)?;
        let limits = plan.limits();

        // Retrieval lanes, executed in deterministic fusion order
        // (lexical, grep, graph) so RRF tie-breaking stays stable.
        let lanes: [&dyn RetrievalLane; 3] = [&LexicalLane, &GrepLane, &GraphLane];
        let lane_context = LaneContext {
            plan: &plan,
            db: &self.db,
            config: &self.config,
            chunk_text_cache: &self.chunk_text_cache,
        };
        let lane_outcomes = run_lanes(&lanes, &lane_context)?;

        // RRF fusion across all lane outcomes.
        let fused = fuse_outcomes(&lane_outcomes, self.config.rrf_k);

        let mut candidates: Vec<(String, f64)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
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
            let rows = self.db.chunk_rows_by_ids(&chunk_ids_refs, &cached_texts)?;
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
        fuse_outcomes, run_lanes, GraphLane, GrepLane, LaneContext, LaneOutcome, LexicalLane,
        RetrievalLane, LANE_GRAPH, LANE_GREP, LANE_LEXICAL,
    };
    use cc_model::{CallEdgeRecord, SymbolRecord};

    fn build_plan(engine: &SearchEngine, request: &SearchRequest) -> SearchPlan {
        SearchPlan::build(&engine.db, &engine.config, request, None).unwrap()
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

        let conn = engine.db.read_conn().unwrap();
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
    fn grep_lane_adapter_ranks_matches_and_populates_text_cache() {
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

        // Side effect: every scanned chunk's decompressed text lands in the
        // engine's chunk text cache (matching and non-matching alike).
        let mut cache = engine.chunk_text_cache.lock().unwrap();
        assert!(cache.get("chunk:src/g.rs").is_some());
        assert!(cache.get("chunk:src/other.rs").is_some());
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
            .hit_from_chunk(fake_candidate_chunk(), 0.5, &lane_ranks)
            .unwrap();

        assert_eq!(
            hit.reasons,
            vec!["fourth@1".to_string(), "fifth@2".to_string()],
            "annotating lanes must contribute {{lane_id}}@{{rank}} reasons in lane order"
        );
        // Unknown lane ids have no dedicated score field in SearchHit
        // (cc-model fields are fixed); the built-in fields stay 0.0.
        assert_eq!(hit.lexical_score, 0.0);
        assert_eq!(hit.grep_score, 0.0);
        assert_eq!(hit.graph_score, 0.0);
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
            .hit_from_chunk(fake_candidate_chunk(), fused["chunk:src/x.rs"], &lane_ranks)
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
                hits: vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
            },
            LaneOutcome {
                lane_id: "fake-b",
                weight: 0.5,
                annotates_hits: false,
                hits: vec![("y".to_string(), 1.0)],
            },
        ];
        let fused = fuse_outcomes(&outcomes, 50);

        // score(d) = sum over lanes of weight / (k + rank)
        assert!((fused["x"] - 1.0 / 51.0).abs() < 1e-12);
        assert!((fused["y"] - (1.0 / 52.0 + 0.5 / 51.0)).abs() < 1e-12);
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
