//! Shared data types for graph query tools (trace_path, explore_flow, find_circular_deps, type_hierarchy).

use serde::{Deserialize, Serialize};

pub use cc_db::index_db::EdgeLiteBfs as EdgeLite;

/// A node in a trace path, carrying full metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceNode {
    pub uid: String,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing_calls: Option<Vec<String>>,
}

/// An edge in a trace path with full dispatch/synthesis metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEdge {
    pub from_uid: String,
    pub to_uid: String,
    pub dispatch_kind: String,
    pub synthesized_by: Option<String>,
    pub synthesis_key: Option<String>,
    pub confidence: f64,
    pub file_path: String,
    pub line: u32,
    pub registered_file: Option<String>,
    pub registered_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parser_confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<TraceEdgeEvidence>,
}

/// Runtime evidence attached to a trace edge (from runtime_evidence table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEdgeEvidence {
    pub observed_count: u32,
    pub last_seen: String,
}

/// A candidate symbol surfaced during disambiguation.
#[derive(Debug, Clone, Serialize)]
pub struct DisambiguationCandidate {
    pub uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub start_line: u32,
}

/// Disambiguation information when a query matched multiple symbols.
#[derive(Debug, Clone, Serialize)]
pub struct DisambiguationInfo {
    /// "from" or "to"
    pub role: String,
    /// Original query string
    pub query: String,
    /// The UID that was used
    pub chosen_uid: String,
    /// File path of chosen symbol
    pub chosen_file: String,
    /// All candidate symbols (including the chosen one)
    pub candidates: Vec<DisambiguationCandidate>,
}

/// Rich trace_path result with backward-compatible paths + rich nodes/edges.
#[derive(Debug, Clone, Serialize)]
pub struct TracePathResult {
    pub paths: Vec<Vec<String>>,
    pub nodes: Vec<TraceNode>,
    pub edges: Vec<TraceEdge>,
    pub path_count: usize,
    /// Non-empty when a `from` or `to` query matched multiple symbols.
    /// The first candidate was used; pass `from_uid` / `to_uid` to override.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub disambiguation: Vec<DisambiguationInfo>,
    /// Diagnostic hint when no path is found (e.g. reasons + suggested next action).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    /// Unified explainability envelope (additive): which budget clipped the
    /// walk ("max_depth"/"max_paths"/"max_expansions"), counts of synthetic
    /// vs runtime-evidence edges actually traversed, and any DB read errors
    /// degraded to partial results. Absent when there is nothing to report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_explain: Option<cc_model::GraphExplain>,
}

/// A single labeled path from BFS, carrying both node UIDs and edge data.
#[derive(Debug, Clone)]
pub struct LabeledPath {
    pub node_uids: Vec<String>,
    pub edge_lites: Vec<cc_db::index_db::EdgeLiteBfs>,
}

/// Adjacency map for edge-labeled BFS.
#[derive(Debug)]
pub struct BfsAdj {
    pub adj: std::collections::HashMap<String, Vec<cc_db::index_db::EdgeLiteBfs>>,
}

/// A discovered flow path between two named symbols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowPath {
    pub from_symbol: String,
    pub to_symbol: String,
    pub path_names: Vec<String>,
    pub path_uids: Vec<String>,
    pub length: usize,
}

/// Result of explore_flow: paths + three-state symbol resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowResult {
    pub flow_paths: Vec<FlowPath>,
    pub flow_nodes: Vec<TraceNode>,
    pub flow_edges: Vec<TraceEdge>,
    pub unresolved: Vec<UnresolvedSymbol>,
    pub ambiguous: Vec<AmbiguousSymbol>,
    pub disconnected: Vec<DisconnectedSymbol>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedSymbol {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbiguousSymbol {
    pub query: String,
    pub candidates: Vec<SymbolBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectedSymbol {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub uid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolBrief {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub uid: String,
}

/// A strongly connected component from Tarjan SCC.
#[derive(Debug, Clone, Serialize)]
pub struct CycleComponent {
    pub nodes: Vec<String>,
    pub size: usize,
    pub severity: String,
    pub internal_edges: Vec<InternalEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InternalEdge {
    pub from: String,
    pub to: String,
    pub import: Option<String>,
    pub line: Option<u32>,
}

/// Result of find_circular_deps.
#[derive(Debug, Clone, Serialize)]
pub struct CircularDepsResult {
    pub granularity: String,
    pub components: Vec<CycleComponent>,
    pub total_components: usize,
    pub shown: usize,
    /// Additive contract metadata: the tool's declared graph subset
    /// (`tool_graph_subsets::CYCLES`). cycles has no dynamic collector, so
    /// the envelope carries only `declared_edge_kinds`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_explain: Option<cc_model::GraphExplain>,
}

/// Result of type_hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeHierarchyResult {
    pub type_info: TypeNodeInfo,
    pub ancestors: Vec<HierarchyNode>,
    pub descendants: Vec<HierarchyNode>,
    pub implementors: Vec<HierarchyNode>,
    pub overrides: Vec<OverrideInfo>,
    /// Additive explain envelope: the declared graph subset
    /// (`tool_graph_subsets::TYPE_HIERARCHY`) plus any semantic-edge load
    /// failure recorded as a `read_errors` entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_explain: Option<cc_model::GraphExplain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeNodeInfo {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub uid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HierarchyNode {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub uid: String,
    pub relation: String,
    pub depth: usize,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideInfo {
    pub method: String,
    pub overriding_type: String,
    pub base_type: String,
    pub param_count: Option<u32>,
}
