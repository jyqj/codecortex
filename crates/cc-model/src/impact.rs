use serde::{Deserialize, Serialize};

/// Risk level for impacted symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn from_hop_depth(depth: u32) -> Self {
        match depth {
            0 => Self::Critical,
            1 => Self::High,
            2 => Self::Medium,
            _ => Self::Low,
        }
    }
}

/// A symbol impacted by a code change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactedSymbol {
    pub symbol_uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub risk_level: RiskLevel,
    pub hop_depth: u32,
    pub community_id: Option<u32>,
    pub confidence: f64,
}

/// A cross-service impact discovered via HTTP call edges.
/// Advisory — not deterministic like call-graph BFS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossServiceImpact {
    pub caller_symbol_uid: String,
    pub caller_name: String,
    pub caller_file: String,
    pub route_path: String,
    pub method: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub handler_name: Option<String>,
    pub handler_file: Option<String>,
    pub confidence: f64, // 0.55-0.75
    pub source: String,  // "http_call_reverse"
}

/// A file historically co-changed with the modified files.
/// Advisory — based on git history, not structural dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalImpact {
    pub file_path: String,
    pub co_change_count: u32,
    pub confidence: f64, // Jaccard similarity
    pub source: String,  // "co_change"
}

/// Complete impact analysis report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactReport {
    pub changed_files: Vec<String>,
    pub impacted_symbols: Vec<ImpactedSymbol>,
    pub suggested_tests: Vec<String>,
    pub boundary_crossings: Vec<BoundaryCrossing>,
    pub risk_summary: RiskSummary,
    pub confidence_weighted_risk: f64,
    pub cross_service_impacts: Vec<CrossServiceImpact>,
    pub historical_impacts: Vec<HistoricalImpact>,
    /// True when the BFS stopped early (max_nodes / max_per_layer) or the
    /// returned `impacted_symbols` were clipped to a result limit. Signals that
    /// the blast radius is NOT fully covered — AI agents must not assume the
    /// returned set is exhaustive.
    pub truncated: bool,
    /// Number of impacted symbols actually returned in `impacted_symbols`.
    pub returned_symbol_count: usize,
    /// Number of distinct impacted symbols discovered by the BFS before any
    /// result-limit clipping. When `truncated` is true due to a BFS cap, this
    /// is a lower bound on the true blast radius.
    pub total_impacted_discovered: usize,
}

/// A community boundary crossing detected during impact analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCrossing {
    pub from_community: u32,
    pub to_community: u32,
    pub edge_symbol: String,
    pub edge_file: String,
}

/// Summary of risk distribution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskSummary {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub total_impacted: usize,
    pub risk: String,
    pub boundary_crossing_count: usize,
    pub suggested_test_count: usize,
    pub cross_service_count: usize,
    pub historical_count: usize,
}
