//! Tool parameter types and shared result type for the consolidated MCP tool surface.

use rmcp::schemars;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Sanitization constants
// ---------------------------------------------------------------------------

const MAX_TOP_K: usize = 200;
const MAX_LIMIT: usize = 500;
const MAX_DEPTH: usize = 15;
const MAX_SYMBOLS: usize = 50;
const MAX_CONTEXT_LINES: u32 = 200;
const MAX_QUERY_LEN: usize = 4096;
const MAX_PATH_LEN: usize = 1024;
const MAX_BRANCH_LEN: usize = 256;
const MAX_TRACE_ITEMS: usize = 1000;
const MAX_FILE_ITEMS: usize = 200;
const MAX_SYMBOL_ITEMS: usize = 10;
const MAX_ADR_TEXT_LEN: usize = 65536;

// ---------------------------------------------------------------------------
// Sanitization helpers
// ---------------------------------------------------------------------------

fn clamp_str(s: &mut String, max: usize) {
    if s.len() > max {
        s.truncate(max);
    }
}

fn clamp_opt_str(s: &mut Option<String>, max: usize) {
    if let Some(ref mut v) = s {
        clamp_str(v, max);
    }
}

fn is_valid_branch_name(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'/' || b == b'-')
}

// ---------------------------------------------------------------------------
// Shared result type
// ---------------------------------------------------------------------------

#[derive(Serialize, schemars::JsonSchema)]
pub struct JsonResult {
    pub result: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Default helper functions
// ---------------------------------------------------------------------------

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
fn default_granularity() -> String {
    "file".to_string()
}
fn default_direction() -> String {
    "both".to_string()
}
fn default_context_lines() -> u32 {
    20
}
fn default_aspect_all() -> String {
    "all".to_string()
}
fn default_mode_hybrid() -> String {
    "hybrid".to_string()
}
fn default_mode_symbols() -> String {
    "symbols".to_string()
}
fn default_include_trail() -> String {
    "trail".to_string()
}
fn default_kind_both() -> String {
    "both".to_string()
}
fn default_scope_changes() -> String {
    "changes".to_string()
}
fn default_aspect_overview() -> String {
    "overview".to_string()
}
fn default_action_list() -> String {
    "list".to_string()
}

// ---------------------------------------------------------------------------
// 1. StatusParams — cc_status
// ---------------------------------------------------------------------------

/// Parameters for the `cc_status` tool.
/// Returns index readiness, capability list, graph schema, or all combined.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct StatusParams {
    /// What aspect of the server to report.
    /// - `"index"` – index build state and statistics
    /// - `"capabilities"` – list of available analysis capabilities
    /// - `"schema"` – graph node / edge schema
    /// - `"all"` – everything above (default)
    #[serde(default = "default_aspect_all")]
    pub aspect: String,

    /// Optional project root to inspect; uses the current project if omitted.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl StatusParams {
    pub fn sanitize(&mut self) {
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for StatusParams {
    fn default() -> Self {
        Self {
            aspect: default_aspect_all(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 2. IndexParams — cc_index
// ---------------------------------------------------------------------------

/// Parameters for the `cc_index` tool.
/// Triggers an incremental or full index build.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct IndexParams {
    /// Absolute or relative path to the project root to index.
    pub path: String,

    /// When `true`, discard existing index data and rebuild from scratch.
    /// Default is `false` (incremental).
    #[serde(default)]
    pub full: bool,
}

impl IndexParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.path, MAX_PATH_LEN);
    }
}

impl Default for IndexParams {
    fn default() -> Self {
        Self {
            path: String::new(),
            full: false,
        }
    }
}

// ---------------------------------------------------------------------------
// 3. SearchParams — cc_search
// ---------------------------------------------------------------------------

/// Parameters for the `cc_search` tool.
/// Performs hybrid or symbol-only search across the indexed codebase.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// The search query string (natural language or symbol name).
    pub query: String,

    /// Search strategy.
    /// - `"hybrid"` – combines text and graph signals (default)
    /// - `"symbol"` – exact / fuzzy symbol-name lookup only
    #[serde(default = "default_mode_hybrid")]
    pub mode: String,

    /// Maximum number of results to return. Default 10.
    #[serde(default = "default_top_k")]
    pub top_k: usize,

    /// Optional intent hint to guide ranking (e.g. "find error handling logic").
    #[serde(default)]
    pub intent: Option<String>,

    /// When `true`, require an exact symbol-name match instead of fuzzy.
    #[serde(default)]
    pub exact: bool,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl SearchParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.query, MAX_QUERY_LEN);
        self.top_k = self.top_k.clamp(1, MAX_TOP_K);
        clamp_opt_str(&mut self.intent, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            mode: default_mode_hybrid(),
            top_k: default_top_k(),
            intent: None,
            exact: false,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 4. ContextParams — cc_context
// ---------------------------------------------------------------------------

/// Parameters for the `cc_context` tool.
/// Given a task description, returns the most relevant symbols and source
/// snippets needed to understand or implement the task.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ContextParams {
    /// Natural-language description of the task or question.
    pub task: String,

    /// Maximum number of symbols to include in the context window.
    #[serde(default)]
    pub max_symbols: Option<usize>,

    /// Whether to include source code for each symbol. Default `true`.
    #[serde(default = "default_true")]
    pub include_source: bool,

    /// Optional intent hint to steer retrieval (e.g. "refactor", "debug").
    #[serde(default)]
    pub intent: Option<String>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ContextParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.task, MAX_QUERY_LEN);
        if let Some(ref mut n) = self.max_symbols {
            *n = (*n).clamp(1, MAX_SYMBOLS);
        }
        clamp_opt_str(&mut self.intent, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for ContextParams {
    fn default() -> Self {
        Self {
            task: String::new(),
            max_symbols: None,
            include_source: default_true(),
            intent: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 5. NodeParams — cc_node
// ---------------------------------------------------------------------------

/// Parameters for the `cc_node` tool.
/// Retrieves detailed information about a single symbol node.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct NodeParams {
    /// Fully-qualified or short symbol name to look up.
    pub symbol: String,

    /// What to include in the response.
    /// - `"trail"` – caller / callee edges and metrics (default)
    /// - `"source"` – full source code of the symbol
    /// - `"outline"` – signature and member list without body
    /// - `"summary"` – AI-generated one-paragraph summary
    #[serde(default = "default_include_trail")]
    pub include: String,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl NodeParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.symbol, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for NodeParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            include: default_include_trail(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 6. ExploreParams — cc_explore
// ---------------------------------------------------------------------------

/// Parameters for the `cc_explore` tool.
/// Batch-explores one or more symbols, returning relations, source, and flow paths.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ExploreParams {
    /// One or more symbol names to explore (max 10).
    pub symbols: Vec<String>,

    /// Exploration mode.
    /// - `"symbols"` – per-symbol relations and source (default)
    /// - `"flow"` – discover data / control-flow paths between the listed symbols
    #[serde(default = "default_mode_symbols")]
    pub mode: String,

    /// Include source code for each discovered symbol. Default `true`.
    #[serde(default = "default_true")]
    pub include_source: bool,

    /// When `true`, return only signatures and member lists instead of full source.
    #[serde(default)]
    pub outline: bool,

    /// Maximum graph traversal depth for flow discovery. Default 5.
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ExploreParams {
    pub fn sanitize(&mut self) {
        self.symbols.truncate(MAX_SYMBOL_ITEMS);
        for s in &mut self.symbols {
            clamp_str(s, MAX_QUERY_LEN);
        }
        self.max_depth = self.max_depth.clamp(1, MAX_DEPTH);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for ExploreParams {
    fn default() -> Self {
        Self {
            symbols: Vec::new(),
            mode: default_mode_symbols(),
            include_source: default_true(),
            outline: false,
            max_depth: default_max_depth(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 7. TraceParams — cc_trace
// ---------------------------------------------------------------------------

/// Parameters for the `cc_trace` tool.
/// Finds the shortest call-graph path between two symbols.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct TraceParams {
    /// Starting symbol (caller side).
    pub from: String,

    /// Target symbol (callee side).
    pub to: String,

    /// Maximum traversal depth. Default 5.
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Include source snippets for each node on the path. Default `false`.
    /// Ignored when `source_mode` is set.
    #[serde(default)]
    pub include_source: bool,

    /// Source detail level for each hop node:
    /// - `"none"` – no source (fastest)
    /// - `"snippet"` – first 3 lines of each function (default when include_source=true)
    /// - `"body"` – complete function body + outgoing calls list (one call = full understanding)
    /// - `"outline"` – signature only, no body
    /// When omitted, falls back to include_source: true→snippet, false→none.
    #[serde(default)]
    pub source_mode: Option<String>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl TraceParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.from, MAX_QUERY_LEN);
        clamp_str(&mut self.to, MAX_QUERY_LEN);
        self.max_depth = self.max_depth.clamp(1, MAX_DEPTH);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for TraceParams {
    fn default() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            max_depth: default_max_depth(),
            include_source: false,
            source_mode: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 8. RelationsParams — cc_relations
// ---------------------------------------------------------------------------

/// Parameters for the `cc_relations` tool.
/// Returns callers, callees, references, or type-hierarchy edges for a symbol.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct RelationsParams {
    /// Symbol name to query relations for.
    pub symbol: String,

    /// Which relation kind to retrieve.
    /// - `"callers"` – functions that call this symbol
    /// - `"callees"` – functions this symbol calls
    /// - `"both"` – callers and callees (default)
    /// - `"refs"` – all reference sites
    /// - `"hierarchy"` – type inheritance / implementation edges
    #[serde(default = "default_kind_both")]
    pub kind: String,

    /// Maximum number of related symbols to return. Default 20.
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Direction for hierarchy traversal.
    /// - `"up"` – supertypes / interfaces
    /// - `"down"` – subtypes / implementors
    /// - `"both"` – both directions (default)
    #[serde(default = "default_direction")]
    pub direction: String,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl RelationsParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.symbol, MAX_QUERY_LEN);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for RelationsParams {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            kind: default_kind_both(),
            limit: default_limit(),
            direction: default_direction(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 9. ImpactParams — cc_impact
// ---------------------------------------------------------------------------

/// Parameters for the `cc_impact` tool.
/// Analyses change impact: affected symbols, tests, dead code, circular deps, or dependents.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ImpactParams {
    /// Analysis scope to run.
    /// - `"changes"` – symbols affected by changed files (default)
    /// - `"tests"` – tests that cover the changed symbols
    /// - `"dead_code"` – unreachable symbols in the project
    /// - `"circular"` – circular dependency cycles
    /// - `"dependents"` – reverse-dependency fan-out for a file
    #[serde(default = "default_scope_changes")]
    pub scope: String,

    /// Explicit list of changed file paths; auto-detected from git diff if omitted.
    #[serde(default)]
    pub files: Vec<String>,

    /// Git branch to diff against (e.g. `"main"`). Used when `scope` is `"changes"` or `"tests"`.
    #[serde(default)]
    pub base_branch: Option<String>,

    /// Granularity of the analysis: `"file"`, `"symbol"`, `"package"`, or `"community"`.
    /// Default `"file"`.
    #[serde(default = "default_granularity")]
    pub granularity: String,

    /// Single file path for scoped analysis (e.g. dead-code check within one file).
    #[serde(default)]
    pub file_path: Option<String>,

    /// Maximum number of results to return. Default 20.
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ImpactParams {
    pub fn sanitize(&mut self) {
        self.files.truncate(MAX_FILE_ITEMS);
        for f in &mut self.files {
            clamp_str(f, MAX_PATH_LEN);
        }
        if let Some(ref mut branch) = self.base_branch {
            clamp_str(branch, MAX_BRANCH_LEN);
            if !is_valid_branch_name(branch) {
                self.base_branch = None;
            }
        }
        clamp_opt_str(&mut self.file_path, MAX_PATH_LEN);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for ImpactParams {
    fn default() -> Self {
        Self {
            scope: default_scope_changes(),
            files: Vec::new(),
            base_branch: None,
            granularity: default_granularity(),
            file_path: None,
            limit: default_limit(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 10. ArchitectureParams — cc_architecture
// ---------------------------------------------------------------------------

/// Parameters for the `cc_architecture` tool.
/// Returns high-level architectural views of the project.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ArchitectureParams {
    /// Which architectural aspect to report.
    /// - `"overview"` – top-level package / module structure (default)
    /// - `"communities"` – detected module communities
    /// - `"frameworks"` – detected framework usage
    /// - `"routes"` – HTTP / RPC route handlers
    /// - `"services"` – service bindings and dependency injection
    /// - `"async"` – async consumers (queues, topics, event handlers)
    /// - `"boundaries"` – package boundary violations
    /// - `"env"` – environment variable usage
    /// - `"unresolved"` – unresolved references / imports
    #[serde(default = "default_aspect_overview")]
    pub aspect: String,

    /// Optional filter string to narrow results (e.g. a package prefix or file glob).
    #[serde(default)]
    pub filter: Option<String>,

    /// Maximum number of results to return. Default 20.
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ArchitectureParams {
    pub fn sanitize(&mut self) {
        clamp_opt_str(&mut self.filter, MAX_QUERY_LEN);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for ArchitectureParams {
    fn default() -> Self {
        Self {
            aspect: default_aspect_overview(),
            filter: None,
            limit: default_limit(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 11. FilesParams — cc_files
// ---------------------------------------------------------------------------

/// Parameters for the `cc_files` tool.
/// Lists project files, retrieves a code region, or expands a region with context.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct FilesParams {
    /// Action to perform.
    /// - `"list"` – list files in the project (default)
    /// - `"region"` – return a specific line range from a file
    /// - `"expand"` – return a line range plus surrounding context lines
    #[serde(default = "default_action_list")]
    pub action: String,

    /// File path (required for `"region"` and `"expand"` actions).
    #[serde(default)]
    pub path: Option<String>,

    /// Start line number (1-based, inclusive). Used with `"region"` and `"expand"`.
    #[serde(default)]
    pub start_line: Option<u32>,

    /// End line number (1-based, inclusive). Used with `"region"` and `"expand"`.
    #[serde(default)]
    pub end_line: Option<u32>,

    /// Number of additional context lines above and below the region.
    /// Only used with `"expand"`. Default 20.
    #[serde(default = "default_context_lines")]
    pub context_lines: u32,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl FilesParams {
    pub fn sanitize(&mut self) {
        clamp_opt_str(&mut self.path, MAX_PATH_LEN);
        self.context_lines = self.context_lines.min(MAX_CONTEXT_LINES);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for FilesParams {
    fn default() -> Self {
        Self {
            action: default_action_list(),
            path: None,
            start_line: None,
            end_line: None,
            context_lines: default_context_lines(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 12. GraphQueryParams — cc_graph_query
// ---------------------------------------------------------------------------

/// Parameters for the `cc_graph_query` tool.
/// Executes a raw Cypher-like query against the code graph.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct GraphQueryParams {
    /// The graph query string (Cypher-like syntax).
    pub query: String,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl GraphQueryParams {
    pub fn sanitize(&mut self) {
        clamp_str(&mut self.query, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

impl Default for GraphQueryParams {
    fn default() -> Self {
        Self {
            query: String::new(),
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 13. IngestTracesParams
// ---------------------------------------------------------------------------

/// Parameters for the `ingest_traces` tool.
/// Accepts OTLP-style span observations to validate HTTP/async call edges.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct IngestTracesParams {
    /// Array of trace observations. Each entry should have:
    /// `service_name` (string), `method` (string, optional), `path` (string),
    /// `status_code` (string, optional).
    pub traces: Vec<serde_json::Value>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl IngestTracesParams {
    pub fn sanitize(&mut self) {
        self.traces.truncate(MAX_TRACE_ITEMS);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}

// ---------------------------------------------------------------------------
// 14. AdrParams
// ---------------------------------------------------------------------------

/// Parameters for the `adr` tool.
/// Manage Architecture Decision Records stored in the index.
#[derive(Deserialize, schemars::JsonSchema)]
pub struct AdrParams {
    /// Action: `"list"` | `"get"` | `"store"` | `"delete"`.
    #[serde(default = "default_action_list")]
    pub action: String,

    /// ADR identifier (required for get/store/delete).
    #[serde(default)]
    pub adr_id: Option<String>,

    /// ADR title (required for store).
    #[serde(default)]
    pub title: Option<String>,

    /// ADR status: `"proposed"` | `"accepted"` | `"deprecated"` | `"superseded"`.
    #[serde(default)]
    pub status: Option<String>,

    /// Context/motivation for the decision.
    #[serde(default)]
    pub context: Option<String>,

    /// The decision itself.
    #[serde(default)]
    pub decision: Option<String>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl AdrParams {
    pub fn sanitize(&mut self) {
        clamp_opt_str(&mut self.adr_id, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.title, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.context, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.decision, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
    }
}
