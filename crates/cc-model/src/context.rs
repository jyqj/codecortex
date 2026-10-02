use serde::{Deserialize, Serialize};

pub const CONTEXT_PACKING_SPEC: &str =
    "whole-json-intent-facets-source-support-before-incidental-v5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidencePriority {
    IntentFacet,
    DistinctiveSourceSupport,
}

use crate::Intent;

/// Bounded query-level explanation. Does not enumerate excluded index inventory
/// or raw soft paths; per-hit numeric contributions remain on the returned hits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchScopeExplain {
    pub schema_version: u32,
    pub policy: String,
    pub hard: SearchHardScopeExplain,
    pub soft: SearchSoftScopeExplain,
    pub budget: SearchBudgetExplain,
    pub ordering: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHardScopeExplain {
    pub path_prefix: Option<String>,
    pub path_prefix_truncated: bool,
    pub languages: Option<Vec<crate::Language>>,
    pub explicit_file_count: Option<usize>,
    pub empty: bool,
    pub semantics: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchSoftScopeExplain {
    pub role: String,
    /// Counts of normalized requested hints, not counts of indexed/authorized files.
    pub hint_entries: std::collections::BTreeMap<String, usize>,
    pub preselected_count: usize,
    pub contributions: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchBudgetExplain {
    pub top_k: usize,
    #[serde(default)]
    pub exact_symbol_candidates: usize,
    #[serde(default)]
    pub path_candidates: usize,
    pub lexical_candidates: usize,
    pub grep_candidates: usize,
    pub grep_scan_cap: usize,
    pub rerank_window: usize,
    pub grep_enabled: bool,
    pub units: String,
}

/// Per-lane coverage explanation projected verbatim from a versioned lane
/// receipt (`cc_model::retrieval::LaneOutcome`, P7-012 partial-coverage
/// semantics). The receipt is the single source of truth: this projection
/// never folds `Partial`, `Timeout`, `Unavailable`, or `Error` into a
/// complete story and never drops a `truncation_reason`, so "executed, zero
/// candidates" (`Complete` + `candidate_count == 0`) and "did not finish"
/// (`Timeout`/`Unavailable` + reason) stay distinguishable downstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaneCoverageExplain {
    pub lane_id: String,
    pub status: crate::retrieval::LaneStatus,
    pub candidate_count: usize,
    pub truncation_reason: Option<String>,
    pub coverage: crate::retrieval::LaneCoverage,
}

impl LaneCoverageExplain {
    /// Project one receipt verbatim; all eight `LaneStatus` values pass
    /// through unmodified, none is collapsed into another.
    pub fn from_lane_outcome(outcome: &crate::retrieval::LaneOutcome) -> Self {
        Self {
            lane_id: outcome.lane_id.clone(),
            status: outcome.status,
            candidate_count: outcome.candidate_count,
            truncation_reason: outcome.truncation_reason.clone(),
            coverage: outcome.coverage.clone(),
        }
    }

    /// The dense lane's receipt, if this query carried one at all. `None`
    /// means the query had no semantic lane (local strategy or unwired
    /// port): nothing may then be claimed about semantic coverage, and the
    /// fused result rests on the local lanes alone.
    pub fn semantic_from_outcomes(outcomes: &[crate::retrieval::LaneOutcome]) -> Option<Self> {
        outcomes
            .iter()
            .find(|outcome| outcome.lane_id == "semantic")
            .map(Self::from_lane_outcome)
    }
}

/// Context node type — the kind of code-index evidence in a context envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NodeType {
    FileSlice,
    SearchHit,
    SymbolDef,
    SymbolRef,
    SymbolCluster,
    CallEdge,
    TestEdge,
    RouteEdge,
    HttpCallEdge,
    ImportEdge,
    ReverseImportEdge,
    IndexDiagnostic,
    Diagnostic,
    LiteralHit,
    EditTarget,
    RiskRegion,
    CommunityBoundary,
    FrameworkHint,
}

/// Role determines the bucket a node is packed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    DirtyBuffer,
    Primary,
    RecentEdit,
    Neighbor,
    Import,
    ReverseImport,
    Test,
    Diagnostic,
    IndexDiagnostic,
    SymbolDef,
    SymbolRef,
    SymbolCluster,
    CallEdge,
    RouteEdge,
    HttpCall,
    CommunityBoundary,
    WhyIncluded,
    EditTarget,
    RiskRegion,
    DocContext,
}

impl Role {
    /// Map role to budget bucket name.
    pub fn bucket(&self) -> &'static str {
        match self {
            Self::DirtyBuffer | Self::Primary | Self::EditTarget | Self::DocContext => "primary",
            Self::Neighbor => "neighbors",
            Self::Import
            | Self::ReverseImport
            | Self::Test
            | Self::SymbolDef
            | Self::SymbolRef
            | Self::SymbolCluster
            | Self::CallEdge
            | Self::RouteEdge
            | Self::HttpCall
            | Self::CommunityBoundary => "graph",
            Self::RecentEdit => "recent",
            Self::Diagnostic | Self::IndexDiagnostic | Self::RiskRegion | Self::WhyIncluded => {
                "diagnostics"
            }
        }
    }

    /// Ordered list of roles for rendering.
    pub fn render_order() -> &'static [Role] {
        &[
            Self::DirtyBuffer,
            Self::Primary,
            Self::RecentEdit,
            Self::Neighbor,
            Self::Import,
            Self::ReverseImport,
            Self::Test,
            Self::Diagnostic,
            Self::IndexDiagnostic,
            Self::SymbolDef,
            Self::SymbolRef,
            Self::SymbolCluster,
            Self::CallEdge,
            Self::RouteEdge,
            Self::HttpCall,
            Self::CommunityBoundary,
            Self::EditTarget,
            Self::RiskRegion,
            Self::WhyIncluded,
            Self::DocContext,
        ]
    }
}

/// A single node in the context evidence graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub node_id: String,
    pub node_type: NodeType,
    pub role: Role,
    pub title: String,
    pub text: String,
    // Location
    pub file_path: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    // Scoring
    pub score: f64,
    pub confidence: f64,
    pub freshness: Option<f64>,
    pub token_estimate: u32,
    // Provenance
    pub source: String,
    pub reasons: Vec<String>,
    pub invalidation_keys: Vec<String>,
    pub rank_explanation: Option<String>,
    // Relations
    pub relation: Option<String>,
    pub depends_on: Vec<String>,
    // Metadata (flexible JSON for domain-specific data)
    pub metadata: serde_json::Value,
    // Source span — for reading actual content from disk
    pub span_kind: Option<String>,
    pub backing_file_path: Option<String>,
    pub backing_source: Option<String>,
    pub source_start_line: Option<u32>,
    pub source_end_line: Option<u32>,
}

impl ContextNode {
    pub fn new(
        node_id: String,
        node_type: NodeType,
        role: Role,
        title: String,
        text: String,
    ) -> Self {
        let token_estimate = crate::approx_tokens(&text);
        Self {
            node_id,
            node_type,
            role,
            title,
            text,
            file_path: None,
            start_line: None,
            end_line: None,
            score: 0.0,
            confidence: 0.5,
            freshness: None,
            token_estimate,
            source: String::new(),
            reasons: Vec::new(),
            invalidation_keys: Vec::new(),
            rank_explanation: None,
            relation: None,
            depends_on: Vec::new(),
            metadata: serde_json::Value::Null,
            span_kind: None,
            backing_file_path: None,
            backing_source: None,
            source_start_line: None,
            source_end_line: None,
        }
    }
}

/// A code span reference within a context envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSpan {
    pub node_id: String,
    pub file_path: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub label: String,
}

/// The complete context packaging result for code-index search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEnvelope {
    pub task: String,
    pub intent: Intent,
    pub query: String,
    pub token_budget: u32,
    pub token_estimate: u32,
    pub summary: String,
    pub rendered_prompt: String,
    pub revision: u32,
    pub nodes: Vec<ContextNode>,
    pub spans: Vec<ContextSpan>,
    pub reasons: Vec<String>,
    pub invalidations: Vec<String>,
    pub machine_pack: serde_json::Value,
    pub evidence_summary: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::{
        LaneCoverage, LaneOutcome, LaneStatus, LANE_OUTCOME_SCHEMA_VERSION,
    };

    fn receipt(lane_id: &str, status: LaneStatus, reason: Option<&str>, count: usize) -> LaneOutcome {
        LaneOutcome {
            schema_version: LANE_OUTCOME_SCHEMA_VERSION,
            lane_id: lane_id.into(),
            weight: 1.0,
            status,
            elapsed_us: 0,
            candidate_count: count,
            coverage: match status {
                LaneStatus::Complete => LaneCoverage::complete(None, count),
                LaneStatus::Partial => LaneCoverage::partial(None, count),
                _ => LaneCoverage::not_run(),
            },
            truncation_reason: reason.map(Into::into),
            candidates: Vec::new(),
        }
    }

    #[test]
    fn node_type_serde_round_trip() {
        let all_variants = [
            NodeType::FileSlice,
            NodeType::SearchHit,
            NodeType::SymbolDef,
            NodeType::SymbolRef,
            NodeType::SymbolCluster,
            NodeType::CallEdge,
            NodeType::TestEdge,
            NodeType::RouteEdge,
            NodeType::HttpCallEdge,
            NodeType::ImportEdge,
            NodeType::ReverseImportEdge,
            NodeType::IndexDiagnostic,
            NodeType::Diagnostic,
            NodeType::LiteralHit,
            NodeType::EditTarget,
            NodeType::RiskRegion,
            NodeType::CommunityBoundary,
            NodeType::FrameworkHint,
        ];
        for variant in &all_variants {
            let json = serde_json::to_string(variant).unwrap();
            let back: NodeType = serde_json::from_str(&json).unwrap();
            assert_eq!(*variant, back);
        }
    }

    #[test]
    fn role_buckets_are_non_empty() {
        for role in Role::render_order() {
            assert!(!role.bucket().is_empty());
        }
    }

    #[test]
    fn context_node_new_sets_token_estimate() {
        let node = ContextNode::new(
            "n1".into(),
            NodeType::SearchHit,
            Role::Primary,
            "title".into(),
            "one two three four".into(),
        );
        assert!(node.token_estimate > 0);
    }

    #[test]
    fn coverage_explain_projects_the_receipt_verbatim() {
        let ran_empty = receipt("semantic", LaneStatus::Complete, None, 0);
        let ran_empty = LaneCoverageExplain::from_lane_outcome(&ran_empty);
        assert_eq!(ran_empty.status, LaneStatus::Complete);
        assert_eq!(ran_empty.truncation_reason, None);
        assert_eq!(ran_empty.candidate_count, 0);
        assert!(ran_empty.coverage.complete);

        let timed_out = receipt("semantic", LaneStatus::Timeout, Some("semantic_deadline"), 0);
        let timed_out = LaneCoverageExplain::from_lane_outcome(&timed_out);
        assert_eq!(timed_out.status, LaneStatus::Timeout);
        assert_eq!(timed_out.truncation_reason.as_deref(), Some("semantic_deadline"));
        assert_eq!(timed_out.candidate_count, 0);
        assert!(!timed_out.coverage.complete);

        let partial = receipt(
            "semantic",
            LaneStatus::Partial,
            Some("semantic_coverage_uncovered"),
            3,
        );
        let partial = LaneCoverageExplain::from_lane_outcome(&partial);
        assert_eq!(partial.status, LaneStatus::Partial);
        assert_eq!(
            partial.truncation_reason.as_deref(),
            Some("semantic_coverage_uncovered")
        );
        assert_eq!(partial.candidate_count, 3);
        assert!(!partial.coverage.complete);

        let disabled = LaneOutcome::disabled("semantic", 1.0);
        let disabled = LaneCoverageExplain::from_lane_outcome(&disabled);
        assert_eq!(disabled.status, LaneStatus::Disabled);
        assert_eq!(disabled.truncation_reason, None);
        assert!(!disabled.coverage.complete);
    }

    #[test]
    fn semantic_explain_is_absent_exactly_when_no_semantic_lane_ran() {
        let lanes = vec![receipt("lexical", LaneStatus::Complete, None, 1)];
        let mut with_semantic = lanes.clone();
        with_semantic.push(receipt(
            "semantic",
            LaneStatus::Partial,
            Some("semantic_coverage_uncovered"),
            0,
        ));
        let projected = LaneCoverageExplain::semantic_from_outcomes(&with_semantic).unwrap();
        assert_eq!(projected.lane_id, "semantic");
        assert_eq!(
            projected.truncation_reason.as_deref(),
            Some("semantic_coverage_uncovered")
        );
        // A purely local query carries no semantic claim at all.
        assert!(LaneCoverageExplain::semantic_from_outcomes(&lanes).is_none());
    }
}
