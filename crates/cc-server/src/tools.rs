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
/// Hard ceiling for impact blast-radius BFS caps (max_nodes / max_per_layer).
const MAX_BFS_NODES: usize = 5000;
const MAX_FILE_ITEMS: usize = 200;
const MAX_SYMBOL_ITEMS: usize = 10;
const MAX_ADR_TEXT_LEN: usize = 65536;
const MAX_SOURCE_CHARS: usize = 1_000_000;
const MAX_SNIPPET_LINES: usize = 1000;

// ---------------------------------------------------------------------------
// Sanitization helpers
// ---------------------------------------------------------------------------

/// Return a prefix that never splits a UTF-8 code point.
///
/// The MCP surface accepts arbitrary user text, paths, and JSON payloads. Rust
/// string slicing/truncation by raw byte length panics when the boundary falls
/// inside a multibyte character, so all byte-budget truncation must pass
/// through this helper.
pub(crate) fn utf8_prefix(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn clamp_str(s: &mut String, max: usize) {
    if s.len() > max {
        let end = utf8_prefix(s, max).len();
        s.truncate(end);
    }
}

fn clamp_opt_str(s: &mut Option<String>, max: usize) {
    if let Some(ref mut v) = s {
        clamp_str(v, max);
    }
}

fn clamp_path_list(list: &mut Option<Vec<String>>) {
    if let Some(ref mut v) = list {
        v.truncate(MAX_FILE_ITEMS);
        for p in v.iter_mut() {
            clamp_str(p, MAX_PATH_LEN);
        }
    }
}

fn clamp_query_list(list: &mut Option<Vec<String>>) {
    if let Some(ref mut v) = list {
        v.truncate(MAX_FILE_ITEMS);
        for q in v.iter_mut() {
            clamp_str(q, MAX_QUERY_LEN);
        }
    }
}

fn is_valid_branch_name(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'/' || b == b'-')
}

fn validate_enum(value: &str, valid: &[&str], param_name: &str) -> Result<(), String> {
    if valid.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "invalid {}: '{}' (valid: {})",
            param_name,
            value,
            valid.join(", ")
        ))
    }
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
// 1. StatusParams — status
// ---------------------------------------------------------------------------

/// Parameters for the `status` tool.
/// Returns index readiness, capability list, graph schema, or all combined.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(
            &self.aspect,
            &["index", "capabilities", "schema", "all"],
            "aspect",
        )
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
// 2. IndexParams — index
// ---------------------------------------------------------------------------

/// Parameters for the `index` tool.
/// Triggers an incremental or full index build.
#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IndexParams {
    /// Absolute or relative path to the project root to index.
    pub path: String,

    /// When `true`, discard existing index data and rebuild from scratch.
    /// Default is `false` (incremental).
    #[serde(default)]
    pub full: bool,

    /// Optional event-scoped hint for incremental builds: project-relative
    /// paths known to have been created or modified since the last build.
    /// When present, the scan/diff phase stats/hashes only these paths
    /// instead of walking the whole tree (the same scoped path watcher
    /// ticks use). Safety fallbacks to the full walk are decided inside
    /// the build (first build, oversized event set, dot-path events).
    /// Only valid with `full=false`.
    #[serde(default)]
    pub changed_paths: Option<Vec<String>>,

    /// Optional event-scoped hint: project-relative paths known to have
    /// been removed since the last build. See `changed_paths`.
    #[serde(default)]
    pub removed_paths: Option<Vec<String>>,
}

/// Above this many combined scope paths the hint is dropped (unscoped
/// incremental build) instead of truncated — dropping individual entries
/// would silently miss changes, while an unscoped build is always correct.
/// The indexer's own oversized-event fallback (512) triggers the full walk
/// well before this bound; this cap only bounds request memory.
const MAX_SCOPE_PATHS: usize = 10_000;

impl IndexParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.path, MAX_PATH_LEN);
        let scope_len = self.changed_paths.as_ref().map_or(0, |v| v.len())
            + self.removed_paths.as_ref().map_or(0, |v| v.len());
        if scope_len > 0 && self.full {
            return Err(
                "changed_paths/removed_paths only apply to incremental builds (full=false)"
                    .to_string(),
            );
        }
        if scope_len > MAX_SCOPE_PATHS {
            self.changed_paths = None;
            self.removed_paths = None;
        }
        for list in [&mut self.changed_paths, &mut self.removed_paths]
            .into_iter()
            .flatten()
        {
            for p in list.iter_mut() {
                clamp_str(p, MAX_PATH_LEN);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. SearchParams — search
// ---------------------------------------------------------------------------

/// Parameters for the `search` tool.
/// Performs hybrid or symbol-only search across the indexed codebase.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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

    /// File paths the agent is currently focused on. These files receive a
    /// ranking boost in search results (they are not used as an exclusive filter).
    #[serde(default)]
    pub boost_files: Option<Vec<String>>,

    /// Recently viewed / edited file paths. Receives a smaller ranking boost
    /// than `boost_files`.
    #[serde(default)]
    pub recent_files: Option<Vec<String>>,

    /// Pinned / bookmarked file paths that should always rank highly when they
    /// match the query.
    #[serde(default)]
    pub pinned_files: Option<Vec<String>>,

    /// Prior conversational queries that bias lexical / semantic retrieval
    /// toward the ongoing discussion.
    #[serde(default)]
    pub conversation_queries: Option<Vec<String>>,

    /// In-memory / unsaved file paths (editor overlays) that should be treated
    /// as part of the working set when ranking results.
    #[serde(default)]
    pub overlay_files: Option<Vec<String>>,

    /// Maximum number of candidate files to consider during the preselection
    /// stage. Higher values trade latency for recall on large repos. Default
    /// is auto-tuned based on `top_k`.
    #[serde(default)]
    pub file_preselect_limit: Option<usize>,

    /// Restrict results to files whose path begins with this prefix
    /// (e.g. `"src/api/"`). Applied as an exclusive filter.
    #[serde(default)]
    pub path_prefix: Option<String>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl SearchParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.query, MAX_QUERY_LEN);
        self.top_k = self.top_k.clamp(1, MAX_TOP_K);
        clamp_opt_str(&mut self.intent, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.path_prefix, MAX_PATH_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        clamp_path_list(&mut self.boost_files);
        clamp_path_list(&mut self.recent_files);
        clamp_path_list(&mut self.pinned_files);
        clamp_query_list(&mut self.conversation_queries);
        clamp_path_list(&mut self.overlay_files);
        if let Some(ref mut limit) = self.file_preselect_limit {
            // Candidate-file-set size, not an output count: cap at the same
            // ceiling the default path can reach (MAX_TOP_K * largest tier
            // multiplier in default_preselect_limit), so an explicit override
            // cannot make preselect degenerate into a full-repo scan.
            *limit = (*limit).clamp(1, 4_000);
        }
        validate_enum(&self.mode, &["hybrid", "symbol"], "mode")
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
            boost_files: None,
            recent_files: None,
            pinned_files: None,
            conversation_queries: None,
            overlay_files: None,
            file_preselect_limit: None,
            path_prefix: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 4. ContextParams — context
// ---------------------------------------------------------------------------

/// Parameters for the `context` tool.
/// Given a task description, returns the most relevant symbols and source
/// snippets needed to understand or implement the task.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.task, MAX_QUERY_LEN);
        if let Some(ref mut n) = self.max_symbols {
            *n = (*n).clamp(1, MAX_SYMBOLS);
        }
        clamp_opt_str(&mut self.intent, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        Ok(())
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
// 5. NodeParams — node
// ---------------------------------------------------------------------------

/// Parameters for the `node` tool.
/// Retrieves detailed information about a single symbol node.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.symbol, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(
            &self.include,
            &["trail", "source", "outline", "summary"],
            "include",
        )
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
// 6. ExploreParams — explore
// ---------------------------------------------------------------------------

/// Parameters for the `explore` tool.
/// Batch-explores one or more symbols, returning relations, source, and flow paths.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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

    /// `flow` mode: maximum number of distinct flow paths to return. Default 3.
    #[serde(default)]
    pub max_paths: Option<usize>,

    /// `flow` mode: require exact symbol-name matches for the endpoints
    /// instead of fuzzy lookup. Default `true`.
    #[serde(default)]
    pub exact: Option<bool>,

    /// `flow` mode: restrict candidate symbols to this file path.
    #[serde(default)]
    pub file_path: Option<String>,

    /// `flow` mode: maximum number of candidate symbols considered per endpoint
    /// name when resolving ambiguous symbols. Default 5.
    #[serde(default)]
    pub max_candidates: Option<usize>,

    /// `symbols` mode: maximum number of callers to list per symbol.
    /// Defaults to a repo-size-adaptive value.
    #[serde(default)]
    pub max_callers: Option<usize>,

    /// `symbols` mode: maximum number of callees to list per symbol.
    /// Defaults to a repo-size-adaptive value.
    #[serde(default)]
    pub max_callees: Option<usize>,

    /// `symbols` mode: maximum source characters returned per symbol.
    /// Defaults to a repo-size-adaptive value.
    #[serde(default)]
    pub max_source_per_file: Option<usize>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ExploreParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
        self.symbols.truncate(MAX_SYMBOL_ITEMS);
        for s in &mut self.symbols {
            clamp_str(s, MAX_QUERY_LEN);
        }
        self.max_depth = self.max_depth.clamp(1, MAX_DEPTH);
        if let Some(ref mut n) = self.max_paths {
            *n = (*n).clamp(1, MAX_LIMIT);
        }
        if let Some(ref mut n) = self.max_candidates {
            *n = (*n).clamp(1, MAX_LIMIT);
        }
        if let Some(ref mut n) = self.max_callers {
            *n = (*n).clamp(1, MAX_LIMIT);
        }
        if let Some(ref mut n) = self.max_callees {
            *n = (*n).clamp(1, MAX_LIMIT);
        }
        if let Some(ref mut n) = self.max_source_per_file {
            *n = (*n).clamp(1, MAX_SOURCE_CHARS);
        }
        clamp_opt_str(&mut self.file_path, MAX_PATH_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(&self.mode, &["symbols", "flow"], "mode")
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
            max_paths: None,
            exact: None,
            file_path: None,
            max_candidates: None,
            max_callers: None,
            max_callees: None,
            max_source_per_file: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 7. TraceParams — trace
// ---------------------------------------------------------------------------

/// Parameters for the `trace` tool.
/// Finds the shortest call-graph path between two symbols.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    ///   When omitted, falls back to include_source: true→snippet, false→none.
    #[serde(default)]
    pub source_mode: Option<String>,

    /// Explicit symbol UID for the source endpoint. When provided (must contain ":"),
    /// skips the `from` name lookup and uses this UID directly.
    /// Use this after seeing disambiguation candidates to select the correct symbol.
    #[serde(default)]
    pub from_uid: Option<String>,

    /// Explicit symbol UID for the target endpoint. When provided (must contain ":"),
    /// skips the `to` name lookup and uses this UID directly.
    /// Use this after seeing disambiguation candidates to select the correct symbol.
    #[serde(default)]
    pub to_uid: Option<String>,

    /// Maximum source lines per hop node when `source_mode="snippet"`
    /// (or `include_source=true`). Default 3.
    #[serde(default)]
    pub max_snippet_lines: Option<usize>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl TraceParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.from, MAX_QUERY_LEN);
        clamp_str(&mut self.to, MAX_QUERY_LEN);
        self.max_depth = self.max_depth.clamp(1, MAX_DEPTH);
        clamp_opt_str(&mut self.from_uid, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.to_uid, MAX_QUERY_LEN);
        if let Some(ref mut n) = self.max_snippet_lines {
            *n = (*n).clamp(1, MAX_SNIPPET_LINES);
        }
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        if let Some(ref mode) = self.source_mode {
            validate_enum(mode, &["none", "snippet", "body", "outline"], "source_mode")?;
        }
        Ok(())
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
            from_uid: None,
            to_uid: None,
            max_snippet_lines: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 8. RelationsParams — relations
// ---------------------------------------------------------------------------

/// Parameters for the `relations` tool.
/// Returns callers, callees, references, or type-hierarchy edges for a symbol.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.symbol, MAX_QUERY_LEN);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(
            &self.kind,
            &["callers", "callees", "both", "refs", "hierarchy"],
            "kind",
        )?;
        validate_enum(
            &self.direction,
            &["up", "down", "both", "ancestors", "descendants"],
            "direction",
        )
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
// 9. ImpactParams — impact
// ---------------------------------------------------------------------------

/// Parameters for the `impact` tool.
/// Analyses change impact: affected symbols, tests, dead code, circular deps, or dependents.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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

    /// Minimum parser confidence (0.0–1.0) for call edges traversed during
    /// blast-radius BFS. Edges below this threshold are skipped, filtering out
    /// low-confidence impacted symbols. Applies to `changes` / `tests` scopes.
    #[serde(default)]
    pub confidence_threshold: Option<f32>,

    /// Safety cap on the total number of impacted symbols the blast-radius BFS
    /// will expand (deduplicated). When reached, expansion stops early and the
    /// report's `truncated` flag is set. Only applies to the `changes` scope.
    /// Defaults to a bounded value derived from `limit` when omitted.
    #[serde(default)]
    pub max_nodes: Option<usize>,

    /// Safety cap on the number of callers fetched per BFS layer (pushed down
    /// as a SQL LIMIT). Prevents a hub callee from returning every caller. When
    /// a layer hits this cap, the report's `truncated` flag is set. Only
    /// applies to the `changes` scope. Defaults to 500 when omitted.
    #[serde(default)]
    pub max_per_layer: Option<usize>,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl ImpactParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
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
        if let Some(ref mut t) = self.confidence_threshold {
            *t = t.clamp(0.0, 1.0);
        }
        if let Some(ref mut n) = self.max_nodes {
            *n = (*n).clamp(1, MAX_BFS_NODES);
        }
        if let Some(ref mut n) = self.max_per_layer {
            *n = (*n).clamp(1, MAX_BFS_NODES);
        }
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(
            &self.scope,
            &["changes", "tests", "dead_code", "circular", "dependents"],
            "scope",
        )
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
            confidence_threshold: None,
            max_nodes: None,
            max_per_layer: None,
            project_path: None,
        }
    }
}

// ---------------------------------------------------------------------------
// 10. ArchitectureParams — architecture
// ---------------------------------------------------------------------------

/// Parameters for the `architecture` tool.
/// Returns high-level architectural views of the project.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_opt_str(&mut self.filter, MAX_QUERY_LEN);
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        validate_enum(
            &self.aspect,
            &[
                "overview",
                "communities",
                "frameworks",
                "routes",
                "services",
                "async",
                "boundaries",
                "env",
                "unresolved",
            ],
            "aspect",
        )
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
// 11. FilesParams — files
// ---------------------------------------------------------------------------

/// Parameters for the `files` tool.
/// Lists project files, retrieves a code region, or expands a region with context.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_opt_str(&mut self.path, MAX_PATH_LEN);
        self.context_lines = self.context_lines.min(MAX_CONTEXT_LINES);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        Ok(())
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
// 12. GraphQueryParams — graph_query
// ---------------------------------------------------------------------------

/// Parameters for the `graph_query` tool.
/// Executes a raw Cypher-like query against the code graph.
#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphQueryParams {
    /// The graph query string (Cypher-like syntax).
    pub query: String,

    /// Optional project root override.
    #[serde(default)]
    pub project_path: Option<String>,
}

impl GraphQueryParams {
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_str(&mut self.query, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 13. IngestTracesParams
// ---------------------------------------------------------------------------

/// Parameters for the `ingest_traces` tool.
/// Accepts OTLP-style span observations to validate HTTP/async call edges.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        self.traces.truncate(MAX_TRACE_ITEMS);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 14. AdrParams
// ---------------------------------------------------------------------------

/// Parameters for the `adr` tool.
/// Manage Architecture Decision Records stored in the index.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
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
    pub fn sanitize(&mut self) -> Result<(), String> {
        clamp_opt_str(&mut self.adr_id, MAX_QUERY_LEN);
        clamp_opt_str(&mut self.title, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.context, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.decision, MAX_ADR_TEXT_LEN);
        clamp_opt_str(&mut self.project_path, MAX_PATH_LEN);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_prefix_never_splits_multibyte_chars() {
        let s = "a你好🙂";
        for max in 0..s.len() {
            let prefix = utf8_prefix(s, max);
            assert!(s.starts_with(prefix));
            assert!(prefix.len() <= max);
        }
    }

    #[test]
    fn index_params_scope_rejected_on_full_build() {
        let mut params = IndexParams {
            path: ".".into(),
            full: true,
            changed_paths: Some(vec!["src/lib.rs".into()]),
            removed_paths: None,
        };
        let err = params.sanitize().unwrap_err();
        assert!(err.contains("full=false"), "unexpected error: {err}");
    }

    #[test]
    fn index_params_oversized_scope_dropped_not_truncated() {
        let mut params = IndexParams {
            path: ".".into(),
            full: false,
            changed_paths: Some((0..MAX_SCOPE_PATHS).map(|i| format!("f{i}.rs")).collect()),
            removed_paths: Some(vec!["gone.rs".into()]),
        };
        params.sanitize().unwrap();
        // Over the cap: the whole hint is dropped (unscoped incremental is
        // always correct) — never truncated, which could miss changes.
        assert!(params.changed_paths.is_none());
        assert!(params.removed_paths.is_none());
    }

    #[test]
    fn index_params_scope_within_cap_kept_and_clamped() {
        let mut params = IndexParams {
            path: ".".into(),
            full: false,
            changed_paths: Some(vec!["x".repeat(MAX_PATH_LEN + 10)]),
            removed_paths: None,
        };
        params.sanitize().unwrap();
        let kept = params.changed_paths.unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].len(), MAX_PATH_LEN);
    }

    #[test]
    fn sanitize_long_multibyte_query_does_not_panic() {
        let mut params = SearchParams {
            query: "测".repeat(MAX_QUERY_LEN),
            ..Default::default()
        };
        params.query.push('试');
        params.sanitize().unwrap();
        assert!(params.query.len() <= MAX_QUERY_LEN);
        assert!(params.query.is_char_boundary(params.query.len()));
    }

    #[test]
    fn explore_params_sanitize_clamps_new_optionals() {
        let mut params = ExploreParams {
            symbols: vec!["A".into()],
            max_paths: Some(99_999),
            max_candidates: Some(99_999),
            max_callers: Some(99_999),
            max_callees: Some(99_999),
            max_source_per_file: Some(usize::MAX),
            file_path: Some("x".repeat(MAX_PATH_LEN + 100)),
            exact: Some(true),
            ..Default::default()
        };
        params.sanitize().unwrap();
        assert!(params.max_paths.unwrap() <= MAX_LIMIT);
        assert!(params.max_candidates.unwrap() <= MAX_LIMIT);
        assert!(params.max_callers.unwrap() <= MAX_LIMIT);
        assert!(params.max_callees.unwrap() <= MAX_LIMIT);
        assert!(params.max_source_per_file.unwrap() <= MAX_SOURCE_CHARS);
        assert!(params.file_path.as_ref().unwrap().len() <= MAX_PATH_LEN);
        assert_eq!(params.exact, Some(true));
    }

    #[test]
    fn trace_params_sanitize_clamps_max_snippet_lines() {
        let mut params = TraceParams {
            from: "A".into(),
            to: "B".into(),
            max_snippet_lines: Some(usize::MAX),
            ..Default::default()
        };
        params.sanitize().unwrap();
        assert!(params.max_snippet_lines.unwrap() <= MAX_SNIPPET_LINES);
    }

    #[test]
    fn search_params_sanitize_clamps_path_prefix() {
        let mut params = SearchParams {
            query: "q".into(),
            path_prefix: Some("p".repeat(MAX_PATH_LEN + 50)),
            ..Default::default()
        };
        params.sanitize().unwrap();
        assert!(params.path_prefix.as_ref().unwrap().len() <= MAX_PATH_LEN);
    }

    #[test]
    fn impact_params_sanitize_clamps_confidence_threshold() {
        let mut high = ImpactParams {
            confidence_threshold: Some(5.0),
            ..Default::default()
        };
        high.sanitize().unwrap();
        assert_eq!(high.confidence_threshold, Some(1.0));

        let mut low = ImpactParams {
            confidence_threshold: Some(-1.0),
            ..Default::default()
        };
        low.sanitize().unwrap();
        assert_eq!(low.confidence_threshold, Some(0.0));
    }
}
