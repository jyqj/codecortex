use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Tool enumeration (the 14 MCP tools) ─────────────────────────────

pub const MCP_TOOLS: &[&str] = &[
    "status",
    "index",
    "search",
    "context",
    "node",
    "explore",
    "trace",
    "relations",
    "impact",
    "architecture",
    "files",
    "graph_query",
    "ingest_traces",
    "adr",
];

// ── Assertion types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assertion {
    /// Kind of assertion.
    /// - `output_contains` — serialized JSON output contains string
    /// - `output_not_contains` — serialized JSON output must NOT contain string
    /// - `field_exists` — a specific JSON path exists in output
    /// - `min_results` — array at path has >= N items
    /// - `field_matches_regex` — field value matches a regex pattern
    /// - `array_contains_item` — array at field contains item with sub-field match
    /// - `expected_symbols` — gold 检索断言：`value` 为逗号分隔的期望符号名，
    ///   对 `field`（缺省为根）处的结果数组计算 Recall@5 / MRR，Recall@5 低于
    ///   阈值（`min_recall`，默认 0.7）则失败
    /// - `is_success` — tool didn't error (always implicitly checked)
    pub kind: String,

    /// Value to check against (string for contains, number-as-string for min_results).
    #[serde(default)]
    pub value: Option<String>,

    /// Optional field/path to inspect (e.g. "hits", "dead_code").
    #[serde(default)]
    pub field: Option<String>,

    /// When true, the assertion result is inverted (passes when the check would
    /// normally fail, fails when it would normally pass).
    #[serde(default)]
    pub negate: bool,

    /// `expected_symbols` only: per-case Recall@5 pass threshold (0.0–1.0).
    /// 缺省 0.7。单符号精确用例可设 1.0；多符号、排名易抖动的用例可放宽
    /// （成员性断言优于位置断言）。
    #[serde(default)]
    pub min_recall: Option<f64>,
}

impl Assertion {
    pub fn describe(&self) -> String {
        let prefix = if self.negate { "(negated) " } else { "" };
        let desc = match self.kind.as_str() {
            "output_contains" => {
                format!(
                    "output_contains: {:?}",
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            "output_not_contains" => {
                format!(
                    "output_not_contains: {:?}",
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            "field_exists" => {
                format!(
                    "field_exists: {:?}",
                    self.field.as_deref().unwrap_or("(none)")
                )
            }
            "min_results" => {
                format!(
                    "min_results(field={:?}, min={})",
                    self.field.as_deref().unwrap_or("(root)"),
                    self.value.as_deref().unwrap_or("1")
                )
            }
            "field_equals" => {
                format!(
                    "field_equals: {:?} == {:?}",
                    self.field.as_deref().unwrap_or("(none)"),
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            "field_matches_regex" => {
                format!(
                    "field_matches_regex: {:?} =~ {:?}",
                    self.field.as_deref().unwrap_or("(none)"),
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            "array_contains_item" => {
                format!(
                    "array_contains_item(field={:?}, match={:?})",
                    self.field.as_deref().unwrap_or("(root)"),
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            "expected_symbols" => {
                format!(
                    "expected_symbols(field={:?}, expected={:?}, min_recall={})",
                    self.field.as_deref().unwrap_or("(root)"),
                    self.value.as_deref().unwrap_or("(none)"),
                    self.min_recall.unwrap_or(0.7)
                )
            }
            other => format!("{}: value={:?}", other, self.value),
        };
        format!("{}{}", prefix, desc)
    }
}

// ── Eval case (from corpus TOML) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCase {
    /// Human-readable name for this test case.
    pub name: String,

    /// One of the 14 MCP tool names.
    pub tool: String,

    /// Description of what this case tests.
    #[serde(default)]
    pub description: String,

    /// Parameters to pass to the tool (as a JSON-like TOML table).
    #[serde(default)]
    pub params: serde_json::Value,

    /// Assertions to check against the tool output.
    #[serde(default)]
    pub assertions: Vec<Assertion>,

    /// When true, the case PASSES if the tool returns Err, and FAILS if it
    /// returns Ok. Useful for negative / error-path testing.
    #[serde(default)]
    pub expect_error: bool,
}

// ── Eval case result ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCaseResult {
    pub case_name: String,
    pub tool: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub output_size_bytes: usize,
    pub assertions_passed: usize,
    pub assertions_failed: Vec<String>,
    pub error: Option<String>,
    /// Fraction of expected symbols found in top 5 results.
    pub recall_at_5: Option<f64>,
    /// 1/rank of first expected symbol found, or 0 if none found.
    pub mrr: Option<f64>,
}

// ── Eval report ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub results: Vec<EvalCaseResult>,
    pub summary: EvalSummary,
    pub generated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalSummary {
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub total_duration_ms: u64,
    pub p95_duration_ms: u64,
    pub max_duration_ms: u64,
    pub total_output_bytes: usize,
    pub per_tool: HashMap<String, ToolSummary>,
    /// Average Recall@5 across cases that have expected_symbols assertions.
    pub avg_recall_at_5: Option<f64>,
    /// Average MRR across cases that have expected_symbols assertions.
    pub avg_mrr: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolSummary {
    pub case_count: usize,
    pub passed: usize,
    pub failed: usize,
    pub avg_duration_ms: u64,
    pub p95_duration_ms: u64,
    pub max_duration_ms: u64,
}
