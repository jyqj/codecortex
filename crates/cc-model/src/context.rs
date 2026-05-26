use serde::{Deserialize, Serialize};

use crate::Intent;

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
    WorkingSet,
}

impl Role {
    /// Map role to budget bucket name.
    pub fn bucket(&self) -> &'static str {
        match self {
            Self::DirtyBuffer | Self::Primary | Self::EditTarget => "primary",
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
            Self::WorkingSet => "working",
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
            Self::WorkingSet,
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
}
