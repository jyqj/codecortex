//! Search planning — normalizes caller requests into a compact execution plan.
//!
//! `SearchEngine` owns execution (lane queries, fusion, DB fetch).  `SearchPlan`
//! owns the caller-facing search semantics that should stay consistent across
//! those execution steps: DSL normalization, file preselection, lane limits,
//! materialized filters, and rerank metadata/reasons.

use std::collections::{HashMap, HashSet};

use cc_db::fts::{expand_query_text, tokenize_codeish};
use cc_db::index_db::IndexDb;
use cc_model::config::{RankingConfig, RepoSizeTier, SearchConfig};
use cc_model::retrieval::{HardScope, SoftHints};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcResult, Language};

use crate::lanes::{FusedScore, LaneOutcome, ScoreSlot};
use crate::preselect::{PreselectRequest, PreselectResult};
use crate::score_trace::ScoreTrace;

#[derive(Debug)]
pub(crate) struct SearchPlan {
    pub(crate) policy: crate::query_policy::QueryPolicy,
    control: cc_model::query::QueryControl,
    request: SearchRequest,
    dsl: crate::dsl::ParsedQuery,
    expanded_query: String,
    query_tokens: Vec<String>,
    primary_query_tokens: Vec<String>,
    limits: LaneLimits,
    filters: HardScope,
    soft_hints: SoftHints,
    preselect: PreselectResult,
    rerank_inputs: RerankInputs,
    ranking: RankingConfig,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneLimits {
    pub top_k: usize,
    pub exact_symbol: usize,
    pub path: usize,
    pub lexical: usize,
    pub grep: usize,
    pub rerank_window: usize,
}

#[derive(Debug, Clone, Default)]
struct RerankInputs {
    boost_files: HashSet<String>,
    recent_files: HashSet<String>,
    pinned_files: HashSet<String>,
    overlay_files: HashSet<String>,
}

/// Per-lane 1-based rank lookups, uniformly keyed by lane id.
///
/// Also carries the ordered list of lanes that opted into per-hit
/// annotation (`RetrievalLane::annotates_hits()`), so `hit_from_chunk`
/// can produce `{lane_id}@{rank}` reasons generically in lane-collection
/// order instead of consulting a lane-id whitelist.
#[derive(Debug)]
pub(crate) struct LaneRanks<'a> {
    by_lane: HashMap<&'static str, HashMap<&'a str, usize>>,
    /// Lanes with `annotates_hits() == true`, in lane-collection order,
    /// each paired with its declared `SearchHit` score slot (if any).
    annotating: Vec<(&'static str, Option<ScoreSlot>)>,
}

#[derive(Debug)]
pub(crate) struct CandidateChunk {
    pub document: Option<cc_model::identity::DocumentRef>,
    pub source_evidence: Option<cc_model::source::ChunkSource>,
    pub chunk_id: String,
    pub file_path: String,
    pub language_name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub text: String,
}

impl SearchPlan {
    #[cfg(test)]
    pub(crate) fn build(
        db: &IndexDb,
        config: &SearchConfig,
        ranking: &RankingConfig,
        request: &SearchRequest,
        repo_tier: Option<RepoSizeTier>,
    ) -> CcResult<Self> {
        Self::build_configured(
            db,
            config,
            ranking,
            &cc_model::query::QueryConfig::default(),
            request,
            repo_tier,
        )
    }

    pub(crate) fn build_configured(
        db: &IndexDb,
        config: &SearchConfig,
        ranking: &RankingConfig,
        query_config: &cc_model::query::QueryConfig,
        request: &SearchRequest,
        repo_tier: Option<RepoSizeTier>,
    ) -> CcResult<Self> {
        let policy = crate::query_policy::QueryPolicy::resolve(
            query_config,
            request,
            request.semantic.is_some(),
        )?;
        let control = match &request.control {
            Some(value) => value.clone(),
            None => cc_model::query::QueryControl::new(std::time::Duration::from_millis(
                policy.deadline_ms,
            ))?,
        };
        control.check()?;
        config.validate_retrieval()?;
        ranking.validate_retrieval()?;
        let dsl = crate::dsl::parse_search_dsl(&request.query);
        let mut request = request.clone();

        let filters = crate::scope::normalize_request(&mut request, &dsl)?;
        let mut soft_hints = SoftHints::from(&request);

        let query_text = augmented_query_text(&request);
        let expanded_query = expand_query_text(&query_text);
        let top_k = if request.top_k == 0 {
            10
        } else {
            request.top_k
        };

        let preselect_limit = request
            .file_preselect_limit
            .unwrap_or_else(|| default_preselect_limit(top_k, repo_tier));
        let preselect = crate::preselect::preselect(
            db,
            &PreselectRequest {
                query: &query_text,
                path_prefix: request.path_prefix.as_deref(),
                boost_paths: request.boost_file_paths.as_deref(),
                recent_paths: request.recent_file_paths.as_deref(),
                pinned_paths: request.pinned_file_paths.as_deref(),
                overlay_paths: request.overlay_file_paths.as_deref(),
                explicit_file_paths: request.file_paths.as_deref(),
                limit: preselect_limit,
                ranking,
            },
        )?;
        control.check()?;
        // Preselection is a ranking hint, never a caller-owned hard constraint.
        soft_hints.preselected_files = preselect.files.clone();
        let rerank_inputs = RerankInputs::from_hints(&soft_hints);
        let query_tokens = tokenize_codeish(&query_text);
        let primary_query_tokens = tokenize_codeish(&dsl.text);
        let limits = LaneLimits {
            top_k,
            exact_symbol: config.exact_symbol_top_k.max(top_k),
            path: config.path_top_k.max(top_k),
            lexical: config.lexical_top_k.max(top_k),
            grep: config.grep_top_k.max(top_k),
            rerank_window: config.rerank_window.max(top_k),
        };

        Ok(Self {
            request,
            policy,
            control,
            dsl,
            expanded_query,
            query_tokens,
            primary_query_tokens,
            limits,
            filters,
            preselect,
            soft_hints,
            rerank_inputs,
            ranking: ranking.clone(),
        })
    }

    pub(crate) fn control(&self) -> &cc_model::query::QueryControl {
        &self.control
    }
    pub(crate) fn semantic(&self) -> Option<&cc_model::semantic::SemanticResponse> {
        if self.policy.effective == cc_model::query::RetrievalStrategy::Local {
            None
        } else {
            self.request.semantic.as_deref()
        }
    }

    pub(crate) fn exact_symbol_query(&self) -> Option<&str> {
        self.dsl.name_filter.as_deref().or_else(|| {
            let text = self.dsl.text.trim();
            // Equality lookup also accepts literal stored signatures containing
            // whitespace; natural-language queries simply produce an empty lane.
            (!text.is_empty()).then_some(text)
        })
    }

    pub(crate) fn primary_query_text(&self) -> &str {
        self.dsl.text.trim()
    }

    pub(crate) fn primary_query_tokens(&self) -> &[String] {
        &self.primary_query_tokens
    }

    pub(crate) fn ranking(&self) -> &RankingConfig {
        &self.ranking
    }

    pub(crate) fn lexical_query(&self) -> &str {
        &self.expanded_query
    }

    pub(crate) fn grep_query(&self) -> &str {
        &self.request.query
    }

    /// Execution eligibility is independent of fusion weight. Legacy grep can
    /// still supply candidates with zero RRF weight, but an empty hard scope
    /// performs no scan. Diagnostics and the lane use this one predicate.
    pub(crate) fn grep_enabled(&self) -> bool {
        self.request.include_grep
            && !self.filters.is_empty()
            && !self.grep_query().trim().is_empty()
    }

    pub(crate) fn query_tokens(&self) -> &[String] {
        &self.query_tokens
    }

    pub(crate) fn limits(&self) -> LaneLimits {
        self.limits
    }

    pub(crate) fn hard_scope(&self) -> &HardScope {
        &self.filters
    }

    pub(crate) fn passes_filters(&self, file_path: &str, language: Language) -> bool {
        self.filters.passes(file_path, language)
    }

    /// Structured chunk-scan scope for the lexical/grep lanes; the scope SQL
    /// itself is owned by cc-db (`RetrievalReadModel::fts_chunk_candidates` /
    /// `scan_chunks_for_grep`).
    pub(crate) fn chunk_scope(&self) -> cc_db::ChunkScope {
        crate::scope::chunk_scope(&self.filters)
    }

    /// Whether the request carries an explicit file-paths scope (which
    /// bounds the grep scan's cardinality).
    pub(crate) fn has_file_scope(&self) -> bool {
        self.filters.file_paths.is_some()
    }

    /// Bounded soft worklist intersects, but never replaces, caller constraints.
    pub(crate) fn soft_chunk_scope(&self) -> Option<cc_db::ChunkScope> {
        let mut seen = HashSet::new();
        let files: Vec<String> = self
            .soft_hints
            .pinned_files
            .iter()
            .chain(&self.soft_hints.working_files)
            .chain(&self.soft_hints.recent_files)
            .chain(&self.soft_hints.overlay_files)
            .chain(&self.soft_hints.preselected_files)
            .filter(|p| seen.insert((*p).clone()))
            .take(512)
            .cloned()
            .collect();
        if files.is_empty() {
            return None;
        }
        let soft = self.filters.intersect(&HardScope {
            file_paths: Some(files),
            ..Default::default()
        });
        if soft.is_empty() {
            None
        } else {
            Some(crate::scope::chunk_scope(&soft))
        }
    }

    pub(crate) fn is_empty_scope(&self) -> bool {
        self.filters.is_empty()
    }

    pub(crate) fn scope_explain(
        &self,
        config: &SearchConfig,
    ) -> cc_model::context::SearchScopeExplain {
        use cc_model::context::{
            SearchBudgetExplain, SearchHardScopeExplain, SearchScopeExplain, SearchSoftScopeExplain,
        };
        let mut prefix = self.filters.path_prefix.clone();
        let mut truncated = false;
        if let Some(p) = &mut prefix {
            if p.len() > 1024 {
                let mut n = 1024;
                while !p.is_char_boundary(n) {
                    n -= 1;
                }
                p.truncate(n);
                truncated = true;
            }
        }
        let hint_entries = [
            ("working", self.soft_hints.working_files.len()),
            ("recent", self.soft_hints.recent_files.len()),
            ("pinned", self.soft_hints.pinned_files.len()),
            ("overlay", self.soft_hints.overlay_files.len()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        SearchScopeExplain {
            schema_version: 1,
            policy: crate::engine_cache::RETRIEVAL_POLICY.into(),
            hard: SearchHardScopeExplain { path_prefix: prefix, path_prefix_truncated: truncated,
                languages: self.filters.languages.clone(), explicit_file_count: self.filters.file_paths.as_ref().map(Vec::len),
                empty: self.filters.is_empty(), semantics: "conjunctive_repository_relative_case_sensitive_components; empty means contradictory/explicit-empty constraint, not an inventory count".into() },
            soft: SearchSoftScopeExplain { role: "ranking_and_scan_priority_only".into(), hint_entries,
                preselected_count: self.soft_hints.preselected_files.len(),
                contributions: "per-hit score_trace is the additive bill; stage_a_layer_scores explain its capped stage-a component and must not be added twice; raw hint and excluded paths omitted".into() },
            budget: SearchBudgetExplain { top_k: self.limits.top_k, exact_symbol_candidates: self.limits.exact_symbol, path_candidates: self.limits.path, lexical_candidates: self.limits.lexical, grep_candidates: self.limits.grep,
                grep_scan_cap: config.grep_scan_cap, rerank_window: self.limits.rerank_window,
                grep_enabled: self.grep_enabled(),
                units: "candidate limits count chunks; grep_scan_cap counts decompressions, not SQL work, total memory or time; cached diagnostics describe originating computation, not work repeated on a cache hit".into() },
            ordering: vec!["exact-target first (identity tier, not an additive score)".into(), "rerank_score descending = sum(score_trace)".into(), "chunk_id ascending".into()],
        }
    }

    pub(crate) fn lane_ranks<'a>(&self, outcomes: &'a [LaneOutcome]) -> LaneRanks<'a> {
        LaneRanks::from_outcomes(outcomes)
    }

    pub(crate) fn hit_from_chunk(
        &self,
        chunk: CandidateChunk,
        fused: &FusedScore,
        lane_ranks: &LaneRanks<'_>,
    ) -> Option<SearchHit> {
        let CandidateChunk {
            document,
            source_evidence,
            chunk_id,
            file_path,
            language_name,
            start_line,
            end_line,
            breadcrumb,
            symbol_name,
            symbol_kind,
            text,
        } = chunk;

        let language = parse_language_name(&language_name);
        if !self.filters.passes(&file_path, language) {
            return None;
        }

        let path_text = format!(
            "{} {} {}",
            file_path,
            breadcrumb,
            symbol_name.as_deref().unwrap_or("")
        );
        let overlap =
            crate::rrf::overlap_score(&self.query_tokens, &format!("{}\n{}", path_text, text));

        // Every additive rerank contribution flows through the trace; the
        // final `rerank_score` is the trace total.  Components are pushed in
        // the historical addition order (RRF lanes, overlap, then boosts),
        // so the float result is bit-identical to the old incremental sum.
        let mut trace = ScoreTrace::new();
        if fused.by_lane.is_empty() {
            // Fused total without a per-lane breakdown (ad-hoc construction
            // in tests): bill it as one opaque RRF component.
            trace.push("rrf", fused.total);
        } else {
            for (lane_id, lane_contribution) in &fused.by_lane {
                trace.push(&format!("rrf:{lane_id}"), *lane_contribution);
            }
        }
        trace.push("overlap", overlap * self.ranking.overlap_weight);

        let mut reasons = Vec::new();
        if fused.exact_identity {
            reasons.push("exact-target".into());
        }
        // Per-hit annotation is lane-driven: every lane that opted in via
        // `RetrievalLane::annotates_hits()` contributes a `{lane_id}@{rank}`
        // reason and a rank-derived score, iterated in lane-collection order.
        // Opted-out lanes (graph) are fusion-only and never appear here.
        // Lanes that declared a `ScoreSlot` accumulate their rank-derived
        // score here, keyed by slot; lanes without a slot surface through
        // their reason string only.
        let mut slot_scores: Vec<(ScoreSlot, f64)> = Vec::new();
        for (lane_id, score_slot) in lane_ranks.annotating_lanes() {
            let Some(rank) = lane_ranks.rank(lane_id, &chunk_id) else {
                continue;
            };
            reasons.push(format!("{lane_id}@{rank}"));
            if let Some(slot) = score_slot {
                slot_scores.push((slot, 1.0 / rank as f64));
            }
        }
        // SCHEMA PROJECTION — the single, centralized slot → `SearchHit`
        // field table.  `SearchHit`'s per-lane score fields live in cc-model
        // and are fixed (MCP output schema), so this match is over the
        // closed `ScoreSlot` set and never grows when a lane is added: a
        // new lane declares a slot via `RetrievalLane::score_slot()` (or
        // `None`) and is registered in `lanes::default_lanes()` — nothing
        // here changes.
        let mut lexical_score = 0.0;
        let mut grep_score = 0.0;
        let mut graph_score = 0.0;
        for (slot, lane_score) in slot_scores {
            match slot {
                ScoreSlot::Lexical => lexical_score = lane_score,
                ScoreSlot::Grep => grep_score = lane_score,
                ScoreSlot::Graph => graph_score = lane_score,
            }
        }

        if let Some(ref sym_name) = symbol_name {
            let sym_lower = sym_name.to_lowercase();
            if self.query_tokens.contains(&sym_lower) {
                trace.push("boost:symbol-exact", self.ranking.symbol_exact_bonus);
                reasons.push("symbol-exact".into());
            }
        }

        if let Some(prefix) = self.filters.path_prefix.as_deref() {
            if file_path.starts_with(prefix) {
                trace.push("boost:path-prefix", self.ranking.path_prefix_bonus);
            }
        }

        if is_project_doc(&file_path) {
            trace.push("boost:doc-file", self.ranking.doc_file_bonus);
            reasons.push("doc-file".into());
        }

        if self.rerank_inputs.boost_files.contains(file_path.as_str()) {
            trace.push("boost:working-set-boost", self.ranking.working_set_boost);
            reasons.push("working-set-boost".into());
        }
        if self.rerank_inputs.recent_files.contains(file_path.as_str()) {
            trace.push("boost:recent-file", self.ranking.recent_file_boost);
            reasons.push("recent-file".into());
        }
        if self.rerank_inputs.pinned_files.contains(file_path.as_str()) {
            trace.push("boost:pinned-context", self.ranking.pinned_context_boost);
            reasons.push("pinned-context".into());
        }
        if self
            .rerank_inputs
            .overlay_files
            .contains(file_path.as_str())
        {
            trace.push(
                "boost:overlay-neighbor",
                self.ranking.overlay_neighbor_boost,
            );
            reasons.push("overlay-neighbor".into());
        }

        let stage_a_score = self
            .preselect
            .scores
            .get(&file_path)
            .copied()
            .unwrap_or(0.0);
        if stage_a_score > 0.0 {
            trace.push(
                "boost:stage-a",
                (stage_a_score * self.ranking.stage_a_weight).min(self.ranking.stage_a_cap),
            );
            if let Some(file_reasons) = self.preselect.reasons.get(&file_path) {
                for r in file_reasons.iter().take(3) {
                    reasons.push(r.clone());
                }
            }
            // Per-layer score bill from preselect: explains how stage-A
            // arrived at this file's score (e.g. `preselect:working-set:+2.00`).
            // Additive on top of the legacy reason strings above; bounded by
            // the number of preselect layers.
            if let Some(bill) = self.preselect.layer_scores.get(&file_path) {
                for (layer, layer_score) in bill {
                    reasons.push(format!("preselect:{layer}:+{layer_score:.2}"));
                }
            }
        }

        dedupe_reasons(&mut reasons);
        let mut metadata = self.rerank_metadata(&file_path, stage_a_score);
        metadata["source_freshness"] =
            serde_json::json!({"status":"indexed_snapshot","disk_checked":false});
        if let Some(reference) = document {
            metadata["document"] =
                serde_json::to_value(reference).expect("document reference serializable");
        }
        if let Some(proof) = source_evidence {
            // Indexed-source coordinates, not a claim that the filesystem is current.
            metadata["source_evidence"] =
                serde_json::to_value(proof).expect("finite source coordinates");
        }

        Some(SearchHit {
            chunk_id,
            file_path,
            language,
            start_line,
            end_line,
            breadcrumb,
            symbol_name,
            symbol_kind: symbol_kind
                .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
            text,
            fused_score: fused.total,
            lexical_score,
            grep_score,
            graph_score,
            // INVARIANT: the final score IS the trace total — every later
            // mutation (dsl-name bonus, graph rerank) must append a matching
            // component so `sum(score_trace) == rerank_score` always holds.
            rerank_score: trace.total(),
            reasons,
            score_trace: trace.into_components(),
            source: "index".into(),
            lane: None,
            metadata,
        })
    }

    pub(crate) fn finalize_results(&self, results: &mut Vec<SearchHit>) {
        self.finalize_results_with_limit(results, self.limits.top_k);
    }

    /// Like `finalize_results` but truncates to an explicit `limit` instead
    /// of `self.limits.top_k`.  Used by `search_with_graph_context` to keep
    /// up to `rerank_window` results for the graph-rerank step.
    pub(crate) fn finalize_results_with_limit(&self, results: &mut Vec<SearchHit>, limit: usize) {
        if let Some(ref kind_filter) = self.dsl.kind_filter {
            results.retain(|hit| match &hit.symbol_kind {
                Some(sk) => crate::dsl::matches_kind(sk, kind_filter),
                None => false,
            });
        }

        if let Some(ref name_filter) = self.dsl.name_filter {
            let nf_lower = name_filter.to_lowercase();
            for hit in results.iter_mut() {
                if let Some(ref sn) = hit.symbol_name {
                    if sn.to_lowercase().contains(&nf_lower) {
                        crate::score_trace::apply_traced_boost(
                            hit,
                            "boost:dsl-name",
                            self.ranking.dsl_name_bonus,
                        );
                        hit.reasons.push(format!("dsl-name:{}", name_filter));
                    }
                }
            }
            results.retain(|hit| {
                hit.symbol_name
                    .as_ref()
                    .map(|sn| sn.to_lowercase().contains(&nf_lower))
                    .unwrap_or(false)
            });
        }

        results.sort_by(compare_hits);
        results.truncate(limit);
    }

    fn rerank_metadata(&self, file_path: &str, stage_a_score: f64) -> serde_json::Value {
        serde_json::json!({
            "stage_a_file_score": stage_a_score,
            "stage_a_files_considered": self.soft_hints.preselected_files.len(),
            "stage_a_file_reasons": self.preselect.reasons.get(file_path).cloned().unwrap_or_default(),
            "stage_a_layer_scores": self.preselect.layer_scores.get(file_path).cloned().unwrap_or_default(),
        })
    }
}

impl RerankInputs {
    fn from_hints(hints: &SoftHints) -> Self {
        Self {
            boost_files: hints.working_files.iter().cloned().collect(),
            recent_files: hints.recent_files.iter().cloned().collect(),
            pinned_files: hints.pinned_files.iter().cloned().collect(),
            overlay_files: hints.overlay_files.iter().cloned().collect(),
        }
    }
}

impl<'a> LaneRanks<'a> {
    fn from_outcomes(outcomes: &'a [LaneOutcome]) -> Self {
        let mut by_lane = HashMap::with_capacity(outcomes.len());
        let mut annotating = Vec::new();
        for outcome in outcomes {
            if outcome.annotates_hits {
                annotating.push((outcome.lane_id, outcome.score_slot));
            }
            by_lane.insert(
                outcome.lane_id,
                outcome
                    .hits
                    .iter()
                    .enumerate()
                    .map(|(position, (chunk_id, _))| (chunk_id.as_str(), position + 1))
                    .collect(),
            );
        }
        Self {
            by_lane,
            annotating,
        }
    }

    /// Lanes that opted into per-hit annotation, in lane-collection order,
    /// as `(lane_id, score_slot)` pairs.
    fn annotating_lanes(&self) -> impl Iterator<Item = (&'static str, Option<ScoreSlot>)> + '_ {
        self.annotating.iter().copied()
    }

    fn rank(&self, lane_id: &str, chunk_id: &str) -> Option<usize> {
        self.by_lane
            .get(lane_id)
            .and_then(|ranks| ranks.get(chunk_id).copied())
    }
}

impl From<cc_db::index_db::ChunkDetailRow> for CandidateChunk {
    fn from(row: cc_db::index_db::ChunkDetailRow) -> Self {
        Self {
            chunk_id: row.chunk_id,
            source_evidence: row.source_evidence,
            document: row.document,
            file_path: row.file_path,
            language_name: row.language,
            start_line: row.start_line,
            end_line: row.end_line,
            breadcrumb: row.breadcrumb,
            symbol_name: row.symbol_name,
            symbol_kind: row.symbol_kind,
            text: row.text,
        }
    }
}

/// Compute the default file preselect limit based on `top_k` and the
/// repository size tier.  Larger repos get a wider multiplier so that
/// preselection covers a meaningful fraction of the codebase.
pub(crate) fn default_preselect_limit(top_k: usize, tier: Option<RepoSizeTier>) -> usize {
    let multiplier = match tier {
        Some(RepoSizeTier::Tiny) | Some(RepoSizeTier::Small) | None => 12,
        Some(RepoSizeTier::Medium) => 15,
        Some(RepoSizeTier::Large) => 20,
    };
    60usize.max(top_k * multiplier)
}

fn augmented_query_text(request: &SearchRequest) -> String {
    let mut parts = Vec::new();
    let primary = request.query.trim();
    if !primary.is_empty() {
        parts.push(primary.to_string());
    }
    if let Some(extra) = &request.conversation_queries {
        for query in extra.iter().rev().take(4) {
            let q = query.trim();
            if !q.is_empty() && !parts.iter().any(|existing| existing == q) {
                parts.push(q.to_string());
            }
        }
    }
    parts.join("\n")
}

fn dedupe_reasons(reasons: &mut Vec<String>) {
    let mut seen = HashSet::new();
    reasons.retain(|r| seen.insert(r.clone()));
}

/// Exact identity is a ranking tier, not an arbitrary tunable score bonus.
/// Within each tier the replayable numeric score and stable chunk id decide order.
pub(crate) fn compare_hits(a: &SearchHit, b: &SearchHit) -> std::cmp::Ordering {
    let exact = |h: &SearchHit| h.reasons.iter().any(|r| r == "exact-target");
    exact(b)
        .cmp(&exact(a))
        .then_with(|| b.rerank_score.total_cmp(&a.rerank_score))
        .then_with(|| a.chunk_id.cmp(&b.chunk_id))
}

pub(crate) fn parse_language_name(value: &str) -> Language {
    Language::from_name(value)
}

/// Infer a file's language from its path extension.
///
/// Mirrors how the indexer assigns languages to files
/// (`cc_parsers::detect_language` → `Language::from_extension`), for call
/// sites that only have a file path — e.g. graph-lane symbol rows, which
/// don't carry a language column.
pub(crate) fn language_from_path(file_path: &str) -> Language {
    let ext = file_path.rsplit('.').next().unwrap_or("");
    Language::from_extension(ext)
}

/// Return true if the file path looks like a project documentation file.
///
/// Public so other crates can reuse the heuristic (e.g. for role tagging).
///
/// Matches: README.md, DESIGN.md, CHANGELOG.md, CONTRIBUTING.md, docs/*.md,
/// and similar top-level or docs-directory markdown files commonly used for
/// project documentation.
pub fn is_project_doc(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    if !lower.ends_with(".md") {
        return false;
    }
    // Top-level doc files (no directory separator or single-level path)
    let segments: Vec<&str> = file_path.split('/').collect();
    if segments.len() <= 2 {
        let name = segments.last().unwrap_or(&"").to_uppercase();
        if matches!(
            name.trim_end_matches(".MD").trim_end_matches(".md"),
            "README"
                | "DESIGN"
                | "ARCHITECTURE"
                | "CHANGELOG"
                | "CONTRIBUTING"
                | "LICENSE"
                | "ADR"
                | "DECISIONS"
        ) {
            return true;
        }
    }
    // Files under docs/ or doc/ directory
    if lower.starts_with("docs/") || lower.starts_with("doc/") {
        return true;
    }
    // ADR directory pattern
    if lower.contains("/adr/") || lower.contains("/adrs/") {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preselect_scales_with_tier() {
        assert_eq!(default_preselect_limit(10, None), 120); // 60.max(10*12)
        assert_eq!(default_preselect_limit(10, Some(RepoSizeTier::Small)), 120);
        assert_eq!(default_preselect_limit(10, Some(RepoSizeTier::Medium)), 150); // 60.max(10*15)
        assert_eq!(default_preselect_limit(10, Some(RepoSizeTier::Large)), 200); // 60.max(10*20)
                                                                                 // Large top_k scenario
        assert_eq!(default_preselect_limit(20, Some(RepoSizeTier::Large)), 400); // 60.max(20*20)
                                                                                 // Tiny tier
        assert_eq!(default_preselect_limit(10, Some(RepoSizeTier::Tiny)), 120); // 60.max(10*12)
                                                                                // Small top_k should still respect the floor
        assert_eq!(default_preselect_limit(3, None), 60); // 60.max(3*12=36) => 60
    }

    /// The structured [`cc_db::ChunkScope`] handed to cc-db must carry
    /// exactly the request filters, with languages converted to their stored
    /// string form. (Scope→SQL rendering — LIKE escaping, IN placeholders,
    /// recency ordering — is pinned by cc-db's own retrieval tests.)
    #[test]
    fn materialized_filters_map_to_chunk_scope() {
        let request = SearchRequest {
            path_prefix: Some("src/%special".into()),
            languages: Some(vec![Language::Rust, Language::Python]),
            file_paths: Some(vec!["src/lib.rs".into(), "src/main.py".into()]),
            ..Default::default()
        };

        let scope = crate::scope::chunk_scope(&HardScope::from(&request));
        assert_eq!(scope.path_prefix.as_deref(), Some("src/%special"));
        assert_eq!(
            scope.languages,
            Some(vec!["rust".to_string(), "python".to_string()])
        );
        assert_eq!(
            scope.file_paths,
            Some(vec!["src/lib.rs".to_string(), "src/main.py".to_string()])
        );

        let empty = crate::scope::chunk_scope(&HardScope::default());
        assert_eq!(empty.path_prefix, None);
        assert_eq!(empty.languages, None);
        assert_eq!(empty.file_paths, None);
    }

    #[test]
    fn test_is_project_doc_top_level_files() {
        assert!(is_project_doc("README.md"));
        assert!(is_project_doc("DESIGN.md"));
        assert!(is_project_doc("CHANGELOG.md"));
        assert!(is_project_doc("CONTRIBUTING.md"));
        assert!(is_project_doc("ARCHITECTURE.md"));
        assert!(is_project_doc("readme.md"));
        assert!(is_project_doc("Readme.md"));
    }

    #[test]
    fn test_is_project_doc_docs_directory() {
        assert!(is_project_doc("docs/getting-started.md"));
        assert!(is_project_doc("docs/adr/0001-use-sqlite.md"));
        assert!(is_project_doc("doc/api.md"));
    }

    #[test]
    fn test_is_project_doc_adr_directory() {
        assert!(is_project_doc("architecture/adr/0002-rrf-fusion.md"));
        assert!(is_project_doc("decisions/adrs/0003-index-cache.md"));
    }

    #[test]
    fn test_is_project_doc_non_doc_files() {
        assert!(!is_project_doc("src/main.rs"));
        assert!(!is_project_doc("src/lib.rs"));
        assert!(!is_project_doc("tests/test_main.rs"));
        assert!(!is_project_doc("src/deep/nested/notes.md"));
        assert!(!is_project_doc("README.txt"));
    }
}
