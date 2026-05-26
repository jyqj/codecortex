//! Tool parameter types, domain management, and shared result type.

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, schemars::JsonSchema)]
pub struct JsonResult {
    pub result: serde_json::Value,
}

pub const DOMAIN_META: &str = "meta";
pub const DOMAIN_CORE: &str = "core";
pub const DOMAIN_CONTEXT: &str = "context";
pub const DOMAIN_GRAPH: &str = "graph";

pub fn default_active_domains() -> HashSet<&'static str> {
    HashSet::from([DOMAIN_CORE])
}

pub fn tool_domain(tool_name: &str) -> &'static str {
    match tool_name {
        "list_tool_domains" | "activate_domain" => DOMAIN_META,
        "set_project" | "build_index" | "index_status" | "search" | "find_symbol"
        | "list_files" | "file_symbols" | "callers" | "callees" | "analyze_impact"
        | "summarize_file" | "search_in_context" | "explore_symbols" | "get_symbol_source"
        | "graph_schema" | "ingest_trace" => DOMAIN_CORE,
        "prepare_edit_region" | "expand_code_region" | "task_symbols" => DOMAIN_CONTEXT,
        "graph_query"
        | "trace_path"
        | "symbol_refs"
        | "find_impacted_tests"
        | "get_dependents"
        | "find_dead_code"
        | "find_references"
        | "get_architecture"
        | "find_route_handlers"
        | "find_async_consumers"
        | "find_service_bindings"
        | "list_package_boundaries"
        | "list_unresolved_refs"
        | "list_communities"
        | "list_frameworks"
        | "index_capabilities" => DOMAIN_GRAPH,
        _ => DOMAIN_CORE,
    }
}

pub fn domain_listing(active: &HashSet<&'static str>) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "domain": DOMAIN_META,
            "active": true,
            "tools": ["list_tool_domains", "activate_domain"]
        }),
        serde_json::json!({
            "domain": DOMAIN_CORE,
            "active": active.contains(DOMAIN_CORE),
            "tools": ["set_project", "build_index", "index_status", "search", "find_symbol",
                       "list_files", "file_symbols", "callers", "callees", "analyze_impact",
                       "summarize_file", "search_in_context", "explore_symbols",
                       "get_symbol_source", "graph_schema", "ingest_trace"]
        }),
        serde_json::json!({
            "domain": DOMAIN_CONTEXT,
            "active": active.contains(DOMAIN_CONTEXT),
            "tools": ["prepare_edit_region", "expand_code_region", "task_symbols"]
        }),
        serde_json::json!({
            "domain": DOMAIN_GRAPH,
            "active": active.contains(DOMAIN_GRAPH),
            "tools": ["graph_query", "trace_path", "symbol_refs", "find_impacted_tests",
                       "get_dependents", "find_dead_code", "find_references",
                       "get_architecture", "find_route_handlers", "find_async_consumers",
                       "find_service_bindings", "list_package_boundaries", "list_unresolved_refs",
                       "list_communities", "list_frameworks", "index_capabilities"]
        }),
    ]
}

pub fn resolve_domain(name: &str) -> Option<&'static str> {
    match name {
        "meta" => Some(DOMAIN_META),
        "core" => Some(DOMAIN_CORE),
        "context" => Some(DOMAIN_CONTEXT),
        "graph" => Some(DOMAIN_GRAPH),
        _ => None,
    }
}

fn default_top_k() -> usize {
    10
}
fn default_true() -> bool {
    true
}
fn default_limit() -> usize {
    20
}
fn default_max_depth() -> usize {
    5
}
fn default_context_lines() -> u32 {
    20
}
fn default_boundary_limit() -> u32 {
    10
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ActivateDomainParams {
    pub domain: String,
    #[serde(default = "default_true")]
    pub active: bool,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SetProjectParams {
    pub path: String,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct BuildIndexParams {
    #[serde(default)]
    pub full: bool,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SearchParams {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    pub intent: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct FindSymbolParams {
    pub name: String,
    #[serde(default)]
    pub exact: bool,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct FileSymbolsParams {
    pub file_path: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct CallersCalleesParams {
    pub symbol: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ImpactParams {
    #[serde(default)]
    pub files: Vec<String>,
    pub base_branch: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct GraphQueryParams {
    pub query: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct TracePathParams {
    pub from: String,
    pub to: String,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SymbolRefsParams {
    pub symbol: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct EditRegionParams {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ExpandRegionParams {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default = "default_context_lines")]
    pub context_lines: u32,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ImpactedTestsParams {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct FindAsyncConsumersParams {
    pub topic_or_queue: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct FindServiceBindingsParams {
    pub service_or_route: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ListPackageBoundariesParams {
    #[serde(default = "default_boundary_limit")]
    pub limit: u32,
    #[serde(default)]
    pub project_path: Option<String>,
}

impl Default for ListPackageBoundariesParams {
    fn default() -> Self {
        Self {
            limit: default_boundary_limit(),
            project_path: None,
        }
    }
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct SummarizeFileParams {
    pub file_path: String,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ExploreSymbolsParams {
    pub symbols: Vec<String>,
    #[serde(default)]
    pub max_callers: Option<usize>,
    #[serde(default)]
    pub max_callees: Option<usize>,
    #[serde(default = "default_true")]
    pub include_source: bool,
    #[serde(default = "default_true")]
    pub include_relations: bool,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct TaskSymbolsParams {
    pub task: String,
    #[serde(default)]
    pub max_symbols: Option<usize>,
    #[serde(default)]
    pub expand_depth: Option<usize>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct GetSymbolSourceParams {
    pub symbol: String,
    #[serde(default)]
    pub exact: bool,
    #[serde(default = "default_true")]
    pub include_line_numbers: bool,
    #[serde(default)]
    pub max_chars: Option<usize>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct ListUnresolvedRefsParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct GraphSchemaParams {
    #[serde(default)]
    pub project_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema, Clone)]
pub struct TraceInput {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub source_service: Option<String>,
    #[serde(default)]
    pub target_service: Option<String>,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub duration_ms: Option<f64>,
}

#[derive(Deserialize, schemars::JsonSchema, Default)]
pub struct IngestTraceParams {
    pub traces: Vec<TraceInput>,
    #[serde(default)]
    pub project_path: Option<String>,
}
