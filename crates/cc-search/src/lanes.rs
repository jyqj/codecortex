//! Retrieval lanes — independent ranked candidate sources feeding deterministic RRF.
//!
//! Execution stays internal, while each completed run is materialized into the
//! versioned `cc-model::retrieval::LaneOutcome` contract before fusion.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use lru::LruCache;

use cc_db::index_db::IndexDb;
use cc_model::config::SearchConfig;
use cc_model::retrieval::{
    CandidateRef, LaneCoverage, LaneOutcome as PublicLaneOutcome, LaneStatus,
    CANDIDATE_REF_SCHEMA_VERSION, LANE_OUTCOME_SCHEMA_VERSION,
};
use cc_model::{CcError, CcResult};

use crate::plan::{language_from_path, parse_language_name, SearchPlan};

#[path = "lanes/exact_symbol.rs"]
mod exact_symbol;
#[path = "lanes/path.rs"]
mod path;
pub(crate) use crate::fusion::{fuse_outcomes, fused_candidate_ordering, FusedScore};
pub(crate) use exact_symbol::ExactSymbolLane;
pub(crate) use path::PathLane;

pub(crate) const LANE_EXACT_SYMBOL: &str = "exact_symbol";
pub(crate) const LANE_PATH: &str = "path";
pub(crate) const LANE_LEXICAL: &str = "lexical";
pub(crate) const LANE_GREP: &str = "grep";
pub(crate) const LANE_GRAPH: &str = "graph";

/// Dedicated legacy score slot in the stable SearchHit wire schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScoreSlot {
    Lexical,
    Grep,
    Graph,
}

/// Registry order is execution, fusion-bill, annotation, and tie-break order.
pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    vec![
        &ExactSymbolLane,
        &PathLane,
        &LexicalLane,
        &GrepLane,
        &GraphLane,
    ]
}

pub(crate) struct LaneContext<'a> {
    pub(crate) plan: &'a SearchPlan,
    pub(crate) db: &'a IndexDb,
    pub(crate) config: &'a SearchConfig,
    pub(crate) chunk_text_cache: &'a Mutex<LruCache<(u64, String), Arc<str>>>,
    pub(crate) cache_epoch: u64,
}

/// One ranked source. Native scores are diagnostic; only rank enters RRF.
pub(crate) trait RetrievalLane: Sync {
    fn lane_id(&self) -> &'static str;
    fn weight(&self, config: &SearchConfig) -> f64;
    fn is_enabled(&self, context: &LaneContext<'_>) -> bool;
    fn annotates_hits(&self) -> bool;
    fn score_slot(&self) -> Option<ScoreSlot> {
        None
    }
    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>>;
    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        Ok(LaneRun::complete(self.run(context)?))
    }
}

pub(crate) struct LaneRun {
    pub hits: Vec<(String, f64)>,
    pub exact_ids: HashSet<String>,
    pub status: LaneStatus,
    pub coverage: LaneCoverage,
    pub truncation_reason: Option<String>,
    pub grep: Option<cc_model::retrieval::GrepDiagnostics>,
    pub lexical_work: cc_model::retrieval_cost::SqlWork,
}
impl LaneRun {
    pub(crate) fn complete(hits: Vec<(String, f64)>) -> Self {
        let count = hits.len();
        Self {
            hits,
            exact_ids: HashSet::new(),
            status: LaneStatus::Complete,
            coverage: LaneCoverage::complete(None, count),
            truncation_reason: None,
            grep: None,
            lexical_work: Default::default(),
        }
    }
    pub(crate) fn error(reason: impl Into<String>) -> Self {
        Self {
            hits: Vec::new(),
            exact_ids: HashSet::new(),
            status: LaneStatus::Error,
            coverage: LaneCoverage::not_run(),
            truncation_reason: Some(reason.into()),
            grep: None,
            lexical_work: Default::default(),
        }
    }
}
impl Default for LaneRun {
    fn default() -> Self {
        Self::complete(Vec::new())
    }
}

/// Internal execution envelope plus the public contract after identity materialization.
pub(crate) struct LaneOutcome {
    pub(crate) lane_id: &'static str,
    pub(crate) weight: f64,
    pub(crate) annotates_hits: bool,
    pub(crate) score_slot: Option<ScoreSlot>,
    pub(crate) hits: Vec<(String, f64)>,
    pub(crate) exact_ids: HashSet<String>,
    pub(crate) status: LaneStatus,
    pub(crate) coverage: LaneCoverage,
    pub(crate) truncation_reason: Option<String>,
    pub(crate) elapsed_us: u64,
    pub(crate) public: Option<PublicLaneOutcome>,
    pub(crate) grep: Option<cc_model::retrieval::GrepDiagnostics>,
    pub(crate) lexical_work: cc_model::retrieval_cost::SqlWork,
}

/// Fixed process-wide local-lane pool. No per-request thread creation; the
/// request executor separately bounds requests admitted to this pool.
fn lane_pool() -> CcResult<&'static rayon::ThreadPool> {
    static POOL: std::sync::OnceLock<Result<rayon::ThreadPool, String>> =
        std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .thread_name(|i| format!("cc-recall-{i}"))
            .build()
            .map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| CcError::Search(format!("local lane executor unavailable: {e}")))
}

type LaneHitSlot = Option<(u64, CcResult<LaneRun>)>;

/// Execute enabled lanes concurrently but collect in registry order. Disabled
/// is distinct from a completed lane with zero candidates.
pub(crate) fn run_lanes(
    lanes: &[&dyn RetrievalLane],
    context: &LaneContext<'_>,
) -> CcResult<Vec<LaneOutcome>> {
    let mut ids = HashSet::new();
    for lane in lanes {
        let weight = lane.weight(context.config);
        if !ids.insert(lane.lane_id()) || !weight.is_finite() || weight < 0.0 {
            return Err(CcError::Config(
                "duplicate lane id or invalid weight".into(),
            ));
        }
    }
    let enabled: Vec<bool> = lanes.iter().map(|lane| lane.is_enabled(context)).collect();
    context.plan.control().check()?;
    let run_one = |lane: &dyn RetrievalLane| {
        let started = Instant::now();
        let child = context
            .plan
            .control()
            .child(std::time::Duration::from_millis(
                context.plan.policy.lane_timeout_ms,
            ));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            child.check()?;
            let result = lane.run_detailed(context);
            child.check()?;
            result
        }))
        .unwrap_or_else(|_| {
            Err(CcError::Search(format!(
                "retrieval lane '{}' panicked",
                lane.lane_id()
            )))
        });
        let result = match result {
            Err(CcError::QueryTimedOut) => {
                let mut run = LaneRun::error("lane_deadline");
                run.status = LaneStatus::Timeout;
                Ok(run)
            }
            other => other,
        };
        (
            started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            result,
        )
    };
    use rayon::prelude::*;
    let mut results: Vec<LaneHitSlot> = if enabled.iter().filter(|flag| **flag).count() <= 1 {
        lanes
            .iter()
            .zip(&enabled)
            .map(|(lane, enabled)| enabled.then(|| run_one(*lane)))
            .collect()
    } else {
        lane_pool()?.install(|| {
            lanes
                .par_iter()
                .zip(&enabled)
                .map(|(lane, enabled)| enabled.then(|| run_one(*lane)))
                .collect()
        })
    };
    context.plan.control().check()?;

    let mut outcomes = Vec::with_capacity(lanes.len());
    for (index, lane) in lanes.iter().enumerate() {
        let weight = lane.weight(context.config);
        if !weight.is_finite() || weight < 0.0 {
            return Err(CcError::Config(format!(
                "invalid retrieval lane weight: {}",
                lane.lane_id()
            )));
        }
        let (elapsed_us, run, public) = match results[index].take() {
            Some((elapsed, result)) => (elapsed, result?, None),
            None => (
                0,
                LaneRun {
                    status: LaneStatus::Disabled,
                    coverage: LaneCoverage::not_run(),
                    ..Default::default()
                },
                Some(PublicLaneOutcome::disabled(lane.lane_id(), weight)),
            ),
        };
        outcomes.push(LaneOutcome {
            lane_id: lane.lane_id(),
            weight,
            annotates_hits: lane.annotates_hits(),
            score_slot: lane.score_slot(),
            hits: run.hits,
            exact_ids: run.exact_ids,
            status: run.status,
            coverage: run.coverage,
            truncation_reason: run.truncation_reason,
            elapsed_us,
            public,
            grep: run.grep,
            lexical_work: run.lexical_work,
        });
    }
    Ok(outcomes)
}

/// Async providers are optional ranked-reference sources, never source authority.
/// Verify their basis, scope and exact current source identity before fusion.
pub(crate) fn append_semantic_outcome(
    outcomes: &mut Vec<LaneOutcome>,
    context: &LaneContext<'_>,
) -> CcResult<()> {
    let Some(reply) = context.plan.semantic() else {
        return Ok(());
    };
    context.plan.control().check()?;
    let generation = context.db.reads().read_generation()?;
    if reply.generation != generation {
        return Err(CcError::RetrievalChanged { attempts: 1 });
    }
    let mut public = reply.outcome.clone();
    public.validate()?;
    if public.lane_id != "semantic"
        || public.candidate_count > context.plan.policy.semantic_top_k
        || public.candidates.iter().any(|c| c.exact_identity)
    {
        return Err(CcError::InvalidParams("invalid semantic receipt".into()));
    }
    public.weight = 1.0;
    let ids: Vec<_> = public
        .candidates
        .iter()
        .map(|c| c.legacy_chunk_id.as_str())
        .collect();
    let rows = context.db.retrieval().chunk_candidate_rows_by_ids(&ids)?;
    let rows: HashMap<_, _> = rows.into_iter().map(|r| (r.chunk_id.clone(), r)).collect();
    for candidate in &public.candidates {
        let row = rows.get(&candidate.legacy_chunk_id).ok_or_else(|| {
            CcError::Search("semantic candidate is not a current document".into())
        })?;
        if row.document != candidate.document
            || row.source_evidence.span != candidate.source_span
            || !context
                .plan
                .passes_filters(&row.file_path, parse_language_name(&row.language))
        {
            return Err(CcError::Search(
                "semantic candidate identity/span/hard scope mismatch".into(),
            ));
        }
    }
    outcomes.push(LaneOutcome {
        lane_id: "semantic",
        weight: public.weight,
        annotates_hits: true,
        score_slot: None,
        hits: public
            .candidates
            .iter()
            .map(|c| (c.legacy_chunk_id.clone(), c.raw_score))
            .collect(),
        exact_ids: HashSet::new(),
        status: public.status,
        coverage: public.coverage.clone(),
        truncation_reason: public.truncation_reason.clone(),
        elapsed_us: public.elapsed_us,
        public: Some(public),
        grep: None,
        lexical_work: Default::default(),
    });
    Ok(())
}

/// Resolve raw chunk locators to versioned documents once, after all lanes
/// finish. Scope is rechecked before fusion and malformed/missing identities fail.
pub(crate) fn materialize_lane_outcomes(
    outcomes: &mut [LaneOutcome],
    context: &LaneContext<'_>,
) -> CcResult<Vec<PublicLaneOutcome>> {
    let mut unique = HashSet::new();
    let chunk_ids: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| outcome.status.is_fusable())
        .flat_map(|outcome| outcome.hits.iter().map(|(id, _)| id.as_str()))
        .filter(|id| unique.insert((*id).to_string()))
        .collect();
    let rows = context
        .db
        .retrieval()
        .chunk_candidate_rows_by_ids(&chunk_ids)?;
    let rows: HashMap<_, _> = rows
        .into_iter()
        .map(|row| (row.chunk_id.clone(), row))
        .collect();

    let mut public = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        if outcome.public.is_none() {
            let mut seen_chunks = HashSet::new();
            let mut candidates = Vec::with_capacity(outcome.hits.len());
            for (position, (chunk_id, raw_score)) in outcome.hits.iter().enumerate() {
                if !seen_chunks.insert(chunk_id.as_str()) {
                    return Err(CcError::InvalidParams(format!(
                        "retrieval lane '{}' returned duplicate chunk candidate",
                        outcome.lane_id
                    )));
                }
                let row = rows.get(chunk_id).ok_or_else(|| {
                    CcError::Database(format!(
                        "retrieval candidate '{}' has no current document identity",
                        chunk_id
                    ))
                })?;
                let language = parse_language_name(&row.language);
                if !context.plan.passes_filters(&row.file_path, language) {
                    return Err(CcError::InvalidParams(format!(
                        "retrieval lane '{}' returned a candidate outside hard scope",
                        outcome.lane_id
                    )));
                }
                candidates.push(CandidateRef {
                    schema_version: CANDIDATE_REF_SCHEMA_VERSION,
                    document: row.document.clone(),
                    source_span: row.source_evidence.span,
                    legacy_chunk_id: row.chunk_id.clone(),
                    lane_id: outcome.lane_id.to_string(),
                    lane_rank: position + 1,
                    raw_score: *raw_score,
                    scoring_spec: lanes_scoring_spec(outcome.lane_id).to_string(),
                    exact_identity: outcome.exact_ids.contains(chunk_id),
                });
            }
            let value = PublicLaneOutcome {
                schema_version: LANE_OUTCOME_SCHEMA_VERSION,
                lane_id: outcome.lane_id.to_string(),
                weight: outcome.weight,
                status: outcome.status,
                elapsed_us: outcome.elapsed_us,
                candidate_count: candidates.len(),
                coverage: outcome.coverage.clone(),
                truncation_reason: outcome.truncation_reason.clone(),
                candidates,
            };
            value.validate()?;
            outcome.public = Some(value);
        }
        public.push(outcome.public.clone().expect("materialized lane outcome"));
    }
    Ok(public)
}

fn lanes_scoring_spec(lane_id: &str) -> &'static str {
    match lane_id {
        LANE_EXACT_SYMBOL => "exact-symbol-name-qname-signature-v1",
        LANE_PATH => "canonical-scoped-exact-path-domain-token-fallback-v3",
        LANE_LEXICAL => "fts5-bm25-order-v1",
        LANE_GREP => "bounded-case-insensitive-literal-v1",
        LANE_GRAPH => "call-graph-one-hop-uid-byte-anchor-v2",
        _ => "ranked-chunk-v1",
    }
}

#[cfg(test)]
pub(crate) fn materialize_test_outcomes(outcomes: &mut [LaneOutcome]) {
    for outcome in outcomes {
        if outcome.public.is_some() {
            continue;
        }
        let candidates = outcome
            .hits
            .iter()
            .enumerate()
            .map(|(index, (id, raw_score))| CandidateRef {
                schema_version: CANDIDATE_REF_SCHEMA_VERSION,
                document: cc_model::identity::DocumentRef {
                    doc_key: cc_model::identity::bytes_hash(
                        format!("test-doc-key:{id}").as_bytes(),
                    ),
                    doc_version: cc_model::identity::bytes_hash(
                        format!("test-doc-version:{id}").as_bytes(),
                    ),
                    entity_key: None,
                    encoding_key: None,
                },
                source_span: cc_model::source::ByteSpan { start: 0, end: 1 },
                legacy_chunk_id: id.clone(),
                lane_id: outcome.lane_id.to_string(),
                lane_rank: index + 1,
                raw_score: *raw_score,
                scoring_spec: "test-rank-v1".into(),
                exact_identity: outcome.exact_ids.contains(id),
            })
            .collect::<Vec<_>>();
        let value = PublicLaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: outcome.lane_id.to_string(),
            weight: outcome.weight,
            status: outcome.status,
            elapsed_us: outcome.elapsed_us,
            candidate_count: candidates.len(),
            coverage: outcome.coverage.clone(),
            truncation_reason: outcome.truncation_reason.clone(),
            candidates,
        };
        value.validate().expect("valid authored test lane outcome");
        outcome.public = Some(value);
    }
}

#[cfg(test)]
pub(crate) fn test_lane_outcome(
    lane_id: &'static str,
    weight: f64,
    hits: Vec<(String, f64)>,
) -> LaneOutcome {
    let count = hits.len();
    let mut outcome = LaneOutcome {
        lane_id,
        weight,
        annotates_hits: false,
        score_slot: None,
        exact_ids: HashSet::new(),
        status: LaneStatus::Complete,
        coverage: LaneCoverage::complete(None, count),
        truncation_reason: None,
        elapsed_us: 0,
        public: None,
        hits,
        grep: None,
        lexical_work: Default::default(),
    };
    materialize_test_outcomes(std::slice::from_mut(&mut outcome));
    outcome
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

    fn is_enabled(&self, context: &LaneContext<'_>) -> bool {
        !context.plan.hard_scope().is_empty()
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn score_slot(&self) -> Option<ScoreSlot> {
        Some(ScoreSlot::Lexical)
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        self.run_detailed(context).map(|r| r.hits)
    }

    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        let plan = context.plan;
        let limit = plan.limits().lexical;
        // SearchPlan compiles trusted literal groups. Re-sanitizing here
        // would erase the conjunctive identifier boundary and restore the
        // noisy OR-of-every-camel-fragment behavior.
        let fts_q = plan.lexical_query();
        if fts_q == r#""""# {
            return Ok(LaneRun::complete(Vec::new()));
        }
        let candidates = context.db.retrieval().fts_chunk_candidates_with_work(
            fts_q,
            &plan.chunk_scope(),
            limit.saturating_add(1),
        )?;

        let mut results = Vec::new();
        for (cid, file_path, language_name) in candidates.rows {
            let language = parse_language_name(&language_name);
            if plan.passes_filters(&file_path, language) {
                let score = candidates
                    .raw_scores
                    .get(&cid)
                    .copied()
                    .ok_or_else(|| CcError::Database("missing native BM25 score".into()))?;
                results.push((cid, score));
            }
        }
        let lower_bound = results.len();
        let candidate_truncated = results.len() > limit;
        let query_truncated = plan.lexical_query_budget_exhausted();
        let truncated = candidate_truncated || query_truncated;
        results.truncate(limit);
        Ok(LaneRun {
            hits: results,
            exact_ids: HashSet::new(),
            status: if truncated {
                LaneStatus::Partial
            } else {
                LaneStatus::Complete
            },
            coverage: if truncated {
                LaneCoverage::partial(None, lower_bound)
            } else {
                LaneCoverage::complete(None, lower_bound)
            },
            truncation_reason: match (candidate_truncated, query_truncated) {
                (true, true) => Some("candidate_limit_and_query_expansion_budget".into()),
                (true, false) => Some("candidate_limit".into()),
                (false, true) => Some("query_expansion_atom_budget".into()),
                (false, false) => None,
            },
            lexical_work: candidates.work,
            grep: None,
        })
    }
}

/// Grep search — regex match on chunk text with file-level filtering.
///
/// Side effect: caches the decompressed text of every *matched* chunk in
/// `chunk_text_cache` so the batch-fetch step can reuse it.  Non-matching
/// scanned chunks deliberately stay out of the cache: a cold scan would
/// otherwise rotate the whole LRU and evict hot entries.
///
/// `crate::grep` owns soft-first / FTS-prefilter / hard-scope fallback stages.
/// The shared decode budget and duplicate set are checked before decompression.
/// Detailed results retain counts and complete/limited/partial state, including
/// empty hit lists; MCP exposes them rather than declaring a capped scan complete.
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
        context.plan.grep_enabled()
    }

    fn annotates_hits(&self) -> bool {
        true
    }

    fn score_slot(&self) -> Option<ScoreSlot> {
        Some(ScoreSlot::Grep)
    }

    fn run(&self, context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        crate::grep::retrieve(context).map(|result| result.hits)
    }

    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        crate::grep::retrieve(context)
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
        context.config.graph_weight > 0.0 && !context.plan.hard_scope().is_empty()
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
        Self::search(
            context.db,
            context.plan,
            context.plan.query_tokens(),
            context.config.graph_top_k,
        )
    }

    fn run_detailed(&self, context: &LaneContext<'_>) -> CcResult<LaneRun> {
        let limit = context.config.graph_top_k.max(context.plan.limits().top_k);
        match Self::search_detailed(context.db, context.plan, context.plan.query_tokens(), limit) {
            Ok(run) => Ok(run),
            Err(error) => {
                tracing::warn!(error = %error, "graph retrieval lane degraded");
                Ok(LaneRun::error("graph_read_error"))
            }
        }
    }
}

impl GraphLane {
    /// Core graph retrieval: seed symbols + 1-hop call-edge expansion,
    /// mapped to byte-verified declaration documents. A split symbol does
    /// not require a nonexistent whole-function chunk; an anchor does not
    /// claim that the whole function body was returned.
    pub(crate) fn search(
        db: &IndexDb,
        plan: &SearchPlan,
        query_tokens: &[String],
        limit: usize,
    ) -> CcResult<Vec<(String, f64)>> {
        Self::search_detailed(db, plan, query_tokens, limit).map(|run| run.hits)
    }

    fn search_detailed(
        db: &IndexDb,
        plan: &SearchPlan,
        query_tokens: &[String],
        limit: usize,
    ) -> CcResult<LaneRun> {
        let ranking = plan.ranking();
        // Preserve the existing seed and neighbor budgets, but measure their
        // own truncation: a short final chunk list cannot prove full coverage.
        let (seed_uids, seed_truncated) =
            find_seed_symbol_uids(db, query_tokens, ranking, &plan.chunk_scope())?;
        let seed_keys: Vec<&str> = seed_uids.iter().map(|(uid, _)| uid.as_str()).collect();
        let (neighbors, neighbor_truncated) = db
            .retrieval()
            .graph_neighbor_uids_scoped_with_coverage(&seed_keys, &plan.chunk_scope(), 10)?;

        let mut neighbor_uids: HashMap<String, f64> = HashMap::new();
        for (uid, seed_score) in &seed_uids {
            // Include the seed itself (distance 0)
            neighbor_uids
                .entry(uid.clone())
                .and_modify(|s| *s = s.max(*seed_score))
                .or_insert(*seed_score);

            for (_, neighbor) in neighbors.iter().filter(|(seed, _)| seed == uid) {
                let score = seed_score * ranking.graph_neighbor_decay;
                neighbor_uids
                    .entry(neighbor.clone())
                    .and_modify(|s| *s = s.max(score))
                    .or_insert(score);
            }
        }

        // Step 3: Map symbol UIDs -> chunks, applying file filters
        let uid_list: Vec<String> = neighbor_uids.keys().cloned().collect();
        let sym_rows = db.reads().symbol_rows_by_uids(&uid_list)?;

        // Existing seed/neighbor budgets bound authoritative UID lookups.
        // Scope is checked here and again before DAO declaration admission.
        let mut candidates: Vec<(&str, f64)> = Vec::new();
        for (uid, score) in &neighbor_uids {
            if let Some(sym) = sym_rows.get(uid) {
                // SymbolRow carries no language column — infer it from the
                // file extension, matching the indexer's assignment.
                if !plan.passes_filters(&sym.file_path, language_from_path(&sym.file_path)) {
                    continue;
                }
                candidates.push((uid.as_str(), *score));
            }
        }
        let mut best_per_chunk: HashMap<String, f64> = HashMap::new();
        let mut unmapped_source = false;
        let mut anchor_uids: Vec<&str> = candidates.iter().map(|(uid, _)| *uid).collect();
        anchor_uids.sort_unstable();
        anchor_uids.dedup();
        let anchors = db.retrieval().symbol_uid_anchor_chunks(
            &anchor_uids,
            &plan.chunk_scope(),
            plan.control(),
        )?;
        for (uid, score) in candidates {
            plan.control().check()?;
            // Smallest-container order plus actual start_line/start_col byte
            // containment, without same-line sibling/name substitution.
            let cid = anchors.get(uid).cloned();
            if let Some(cid) = cid {
                best_per_chunk
                    .entry(cid)
                    .and_modify(|s| *s = s.max(score))
                    .or_insert(score);
            } else {
                unmapped_source = true;
            }
        }
        let mut chunk_scores: Vec<(String, f64)> = best_per_chunk.into_iter().collect();

        // Sort by score descending and limit
        chunk_scores.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let lower_bound = chunk_scores.len();
        let reason = if seed_truncated || neighbor_truncated {
            Some("graph_expansion_limit")
        } else if unmapped_source {
            Some("graph_source_unmapped")
        } else if lower_bound > limit {
            Some("candidate_limit")
        } else {
            None
        };
        chunk_scores.truncate(limit);
        let mut run = LaneRun::complete(chunk_scores);
        if let Some(reason) = reason {
            run.status = LaneStatus::Partial;
            run.coverage = LaneCoverage::partial(None, lower_bound);
            run.truncation_reason = Some(reason.into());
        }
        Ok(run)
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
    scope: &cc_db::ChunkScope,
) -> CcResult<(Vec<(String, f64)>, bool)> {
    let mut results: HashMap<String, f64> = HashMap::new();
    let mut truncated = query_tokens.len() > 5;

    for token in query_tokens.iter().take(5) {
        // Use trigram-accelerated LIKE via symbols_fts; surface exact name
        // matches first so the 10-row cap doesn't crowd them out with
        // arbitrary substring hits.
        let seeds = db.retrieval().symbol_seed_hits_scoped(token, scope, 11)?;
        truncated |= seeds.len() > 10;
        for (uid, name) in seeds.into_iter().take(10) {
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
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    truncated |= sorted.len() > 20;
    sorted.truncate(20); // max 20 seed symbols
    Ok((sorted, truncated))
}
