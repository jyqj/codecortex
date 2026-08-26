//! MCP Server — code indexing, search and graph tools only.

use crate::engine::CodeIndex;
use crate::handlers;
use crate::project_session::{normalize_project_path, ProjectSession};
use crate::tools::JsonResult;
use cc_model::config::RepoSizeTier;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_router, ServerHandler};
use std::borrow::Cow;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
// std::sync::RwLock is used for CodeIndex (sync handler access).

pub struct CodeCortexMcpServer {
    project_session: ProjectSession,
    tool_router: ToolRouter<Self>,
}

impl CodeCortexMcpServer {
    async fn index(&self) -> Arc<RwLock<CodeIndex>> {
        self.project_session.active_index().await
    }

    async fn index_for_project_path(
        &self,
        project_path: Option<&str>,
    ) -> Result<Arc<RwLock<CodeIndex>>, rmcp::ErrorData> {
        self.project_session
            .index_for_project_path(project_path)
            .await
            .map_err(handler_error_data)
    }
}

/// The single seam mapping typed handler errors ([`cc_model::CcError`]) to
/// JSON-RPC errors, preserving the documented contract (MCP_TOOLS.md):
///
/// - client-input problems → `-32602` invalid params;
/// - everything else → `-32603` internal error, message = the underlying
///   error text.
///
/// Transient errors additionally carry `data.retryable = true`
/// ([`CcError::is_retryable`]) so clients can programmatically distinguish
/// retry-after-build errors from permanent failures.
fn handler_error_data(e: cc_model::CcError) -> rmcp::ErrorData {
    use cc_model::CcError;
    match &e {
        CcError::InvalidParams(_) => rmcp::ErrorData::invalid_params(e.to_string(), None),
        _ if e.is_retryable() => rmcp::ErrorData::internal_error(
            e.to_string(),
            Some(serde_json::json!({ "retryable": true })),
        ),
        _ => rmcp::ErrorData::internal_error(e.to_string(), None),
    }
}

/// Run a tool handler on the blocking pool and apply the tool's output
/// budget policy at this single dispatch exit (`output_budget::finalize`).
/// Typed handler errors are mapped to JSON-RPC errors by
/// [`handler_error_data`] — the only place error classification becomes wire
/// codes.
macro_rules! spawn_handler {
    ($index:expr, $tool:expr, $body:expr) => {{
        let index = $index;
        let budget_index = index.clone();
        tokio::task::spawn_blocking(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body(index)))
                .unwrap_or_else(|panic_val| {
                    let msg = match panic_val.downcast_ref::<&str>() {
                        Some(s) => (*s).to_string(),
                        None => match panic_val.downcast_ref::<String>() {
                            Some(s) => s.clone(),
                            None => "handler panicked".to_string(),
                        },
                    };
                    tracing::error!("handler panic caught: {}", msg);
                    Err(cc_model::CcError::Other(format!("internal error: {}", msg)))
                })
                .map(|v| handlers::output_budget::finalize(&budget_index, $tool, v))
        })
        .await
        .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?
        .map(|v| Json(JsonResult { result: v }))
        .map_err(handler_error_data)
    }};
}

// ═══════════════════════════════════════════════════════════════════════
// 14 MCP Tools
// ═══════════════════════════════════════════════════════════════════════

#[tool_router]
impl CodeCortexMcpServer {
    // ── 1. status ────────────────────────────────────────────────────

    #[tool(
        name = "status",
        description = "Project status, index health, capabilities, and graph schema"
    )]
    async fn tool_status(
        &self,
        Parameters(p): Parameters<crate::tools::StatusParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let aspect = p.aspect;
        spawn_handler!(index, "status", move |rt| handlers::facade::handle_status(
            rt, &aspect
        ))
    }

    // ── 2. index ─────────────────────────────────────────────────────

    #[tool(
        name = "index",
        description = "Set project directory and build or update the code index"
    )]
    async fn tool_index(
        &self,
        Parameters(p): Parameters<crate::tools::IndexParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let path = normalize_project_path(&p.path);
        let index = self
            .project_session
            .set_active_project(path)
            .await
            .map_err(handler_error_data)?;
        let full = p.full;
        spawn_handler!(index, "index", move |rt| handlers::core::build_index(
            rt, full
        ))
    }

    // ── 3. search ────────────────────────────────────────────────────

    #[tool(
        name = "search",
        description = "Search code by FTS5+grep fusion (default) or symbol name lookup"
    )]
    async fn tool_search(
        &self,
        Parameters(p): Parameters<crate::tools::SearchParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let query = p.query;
        let top_k = p.top_k;
        let mode = p.mode;
        let exact = p.exact;
        let intent = parse_intent_opt(p.intent.as_deref());
        let boost_files = p.boost_files;
        let recent_files = p.recent_files;
        let pinned_files = p.pinned_files;
        let conversation_queries = p.conversation_queries;
        let overlay_files = p.overlay_files;
        let file_preselect_limit = p.file_preselect_limit;
        let path_prefix = p.path_prefix;
        match mode.as_str() {
            "symbol" => {
                spawn_handler!(index, "search", move |rt| handlers::core::find_symbol(
                    rt, &query, exact, top_k, false
                ))
            }
            _ => {
                let has_overrides = boost_files.is_some()
                    || recent_files.is_some()
                    || pinned_files.is_some()
                    || conversation_queries.is_some()
                    || overlay_files.is_some()
                    || file_preselect_limit.is_some()
                    || path_prefix.is_some();
                if has_overrides {
                    let overrides = cc_model::search::SearchRequest {
                        boost_file_paths: boost_files,
                        recent_file_paths: recent_files,
                        pinned_file_paths: pinned_files,
                        conversation_queries,
                        overlay_file_paths: overlay_files,
                        file_preselect_limit,
                        path_prefix,
                        ..Default::default()
                    };
                    spawn_handler!(index, "search", move |rt| {
                        handlers::context::search_in_context_with(
                            rt, &query, top_k, intent, overrides,
                        )
                    })
                } else {
                    spawn_handler!(index, "search", move |rt| {
                        handlers::context::search_in_context(rt, &query, top_k, intent)
                    })
                }
            }
        }
    }

    // ── 4. context ───────────────────────────────────────────────────

    #[tool(
        name = "context",
        description = "Build complete context for a task: extracts relevant symbols, relationships, and source in one call"
    )]
    async fn tool_context(
        &self,
        Parameters(p): Parameters<crate::tools::ContextParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let task = p.task;
        let max_symbols = p.max_symbols;
        let include_source = p.include_source;
        let intent = p.intent;
        spawn_handler!(
            index,
            "context",
            move |rt| handlers::facade::handle_context(
                rt,
                &task,
                max_symbols,
                include_source,
                intent.as_deref(),
            )
        )
    }

    // ── 5. node ──────────────────────────────────────────────────────

    #[tool(
        name = "node",
        description = "Inspect a single symbol: source code, callers+callees trail, outline, or file summary"
    )]
    async fn tool_node(
        &self,
        Parameters(p): Parameters<crate::tools::NodeParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbol = p.symbol;
        let include = p.include;
        spawn_handler!(index, "node", move |rt| handlers::facade::handle_node(
            rt, &symbol, &include
        ))
    }

    // ── 6. explore ───────────────────────────────────────────────────

    #[tool(
        name = "explore",
        description = "Batch explore multiple symbols with source, relations, and flow paths"
    )]
    async fn tool_explore(
        &self,
        Parameters(p): Parameters<crate::tools::ExploreParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbols = p.symbols;
        let mode = p.mode;
        let include_source = p.include_source;
        let outline = p.outline;
        let max_depth = p.max_depth;
        let max_paths = p.max_paths;
        let exact = p.exact;
        let flow_file_path = p.file_path;
        let max_candidates = p.max_candidates;
        let max_callers = p.max_callers;
        let max_callees = p.max_callees;
        let max_source_per_file = p.max_source_per_file;
        match mode.as_str() {
            "flow" => {
                spawn_handler!(index, "explore", move |rt| handlers::graph::explore_flow(
                    rt,
                    &symbols,
                    &handlers::graph::FlowArgs {
                        max_depth,
                        include_source,
                        max_paths,
                        exact,
                        file_path: flow_file_path.as_deref(),
                        max_candidates,
                    },
                ))
            }
            _ => {
                spawn_handler!(index, "explore", move |rt| {
                    handlers::context::explore_symbols(
                        rt,
                        &symbols,
                        &crate::engine_query::ExploreOptions {
                            max_callers,
                            max_callees,
                            include_source,
                            include_relations: true,
                            include_metrics: false,
                            outline,
                            max_source_per_file,
                        },
                    )
                })
            }
        }
    }

    // ── 7. trace ─────────────────────────────────────────────────────

    #[tool(
        name = "trace",
        description = "Find call-graph path between two symbols. Use source_mode='body' for complete function bodies + outgoing calls in one call."
    )]
    async fn tool_trace(
        &self,
        Parameters(p): Parameters<crate::tools::TraceParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let args = handlers::graph::TraceArgs {
            from: p.from,
            to: p.to,
            max_depth: p.max_depth,
            include_source: p.include_source,
            max_snippet_lines: p.max_snippet_lines,
            source_mode: p.source_mode,
            from_uid: p.from_uid,
            to_uid: p.to_uid,
        };
        spawn_handler!(index, "trace", move |rt| handlers::graph::trace_path(
            rt, &args
        ))
    }

    // ── 8. relations ─────────────────────────────────────────────────

    #[tool(
        name = "relations",
        description = "Symbol relationships: callers, callees, references, or type hierarchy"
    )]
    async fn tool_relations(
        &self,
        Parameters(p): Parameters<crate::tools::RelationsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbol = p.symbol;
        let kind = p.kind;
        let limit = p.limit;
        let direction = p.direction;
        spawn_handler!(index, "relations", move |rt| {
            handlers::facade::handle_relations(rt, &symbol, &kind, limit, &direction)
        })
    }

    // ── 9. impact ────────────────────────────────────────────────────

    #[tool(
        name = "impact",
        description = "Impact analysis: change blast radius, affected tests, dead code, circular deps, or file dependents"
    )]
    async fn tool_impact(
        &self,
        Parameters(p): Parameters<crate::tools::ImpactParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let args = handlers::facade::ImpactArgs {
            scope: p.scope,
            files: p.files,
            base_branch: p.base_branch,
            granularity: p.granularity,
            file_path: p.file_path,
            limit: p.limit,
            confidence_threshold: p.confidence_threshold,
            max_nodes: p.max_nodes,
            max_per_layer: p.max_per_layer,
        };
        spawn_handler!(index, "impact", move |rt| handlers::facade::handle_impact(
            rt, &args
        ))
    }

    // ── 10. architecture ─────────────────────────────────────────────

    #[tool(
        name = "architecture",
        description = "Project architecture: overview, communities, frameworks, routes, services, async consumers, boundaries, env vars, unresolved refs"
    )]
    async fn tool_architecture(
        &self,
        Parameters(p): Parameters<crate::tools::ArchitectureParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let aspect = p.aspect;
        let filter = p.filter;
        let limit = p.limit;
        spawn_handler!(index, "architecture", move |rt| {
            handlers::facade::handle_architecture(rt, &aspect, filter.as_deref(), limit)
        })
    }

    // ── 11. files ────────────────────────────────────────────────────

    #[tool(
        name = "files",
        description = "File operations: list indexed files, read code region, or expand region with context"
    )]
    async fn tool_files(
        &self,
        Parameters(p): Parameters<crate::tools::FilesParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let action = p.action;
        let path = p.path;
        let start_line = p.start_line;
        let end_line = p.end_line;
        let context_lines = p.context_lines;
        spawn_handler!(index, "files", move |rt| handlers::facade::handle_files(
            rt,
            &action,
            path.as_deref(),
            start_line,
            end_line,
            context_lines,
        ))
    }

    // ── 12. graph_query ──────────────────────────────────────────────

    #[tool(
        name = "graph_query",
        description = "Execute a Cypher subset query against the code graph. Use status(aspect='schema') to discover available types first."
    )]
    async fn tool_graph_query(
        &self,
        Parameters(p): Parameters<crate::tools::GraphQueryParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let query = p.query;
        spawn_handler!(
            index,
            "graph_query",
            move |rt| handlers::graph::graph_query(rt, &query)
        )
    }

    // ── 13. ingest_traces ───────────────────────────────────────────

    #[tool(
        name = "ingest_traces",
        description = "Ingest runtime trace observations (OTLP spans) to validate HTTP/async call edges and boost their confidence"
    )]
    async fn tool_ingest_traces(
        &self,
        Parameters(p): Parameters<crate::tools::IngestTracesParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let traces = p.traces;
        spawn_handler!(index, "ingest_traces", move |rt| {
            handlers::facade::handle_ingest_traces(rt, &traces)
        })
    }

    // ── 14. adr ─────────────────────────────────────────────────────

    #[tool(
        name = "adr",
        description = "Manage Architecture Decision Records: list, get, store, or delete architectural decisions persisted in the index"
    )]
    async fn tool_adr(
        &self,
        Parameters(p): Parameters<crate::tools::AdrParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize().map_err(handler_error_data)?;
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let action = p.action;
        let adr_id = p.adr_id;
        let title = p.title;
        let status = p.status;
        let context = p.context;
        let decision = p.decision;
        spawn_handler!(index, "adr", move |rt| handlers::facade::handle_adr(
            rt,
            &action,
            adr_id.as_deref(),
            title.as_deref(),
            status.as_deref(),
            context.as_deref(),
            decision.as_deref(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Server construction and lifecycle
// ═══════════════════════════════════════════════════════════════════════

impl CodeCortexMcpServer {
    pub fn new(project_path: Option<&std::path::Path>) -> Self {
        let tool_router = Self::tool_router();
        Self {
            project_session: ProjectSession::new(project_path),
            tool_router,
        }
    }

    fn touch_activity(&self) {
        self.project_session.touch_activity();
    }

    /// Get the current project's `RepoSizeTier`, falling back to `Tiny` on error.
    fn current_tier(&self, index: &Arc<RwLock<CodeIndex>>) -> RepoSizeTier {
        ProjectSession::current_tier(index)
    }

    /// Append budget hints to a tool description based on the project's repo size tier.
    fn enrich_tool_description(
        &self,
        tool_name: &str,
        base_desc: &str,
        tier: RepoSizeTier,
    ) -> String {
        match tool_name {
            "search" => format!("{} [budget: top_k={}]", base_desc, tier.search_top_k()),
            "context" => format!(
                "{} [budget: max_symbols={}]",
                base_desc,
                tier.explore_max_symbols()
            ),
            "explore" => format!(
                "{} [budget: max {} symbols, {} chars/symbol]",
                base_desc,
                tier.explore_max_symbols(),
                tier.max_source_chars_per_symbol()
            ),
            "node" => format!(
                "{} [budget: {} relations, {} chars/symbol]",
                base_desc,
                tier.explore_max_symbols(),
                tier.max_source_chars_per_symbol()
            ),
            "trace" => format!(
                "{} [budget: {} output chars]",
                base_desc,
                tier.max_output_chars()
            ),
            "graph_query" => format!(
                "{} [budget: {} max items]",
                base_desc,
                tier.output_budget("graph_query").max_items
            ),
            "impact" => format!(
                "{} [budget: {} max items]",
                base_desc,
                tier.output_budget("impact").max_items
            ),
            "relations" => format!(
                "{} [budget: {} max items]",
                base_desc,
                tier.output_budget("relations").max_items
            ),
            "architecture" => format!(
                "{} [budget: {} max items]",
                base_desc,
                tier.output_budget("architecture").max_items
            ),
            "files" => format!(
                "{} [budget: {} max items]",
                base_desc,
                tier.output_budget("files").max_items
            ),
            _ => base_desc.to_string(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// ServerHandler impl
// ═══════════════════════════════════════════════════════════════════════

impl ServerHandler for CodeCortexMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("codecortex", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            r#"CodeCortex — code index server with 14 tools.

## Quick Start
index(path) → search(query) → context(task)

## Tool Selection by Intent

| Intent                | Primary Tool                           | Secondary                             |
|-----------------------|----------------------------------------|---------------------------------------|
| Locate code           | search(query)                          | search(query, mode="symbol")          |
| Understand code       | context(task)                          | node(symbol)                          |
| Read source           | node(symbol, include="source")         | node(symbol, include="trail")         |
| Trace call chain      | trace(from, to, source_mode="body")    | relations(symbol, kind="callers")     |
| Multi-symbol flow     | explore(symbols)                       | explore(symbols, mode="flow")         |
| Impact of changes     | impact(scope="changes")                | impact(scope="tests")                 |
| Architecture          | architecture()                         | architecture(aspect="routes")         |
| Type hierarchy        | relations(symbol, kind="hierarchy")    |                                       |
| Env variables         | architecture(aspect="env")             |                                       |
| Dead code             | impact(scope="dead_code")              |                                       |
| Validate HTTP edges   | ingest_traces(traces)                  |                                       |
| Architecture decisions| adr(action="list")                     | adr(action="store", ...)              |

## Common Chains

- **Flow / "how does X reach Y"**: trace(from, to, source_mode="body") FIRST — one call returns the complete path with full function bodies + outgoing calls for each hop. Do NOT reconstruct with search + callers.
- **Onboarding**: context(task) first. If unclear, explore(symbols) for breadth, then node(symbol) on specifics.
- **Refactor planning**: search → relations(kind="callers") → impact(scope="changes"). The blast-radius answer comes from impact, not from walking callers manually.
- **Before editing**: impact(scope="changes") to understand what breaks. impact(scope="tests") to find affected test files.
- **Record decisions**: adr(action="store", adr_id="ADR-001", title="...", decision="...") to persist architectural decisions across sessions.

## Anti-patterns

- Do NOT grep/find when search() is available — it uses FTS5+grep fusion with ranking.
- Do NOT chain search + node when you want context — context(task) is one round-trip.
- Do NOT loop node() over many symbols — one explore(symbols) call returns them all grouped by file.
- Do NOT use trace(include_source=true) for deep understanding — use trace(source_mode="body") instead for complete function bodies.
- File changes are automatically detected and trigger incremental re-indexing (configurable via `.codecortex.json`).

## Rules

1. PREFER context(task) as primary entry point — returns symbols, callers/callees, and source in ONE call.
2. PREFER explore(symbols) over sequential node() calls for multiple symbols.
3. PREFER search() over grep/find.
4. Use trace(source_mode="body") for complete flow understanding in one call.
5. Use node(symbol, include="outline") for large classes to get signatures without full source.
6. Use status(aspect="schema") to discover node/edge types before writing Cypher queries.
7. Edges with `synthesized_by` field are inferred dynamic dispatch — not direct source calls.
8. If no project was discovered at startup, call index(path) once. After a project is set, file changes are detected automatically; use index(path, full=true) to force a full rebuild.
9. For refactoring, always run impact(scope="changes") first to understand blast radius.
10. Treat returned source as already read — do not re-open those files with Read/Grep."#
                .into(),
        );
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        let index = self.index().await;
        let tier = self.current_tier(&index);
        let mut tools = self.tool_router.list_all();
        for tool in &mut tools {
            if let Some(ref desc) = tool.description {
                let enriched = self.enrich_tool_description(&tool.name, desc, tier);
                tool.description = Some(Cow::Owned(enriched));
            }
        }
        Ok(ListToolsResult {
            tools,
            ..Default::default()
        })
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        self.touch_activity();
        async move {
            self.project_session
                .reopen_active_index_if_closed()
                .await
                .map_err(handler_error_data)?;
            let ctx = ToolCallContext::new(self, request, context);
            self.tool_router.call(ctx).await
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Server entry point and infrastructure
// ═══════════════════════════════════════════════════════════════════════

/// Wait for a shutdown signal (SIGTERM, SIGINT, or Ctrl+C).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, initiating graceful shutdown");
            }
            _ = sigint.recv() => {
                tracing::info!("received SIGINT, initiating graceful shutdown");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("received Ctrl+C, initiating graceful shutdown");
    }
}

pub async fn run_mcp_server(project_path: Option<std::path::PathBuf>) -> cc_model::CcResult<()> {
    let server = CodeCortexMcpServer::new(project_path.as_deref());
    server
        .project_session
        .start_initial_project_tasks(project_path.as_deref());
    server.project_session.start_idle_eviction().await;
    // Cheap Arc-clone handle kept for shutdown; `server` moves into the service.
    let session_for_shutdown = server.project_session.clone();

    // Unified shutdown signal for PPID watchdog
    let shutdown = Arc::new(tokio::sync::Notify::new());

    // PPID watchdog: detect parent process death → notify shutdown
    {
        let ppid_poll_ms: u64 = std::env::var("CODECORTEX_PPID_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);

        if ppid_poll_ms > 0 {
            #[cfg(unix)]
            {
                let initial_ppid = std::os::unix::process::parent_id();
                let shutdown_tx = Arc::clone(&shutdown);
                tokio::spawn(async move {
                    let interval = tokio::time::Duration::from_millis(ppid_poll_ms);
                    loop {
                        tokio::time::sleep(interval).await;
                        let current_ppid = std::os::unix::process::parent_id();
                        if current_ppid != initial_ppid || current_ppid == 1 {
                            tracing::info!(
                                initial_ppid,
                                current_ppid,
                                "parent process died, initiating graceful shutdown"
                            );
                            shutdown_tx.notify_one();
                            return;
                        }
                    }
                });
            }
        }
    }

    let transport = rmcp::transport::io::stdio();
    let service = rmcp::serve_server(server, transport)
        .await
        .map_err(|e| cc_model::CcError::Other(format!("MCP server error: {}", e)))?;
    tracing::info!("MCP server started on stdio");

    // Wait for either: normal service end, OS signal, or PPID watchdog
    tokio::select! {
        result = service.waiting() => {
            result.map_err(|e| cc_model::CcError::Other(format!("MCP server error: {}", e)))?;
        }
        _ = shutdown_signal() => {}
        _ = shutdown.notified() => {}
    }
    // Stop the watcher poll task and wait for it before tearing down the
    // service; a plain drop would only detach it.
    session_for_shutdown.shutdown().await;
    // `service` and `server` drop here → ProjectSession drop → DB close
    tracing::info!("MCP server shut down gracefully");
    Ok(())
}

fn parse_intent_opt(value: Option<&str>) -> Option<cc_model::Intent> {
    value.and_then(|i| cc_model::Intent::from_str(i).ok())
}

#[cfg(test)]
mod shutdown_tests {
    #[test]
    fn shutdown_signal_compiles() {
        // Verify the function exists and compiles.
        // Actual signal handling requires manual testing.
        let _ = super::shutdown_signal;
    }
}
