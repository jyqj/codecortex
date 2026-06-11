//! File preselection — narrows candidate files before chunk-level search.
//!
//! Implements the full 7-layer scoring strategy as a layer registry (the
//! seam mirrors `lanes.rs::RetrievalLane`): each layer implements
//! [`PreselectLayer`] and is registered in [`default_preselect_layers`];
//! [`preselect`] is a uniform fold over that registry that merges scores,
//! reasons, and a per-layer score bill (`layer_scores`).
//!
//! Layers, in registry (= execution) order; constants live in
//! [`RankingConfig`], defaults shown:
//!   1. working-set boost   max(2.0, 5.0 / rank)
//!   2. recent files         max(1.2, 3.5 / rank)
//!   3. pinned files         max(2.2, 4.0 / rank)
//!   4. overlay (dirty)      max(1.5, 3.0 / rank)
//!   5. FTS summary search   1.4 + 1.0 / (1.0 + |score|)
//!   6. per-token: symbol name match (exact=2.0, fuzzy=1.2) + path token hit (1.0)
//!
//!   F. fallback: recently-indexed files (0.2) — a gated layer that only
//!   fires when layers 1-6 produced zero scores (see [`FallbackLayer`])
//!
//!   7. graph neighbor expansion   0.8 base, +0.1/edge, capped at 1.2
//!      (1-hop call_edges from top seeds; only fires while budget remains)

use std::collections::HashMap;

use cc_db::fts::{sanitize_fts_query, tokenize_codeish};
use cc_db::index_db::IndexDb;
use cc_model::config::RankingConfig;
use cc_model::CcResult;

// ── Public types ───────────────────────────────────────────────

/// Bundles the 9 parameters that `preselect_files` used to take individually.
pub struct PreselectRequest<'a> {
    pub query: &'a str,
    pub path_prefix: Option<&'a str>,
    pub boost_paths: Option<&'a [String]>,
    pub recent_paths: Option<&'a [String]>,
    pub pinned_paths: Option<&'a [String]>,
    pub overlay_paths: Option<&'a [String]>,
    pub explicit_file_paths: Option<&'a [String]>,
    pub limit: usize,
    /// Scoring constants for every preselect layer.
    pub ranking: &'a RankingConfig,
}

/// Statistics about which scoring lanes fired during preselection.
#[derive(Debug, Clone, Default)]
pub struct LaneStats {
    pub fts_hits: usize,
    pub token_hits: usize,
    pub used_fallback: bool,
}

/// Result of file preselection: ordered file paths + per-file scores + per-file reason lists.
#[derive(Debug, Clone)]
pub struct PreselectResult {
    pub files: Vec<String>,
    pub scores: HashMap<String, f64>,
    pub reasons: HashMap<String, Vec<String>>,
    pub lane_stats: LaneStats,
    /// Per-file score bill: `file -> [(layer name, layer total)]` in layer
    /// execution order.  The per-file sum equals `scores[file]`, so a hit
    /// can explain "preselect 3.7 = working-set:2.0 + fts-summary:1.7".
    pub layer_scores: HashMap<String, Vec<(&'static str, f64)>>,
}

// ── Layer seam ─────────────────────────────────────────────────

/// Layer name for the working-set rank-decay layer (layer 1).
pub const LAYER_WORKING_SET: &str = "working-set";
/// Layer name for the recent-files rank-decay layer (layer 2).
pub const LAYER_RECENT: &str = "recent";
/// Layer name for the pinned-files rank-decay layer (layer 3).
pub const LAYER_PINNED: &str = "pinned";
/// Layer name for the overlay (dirty-buffer) rank-decay layer (layer 4).
pub const LAYER_OVERLAY: &str = "dirty-buffer";
/// Layer name for the FTS file-summary layer (layer 5).
pub const LAYER_FTS_SUMMARY: &str = "fts-summary";
/// Layer name for the per-token symbol/path layer (layer 6).
pub const LAYER_TOKEN_SEARCH: &str = "token-search";
/// Layer name for the gated fallback layer (recently-indexed files).
pub const LAYER_FALLBACK: &str = "fallback-indexed";
/// Layer name for the graph-neighbor expansion layer (layer 7).
pub const LAYER_GRAPH_NEIGHBOR: &str = "graph-neighbor";
/// Pseudo-layer name used by the explicit-scope short circuit.
pub const LAYER_EXPLICIT_SCOPE: &str = "explicit-scope";

/// Per-call context handed to every preselect layer.
///
/// Deliberately narrow: layers see the index database, the query, the
/// caller-supplied context path lists, the scoring constants, and a
/// read-only view of the scores accumulated by *earlier* layers
/// (`current_scores`) — which is how the graph-neighbor layer picks its
/// expansion seeds and how the fallback layer evaluates its gate.
pub struct LayerCtx<'a> {
    pub db: &'a IndexDb,
    pub query: &'a str,
    pub path_prefix: Option<&'a str>,
    /// Overall preselect budget (`PreselectRequest::limit`).
    pub limit: usize,
    pub ranking: &'a RankingConfig,
    pub boost_paths: Option<&'a [String]>,
    pub recent_paths: Option<&'a [String]>,
    pub pinned_paths: Option<&'a [String]>,
    pub overlay_paths: Option<&'a [String]>,
    /// Scores accumulated by the layers that ran before this one, in
    /// registry order.  Read-only; merging is the driver's job.
    pub current_scores: &'a HashMap<String, f64>,
}

/// One file scored by one layer.
#[derive(Debug, Clone)]
pub struct LayerHit {
    pub file_path: String,
    pub score: f64,
    /// Human-readable provenance string (kept byte-identical to the
    /// pre-registry reason vocabulary, e.g. `symbol:getUserById`).
    pub reason: String,
}

/// A preselect scoring layer: one candidate-file source merged additively
/// into the shared score map.
///
/// Contract:
/// - `score` returns the layer's hits; the driver owns merging (path
///   normalization, score accumulation, reason + bill bookkeeping).
///   Emitting the same file twice adds up (layer 6 relies on this).
/// - Layers that must not re-score existing candidates (graph-neighbor)
///   filter against `ctx.current_scores` themselves.
/// - Returning `Err` aborts the whole preselect.  Layers that should
///   degrade gracefully (every DB-backed layer today) swallow their own
///   recoverable failures with a `tracing::warn!` and return `Ok(vec![])`.
pub trait PreselectLayer: Sync {
    /// Stable layer identifier used in reasons, `layer_scores`, and stats.
    fn name(&self) -> &'static str;

    /// Score candidate files for this layer.
    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>>;
}

/// The preselect layer registry — the single place to register a new layer.
///
/// Order is the execution order and must be preserved: the fallback layer's
/// gate reads the scores of layers 1-6, and graph-neighbor seeds off
/// everything before it (including fallback).
pub fn default_preselect_layers() -> Vec<&'static dyn PreselectLayer> {
    static WORKING_SET: RankDecayLayer = RankDecayLayer {
        source: RankDecaySource::WorkingSet,
    };
    static RECENT: RankDecayLayer = RankDecayLayer {
        source: RankDecaySource::Recent,
    };
    static PINNED: RankDecayLayer = RankDecayLayer {
        source: RankDecaySource::Pinned,
    };
    static OVERLAY: RankDecayLayer = RankDecayLayer {
        source: RankDecaySource::Overlay,
    };
    vec![
        &WORKING_SET,
        &RECENT,
        &PINNED,
        &OVERLAY,
        &FtsSummaryLayer,
        &TokenSearchLayer,
        &FallbackLayer,
        &GraphNeighborLayer,
    ]
}

/// Merge one [`LayerHit`] into the shared score / reason / bill maps.
///
/// Semantics are identical to the pre-registry `score_file` helper:
/// backslashes are normalized, scores accumulate additively, every hit
/// appends its reason (deduplicated at the end of `preselect`).  The bill
/// additionally aggregates per-layer totals; because layers run
/// sequentially, a layer's contributions for a file always extend the last
/// bill entry.
fn merge_layer_hit(
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
    layer_scores: &mut HashMap<String, Vec<(&'static str, f64)>>,
    layer_name: &'static str,
    hit: LayerHit,
) {
    let normalized = hit.file_path.replace('\\', "/");
    *scores.entry(normalized.clone()).or_insert(0.0) += hit.score;
    reasons
        .entry(normalized.clone())
        .or_default()
        .push(hit.reason);
    let bill = layer_scores.entry(normalized).or_default();
    match bill.last_mut() {
        Some((name, total)) if *name == layer_name => *total += hit.score,
        _ => bill.push((layer_name, hit.score)),
    }
}

// ── Layer implementations ──────────────────────────────────────

/// Which context path list a [`RankDecayLayer`] instance scores.
#[derive(Debug, Clone, Copy)]
enum RankDecaySource {
    WorkingSet,
    Recent,
    Pinned,
    Overlay,
}

/// Layers 1-4 (working-set / recent / pinned / overlay) share the shape
/// `max(floor, scale / rank)`; one struct, four registry instances —
/// floor and scale come from [`RankingConfig`] per source.
struct RankDecayLayer {
    source: RankDecaySource,
}

impl RankDecayLayer {
    fn paths<'a>(&self, ctx: &LayerCtx<'a>) -> Option<&'a [String]> {
        match self.source {
            RankDecaySource::WorkingSet => ctx.boost_paths,
            RankDecaySource::Recent => ctx.recent_paths,
            RankDecaySource::Pinned => ctx.pinned_paths,
            RankDecaySource::Overlay => ctx.overlay_paths,
        }
    }

    fn params(&self, ranking: &RankingConfig) -> (f64, f64) {
        match self.source {
            RankDecaySource::WorkingSet => (
                ranking.preselect_working_set_floor,
                ranking.preselect_working_set_scale,
            ),
            RankDecaySource::Recent => (
                ranking.preselect_recent_floor,
                ranking.preselect_recent_scale,
            ),
            RankDecaySource::Pinned => (
                ranking.preselect_pinned_floor,
                ranking.preselect_pinned_scale,
            ),
            RankDecaySource::Overlay => (
                ranking.preselect_overlay_floor,
                ranking.preselect_overlay_scale,
            ),
        }
    }
}

impl PreselectLayer for RankDecayLayer {
    fn name(&self) -> &'static str {
        match self.source {
            RankDecaySource::WorkingSet => LAYER_WORKING_SET,
            RankDecaySource::Recent => LAYER_RECENT,
            RankDecaySource::Pinned => LAYER_PINNED,
            RankDecaySource::Overlay => LAYER_OVERLAY,
        }
    }

    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>> {
        let Some(paths) = self.paths(ctx) else {
            return Ok(Vec::new());
        };
        let (floor, scale) = self.params(ctx.ranking);
        Ok(paths
            .iter()
            .enumerate()
            .map(|(rank, fp)| {
                let rank1 = (rank + 1) as f64;
                LayerHit {
                    file_path: fp.clone(),
                    score: f64::max(floor, scale / rank1),
                    reason: self.name().to_string(),
                }
            })
            .collect())
    }
}

/// Layer 5: FTS summary search on `files_fts`.
struct FtsSummaryLayer;

impl PreselectLayer for FtsSummaryLayer {
    fn name(&self) -> &'static str {
        LAYER_FTS_SUMMARY
    }

    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>> {
        let fts_query = sanitize_fts_query(ctx.query);
        if fts_query == r#""""# || fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let fts_limit = if ctx.limit <= 120 {
            ctx.limit.min(80)
        } else {
            80 + (ctx.limit.saturating_sub(120)) / 3
        };
        let rows = match ctx
            .db
            .fts_file_summaries(&fts_query, ctx.path_prefix, fts_limit)
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("preselect: FTS summary query failed: {}", e);
                return Ok(Vec::new());
            }
        };

        Ok(rows
            .into_iter()
            .map(|(file_path, raw_score)| {
                let bm25_score = raw_score.abs();
                LayerHit {
                    file_path,
                    score: ctx.ranking.preselect_fts_base + (1.0 / (1.0 + bm25_score)),
                    reason: LAYER_FTS_SUMMARY.to_string(),
                }
            })
            .collect())
    }
}

/// Layer 6: per-token symbol name match + path token hit.
///
/// Both lookups are batched (`*_many`) so the whole layer costs two pooled
/// connection checkouts instead of two per token.
struct TokenSearchLayer;

impl PreselectLayer for TokenSearchLayer {
    fn name(&self) -> &'static str {
        LAYER_TOKEN_SEARCH
    }

    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>> {
        let query_tokens = tokenize_codeish(ctx.query);
        let candidate_tokens: Vec<&str> = query_tokens
            .iter()
            .filter(|t| t.len() >= 3)
            .take(8)
            .map(|s| s.as_str())
            .collect();

        if candidate_tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut hits = Vec::new();

        // 6a. Path token match (one batched query pass for all tokens)
        match ctx
            .db
            .path_token_file_hits_many(&candidate_tokens, ctx.path_prefix, 20)
        {
            Ok(per_token) => {
                for (token, file_paths) in candidate_tokens.iter().zip(per_token) {
                    for file_path in file_paths {
                        hits.push(LayerHit {
                            file_path,
                            score: ctx.ranking.preselect_path_token_bonus,
                            reason: format!("path-token:{}", token),
                        });
                    }
                }
            }
            Err(e) => tracing::warn!("preselect: path-token query failed: {}", e),
        }

        // 6b. Symbol name match (one batched query pass for all tokens)
        match ctx
            .db
            .symbol_token_hits_many(&candidate_tokens, ctx.path_prefix, 24)
        {
            Ok(per_token) => {
                for (token, symbol_hits) in candidate_tokens.iter().zip(per_token) {
                    for (file_path, name) in symbol_hits {
                        let bonus = if name.to_lowercase() == **token {
                            ctx.ranking.preselect_symbol_exact_bonus
                        } else {
                            ctx.ranking.preselect_symbol_fuzzy_bonus
                        };
                        hits.push(LayerHit {
                            file_path,
                            score: bonus,
                            reason: format!("symbol:{}", name),
                        });
                    }
                }
            }
            Err(e) => tracing::warn!("preselect: symbol-token query failed: {}", e),
        }

        Ok(hits)
    }
}

/// Gated fallback layer: recently-indexed files when nothing scored.
///
/// This *is* registered as a layer (rather than a special driver step) so
/// `preselect` stays a uniform fold over the registry; the cross-layer
/// trigger is expressed through `ctx.current_scores`, which the seam
/// already carries for graph-neighbor seeding.  Gate (unchanged semantics):
/// fires iff every layer before it in the registry produced zero scores.
/// The driver mirrors the same predicate to set `LaneStats::used_fallback`.
struct FallbackLayer;

impl PreselectLayer for FallbackLayer {
    fn name(&self) -> &'static str {
        LAYER_FALLBACK
    }

    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>> {
        if !ctx.current_scores.is_empty() {
            return Ok(Vec::new());
        }
        let file_paths = match ctx.db.recent_indexed_files(ctx.limit) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("preselect: fallback query failed: {}", e);
                return Ok(Vec::new());
            }
        };
        Ok(file_paths
            .into_iter()
            .map(|file_path| LayerHit {
                file_path,
                score: ctx.ranking.preselect_fallback_score,
                reason: LAYER_FALLBACK.to_string(),
            })
            .collect())
    }
}

/// Layer 7: graph-neighbor expansion.
///
/// Only fires while preselect hasn't filled its budget; emits only files
/// absent from `current_scores`, so additive merging is equivalent to the
/// historical insert-if-absent semantics.  Recoverable DB failures are
/// swallowed (expansion is best-effort, as before).
struct GraphNeighborLayer;

impl PreselectLayer for GraphNeighborLayer {
    fn name(&self) -> &'static str {
        LAYER_GRAPH_NEIGHBOR
    }

    fn score(&self, ctx: &LayerCtx<'_>) -> CcResult<Vec<LayerHit>> {
        if ctx.current_scores.len() >= ctx.limit {
            return Ok(Vec::new());
        }
        let budget = ctx.limit.saturating_sub(ctx.current_scores.len());
        let extras = score_graph_neighbors(ctx.db, ctx.current_scores, budget, ctx.ranking)
            .unwrap_or_default();
        Ok(extras
            .into_iter()
            .map(|(file_path, score)| LayerHit {
                file_path,
                score,
                reason: LAYER_GRAPH_NEIGHBOR.to_string(),
            })
            .collect())
    }
}

/// Expand preselect candidates by finding files that are call-graph
/// neighbors of the current top-scoring files.
///
/// Takes seed files from current scores, finds their symbols, walks 1-hop
/// call_edges (both callers and callees), and returns discovered neighbor
/// files with a base score of `preselect_graph_neighbor_base` (0.8 —
/// below FTS ~1.4 but above fallback 0.2).
fn score_graph_neighbors(
    db: &IndexDb,
    current_scores: &HashMap<String, f64>,
    expansion_limit: usize,
    ranking: &RankingConfig,
) -> CcResult<Vec<(String, f64)>> {
    if expansion_limit == 0 || current_scores.is_empty() {
        return Ok(Vec::new());
    }

    // Take top-20 files by score as seeds
    let mut top_files: Vec<(&String, &f64)> = current_scores.iter().collect();
    top_files.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
    let seed_files: Vec<&str> = top_files.iter().take(20).map(|(f, _)| f.as_str()).collect();

    // Find symbol UIDs in seed files
    let symbols = db.symbols_by_file_paths(&seed_files)?;
    let seed_uids: Vec<&str> = symbols
        .iter()
        .filter_map(|s| s.symbol_uid.as_deref())
        .take(50) // cap to avoid explosion
        .collect();

    if seed_uids.is_empty() {
        return Ok(Vec::new());
    }

    // Per-edge increment and accumulation cap come from RankingConfig
    // (defaults +0.1 / 1.2); the base is clamped to the same cap so a base
    // above the cap cannot bypass it on first insertion.
    let accum_cap = ranking.preselect_graph_accum_cap;
    let edge_increment = ranking.preselect_graph_edge_increment;
    let neighbor_base = ranking.preselect_graph_neighbor_base.min(accum_cap);
    let mut neighbor_files: HashMap<String, f64> = HashMap::new();

    // --- Callers: who calls symbols in our seed files? ---
    for uid in &seed_uids {
        if let Ok(callers) = db.caller_rows_by_uid(uid, 5) {
            for edge in &callers {
                let file = &edge.file_path;
                if !current_scores.contains_key(file) {
                    neighbor_files
                        .entry(file.clone())
                        .and_modify(|s| *s = (*s + edge_increment).min(accum_cap))
                        .or_insert(neighbor_base);
                }
            }
        }
    }

    // --- Callees: what do symbols in our seed files call? ---
    // Collect all callee UIDs first, then batch-resolve to file paths.
    let mut callee_uids_to_resolve: Vec<String> = Vec::new();
    for uid in &seed_uids {
        if let Ok(callees) = db.callee_rows_by_uid(uid, 5) {
            for edge in callees {
                if let Some(callee_uid) = edge.callee_symbol_uid {
                    callee_uids_to_resolve.push(callee_uid);
                }
            }
        }
    }

    // Batch resolve callee UIDs -> file paths
    if !callee_uids_to_resolve.is_empty() {
        callee_uids_to_resolve.sort();
        callee_uids_to_resolve.dedup();
        callee_uids_to_resolve.truncate(100);
        if let Ok(sym_rows) = db.symbol_rows_by_uids(&callee_uids_to_resolve) {
            for sym in sym_rows.values() {
                if !current_scores.contains_key(&sym.file_path) {
                    neighbor_files
                        .entry(sym.file_path.clone())
                        .and_modify(|s| *s = (*s + edge_increment).min(accum_cap))
                        .or_insert(neighbor_base);
                }
            }
        }
    }

    // Sort by score and limit
    let mut result: Vec<(String, f64)> = neighbor_files.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result.truncate(expansion_limit);
    Ok(result)
}

// ── Main entry point ───────────────────────────────────────────

/// Pre-select files that are likely relevant to the query (new interface).
///
/// Accepts a [`PreselectRequest`] and returns [`PreselectResult`] with files
/// ranked by relevance score, up to `req.limit`.
pub fn preselect(db: &IndexDb, req: &PreselectRequest) -> CcResult<PreselectResult> {
    // If explicit file_paths given, return them directly (like Python).
    if let Some(fps) = req.explicit_file_paths {
        let explicit_score = req.ranking.preselect_explicit_scope_score;
        let files: Vec<String> = fps.to_vec();
        let scores: HashMap<String, f64> =
            files.iter().map(|f| (f.clone(), explicit_score)).collect();
        let reasons: HashMap<String, Vec<String>> = files
            .iter()
            .map(|f| (f.clone(), vec![LAYER_EXPLICIT_SCOPE.into()]))
            .collect();
        let layer_scores: HashMap<String, Vec<(&'static str, f64)>> = files
            .iter()
            .map(|f| (f.clone(), vec![(LAYER_EXPLICIT_SCOPE, explicit_score)]))
            .collect();
        return Ok(PreselectResult {
            files,
            scores,
            reasons,
            lane_stats: LaneStats::default(),
            layer_scores,
        });
    }

    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut reasons: HashMap<String, Vec<String>> = HashMap::new();
    let mut layer_scores: HashMap<String, Vec<(&'static str, f64)>> = HashMap::new();
    let mut lane_stats = LaneStats::default();

    for layer in default_preselect_layers() {
        // Mirror of FallbackLayer's gate: record that fallback fired even if
        // its DB query then returns nothing (historical semantics).
        if layer.name() == LAYER_FALLBACK && scores.is_empty() {
            lane_stats.used_fallback = true;
        }
        let hits = {
            let ctx = LayerCtx {
                db,
                query: req.query,
                path_prefix: req.path_prefix,
                limit: req.limit,
                ranking: req.ranking,
                boost_paths: req.boost_paths,
                recent_paths: req.recent_paths,
                pinned_paths: req.pinned_paths,
                overlay_paths: req.overlay_paths,
                current_scores: &scores,
            };
            layer.score(&ctx)?
        };
        match layer.name() {
            LAYER_FTS_SUMMARY => lane_stats.fts_hits = hits.len(),
            LAYER_TOKEN_SEARCH => lane_stats.token_hits = hits.len(),
            _ => {}
        }
        for hit in hits {
            merge_layer_hit(
                &mut scores,
                &mut reasons,
                &mut layer_scores,
                layer.name(),
                hit,
            );
        }
    }

    // ── Filter by path_prefix, sort, truncate ──────────────────
    let mut filtered: Vec<(String, f64)> = scores
        .into_iter()
        .filter(|(path, _)| {
            if let Some(prefix) = req.path_prefix {
                path.starts_with(prefix)
            } else {
                true
            }
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    filtered.truncate(req.limit);

    let files: Vec<String> = filtered.iter().map(|(p, _)| p.clone()).collect();
    let final_scores: HashMap<String, f64> = filtered.into_iter().collect();
    // Deduplicate reasons per file
    let final_reasons: HashMap<String, Vec<String>> = files
        .iter()
        .map(|f| {
            let mut r = reasons.remove(f).unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            r.retain(|item| seen.insert(item.clone()));
            (f.clone(), r)
        })
        .collect();
    // Keep the score bill only for surviving files (mirrors reasons)
    let final_layer_scores: HashMap<String, Vec<(&'static str, f64)>> = files
        .iter()
        .map(|f| (f.clone(), layer_scores.remove(f).unwrap_or_default()))
        .collect();

    Ok(PreselectResult {
        files,
        scores: final_scores,
        reasons: final_reasons,
        lane_stats,
        layer_scores: final_layer_scores,
    })
}

/// Backward-compatible wrapper: delegates to [`preselect`] via
/// [`PreselectRequest`] with default scoring constants.
#[allow(clippy::too_many_arguments)]
pub fn preselect_files(
    db: &IndexDb,
    query: &str,
    path_prefix: Option<&str>,
    boost_paths: Option<&[String]>,
    recent_paths: Option<&[String]>,
    pinned_paths: Option<&[String]>,
    overlay_paths: Option<&[String]>,
    explicit_file_paths: Option<&[String]>,
    limit: usize,
) -> CcResult<PreselectResult> {
    preselect(
        db,
        &PreselectRequest {
            query,
            path_prefix,
            boost_paths,
            recent_paths,
            pinned_paths,
            overlay_paths,
            explicit_file_paths,
            limit,
            ranking: &RankingConfig::default(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use tempfile::TempDir;

    /// Build an IndexDb whose file paths deliberately do NOT contain the search
    /// token, so a hit can only come from the symbol-name (Layer 6b) path —
    /// isolating the trigram `symbols_fts` substring lookup.
    fn db_with_symbols() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("preselect_test.db"))
            .unwrap()
            .0;
        let conn = db.read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/a.rs', 'Rust', 'h1', 1.0, 100, '2024-01-01');\
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/b.rs', 'Rust', 'h2', 1.0, 100, '2024-01-01');\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s1', 'src/a.rs', 'getUserById', 'function', 1, 5);\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s2', 'src/b.rs', 'createOrder', 'function', 1, 5);",
        )
        .unwrap();
        (tmp, db)
    }

    /// Minimal LayerCtx over a db + optional path lists, for single-layer tests.
    struct CtxFixture<'a> {
        db: &'a IndexDb,
        ranking: RankingConfig,
        boost_paths: Option<&'a [String]>,
        recent_paths: Option<&'a [String]>,
        pinned_paths: Option<&'a [String]>,
        overlay_paths: Option<&'a [String]>,
        current_scores: HashMap<String, f64>,
        query: &'a str,
        limit: usize,
    }

    impl<'a> CtxFixture<'a> {
        fn new(db: &'a IndexDb, query: &'a str) -> Self {
            Self {
                db,
                ranking: RankingConfig::default(),
                boost_paths: None,
                recent_paths: None,
                pinned_paths: None,
                overlay_paths: None,
                current_scores: HashMap::new(),
                query,
                limit: 10,
            }
        }

        fn ctx(&self) -> LayerCtx<'_> {
            LayerCtx {
                db: self.db,
                query: self.query,
                path_prefix: None,
                limit: self.limit,
                ranking: &self.ranking,
                boost_paths: self.boost_paths,
                recent_paths: self.recent_paths,
                pinned_paths: self.pinned_paths,
                overlay_paths: self.overlay_paths,
                current_scores: &self.current_scores,
            }
        }
    }

    /// A token that appears only mid-identifier (camelCase) must still recall the
    /// symbol — the property that forbids degrading `%token%` to a prefix match.
    #[test]
    fn preselect_recalls_substring_symbol_match() {
        let (_tmp, db) = db_with_symbols();

        let result = preselect_files(&db, "user", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/a.rs".to_string()),
            "substring token 'user' must recall 'getUserById' in src/a.rs (no path-token hit possible); got {:?}",
            result.files
        );
        assert!(
            !result.files.contains(&"src/b.rs".to_string()),
            "'user' must not recall 'createOrder'; got {:?}",
            result.files
        );

        let result = preselect_files(&db, "order", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/b.rs".to_string()),
            "substring token 'order' must recall 'createOrder' in src/b.rs; got {:?}",
            result.files
        );
    }

    /// A token that appears only inside a file *path* (not in any symbol name or
    /// file summary) must still be recalled via Layer 6a — exercising the
    /// trigram `file_paths_fts` mirror that replaced the `files` full scan.
    #[test]
    fn preselect_recalls_path_token_match() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("preselect_path_test.db"))
            .unwrap()
            .0;
        let conn = db.read_conn().unwrap();
        // Path contains "widget"; symbol names deliberately do not, so the only
        // possible hit is the path-token (file_paths_fts) lookup.
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/widgetstore/a.rs', 'Rust', 'h1', 1.0, 100, '2024-01-01');\
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/b.rs', 'Rust', 'h2', 1.0, 100, '2024-01-01');\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s1', 'src/widgetstore/a.rs', 'alpha', 'function', 1, 5);\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s2', 'src/b.rs', 'beta', 'function', 1, 5);",
        )
        .unwrap();

        let result =
            preselect_files(&db, "widget", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/widgetstore/a.rs".to_string()),
            "path-substring token 'widget' must recall 'src/widgetstore/a.rs' via file_paths_fts; got {:?}",
            result.files
        );
        assert!(
            !result.files.contains(&"src/b.rs".to_string()),
            "'widget' must not recall unrelated 'src/b.rs'; got {:?}",
            result.files
        );
    }

    /// Test via PreselectRequest directly — verifies the new interface works.
    #[test]
    fn preselect_request_interface() {
        let (_tmp, db) = db_with_symbols();

        let req = PreselectRequest {
            query: "user",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
            ranking: &RankingConfig::default(),
        };
        let result = preselect(&db, &req).unwrap();
        assert!(
            result.files.contains(&"src/a.rs".to_string()),
            "PreselectRequest interface: 'user' must recall 'getUserById'; got {:?}",
            result.files
        );
        assert!(
            result.lane_stats.token_hits > 0,
            "lane_stats.token_hits should be > 0"
        );
        assert!(!result.lane_stats.used_fallback);
    }

    /// Test explicit_file_paths short-circuit via PreselectRequest.
    #[test]
    fn preselect_request_explicit_paths() {
        let (_tmp, db) = db_with_symbols();
        let explicit = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let req = PreselectRequest {
            query: "anything",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: Some(&explicit),
            limit: 10,
            ranking: &RankingConfig::default(),
        };
        let result = preselect(&db, &req).unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(*result.scores.get("src/a.rs").unwrap(), 10.0);
        assert_eq!(result.lane_stats.fts_hits, 0);
        assert!(!result.lane_stats.used_fallback);
        // Explicit scope shows up in the bill too.
        assert_eq!(
            result.layer_scores.get("src/a.rs").unwrap(),
            &vec![(LAYER_EXPLICIT_SCOPE, 10.0)]
        );
    }

    /// Verify LaneStats reports fallback when query matches nothing.
    #[test]
    fn preselect_reports_fallback() {
        let (_tmp, db) = db_with_symbols();
        let req = PreselectRequest {
            query: "zzznonexistent",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
            ranking: &RankingConfig::default(),
        };
        let result = preselect(&db, &req).unwrap();
        assert!(
            result.lane_stats.used_fallback,
            "should use fallback for unmatched query"
        );
    }

    // ── Layer registry / seam tests ────────────────────────────

    /// Registry order must match the historical execution order: layers 1-6,
    /// then fallback, then graph-neighbor (fallback gate and graph seeding
    /// both depend on it).
    #[test]
    fn registry_preserves_execution_order() {
        let names: Vec<&'static str> = default_preselect_layers()
            .iter()
            .map(|l| l.name())
            .collect();
        assert_eq!(
            names,
            vec![
                LAYER_WORKING_SET,
                LAYER_RECENT,
                LAYER_PINNED,
                LAYER_OVERLAY,
                LAYER_FTS_SUMMARY,
                LAYER_TOKEN_SEARCH,
                LAYER_FALLBACK,
                LAYER_GRAPH_NEIGHBOR,
            ]
        );
    }

    /// Each rank-decay instance scores its own path list with its own
    /// floor/scale — single struct, four configurations.
    #[test]
    fn rank_decay_layers_score_their_own_lists() {
        let (_tmp, db) = db_with_symbols();
        let boost = vec!["src/ws.rs".to_string(), "src/ws2.rs".to_string()];
        let pinned = vec!["src/pin.rs".to_string()];
        let mut fixture = CtxFixture::new(&db, "user");
        fixture.boost_paths = Some(&boost);
        fixture.pinned_paths = Some(&pinned);

        let working_set = RankDecayLayer {
            source: RankDecaySource::WorkingSet,
        };
        let hits = working_set.score(&fixture.ctx()).unwrap();
        assert_eq!(hits.len(), 2);
        // rank 1: max(2.0, 5.0/1) = 5.0; rank 2: max(2.0, 5.0/2) = 2.5
        assert_eq!(hits[0].file_path, "src/ws.rs");
        assert!((hits[0].score - 5.0).abs() < 1e-9);
        assert!((hits[1].score - 2.5).abs() < 1e-9);
        assert_eq!(hits[0].reason, LAYER_WORKING_SET);

        let pinned_layer = RankDecayLayer {
            source: RankDecaySource::Pinned,
        };
        let hits = pinned_layer.score(&fixture.ctx()).unwrap();
        assert_eq!(hits.len(), 1);
        // rank 1: max(2.2, 4.0/1) = 4.0
        assert!((hits[0].score - 4.0).abs() < 1e-9);
        assert_eq!(hits[0].reason, LAYER_PINNED);

        // Layers with no list provided emit nothing.
        let recent_layer = RankDecayLayer {
            source: RankDecaySource::Recent,
        };
        assert!(recent_layer.score(&fixture.ctx()).unwrap().is_empty());
    }

    /// FTS summary layer in isolation: no summaries indexed -> no hits, and a
    /// blank query short-circuits.
    #[test]
    fn fts_summary_layer_isolated() {
        let (_tmp, db) = db_with_symbols();
        let fixture = CtxFixture::new(&db, "");
        assert!(FtsSummaryLayer.score(&fixture.ctx()).unwrap().is_empty());
    }

    /// Token layer in isolation recalls symbol substring matches with the
    /// fuzzy bonus and the historical `symbol:<name>` reason format.
    #[test]
    fn token_search_layer_isolated() {
        let (_tmp, db) = db_with_symbols();
        let fixture = CtxFixture::new(&db, "user");
        let hits = TokenSearchLayer.score(&fixture.ctx()).unwrap();
        assert!(
            hits.iter()
                .any(|h| h.file_path == "src/a.rs" && h.reason == "symbol:getUserById"),
            "token layer must emit symbol:<name> reason for src/a.rs; got {:?}",
            hits
        );
        let hit = hits.iter().find(|h| h.file_path == "src/a.rs").unwrap();
        assert!(
            (hit.score - RankingConfig::default().preselect_symbol_fuzzy_bonus).abs() < 1e-9,
            "substring match uses the fuzzy bonus"
        );
    }

    /// Fallback layer gate: fires only when current_scores is empty.
    #[test]
    fn fallback_layer_gate() {
        let (_tmp, db) = db_with_symbols();

        // Empty scores -> fallback emits recently-indexed files at 0.2.
        let fixture = CtxFixture::new(&db, "zzznonexistent");
        let hits = FallbackLayer.score(&fixture.ctx()).unwrap();
        assert!(!hits.is_empty(), "fallback must fire on empty scores");
        for hit in &hits {
            assert!((hit.score - 0.2).abs() < 1e-9);
            assert_eq!(hit.reason, LAYER_FALLBACK);
        }

        // Non-empty scores -> gated off.
        let mut fixture = CtxFixture::new(&db, "zzznonexistent");
        fixture.current_scores.insert("src/a.rs".to_string(), 1.0);
        assert!(
            FallbackLayer.score(&fixture.ctx()).unwrap().is_empty(),
            "fallback must not fire once any layer scored"
        );
    }

    /// Merge semantics regression: multiple layers (and repeated hits within
    /// one layer) accumulate additively per file, reasons append, and the
    /// bill aggregates per layer.
    #[test]
    fn merge_layer_hit_accumulates_and_bills() {
        let mut scores = HashMap::new();
        let mut reasons = HashMap::new();
        let mut layer_scores = HashMap::new();

        let hit = |score: f64, reason: &str| LayerHit {
            file_path: "src/a.rs".to_string(),
            score,
            reason: reason.to_string(),
        };
        merge_layer_hit(
            &mut scores,
            &mut reasons,
            &mut layer_scores,
            LAYER_WORKING_SET,
            hit(2.0, LAYER_WORKING_SET),
        );
        merge_layer_hit(
            &mut scores,
            &mut reasons,
            &mut layer_scores,
            LAYER_TOKEN_SEARCH,
            hit(1.2, "symbol:foo"),
        );
        merge_layer_hit(
            &mut scores,
            &mut reasons,
            &mut layer_scores,
            LAYER_TOKEN_SEARCH,
            hit(1.0, "path-token:foo"),
        );

        assert!((scores["src/a.rs"] - 4.2).abs() < 1e-9);
        assert_eq!(
            reasons["src/a.rs"],
            vec!["working-set", "symbol:foo", "path-token:foo"]
        );
        // Bill: one entry per layer, same-layer hits aggregated.
        assert_eq!(
            layer_scores["src/a.rs"],
            vec![(LAYER_WORKING_SET, 2.0), (LAYER_TOKEN_SEARCH, 2.2)]
        );

        // Backslash normalization matches the historical score_file helper.
        merge_layer_hit(
            &mut scores,
            &mut reasons,
            &mut layer_scores,
            LAYER_RECENT,
            LayerHit {
                file_path: "src\\win.rs".to_string(),
                score: 1.2,
                reason: LAYER_RECENT.to_string(),
            },
        );
        assert!(scores.contains_key("src/win.rs"));
    }

    /// layer_scores must be self-consistent with the total score: for every
    /// surviving file, the sum of its bill equals scores[file].
    #[test]
    fn layer_scores_sum_to_total() {
        let (_tmp, db) = db_with_symbols();
        let boost = vec!["src/a.rs".to_string()];
        let recent = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let req = PreselectRequest {
            query: "user",
            path_prefix: None,
            boost_paths: Some(&boost),
            recent_paths: Some(&recent),
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
            ranking: &RankingConfig::default(),
        };
        let result = preselect(&db, &req).unwrap();
        assert!(!result.files.is_empty());
        for file in &result.files {
            let total = result.scores[file];
            let bill = result
                .layer_scores
                .get(file)
                .unwrap_or_else(|| panic!("missing bill for {}", file));
            let bill_sum: f64 = bill.iter().map(|(_, s)| s).sum();
            assert!(
                (bill_sum - total).abs() < 1e-9,
                "bill for {} must sum to total: {:?} vs {}",
                file,
                bill,
                total
            );
        }
        // src/a.rs got working-set + recent + token-search contributions.
        let a_bill = &result.layer_scores["src/a.rs"];
        let a_layers: Vec<&'static str> = a_bill.iter().map(|(n, _)| *n).collect();
        assert!(a_layers.contains(&LAYER_WORKING_SET));
        assert!(a_layers.contains(&LAYER_RECENT));
        assert!(a_layers.contains(&LAYER_TOKEN_SEARCH));
    }

    /// RankingConfig defaults must equal the historical magic numbers that
    /// were collected into config by this refactor.
    #[test]
    fn ranking_defaults_match_legacy_magic_numbers() {
        let ranking = RankingConfig::default();
        assert!((ranking.preselect_graph_edge_increment - 0.1).abs() < 1e-9);
        assert!((ranking.preselect_graph_accum_cap - 1.2).abs() < 1e-9);
        assert!((ranking.preselect_explicit_scope_score - 10.0).abs() < 1e-9);
        // Pre-existing fields the new layers read, for completeness.
        assert!((ranking.preselect_graph_neighbor_base - 0.8).abs() < 1e-9);
        assert!((ranking.preselect_fallback_score - 0.2).abs() < 1e-9);
    }

    // ── Layer 7: graph neighbor expansion tests ───────────────

    /// score_graph_neighbors returns empty vec when given empty scores.
    #[test]
    fn graph_neighbors_empty_scores() {
        let (_tmp, db) = db_with_symbols();
        let scores: HashMap<String, f64> = HashMap::new();
        let result = score_graph_neighbors(&db, &scores, 10, &RankingConfig::default()).unwrap();
        assert!(result.is_empty());
    }

    /// score_graph_neighbors returns empty vec when expansion_limit is 0.
    #[test]
    fn graph_neighbors_zero_limit() {
        let (_tmp, db) = db_with_symbols();
        let mut scores = HashMap::new();
        scores.insert("src/a.rs".to_string(), 2.0);
        let result = score_graph_neighbors(&db, &scores, 0, &RankingConfig::default()).unwrap();
        assert!(result.is_empty());
    }

    /// Build a DB with symbols that have UIDs and call_edges connecting them,
    /// then verify that graph neighbor expansion discovers the callee file.
    fn db_with_call_graph() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("graph_test.db")).unwrap().0;
        let conn = db.read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES('src/caller.rs', 'Rust', 'h1', 1.0, 100, '2024-01-01');
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES('src/callee.rs', 'Rust', 'h2', 1.0, 100, '2024-01-01');
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES('src/reverse_caller.rs', 'Rust', 'h3', 1.0, 100, '2024-01-01');

             INSERT INTO symbols(symbol_id, symbol_uid, file_path, name, kind, start_line, end_line)
                 VALUES('s1', 'uid:caller:handle_request', 'src/caller.rs', 'handle_request', 'function', 1, 10);
             INSERT INTO symbols(symbol_id, symbol_uid, file_path, name, kind, start_line, end_line)
                 VALUES('s2', 'uid:callee:process_data', 'src/callee.rs', 'process_data', 'function', 1, 10);
             INSERT INTO symbols(symbol_id, symbol_uid, file_path, name, kind, start_line, end_line)
                 VALUES('s3', 'uid:reverse:invoke_handler', 'src/reverse_caller.rs', 'invoke_handler', 'function', 1, 10);

             -- caller.rs::handle_request calls callee.rs::process_data
             INSERT INTO call_edges(edge_id, file_path, caller_symbol, callee_symbol, line, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence)
                 VALUES('e1', 'src/caller.rs', 'handle_request', 'process_data', 5, 'uid:caller:handle_request', 'uid:callee:process_data', 'resolved', 1.0);

             -- reverse_caller.rs::invoke_handler calls caller.rs::handle_request
             INSERT INTO call_edges(edge_id, file_path, caller_symbol, callee_symbol, line, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence)
                 VALUES('e2', 'src/reverse_caller.rs', 'invoke_handler', 'handle_request', 3, 'uid:reverse:invoke_handler', 'uid:caller:handle_request', 'resolved', 1.0);",
        )
        .unwrap();
        (tmp, db)
    }

    /// Graph expansion from seed file discovers callee and reverse-caller files.
    #[test]
    fn graph_neighbors_discovers_callees_and_callers() {
        let (_tmp, db) = db_with_call_graph();

        // Seed: only src/caller.rs is already scored
        let mut scores = HashMap::new();
        scores.insert("src/caller.rs".to_string(), 2.0);

        let result = score_graph_neighbors(&db, &scores, 10, &RankingConfig::default()).unwrap();
        let neighbor_paths: Vec<&str> = result.iter().map(|(p, _)| p.as_str()).collect();

        // Callee side: handle_request calls process_data -> src/callee.rs
        assert!(
            neighbor_paths.contains(&"src/callee.rs"),
            "callee file should be discovered via call graph; got {:?}",
            neighbor_paths
        );

        // Caller side: invoke_handler calls handle_request -> src/reverse_caller.rs
        assert!(
            neighbor_paths.contains(&"src/reverse_caller.rs"),
            "reverse caller file should be discovered via call graph; got {:?}",
            neighbor_paths
        );

        // Score should be 0.8 (base)
        for (_, score) in &result {
            assert!(
                (*score - 0.8).abs() < 0.01 || *score >= 0.8,
                "neighbor score should be >= 0.8; got {}",
                score
            );
        }
    }

    /// Graph expansion skips files already in current scores.
    #[test]
    fn graph_neighbors_skips_already_scored_files() {
        let (_tmp, db) = db_with_call_graph();

        // Both caller and callee are already scored
        let mut scores = HashMap::new();
        scores.insert("src/caller.rs".to_string(), 2.0);
        scores.insert("src/callee.rs".to_string(), 1.5);
        scores.insert("src/reverse_caller.rs".to_string(), 1.0);

        let result = score_graph_neighbors(&db, &scores, 10, &RankingConfig::default()).unwrap();
        assert!(
            result.is_empty(),
            "all neighbors already scored, should return empty; got {:?}",
            result
        );
    }

    /// GraphNeighborLayer adapter: gated off when the budget is full, fires
    /// (and bills) when budget remains.
    #[test]
    fn graph_neighbor_layer_respects_budget() {
        let (_tmp, db) = db_with_call_graph();

        let mut fixture = CtxFixture::new(&db, "handle_request");
        fixture
            .current_scores
            .insert("src/caller.rs".to_string(), 2.0);
        fixture.limit = 1; // budget already consumed by the seed
        assert!(
            GraphNeighborLayer.score(&fixture.ctx()).unwrap().is_empty(),
            "no expansion when scores.len() >= limit"
        );

        fixture.limit = 10;
        let hits = GraphNeighborLayer.score(&fixture.ctx()).unwrap();
        assert!(!hits.is_empty(), "expansion fires while budget remains");
        for hit in &hits {
            assert_eq!(hit.reason, LAYER_GRAPH_NEIGHBOR);
            assert!(
                hit.file_path != "src/caller.rs",
                "must not re-emit seeded files"
            );
        }
    }

    /// Full preselect integration: graph expansion fires when budget remains.
    #[test]
    fn preselect_graph_expansion_fires_when_budget_remains() {
        let (_tmp, db) = db_with_call_graph();

        // Query "handle_request" should match the symbol in src/caller.rs,
        // and with limit > 1, graph expansion should add callee/caller neighbors.
        let req = PreselectRequest {
            query: "handle_request",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
            ranking: &RankingConfig::default(),
        };
        let result = preselect(&db, &req).unwrap();

        assert!(
            result.files.contains(&"src/caller.rs".to_string()),
            "seed file must be present; got {:?}",
            result.files
        );
        // Graph neighbors should have been added
        let has_callee = result.files.contains(&"src/callee.rs".to_string());
        let has_reverse = result.files.contains(&"src/reverse_caller.rs".to_string());
        assert!(
            has_callee || has_reverse,
            "at least one graph neighbor should be added; got {:?}",
            result.files
        );
        // Discovered neighbors carry a graph-neighbor bill entry.
        if has_callee {
            let bill = &result.layer_scores["src/callee.rs"];
            assert!(bill.iter().any(|(n, _)| *n == LAYER_GRAPH_NEIGHBOR));
        }
    }
}
