use serde::{Deserialize, Serialize};

/// 项目架构分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureInfo {
    pub languages: Vec<LanguageStat>,
    pub packages: Vec<PackageInfo>,
    pub entry_points: Vec<EntryPointInfo>,
    pub routes: Vec<RouteInfo>,
    pub hotspots: Vec<HotspotInfo>,
    pub boundaries: Vec<BoundaryInfo>,
    pub communities: Vec<CommunityInfo>,
    pub layers: Vec<LayerInfo>,
    pub adr_documents: Vec<AdrDocInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStat {
    pub language: String,
    pub file_count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub file_count: usize,
    pub symbol_count: usize,
    pub fan_in: usize,
    pub fan_out: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPointInfo {
    pub name: String,
    pub file_path: String,
    pub kind: String, // "main", "handler", "route", "test_suite"
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    pub method: String,
    pub path: String,
    pub handler: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotInfo {
    pub name: String,
    pub file_path: String,
    pub fan_in: usize,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryInfo {
    pub source_package: String,
    pub target_package: String,
    pub call_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityInfo {
    pub id: i64,
    pub label: String,
    pub member_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerInfo {
    pub package: String,
    pub layer: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdrDocInfo {
    pub file: String,
    pub title: String,
    pub status: Option<String>,
    pub date: Option<String>,
}
