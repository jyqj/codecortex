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

impl ArchitectureInfo {
    /// 生成文本摘要（用于报告与展示）
    pub fn to_summary_text(&self) -> String {
        let mut lines = Vec::new();

        if !self.languages.is_empty() {
            lines.push("## Languages".to_string());
            for lang in &self.languages {
                lines.push(format!(
                    "- {}: {} files ({:.1}%)",
                    lang.language, lang.file_count, lang.percentage
                ));
            }
        }

        if !self.packages.is_empty() {
            lines.push("\n## Packages".to_string());
            for pkg in &self.packages {
                lines.push(format!(
                    "- {}: {} files, {} symbols (in:{}, out:{})",
                    pkg.name, pkg.file_count, pkg.symbol_count, pkg.fan_in, pkg.fan_out
                ));
            }
        }

        if !self.hotspots.is_empty() {
            lines.push("\n## Hotspots (high fan-in)".to_string());
            for h in &self.hotspots {
                lines.push(format!(
                    "- {} ({}) in {} — fan_in: {}",
                    h.name, h.kind, h.file_path, h.fan_in
                ));
            }
        }

        if !self.boundaries.is_empty() {
            lines.push("\n## Cross-package boundaries".to_string());
            for b in &self.boundaries {
                lines.push(format!(
                    "- {} → {}: {} calls",
                    b.source_package, b.target_package, b.call_count
                ));
            }
        }

        lines.join("\n")
    }
}
