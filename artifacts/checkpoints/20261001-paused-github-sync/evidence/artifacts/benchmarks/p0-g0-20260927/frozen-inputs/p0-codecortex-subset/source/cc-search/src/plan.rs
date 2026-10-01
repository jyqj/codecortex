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
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcResult, Language};

use crate::lanes::{FusedScore, LaneOutcome, ScoreSlot};
use crate::preselect::{PreselectRequest, PreselectResult};
use crate::score_trace::ScoreTrace;

#[derive(Debug)]
pub(crate) struct SearchPlan {
    request: SearchRequest,
    dsl: crate::dsl::ParsedQuery,
    expanded_query: String,
    query_tokens: Vec<String>,
    limits: LaneLimits,
    filters: MaterializedFilters,
    preselect: PreselectResult,
    rerank_inputs: RerankInputs,
    ranking: RankingConfig,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneLimits {
    pub top_k: usize,
    pub lexical: usize,
    pub grep: usize,
    pub rerank_window: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MaterializedFilters {
    path_prefix: Option<String>,
    languages: Option<Vec<Language>>,
    file_paths: Option<Vec<String>>,
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
    pub(crate) fn build(
        db: &IndexDb,
        config: &SearchConfig,
        ranking: &RankingConfig,
        request: &SearchRequest,
        repo_tier: Option<RepoSizeTier>,
    ) -> CcResult<Self> {
        let dsl = crate::dsl::parse_search_dsl(&request.query);
        let mut request = request.clone();

        normalize_request_from_dsl(&mut request, &dsl);

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
        if !preselect.files.is_empty() && request.file_paths.is_none() {
            request.file_paths = Some(preselect.files.clone());
        }

        let filters = MaterializedFilters::from_request(&request);
        let rerank_inputs = RerankInputs::from_request(&request);
        let query_tokens = tokenize_codeish(&query_text);
        let limits = LaneLimits {
            top_k,
            lexical: config.lexical_top_k.max(top_k),
            grep: config.grep_top_k.max(top_k),
            rerank_window: config.rerank_window.max(top_k),
        };

        Ok(Self {
            request,
            dsl,
            expanded_query,
            query_tokens,
            limits,
            filters,
            preselect,
            rerank_inputs,
            ranking: ranking.clone(),
        })
    }

    pub(crate) fn ranking(&self) -> &RankingConfig {
        &self.ranking
    }

    pub(crate) fn request(&self) -> &SearchRequest {
        &self.request
    }

    pub(crate) fn lexical_query(&self) -> &str {
        &self.expanded_query
    }

    pub(crate) fn grep_query(&self) -> &str {
        &self.request.query
    }

    pub(crate) fn query_tokens(&self) -> &[String] {
        &self.query_tokens
    }

    pub(crate) fn limits(&self) -> LaneLimits {
        self.limits
    }

    pub(crate) fn passes_filters(&self, file_path: &str, language: Language) -> bool {
        self.filters.passes(file_path, language)
    }

    /// Structured chunk-scan scope for the lexical/grep lanes; the scope SQL
    /// itself is owned by cc-db (`RetrievalReadModel::fts_chunk_candidates` /
    /// `scan_chunks_for_grep`).
    pub(crate) fn chunk_scope(&self) -> cc_db::ChunkScope {
        self.filters.chunk_scope()
    }

    /// Whether the request carries an explicit file-paths scope (which
    /// bounds the grep scan's cardinality).
    pub(crate) fn has_file_scope(&self) -> bool {
        self.filters.has_file_scope()
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

        if let Some(prefix) = self.filters.path_prefix() {
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
        let metadata = self.rerank_metadata(&file_path, stage_a_score);

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

        results.sort_by(|a, b| {
            b.rerank_score
                .partial_cmp(&a.rerank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Deterministic tie-break on chunk id so equal rerank scores
                // (or NaN, which compares Equal) yield a stable order instead
                // of depending on prior fusion/HashMap ordering.
                .then_with(|| a.chunk_id.cmp(&b.chunk_id))
        });
        results.truncate(limit);
    }

    fn rerank_metadata(&self, file_path: &str, stage_a_score: f64) -> serde_json::Value {
        serde_json::json!({
            "stage_a_file_score": stage_a_score,
            "stage_a_files_considered": self.preselect.files.len(),
            "stage_a_file_reasons": self.preselect.reasons.get(file_path).cloned().unwrap_or_default(),
            "stage_a_layer_scores": self.preselect.layer_scores.get(file_path).cloned().unwrap_or_default(),
        })
    }
}

impl MaterializedFilters {
    pub(crate) fn from_request(request: &SearchRequest) -> Self {
        Self {
            path_prefix: request.path_prefix.clone(),
            languages: request.languages.clone(),
            file_paths: request.file_paths.clone(),
        }
    }

    pub(crate) fn passes(&self, file_path: &str, language: Language) -> bool {
        if let Some(prefix) = &self.path_prefix {
            if !file_path.starts_with(prefix) {
                return false;
            }
        }
        if let Some(languages) = &self.languages {
            if !languages.contains(&language) {
                return false;
            }
        }
        if let Some(files) = &self.file_paths {
            if !files.iter().any(|file| file == file_path) {
                return false;
            }
        }
        true
    }

    /// Structured scope for cc-db's chunk-scan queries: same filters the
    /// in-memory `passes` check applies, expressed as data instead of SQL.
    /// Language filters are converted to their stored string form
    /// (`Language::as_str()`).
    pub(crate) fn chunk_scope(&self) -> cc_db::ChunkScope {
        cc_db::ChunkScope {
            path_prefix: self.path_prefix.clone(),
            languages: self
                .languages
                .as_ref()
                .map(|langs| langs.iter().map(|lang| lang.as_str().to_string()).collect()),
            file_paths: self.file_paths.clone(),
        }
    }

    /// Whether the request carries an explicit file-paths scope (which
    /// bounds the grep scan's cardinality).
    pub(crate) fn has_file_scope(&self) -> bool {
        self.file_paths
            .as_ref()
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    }

    fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }
}

impl RerankInputs {
    fn from_request(request: &SearchRequest) -> Self {
        Self {
            boost_files: collect_paths(request.boost_file_paths.as_ref()),
            recent_files: collect_paths(request.recent_file_paths.as_ref()),
            pinned_files: collect_paths(request.pinned_file_paths.as_ref()),
            overlay_files: collect_paths(request.overlay_file_paths.as_ref()),
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

fn normalize_request_from_dsl(request: &mut SearchRequest, dsl: &crate::dsl::ParsedQuery) {
    if dsl.path_filter.is_some() && request.path_prefix.is_none() {
        request.path_prefix = dsl.path_filter.clone();
    }

    if let Some(ref lang_str) = dsl.lang_filter {
        if request.languages.is_none() {
            let lang = Language::from_name(lang_str);
            if lang != Language::Unknown {
                request.languages = Some(vec![lang]);
            }
        }
    }

    if !dsl.text.is_empty() {
        request.query = dsl.text.clone();
    }
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

fn collect_paths(paths: Option<&Vec<String>>) -> HashSet<String> {
    paths
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default()
}

fn dedupe_reasons(reasons: &mut Vec<String>) {
    let mut seen = HashSet::new();
    reasons.retain(|r| seen.insert(r.clone()));
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

        let scope = MaterializedFilters::from_request(&request).chunk_scope();
        assert_eq!(scope.path_prefix.as_deref(), Some("src/%special"));
        assert_eq!(
            scope.languages,
            Some(vec!["rust".to_string(), "python".to_string()])
        );
        assert_eq!(
            scope.file_paths,
            Some(vec!["src/lib.rs".to_string(), "src/main.py".to_string()])
        );

        let empty = MaterializedFilters::default().chunk_scope();
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
