//! Versioned benchmark data. Gold answers never enter SearchInput.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
pub const SCHEMA_VERSION: u32 = 1;
// v7: v2 priority-packing receipts; full and projected lanes stay distinct. Packing
// omissions are partial and never count as a successful absence proof.
pub const ADAPTER_VERSION: &str = "cc-eval-public-v7";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ScoreProfile {
    #[serde(rename = "oce-compat-v1")]
    OceCompat,
    #[serde(rename = "codecortex-native-v1")]
    Native,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceLock {
    pub root: PathBuf,
    pub commit: Option<String>,
    pub digest: String,
    pub files: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub schema_version: u32,
    pub name: String,
    pub source: SourceLock,
    pub queries: PathBuf,
    pub queries_digest: String,
    pub scoring: ScoreProfile,
    pub repetitions: usize,
    pub warmup: usize,
    pub seed: u64,
    pub timeout_ms: u64,
    pub top_k: usize,
    pub engine_config: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolGold {
    pub name: String,
    pub qname: Option<String>,
    pub kind: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub start: u64,
    pub end: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub path: String,
    pub symbol: Option<SymbolGold>,
    pub span: Option<ByteSpan>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerGroup {
    pub id: String,
    pub primary: bool,
    pub grade: u8,
    pub alternatives: Vec<Alternative>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub id: String,
    pub category: String,
    pub difficulty: u8,
    pub language: String,
    pub split: String,
    pub query_family: String,
    pub query: String,
    pub path_prefix: Option<String>,
    pub no_answer: bool,
    pub expected_files: Vec<String>,
    pub answers: Vec<AnswerGroup>,
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    pub query: String,
    pub top_k: usize,
    pub path_prefix: Option<String>,
}
impl Query {
    pub fn input(&self, top_k: usize) -> SearchInput {
        SearchInput {
            query: self.query.clone(),
            top_k,
            path_prefix: self.path_prefix.clone(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Success,
    NoMatch,
    Partial,
    Timeout,
    ToolError,
    ProtocolError,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct Hit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_evidence: Option<Value>,
    pub path: String,
    pub symbol_name: Option<String>,
    pub qname: Option<String>,
    pub kind: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub span: Option<ByteSpan>,
    pub text: Option<String>,
    pub evidence_valid: Option<bool>,
}
impl Hit {
    pub fn path_only(path: String) -> Self {
        Self {
            path,
            ..Self::default()
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub case_id: String,
    pub repetition: usize,
    pub status: ResultStatus,
    pub elapsed_us: u64,
    pub hits: Vec<Hit>,
    pub raw_path: String,
    pub raw_digest: String,
    pub diagnostic: Option<String>,
}
