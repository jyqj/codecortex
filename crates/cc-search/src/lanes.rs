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

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use lru::LruCache;

use cc_db::fts::sanitize_fts_query;
use cc_db::index_db::IndexDb;
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
/// - `Sync` because [`run_lanes`] executes independent lanes concurrently
///   (each checks its own pooled connection out inside `run`).
pub(crate) trait RetrievalLane: Sync {
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

/// Execute lanes concurrently, returning outcomes in the given
/// (deterministic) order.
///
/// Lanes are independent by contract (side effects limited to the engine
/// caches behind their own locks; each lane checks its own read-pool
/// connection out inside `run`), so enabled lanes run on scoped threads
/// while the first enabled lane runs on the calling thread. Outcomes are
/// collected in slice order, so RRF fusion and tie-breaking see exactly the
/// sequence the old sequential loop produced. Error semantics match too:
/// the first failing lane in slice order aborts the search (later lanes may
/// have run — they are side-effect-free beyond the engine caches).
///
/// Per-lane result slot: `None` for disabled lanes (they yield an empty
/// outcome below), `Some` for lanes that ran.
type LaneHitSlot = Option<CcResult<Vec<(String, f64)>>>;

/// Disabled lanes are skipped before any work but still yield an empty
/// outcome, so downstream rank maps stay uniformly keyed by lane id.
pub(crate) fn run_lanes(
    lanes: &[&dyn RetrievalLane],
    context: &LaneContext<'_>,
) -> CcResult<Vec<LaneOutcome>> {
    let enabled: Vec<bool> = lanes.iter().map(|lane| lane.is_enabled(context)).collect();
    let mut hit_results: Vec<LaneHitSlot> = (0..lanes.len()).map(|_| None).collect();

    if enabled.iter().filter(|e| **e).count() <= 1 {
        // Zero or one enabled lane: no concurrency to gain, skip the spawns.
        for (idx, lane) in lanes.iter().enumerate() {
            if enabled[idx] {
                hit_results[idx] = Some(lane.run(context));
            }
        }
    } else {
        std::thread::scope(|scope| {
            let mut first_enabled = None;
            let mut handles: Vec<(usize, std::thread::ScopedJoinHandle<'_, _>)> = Vec::new();
            for (idx, lane) in lanes.iter().enumerate() {
                if !enabled[idx] {
                    continue;
                }
                if first_enabled.is_none() {
                    first_enabled = Some(idx);
                    continue;
                }
                handles.push((idx, scope.spawn(move || lane.run(context))));
            }
            if let Some(idx) = first_enabled {
                hit_results[idx] = Some(lanes[idx].run(context));
            }
            for (idx, handle) in handles {
                hit_results[idx] = Some(match handle.join() {
                    Ok(result) => result,
                    Err(_) => Err(CcError::Search(format!(
                        "retrieval lane '{}' panicked",
                        lanes[idx].lane_id()
                    ))),
                });
            }
        });
    }

    let mut outcomes = Vec::with_capacity(lanes.len());
    for (idx, lane) in lanes.iter().enumerate() {
        let hits = match hit_results[idx].take() {
            Some(result) => result?,
            None => Vec::new(),
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
        let mut seen = HashSet::new();
        for (rank, (id, _)) in outcome.hits.iter().enumerate() {
            // Duplicate candidates consume their original rank but cannot vote
            // twice for the same document within one lane.
            if !seen.insert(id) {
                continue;
            }
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
        let candidates =
            context
                .db
                .retrieval()
                .fts_chunk_candidates(&fts_q, &plan.chunk_scope(), limit)?;

        let mut results = Vec::new();
        for (cid, file_path, language_name) in candidates {
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
///
/// Unscoped scans run in two stages to avoid decompressing the whole scope:
/// stage 1 pulls candidates from a `chunks_fts` MATCH prefilter derived from
/// the grep literal (see [`grep_prefilter_phrase`] — matches at token
/// boundaries are a superset of the FTS phrase hits), stage 2 falls back to
/// the full recency-ordered scan for matches the tokenizer cannot see
/// (mid-token substrings), skipping rows stage 1 already decompressed.
/// Matches from both stages merge in recency (rowid-descending) order, so
/// when the budget covers the scope the result equals the single-pass scan
/// exactly; under budget pressure the prefilter *finds more matches sooner*.
/// File-scoped scans (bounded cardinality) keep the single-pass behaviour.
pub(crate) struct GrepLane;

/// Build the `chunks_fts` MATCH phrase for the grep literal, or `None` when
/// the literal has no tokenizable content (all punctuation).
///
/// The literal's alphanumeric runs appear as adjacent tokens in any text
/// containing the literal at a token boundary (unicode61 separates on
/// non-alphanumerics and case-folds, matching the lane's case-insensitive
/// regex), so a quoted phrase of those runs — with a trailing `*` when the
/// literal ends mid-token — selects a candidate superset of all
/// token-boundary matches. Mid-token starts (e.g. querying `UserById`
/// against `getUserById`) are invisible to the tokenizer; the caller must
/// keep a full-scan stage for those.
pub(crate) fn grep_prefilter_phrase(query: &str) -> Option<String> {
    let tokens: Vec<&str> = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    // Single-character tokens alone are pure noise as a prefilter (nearly
    // every chunk matches) — require at least one token of length >= 2.
    if !tokens.iter().any(|t| t.chars().count() >= 2) {
        return None;
    }
    let prefix = query
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_alphanumeric());
    Some(format!(
        "\"{}\"{}",
        tokens.join(" "),
        if prefix { "*" } else { "" }
    ))
}

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

        // Scan budget: every visited row costs a zstd decode, so cap the
        // number of rows pulled instead of decompressing the whole scope
        // when matches are rare or absent. Shared across both stages.
        let scan_cap = context.config.grep_scan_cap;
        let mut scan = GrepScanState {
            scanned: 0,
            truncated: false,
            matches: Vec::new(),
        };
        let mut prefiltered: HashSet<String> = HashSet::new();

        // Stage 1 — FTS prefilter (unscoped scans only; file-scoped scans
        // are cardinality-bounded and keep the single-pass behaviour).
        let prefilter_phrase = if plan.has_file_scope() {
            None
        } else {
            grep_prefilter_phrase(plan.grep_query())
        };
        if let Some(phrase) = &prefilter_phrase {
            let result = context.db.retrieval().scan_chunks_for_grep_prefiltered(
                phrase,
                scan_cap,
                &plan.grep_scope(),
                |row| {
                    Self::process_row(
                        context,
                        plan,
                        &re,
                        limit,
                        scan_cap,
                        row,
                        Some(&mut prefiltered),
                        &mut scan,
                    )
                },
            );
            if let Err(e) = result {
                // A MATCH the tokenizer rejects must not fail the lane —
                // fall back to the plain full scan.
                tracing::debug!(
                    phrase = %phrase,
                    error = %e,
                    "grep prefilter query failed; falling back to full scan"
                );
                prefiltered.clear();
                scan = GrepScanState {
                    scanned: 0,
                    truncated: false,
                    matches: Vec::new(),
                };
            }
        }

        // Stage 2 — full scoped scan for matches the tokenizer cannot see
        // (mid-token substrings), skipping rows stage 1 already decompressed.
        if scan.matches.len() < limit && !scan.truncated {
            let skip = if prefiltered.is_empty() {
                None
            } else {
                Some(&prefiltered)
            };
            context
                .db
                .retrieval()
                .scan_chunks_for_grep(&plan.grep_scope(), skip, |row| {
                    Self::process_row(context, plan, &re, limit, scan_cap, row, None, &mut scan)
                })?;
        }

        if scan.truncated {
            tracing::info!(
                query = %plan.grep_query(),
                scanned = scan.scanned,
                scan_cap,
                matches = scan.matches.len(),
                "grep lane scan budget exhausted; remaining chunks not scanned \
                 (raise search.grep_scan_cap to widen)"
            );
        }
        // Merge the stages in recency order (matches carry their base-table
        // rowid). Unscoped single-stage results are already rowid-descending
        // so this is a no-op there; scoped scans never run stage 1 and keep
        // SQLite's natural probe order untouched.
        if prefilter_phrase.is_some() {
            scan.matches.sort_by(|a, b| b.0.cmp(&a.0));
            scan.matches.truncate(limit);
        }
        Ok(rank_scored(
            scan.matches.into_iter().map(|(_, cid)| cid).collect(),
        ))
    }
}

/// Mutable scan state shared by the grep lane's two stages: rows
/// decompressed so far (the budget), whether the budget ran out, and the
/// matches as `(chunks.rowid, chunk_id)` for the recency merge.
struct GrepScanState {
    scanned: usize,
    truncated: bool,
    matches: Vec<(i64, String)>,
}

impl GrepLane {
    /// Process one decoded row against the grep regex, under the shared
    /// scan budget. Returns `false` to stop the scan (budget exhausted or
    /// enough matches). `seen_out` records every row this stage decodes so
    /// the later stage can skip it before its zstd decode.
    #[allow(clippy::too_many_arguments)]
    fn process_row(
        context: &LaneContext<'_>,
        plan: &SearchPlan,
        re: &regex::Regex,
        limit: usize,
        scan_cap: usize,
        row: cc_db::GrepChunkRow,
        mut seen_out: Option<&mut HashSet<String>>,
        scan: &mut GrepScanState,
    ) -> bool {
        if scan.scanned >= scan_cap {
            scan.truncated = true;
            return false;
        }
        scan.scanned += 1;
        if let Some(seen) = seen_out.as_deref_mut() {
            seen.insert(row.chunk_id.clone());
        }
        let language = parse_language_name(&row.language_name);
        // File-level filtering: path_prefix, languages, file_paths
        if !plan.passes_filters(&row.file_path, language) {
            return true;
        }
        if re.is_match(&row.text) {
            // Only matches enter the chunk text cache — they are exactly
            // the rows the batch-fetch step re-reads. Caching every scanned
            // chunk would let one cold scan rotate the whole LRU and evict
            // hot entries.
            if let Ok(mut cache) = context.chunk_text_cache.lock() {
                cache.put(row.chunk_id.clone(), Arc::from(row.text.as_str()));
            }
            scan.matches.push((row.rowid, row.chunk_id));
            if scan.matches.len() >= limit {
                return false;
            }
        }
        true
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
    /// mapped to the smallest containing chunk, or the chunks of a split symbol.
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
            if let Some(spans) = chunks_by_file.get(file) {
                for cid in crate::symbol_chunks::project_symbol_chunks(spans, start, end) {
                    best_per_chunk
                        .entry(cid.to_string())
                        .and_modify(|s| *s = s.max(score))
                        .or_insert(score);
                }
            }
        }
        let mut chunk_scores: Vec<(String, f64)> = best_per_chunk.into_iter().collect();

        // Sort by score descending and limit
        chunk_scores.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
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
    sorted.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    sorted.truncate(20); // max 20 seed symbols
    Ok(sorted)
}

#[cfg(test)]
mod fusion_contract_tests {
    use super::*;
    #[test]
    fn duplicate_candidate_gets_one_vote_at_original_rank() {
        let lane = LaneOutcome {
            lane_id: "test",
            weight: 1.0,
            annotates_hits: false,
            score_slot: None,
            hits: vec![("a".into(), 1.0), ("a".into(), 1.0), ("b".into(), 1.0)],
        };
        let fused = fuse_outcomes(&[lane], 50);
        assert_eq!(fused["a"].total, 1.0 / 51.0);
        assert_eq!(fused["b"].total, 1.0 / 53.0);
    }
}
