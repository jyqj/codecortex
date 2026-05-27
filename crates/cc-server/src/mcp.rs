//! MCP Server — code indexing, search and graph tools only.

use crate::engine::CodeIndex;
use crate::handlers;
use crate::tools::JsonResult;
use cc_model::config::RepoSizeTier;
use lru::LruCache;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_router, ServerHandler};
use std::borrow::Cow;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ProjectServices {
    index: Arc<Mutex<CodeIndex>>,
}

impl ProjectServices {
    pub fn new(project_path: Option<&std::path::Path>) -> cc_model::CcResult<Self> {
        Ok(Self {
            index: Arc::new(Mutex::new(CodeIndex::new(project_path)?)),
        })
    }
}

pub struct CodeCortexMcpServer {
    project: Arc<RwLock<ProjectServices>>,
    project_cache: Arc<tokio::sync::Mutex<LruCache<PathBuf, ProjectServices>>>,
    tool_router: ToolRouter<Self>,
    last_activity: Arc<Mutex<Instant>>,
    auto_indexing: Arc<AtomicBool>,
}

impl CodeCortexMcpServer {
    async fn index(&self) -> Arc<Mutex<CodeIndex>> {
        self.project.read().await.index.clone()
    }

    async fn index_for_project_path(
        &self,
        project_path: Option<&str>,
    ) -> Result<Arc<Mutex<CodeIndex>>, rmcp::ErrorData> {
        let Some(raw_path) = project_path.filter(|p| !p.trim().is_empty()) else {
            return Ok(self.index().await);
        };
        let path = normalize_project_path(raw_path);

        let mut cache = self.project_cache.lock().await;
        if let Some(svc) = cache.get(&path).cloned() {
            return Ok(svc.index);
        }
        let services = ProjectServices::new(Some(path.as_path()))
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        let index = services.index.clone();
        cache.put(path, services);
        Ok(index)
    }
}

fn normalize_project_path(raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    std::fs::canonicalize(&path).unwrap_or(path)
}

macro_rules! spawn_handler {
    ($index:expr, $body:expr) => {{
        let index = $index;
        tokio::task::spawn_blocking(move || $body(index))
            .await
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?
            .map(|v| Json(JsonResult { result: v }))
            .map_err(|e| rmcp::ErrorData::internal_error(e, None))
    }};
}

// ═══════════════════════════════════════════════════════════════════════
// 12 MCP Tools
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let aspect = p.aspect;
        spawn_handler!(index, move |rt| handlers::facade::handle_status(
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
        p.sanitize();
        let path = normalize_project_path(&p.path);
        let new_services = ProjectServices::new(Some(path.as_path()))
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        self.project_cache
            .lock()
            .await
            .put(path, new_services.clone());
        *self.project.write().await = new_services;
        self.maybe_auto_index();
        let full = p.full;
        let index = self.index().await;
        spawn_handler!(index, move |rt| handlers::core::build_index(rt, full))
    }

    // ── 3. search ────────────────────────────────────────────────────

    #[tool(
        name = "search",
        description = "Search code by hybrid vector+FTS+grep fusion (default) or symbol name lookup"
    )]
    async fn tool_search(
        &self,
        Parameters(p): Parameters<crate::tools::SearchParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut p = p;
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let query = p.query;
        let top_k = p.top_k;
        let mode = p.mode;
        let exact = p.exact;
        let intent = parse_intent_opt(p.intent.as_deref());
        match mode.as_str() {
            "symbol" => {
                spawn_handler!(index, move |rt| handlers::core::find_symbol(
                    rt, &query, exact, top_k, false
                ))
            }
            _ => {
                spawn_handler!(index, move |rt| handlers::context::search_in_context(
                    rt, &query, top_k, intent
                ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let task = p.task;
        let max_symbols = p.max_symbols;
        let include_source = p.include_source;
        let intent = p.intent;
        spawn_handler!(index, move |rt| handlers::facade::handle_context(
            rt,
            &task,
            max_symbols,
            include_source,
            intent.as_deref(),
        ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbol = p.symbol;
        let include = p.include;
        spawn_handler!(index, move |rt| handlers::facade::handle_node(
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbols = p.symbols;
        let mode = p.mode;
        let include_source = p.include_source;
        let outline = p.outline;
        let max_depth = p.max_depth;
        match mode.as_str() {
            "flow" => {
                spawn_handler!(index, move |rt| handlers::graph::explore_flow(
                    rt,
                    &symbols,
                    max_depth,
                    include_source,
                    None,
                    None,
                    None,
                    None,
                ))
            }
            _ => {
                spawn_handler!(index, move |rt| handlers::context::explore_symbols(
                    rt,
                    &symbols,
                    None,
                    None,
                    include_source,
                    true,
                    false,
                    outline,
                    None,
                ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let from = p.from;
        let to = p.to;
        let max_depth = p.max_depth;
        let include_source = p.include_source;
        let source_mode = p.source_mode;
        let from_uid = p.from_uid;
        let to_uid = p.to_uid;
        spawn_handler!(index, move |rt| handlers::graph::trace_path(
            rt,
            &from,
            &to,
            max_depth,
            include_source,
            None,
            source_mode.as_deref(),
            from_uid.as_deref(),
            to_uid.as_deref(),
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let symbol = p.symbol;
        let kind = p.kind;
        let limit = p.limit;
        let direction = p.direction;
        spawn_handler!(index, move |rt| handlers::facade::handle_relations(
            rt, &symbol, &kind, limit, &direction
        ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let scope = p.scope;
        let files = p.files;
        let base_branch = p.base_branch;
        let granularity = p.granularity;
        let file_path = p.file_path;
        let limit = p.limit;
        spawn_handler!(index, move |rt| handlers::facade::handle_impact(
            rt,
            &scope,
            &files,
            base_branch.as_deref(),
            &granularity,
            file_path.as_deref(),
            limit,
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let aspect = p.aspect;
        let filter = p.filter;
        let limit = p.limit;
        spawn_handler!(index, move |rt| handlers::facade::handle_architecture(
            rt,
            &aspect,
            filter.as_deref(),
            limit,
        ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let action = p.action;
        let path = p.path;
        let start_line = p.start_line;
        let end_line = p.end_line;
        let context_lines = p.context_lines;
        spawn_handler!(index, move |rt| handlers::facade::handle_files(
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let query = p.query;
        spawn_handler!(index, move |rt| handlers::graph::graph_query(rt, &query))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let traces = p.traces;
        spawn_handler!(index, move |rt| handlers::facade::handle_ingest_traces(
            rt, &traces
        ))
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
        p.sanitize();
        let index = self
            .index_for_project_path(p.project_path.as_deref())
            .await?;
        let action = p.action;
        let adr_id = p.adr_id;
        let title = p.title;
        let status = p.status;
        let context = p.context;
        let decision = p.decision;
        spawn_handler!(index, move |rt| handlers::facade::handle_adr(
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
        let services = ProjectServices::new(project_path).unwrap_or_else(|e| {
            tracing::warn!("failed to initialize project: {}", e);
            ProjectServices::new(None).unwrap_or_else(|e2| {
                tracing::error!("fatal: cannot create empty CodeIndex either: {}", e2);
                ProjectServices {
                    index: Arc::new(Mutex::new(CodeIndex::empty())),
                }
            })
        });
        let mut initial_cache = LruCache::new(NonZeroUsize::new(16).unwrap());
        if let Some(path) = project_path {
            initial_cache.put(
                normalize_project_path(&path.to_string_lossy()),
                services.clone(),
            );
        }
        let project = Arc::new(RwLock::new(services));
        let project_cache = Arc::new(tokio::sync::Mutex::new(initial_cache));
        let tool_router = Self::tool_router();
        Self {
            project,
            project_cache,
            tool_router,
            last_activity: Arc::new(Mutex::new(Instant::now())),
            auto_indexing: Arc::new(AtomicBool::new(false)),
        }
    }

    fn touch_activity(&self) {
        if let Ok(mut ts) = self.last_activity.lock() {
            *ts = Instant::now();
        }
    }

    fn maybe_auto_index(&self) {
        let auto_indexing = self.auto_indexing.clone();
        let project = self.project.clone();

        if auto_indexing.load(Ordering::SeqCst) {
            return;
        }

        tokio::spawn(async move {
            let services = project.read().await;
            let index = services.index.clone();
            drop(services);

            let should_index = tokio::task::spawn_blocking({
                let index = index.clone();
                move || {
                    let rt = match index.lock() {
                        Ok(rt) => rt,
                        Err(_) => return false,
                    };
                    let project_path = match rt.project_path.as_deref() {
                        Some(p) => p,
                        None => return false,
                    };
                    let config = cc_model::config::load_project_config(project_path);
                    if !config.auto_index.enabled {
                        return false;
                    }
                    let paths = cc_model::config::IndexPaths::new(project_path);
                    !paths.index_db.exists()
                }
            })
            .await
            .unwrap_or(false);

            if !should_index {
                return;
            }
            if auto_indexing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }

            let result = tokio::task::spawn_blocking(move || {
                if let Ok(mut rt) = index.lock() {
                    if let Err(e) = rt.build_auto_index(false) {
                        tracing::warn!("auto-index failed: {}", e);
                    }
                }
            })
            .await;

            if let Err(e) = result {
                tracing::warn!("auto-index task panicked: {}", e);
            }
            auto_indexing.store(false, Ordering::SeqCst);
        });
    }

    /// Get the current project's `RepoSizeTier`, falling back to `Tiny` on error.
    fn current_tier(&self, index: &Arc<Mutex<CodeIndex>>) -> RepoSizeTier {
        index
            .lock()
            .ok()
            .map(|rt| rt.repo_size_tier())
            .unwrap_or(RepoSizeTier::Tiny)
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

- Do NOT grep/find when search() is available — it uses hybrid vector+FTS+grep fusion with ranking.
- Do NOT chain search + node when you want context — context(task) is one round-trip.
- Do NOT loop node() over many symbols — one explore(symbols) call returns them all grouped by file.
- Do NOT use trace(include_source=true) for deep understanding — use trace(source_mode="body") instead for complete function bodies.
- Do NOT query the index immediately after editing — the watcher needs ~500ms to sync.

## Rules

1. PREFER context(task) as primary entry point — returns symbols, callers/callees, and source in ONE call.
2. PREFER explore(symbols) over sequential node() calls for multiple symbols.
3. PREFER search() over grep/find.
4. Use trace(source_mode="body") for complete flow understanding in one call.
5. Use node(symbol, include="outline") for large classes to get signatures without full source.
6. Use status(aspect="schema") to discover node/edge types before writing Cypher queries.
7. Edges with `synthesized_by` field are inferred dynamic dispatch — not direct source calls.
8. After file edits, wait for watcher or call index(path) to update.
9. For refactoring, always run impact(scope="changes") first to understand blast radius.
10. Treat returned source as already read — do not re-open those files with Read/Grep."#
                .into(),
        );
        info
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, rmcp::ErrorData>> + Send + '_
    {
        async move {
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
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        self.touch_activity();
        async move {
            {
                let index = self.project.read().await.index.clone();
                let mut rt = index
                    .lock()
                    .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
                if rt.is_closed() {
                    if let Err(e) = rt.reopen() {
                        tracing::warn!("failed to reopen index after idle eviction: {}", e);
                    }
                }
            }
            let ctx = ToolCallContext::new(self, request, context);
            self.tool_router.call(ctx).await
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Server entry point and infrastructure
// ═══════════════════════════════════════════════════════════════════════

pub async fn run_mcp_server(project_path: Option<std::path::PathBuf>) -> cc_model::CcResult<()> {
    let server = CodeCortexMcpServer::new(project_path.as_deref());

    if project_path.is_some() {
        server.maybe_auto_index();
    }

    let idle_timeout_secs = {
        let services = server.project.read().await;
        let rt = services.index.lock().ok();
        rt.as_ref()
            .and_then(|rt| rt.project_path.as_deref())
            .map(|p| {
                cc_model::config::load_project_config(p)
                    .auto_index
                    .idle_timeout_secs
            })
            .unwrap_or(60)
    };

    let last_activity = server.last_activity.clone();
    let project = server.project.clone();
    let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);
    let _idle_task = tokio::spawn(async move {
        let check_interval = std::time::Duration::from_secs(30);
        loop {
            tokio::time::sleep(check_interval).await;
            let elapsed = last_activity
                .lock()
                .map(|ts| ts.elapsed())
                .unwrap_or_default();
            if elapsed >= idle_timeout {
                let index = project.read().await.index.clone();
                let mut guard = match index.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                if guard.project_path.is_some() && !guard.is_closed() {
                    tracing::info!("idle eviction: closing after {}s", elapsed.as_secs());
                    guard.close();
                }
            }
        }
    });

    // PPID watchdog: detect parent process death to prevent zombie MCP servers
    {
        let ppid_poll_ms: u64 = std::env::var("CODECORTEX_PPID_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);

        if ppid_poll_ms > 0 {
            #[cfg(unix)]
            {
                let initial_ppid = std::os::unix::process::parent_id();
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
                            std::process::exit(0);
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
    service
        .waiting()
        .await
        .map_err(|e| cc_model::CcError::Other(format!("MCP server error: {}", e)))?;
    Ok(())
}

fn parse_intent_opt(value: Option<&str>) -> Option<cc_model::Intent> {
    value.and_then(|i| match i {
        "fix" => Some(cc_model::Intent::Fix),
        "refactor" => Some(cc_model::Intent::Refactor),
        "trace" => Some(cc_model::Intent::Trace),
        "locate" => Some(cc_model::Intent::Locate),
        "test" => Some(cc_model::Intent::Test),
        "patch" => Some(cc_model::Intent::Patch),
        "explain" => Some(cc_model::Intent::Explain),
        "default" => Some(cc_model::Intent::Default),
        _ => None,
    })
}
