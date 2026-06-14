//! Retrieval lanes — the seam between `SearchEngine::search_internal()` and
//! the individual retrieval strategies (lexical FTS5, grep, call-graph
//! expansion).
//!
//! Each lane is one ranked candidate source feeding RRF fusion.  Adding a
//! lane means implementing [`RetrievalLane`] and registering it in
//! [`default_lanes`] — no `plan.rs` or `engine.rs` edits.
//!
//! What is generic (no `plan.rs` edits needed for a new lane):
//! - execution and rank-map plumbing ([`run_lanes`]);
//! - RRF fusion ([`fuse_outcomes`]);
//! - per-hit annotation: lanes that return `true` from
//!   [`RetrievalLane::annotates_hits`] get a `{lane_id}@{rank}` reason on
//!   every hit they ranked, driven by the lane collection order;
//! - per-lane score projection: a lane that wants a dedicated `SearchHit`
//!   score field declares it via [`RetrievalLane::score_slot`]; the slot →
//!   field projection lives in one place in `plan.rs::hit_from_chunk` and
//!   never needs a new arm (the slot set mirrors the fixed cc-model schema).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::fts::sanitize_fts_query;
use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
use cc_model::config::SearchConfig;
use cc_model::{CcError, CcResult};

use crate::plan::{language_from_path, parse_language_name, SearchPlan};

/// Lane id for the FTS5 lexical lane.
pub(crate) const LANE_LEXICAL: &str = "lexical";
/// Lane id for the substring/grep lane.
pub(crate) const LANE_GREP: &str = "grep";
/// Lane id for the call-graph expansion lane.
pub(crate) const LANE_GRAPH: &str = "graph";

/// Dedicated per-lane score field of `SearchHit` a lane projects its
/// rank-derived score into.
///
/// This is a *closed* set mirroring the fixed cc-model output schema
/// (`lexical_score` / `grep_score` / `graph_score`) — it grows only when
/// cc-model grows a new field, never when a lane is added.  New lanes
/// either reuse a slot or return `None` from
/// [`RetrievalLane::score_slot`] and surface via reason strings only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScoreSlot {
    /// Projects into `SearchHit.lexical_score`.
    Lexical,
    /// Projects into `SearchHit.grep_score`.
    Grep,
    /// Projects into `SearchHit.graph_score`.
    Graph,
}

/// The engine's lane registry — the single place to register a new lane.
///
/// Order is the deterministic fusion order (lexical, grep, graph): RRF
/// accumulation and tie-breaking stay stable because every search runs
/// lanes in this exact sequence.
pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    vec![&LexicalLane, &GrepLane, &GraphLane]
}

/// Per-search context handed to every lane.
///
/// Deliberately narrow: lanes see the immutable [`SearchPlan`], the index
/// database, the search config, and the engine's decompressed chunk-text
/// cache — nothing else of the engine (in particular, not the result cache).
///
/// Lanes that need a SQLite connection check one out of the read pool
/// inside `run()` and release it on return.  The context deliberately does
/// NOT hold a pooled connection: holding one across `db.*` calls (which
/// each check out their own) deadlocks a 1-connection read pool.
pub(crate) struct LaneContext<'a> {
    pub(crate) plan: &'a SearchPlan,
    pub(crate) db: &'a IndexDb,
    pub(crate) config: &'a SearchConfig,
    /// Decompressed chunk-text cache owned by the engine.  Lanes that
    /// already decompress chunk text (grep) populate it for their *matched*
    /// chunks so the later batch-fetch step can skip a second zstd decode;
    /// scan-only rows must stay out so a cold scan can't flush the LRU.
    pub(crate) chunk_text_cache: &'a Mutex<LruCache<String, Arc<str>>>,
}

/// A retrieval lane: one ranked candidate source feeding RRF fusion.
///
/// Contract:
/// - `run` returns `(chunk_id, score)` pairs ranked **best-first**; only the
///   rank position feeds RRF — the score is lane-local and purely
///   diagnostic.
/// - `is_enabled` must be cheap; when it returns `false` the lane is skipped
///   before any work (no DB access, no side effects).
/// - Returning `Err` from `run` aborts the whole search.  Lanes that should
///   degrade gracefully (graph) must swallow their own recoverable failures
///   and return an empty list instead.
/// - Side effects are limited to the engine caches exposed through
///   [`LaneContext`] (currently `chunk_text_cache`).
pub(crate) trait RetrievalLane {
    /// Stable lane identifier used to key rank maps, reasons, and stats.
    fn lane_id(&self) -> &'static str;

    /// RRF weight for this lane, read from the search config.
    fn weight(&self, config: &SearchConfig) -> f64;

    /// Whether the lane should execute for this search.
    fn is_enabled(&self, context: &LaneContext<'_>) -> bool;

    /// Whether this lane contributes per-hit annotations.
    ///
    /// Opting in (`true`) makes every hit the lane ranked carry a
    /// `{lane_id}@{rank}` reason string and surfaces the lane's rank-derived
    /// score (`1/rank`) — for lanes with a dedicated `SearchHit` score field
    /// (see the lane-id → score-field mapping in `plan.rs::hit_from_chunk`)
    /// that field is populated; other lanes still get the reason string.
    ///
    /// Opting out (`false`) makes the lane fusion-only: its ranks feed RRF
    /// but hits show no reason and no per-lane score for it.
    ///
    /// Deliberately has no default impl: a new lane must make this choice
    /// explicitly rather than silently producing no diagnostics.
    fn annotates_hits(&self) -> bool;

    /// Dedicated `SearchHit` score field this lane's rank-derived score
    /// projects into, when the lane annotates hits.
    ///
    /// Defaults to `None`: the lane has no dedicated field and surfaces
    /// only through its `{lane_id}@{rank}` reason string.  Built-in lanes
    /// override this to claim their schema field; `SearchHit`'s per-lane
    /// fields are fixed in cc-model, so the available slots are the closed
    /// [`ScoreSlot`] set.
    fn score_slot(&self) -> Option<ScoreSlot> {
        None
    }

    /// Execute retrieval and return the lane's ranked hits.
    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>>;
}

/// Result of executing one lane: its id, RRF weight, ranked hits, and
/// whether the lane opted into per-hit annotation (see
/// [`RetrievalLane::annotates_hits`]).
pub(crate) struct LaneOutcome {
    pub(crate) lane_id: &'static str,
    pub(crate) weight: f64,
    pub(crate) annotates_hits: bool,
    pub(crate) score_slot: Option<ScoreSlot>,
    pub(crate) hits: Vec<(String, f64)>,
}

/// Execute lanes in the given (deterministic) order.
///
/// Disabled lanes are skipped before any work but still yield an empty
/// outcome, so downstream rank maps stay uniformly keyed by lane id.
pub(crate) fn run_lanes(
    lanes: &[&dyn RetrievalLane],
    context: &LaneContext<'_>,
) -> CcResult<Vec<LaneOutcome>> {
    let mut outcomes = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let hits = if lane.is_enabled(context) {
            lane.run(context)?
        } else {
            Vec::new()
        };
        outcomes.push(LaneOutcome {
            lane_id: lane.lane_id(),
            weight: lane.weight(context.config),
            annotates_hits: lane.annotates_hits(),
            score_slot: lane.score_slot(),
            hits,
        });
    }
    Ok(outcomes)
}

/// Attach the shared rank-position score (`1/(i+1)`) to an ordered list of
/// chunk ids. The score is lane-local and purely diagnostic — only the rank
/// position feeds RRF.
fn rank_scored(ids: Vec<String>) -> Vec<(String, f64)> {
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id, 1.0 / (i + 1) as f64))
        .collect()
}

/// One candidate's RRF fusion result: the fused total plus the per-lane
/// contributions that produced it.
///
/// `by_lane` is recorded in lane-accumulation order, so summing it
/// left-to-right reproduces `total` bit-for-bit — this is what lets a
/// hit's `score_trace` replay the fused part of `rerank_score` exactly.
#[derive(Debug, Clone, Default)]
pub(crate) struct FusedScore {
    pub(crate) total: f64,
    pub(crate) by_lane: Vec<(&'static str, f64)>,
}

/// RRF-fuse lane outcomes, accumulating in lane order.
///
/// Same accumulation as [`crate::rrf::rrf_accumulate`] (`weight / (k + rank)`
/// summed in lane order), but each candidate additionally keeps its per-lane
/// contribution breakdown for score tracing.
pub(crate) fn fuse_outcomes(outcomes: &[LaneOutcome], rrf_k: usize) -> HashMap<String, FusedScore> {
    let mut fused: HashMap<String, FusedScore> = HashMap::new();
    for outcome in outcomes {
        for (rank, (id, _)) in outcome.hits.iter().enumerate() {
            let score = outcome.weight / (rrf_k + rank + 1) as f64;
            let entry = fused.entry(id.clone()).or_default();
            entry.total += score;
            entry.by_lane.push((outcome.lane_id, score));
        }
    }
    fused
}

/// Lexical search via FTS5 (`chunks_fts` MATCH, bm25-ordered).
pub(crate) struct LexicalLane;

impl RetrievalLane for LexicalLane {
    fn lane_id(&self) -> &'static str {
        LANE_LEXICAL
    }

    fn weight(&self, config: &SearchConfig) -> f64 {
        config.lexical_weight
    }

    fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
        true
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn score_slot(&self) -> Option<ScoreSlot> {
        Some(ScoreSlot::Lexical)
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        let plan = context.plan;
        let limit = plan.limits().lexical;
        let fts_q = sanitize_fts_query(plan.lexical_query());
        if fts_q == r#""""# {
            return Ok(Vec::new());
        }
        let (sql, mut params) = plan.lexical_scope_sql(limit);
        params.insert(0, fts_q);
        let conn = context.db.reads().read_conn()?;
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
        Ok(rank_scored(results))
    }
}

/// Grep search — regex match on chunk text with file-level filtering.
///
/// Side effect: caches the decompressed text of every *matched* chunk in
/// `chunk_text_cache` so the batch-fetch step can reuse it.  Non-matching
/// scanned chunks deliberately stay out of the cache: a cold scan would
/// otherwise rotate the whole LRU and evict hot entries.
///
/// Scan work is bounded by `search.grep_scan_cap` decompressed rows; an
/// exhausted budget is reported via a `tracing::debug!` line (the lane has
/// no structured truncation outlet) and the lane returns whatever matched
/// within budget, deterministically (the unscoped scan is recency-ordered
/// by `grep_chunk_scope_sql`).
pub(crate) struct GrepLane;

impl RetrievalLane for GrepLane {
    fn lane_id(&self) -> &'static str {
        LANE_GREP
    }

    fn weight(&self, config: &SearchConfig) -> f64 {
        config.grep_weight
    }

    fn is_enabled(&self, context: &LaneContext<'_>) -> bool {
        context.plan.request().include_grep
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn score_slot(&self) -> Option<ScoreSlot> {
        Some(ScoreSlot::Grep)
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        let plan = context.plan;
        let limit = plan.limits().grep;
        // Build a simple case-insensitive regex from the query
        let escaped = regex::escape(plan.grep_query());
        let re = match regex::RegexBuilder::new(&escaped)
            .case_insensitive(true)
            .build()
        {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let (sql, params) = plan.grep_scope_sql();
        let conn = context.db.reads().read_conn()?;
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

        // Scan budget: every fetched row costs a zstd decode, so cap the
        // number of rows pulled instead of decompressing the whole scope
        // when matches are rare or absent.
        let scan_cap = context.config.grep_scan_cap;
        let mut scanned = 0usize;
        let mut truncated = false;
        let mut matches = Vec::new();
        for row in rows {
            if scanned >= scan_cap {
                truncated = true;
                break;
            }
            scanned += 1;
            let (cid, file_path, language_name, text) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            // File-level filtering: path_prefix, languages, file_paths
            if !plan.passes_filters(&file_path, language) {
                continue;
            }

            if re.is_match(&text) {
                // Only matches enter the chunk text cache — they are exactly
                // the rows the batch-fetch step re-reads.  Caching every
                // scanned chunk would let one cold scan rotate the whole LRU
                // and evict hot entries.
                if let Ok(mut cache) = context.chunk_text_cache.lock() {
                    cache.put(cid.clone(), Arc::from(text.as_str()));
                }
                matches.push(cid);
                if matches.len() >= limit {
                    break;
                }
            }
        }
        if truncated {
            tracing::info!(
                query = %plan.grep_query(),
                scanned,
                scan_cap,
                matches = matches.len(),
                "grep lane scan budget exhausted; remaining chunks not scanned \
                 (raise search.grep_scan_cap to widen)"
            );
        }
        Ok(rank_scored(matches))
    }
}

/// Graph retrieval lane: find chunks connected to query-matching symbols
/// via the call graph (1-hop callers + callees).
pub(crate) struct GraphLane;

impl RetrievalLane for GraphLane {
    fn lane_id(&self) -> &'static str {
        LANE_GRAPH
    }

    fn weight(&self, config: &SearchConfig) -> f64 {
        config.graph_weight
    }

    fn is_enabled(&self, context: &LaneContext<'_>) -> bool {
        context.config.graph_weight > 0.0
    }

    /// Fusion-only by design: the graph lane influences ranking through RRF
    /// but deliberately produces no `graph@rank` reason and leaves
    /// `SearchHit.graph_score` at 0.0, preserving pre-seam output exactly.
    fn annotates_hits(&self) -> bool {
        false
    }

    /// Dormant today (the lane opts out of annotation) but keeps
    /// `SearchHit.graph_score` wired if the graph lane ever opts in.
    fn score_slot(&self) -> Option<ScoreSlot> {
        Some(ScoreSlot::Graph)
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        // Graph failures degrade to an empty contribution instead of
        // aborting the whole search.
        Ok(Self::search(
            context.db,
            context.plan,
            context.plan.query_tokens(),
            context.config.graph_top_k,
        )
        .unwrap_or_default())
    }
}

impl GraphLane {
    /// Core graph retrieval: seed symbols + 1-hop call-edge expansion,
    /// mapped back to the smallest containing chunks.
    pub(crate) fn search(
        db: &IndexDb,
        plan: &SearchPlan,
        query_tokens: &[String],
        limit: usize,
    ) -> CcResult<Vec<(String, f64)>> {
        let ranking = plan.ranking();
        // Step 1: Find seed symbols matching query tokens via symbols_fts (trigram LIKE)
        let seed_uids = find_seed_symbol_uids(db, query_tokens, ranking)?;
        if seed_uids.is_empty() {
            return Ok(Vec::new());
        }

        // Step 2: 1-hop expansion via call_edges (callers + callees),
        // fetched in two batched queries instead of two point queries per
        // seed. Failures degrade to no expansion, matching the old
        // per-seed `if let Ok(..)` swallowing.
        let seed_keys: Vec<&str> = seed_uids.iter().map(|(uid, _)| uid.as_str()).collect();
        let callees_by_seed = db
            .reads()
            .callee_rows_by_uids(&seed_keys, 10)
            .unwrap_or_default();
        let callers_by_seed = db
            .reads()
            .caller_rows_by_uids(&seed_keys, 10)
            .unwrap_or_default();

        let mut neighbor_uids: HashMap<String, f64> = HashMap::new();
        for (uid, seed_score) in &seed_uids {
            // Include the seed itself (distance 0)
            neighbor_uids
                .entry(uid.clone())
                .and_modify(|s| *s = s.max(*seed_score))
                .or_insert(*seed_score);

            // Callees of seed (distance 1)
            if let Some(callees) = callees_by_seed.get(uid.as_str()) {
                for edge in callees {
                    if let Some(ref callee_uid) = edge.callee_symbol_uid {
                        let score = seed_score * ranking.graph_neighbor_decay;
                        neighbor_uids
                            .entry(callee_uid.clone())
                            .and_modify(|s| *s = s.max(score))
                            .or_insert(score);
                    }
                }
            }

            // Callers of seed (distance 1)
            if let Some(callers) = callers_by_seed.get(uid.as_str()) {
                for edge in callers {
                    if let Some(ref caller_uid) = edge.caller_symbol_uid {
                        let score = seed_score * ranking.graph_neighbor_decay;
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
        let sym_rows = db.reads().symbol_rows_by_uids(&uid_list)?;

        // Apply file filters first, then batch-load chunk spans for the
        // surviving files in one query instead of one point query per neighbor.
        let mut candidates: Vec<(&str, u32, u32, f64)> = Vec::new(); // (file, start, end, score)
        for (uid, score) in &neighbor_uids {
            if let Some(sym) = sym_rows.get(uid) {
                // SymbolRow carries no language column — infer it from the
                // file extension, matching the indexer's assignment.
                if !plan.passes_filters(&sym.file_path, language_from_path(&sym.file_path)) {
                    continue;
                }
                candidates.push((sym.file_path.as_str(), sym.start_line, sym.end_line, *score));
            }
        }
        let candidate_files: Vec<&str> = candidates
            .iter()
            .map(|(f, ..)| *f)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let chunks_by_file = db.retrieval().chunk_spans_for_files(&candidate_files)?;

        let mut best_per_chunk: HashMap<String, f64> = HashMap::new();
        for (file, start, end, score) in candidates {
            // Smallest containing chunk, matching the old per-symbol query.
            let cid = chunks_by_file.get(file).and_then(|spans| {
                spans
                    .iter()
                    .filter(|(_, cs, ce)| *cs <= start && *ce >= end)
                    .min_by_key(|(_, cs, ce)| ce - cs)
                    .map(|(cid, _, _)| cid.clone())
            });
            if let Some(cid) = cid {
                best_per_chunk
                    .entry(cid)
                    .and_modify(|s| *s = s.max(score))
                    .or_insert(score);
            }
        }
        let mut chunk_scores: Vec<(String, f64)> = best_per_chunk.into_iter().collect();

        // Sort by score descending and limit
        chunk_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        chunk_scores.truncate(limit);
        Ok(chunk_scores)
    }
}

/// Find seed symbols matching query tokens via the symbols_fts trigram table.
///
/// Uses LIKE substring matching (symbols_fts is an FTS5 trigram table, not
/// a standard BM25 table).
fn find_seed_symbol_uids(
    db: &IndexDb,
    query_tokens: &[String],
    ranking: &cc_model::config::RankingConfig,
) -> CcResult<Vec<(String, f64)>> {
    let mut results: HashMap<String, f64> = HashMap::new();

    for token in query_tokens.iter().take(5) {
        if token.len() < 3 {
            // The trigram table cannot accelerate sub-3-char LIKE, and a
            // substring match on 2 chars would be pure noise — but exact
            // short names (Go's `do`, Rust's `ok`) are valid seeds.
            // Equality on idx_symbols_name (BINARY) via the two common
            // casings keeps this an index lookup.
            let capitalized = {
                let mut chars = token.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => continue,
                }
            };
            let uids = db
                .retrieval()
                .symbol_uids_by_exact_names(&[token.as_str(), capitalized.as_str()], 10)?;
            for uid in uids {
                results
                    .entry(uid)
                    .and_modify(|s| *s = s.max(ranking.graph_seed_exact_score))
                    .or_insert(ranking.graph_seed_exact_score);
            }
            continue;
        }
        // Use trigram-accelerated LIKE via symbols_fts; surface exact name
        // matches first so the 10-row cap doesn't crowd them out with
        // arbitrary substring hits.
        for (uid, name) in db.retrieval().symbol_seed_hits(token, 10)? {
            // Score: exact match > contains
            let relevance = if name.to_lowercase() == *token {
                ranking.graph_seed_exact_score
            } else {
                ranking.graph_seed_fuzzy_score
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
