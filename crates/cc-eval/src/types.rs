use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EvalCategory {
    RouteHandler,
    ImpactRadius,
    DynamicCallback,
    TypeHierarchy,
    CircularDeps,
    DeadCode,
}

impl EvalCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RouteHandler => "route_handler",
            Self::ImpactRadius => "impact_radius",
            Self::DynamicCallback => "dynamic_callback",
            Self::TypeHierarchy => "type_hierarchy",
            Self::CircularDeps => "circular_deps",
            Self::DeadCode => "dead_code",
        }
    }
}

impl Default for EvalCategory {
    fn default() -> Self {
        Self::RouteHandler
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalDifficulty {
    Easy,
    Medium,
    Hard,
}

impl Default for EvalDifficulty {
    fn default() -> Self {
        Self::Medium
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    pub name: String,
    #[serde(default)]
    pub category: EvalCategory,
    #[serde(default)]
    pub difficulty: EvalDifficulty,
    pub prompt: String,
    pub repo_path: PathBuf,
    pub expected_markers: Vec<String>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(with = "duration_secs")]
    pub timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRun {
    pub case_name: String,
    pub with_codecortex: bool,
    pub metrics: EvalMetrics,
    pub tool_calls: Vec<ToolCallRecord>,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalMetrics {
    pub wall_clock_ms: u64,
    pub token_count_input: u64,
    pub token_count_output: u64,
    pub tool_call_count: u32,
    pub file_read_count: u32,
    pub grep_count: u32,
    pub mcp_call_count: u32,
    pub estimated_cost_usd: f64,
    pub markers_found: u32,
    pub markers_total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub duration_ms: u64,
    pub input_summary: String,
    pub output_size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCaseReport {
    pub case_name: String,
    #[serde(default)]
    pub category: EvalCategory,
    pub with_cc: EvalRun,
    pub without_cc: EvalRun,
    pub delta: EvalDelta,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalDelta {
    pub wall_clock_reduction_pct: f64,
    pub token_reduction_pct: f64,
    pub tool_call_reduction_pct: f64,
    pub cost_reduction_pct: f64,
    pub file_read_reduction_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub cases: Vec<EvalCaseReport>,
    pub summary: EvalSummary,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total_cases: usize,
    pub avg_wall_clock_reduction_pct: f64,
    pub avg_token_reduction_pct: f64,
    pub avg_tool_call_reduction_pct: f64,
    pub avg_cost_reduction_pct: f64,
    #[serde(default)]
    pub per_category: HashMap<String, CategorySummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategorySummary {
    pub case_count: usize,
    pub avg_wall_clock_reduction_pct: f64,
    pub avg_token_reduction_pct: f64,
    pub success_rate: f64,
}

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_secs().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(d)?;
        Ok(Duration::from_secs(secs))
    }
}
