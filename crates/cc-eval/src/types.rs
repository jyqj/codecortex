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
    /// - `field_exists` — a specific JSON path exists in output
    /// - `min_results` — array at path has >= N items
    /// - `is_success` — tool didn't error (always implicitly checked)
    pub kind: String,

    /// Value to check against (string for contains, number-as-string for min_results).
    #[serde(default)]
    pub value: Option<String>,

    /// Optional field/path to inspect (e.g. "hits", "dead_code").
    #[serde(default)]
    pub field: Option<String>,
}

impl Assertion {
    pub fn describe(&self) -> String {
        match self.kind.as_str() {
            "output_contains" => {
                format!(
                    "output_contains: {:?}",
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
            "expected_symbols" => {
                format!(
                    "expected_symbols(field={:?}, expected={:?})",
                    self.field.as_deref().unwrap_or("(root)"),
                    self.value.as_deref().unwrap_or("(none)")
                )
            }
            other => format!("{}: value={:?}", other, self.value),
        }
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
