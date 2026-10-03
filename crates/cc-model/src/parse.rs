use crate::{
    chunk::ChunkRecord,
    diagnostic::{DiagnosticRecord, LiteralRecord},
    dispatch_site::DispatchSiteRecord,
    edge::{
        CallEdgeRecord, DataFlowEdgeRecord, HttpCallEdgeRecord, ImportRecord, RouteEdgeRecord,
        SemanticEdgeRecord, TestEdgeRecord,
    },
    symbol::{SymbolRecord, SymbolRefRecord},
    type_assign::TypeAssignRecord,
    ParserTier,
};
use serde::{Deserialize, Serialize};

/// The complete output of parsing a single source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseOutcome {
    /// Proven declaration associations prepared against the original parser snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_identities: Vec<crate::symbol_identity::ChunkSymbolIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_spec: Option<String>,
    /// Prepared document projections; None for raw parser/synthetic/dirty-only units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documents: Option<crate::identity::DocumentBatch>,
    /// Policy that actually produced these chunks; null for synthetic fixtures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_policy: Option<String>,
    /// Owned coordinates copied from the existing tree; no tree or source clone is retained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_structure: Option<crate::source::SourceStructure>,
    #[serde(default)]
    pub resolution: crate::resolution::ResolutionManifest,
    /// Missing legacy/parser evidence is unknown, never a known empty API.
    #[serde(default)]
    pub public_surface: crate::public_surface::PublicSurface,
    pub summary: String,
    pub chunks: Vec<ChunkRecord>,
    pub symbols: Vec<SymbolRecord>,
    pub imports: Vec<ImportRecord>,
    pub symbol_refs: Vec<SymbolRefRecord>,
    pub call_edges: Vec<CallEdgeRecord>,
    pub test_edges: Vec<TestEdgeRecord>,
    pub route_edges: Vec<RouteEdgeRecord>,
    pub http_call_edges: Vec<HttpCallEdgeRecord>,
    pub semantic_edges: Vec<SemanticEdgeRecord>,
    pub data_flow_edges: Vec<DataFlowEdgeRecord>,
    pub diagnostics: Vec<DiagnosticRecord>,
    pub literal_index: Vec<LiteralRecord>,
    pub dispatch_sites: Vec<DispatchSiteRecord>,
    pub type_assigns: Vec<TypeAssignRecord>,
    pub parser_tier: ParserTier,
    pub parser_confidence: f64,
    pub is_test_file: bool,
}

impl Default for ParseOutcome {
    fn default() -> Self {
        Self {
            symbol_identities: Vec::new(),
            document_spec: None,
            documents: None,
            chunk_policy: None,
            source_structure: None,
            resolution: crate::resolution::ResolutionManifest::default(),
            public_surface: crate::public_surface::PublicSurface::default(),
            summary: String::new(),
            chunks: Vec::new(),
            symbols: Vec::new(),
            imports: Vec::new(),
            symbol_refs: Vec::new(),
            call_edges: Vec::new(),
            test_edges: Vec::new(),
            route_edges: Vec::new(),
            http_call_edges: Vec::new(),
            semantic_edges: Vec::new(),
            data_flow_edges: Vec::new(),
            diagnostics: Vec::new(),
            literal_index: Vec::new(),
            dispatch_sites: Vec::new(),
            type_assigns: Vec::new(),
            parser_tier: ParserTier::Generic,
            parser_confidence: 0.3,
            is_test_file: false,
        }
    }
}
