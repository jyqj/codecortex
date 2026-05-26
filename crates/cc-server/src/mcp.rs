//! MCP Server — code indexing, search and graph tools only.

use crate::engine::CodeIndex;
use crate::handlers;
use crate::tools::{self, JsonResult};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{tool, tool_router, ServerHandler};
use std::collections::{HashMap, HashSet};
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
    project_cache: Arc<RwLock<HashMap<PathBuf, ProjectServices>>>,
    active_domains: Arc<Mutex<HashSet<&'static str>>>,
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

        if let Some(svc) = self.project_cache.read().await.get(&path).cloned() {
            return Ok(svc.index);
        }

        let mut cache = self.project_cache.write().await;
        if let Some(svc) = cache.get(&path).cloned() {
            return Ok(svc.index);
        }
        let services = ProjectServices::new(Some(path.as_path()))
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        let index = services.index.clone();
        cache.insert(path, services);
        Ok(index)
    }
}

fn normalize_project_path(raw_path: &str) -> PathBuf {
    let path = PathBuf::from(raw_path);
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn project_path_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("project_path")
        .or_else(|| value.get("projectPath"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
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

#[tool_router]
impl CodeCortexMcpServer {
    #[tool(
        name = "list_tool_domains",
        description = "List tool domains and activation status"
    )]
    async fn tool_list_tool_domains(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let active = self
            .active_domains
            .lock()
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        let domains = tools::domain_listing(&active);
        Ok(Json(JsonResult {
            result: serde_json::json!({
                "active_domains": active.iter().copied().collect::<Vec<_>>(),
                "domains": domains,
            }),
        }))
    }

    #[tool(
        name = "activate_domain",
        description = "Activate or deactivate a tool domain"
    )]
    async fn tool_activate_domain(
        &self,
        Parameters(p): Parameters<tools::ActivateDomainParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let mut active = self
            .active_domains
            .lock()
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        let domain = tools::resolve_domain(&p.domain).ok_or_else(|| {
            rmcp::ErrorData::invalid_params(format!("unknown domain: {}", p.domain), None)
        })?;
        if domain != tools::DOMAIN_META {
            if p.active {
                active.insert(domain);
            } else {
                active.remove(domain);
            }
        }
        Ok(Json(JsonResult {
            result: serde_json::json!({
                "domain": p.domain,
                "active": p.active,
                "active_domains": active.iter().copied().collect::<Vec<_>>(),
            }),
        }))
    }

    // ── core ──────────────────────────────────────────────────

    #[tool(name = "set_project", description = "Set the project directory")]
    async fn tool_set_project(
        &self,
        Parameters(p): Parameters<tools::SetProjectParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let path = normalize_project_path(&p.path);
        let new_services = ProjectServices::new(Some(path.as_path()))
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        self.project_cache
            .write()
            .await
            .insert(path, new_services.clone());
        *self.project.write().await = new_services;
        self.maybe_auto_index();
        Ok(Json(JsonResult {
            result: serde_json::json!({"status": "ok"}),
        }))
    }

    #[tool(name = "build_index", description = "Build or update the code index")]
    async fn tool_build_index(
        &self,
        Parameters(p): Parameters<tools::BuildIndexParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let full = p.full;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::build_index(rt, full))
    }

    #[tool(name = "index_status", description = "Get index statistics")]
    async fn tool_index_status(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        spawn_handler!(self.index().await, handlers::core::index_status)
    }

    #[tool(
        name = "search",
        description = "Search code with a context envelope. Accepts optional intent: fix, refactor, trace, locate, explain"
    )]
    async fn tool_search(
        &self,
        Parameters(p): Parameters<tools::SearchParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let intent = parse_intent_opt(p.intent.as_deref());
        let query = p.query;
        let top_k = p.top_k;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::search(
            rt, &query, top_k, intent
        ))
    }

    #[tool(name = "find_symbol", description = "Find symbols by name")]
    async fn tool_find_symbol(
        &self,
        Parameters(p): Parameters<tools::FindSymbolParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let name = p.name;
        let exact = p.exact;
        let top_k = p.top_k;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::find_symbol(
            rt, &name, exact, top_k
        ))
    }

    #[tool(name = "list_files", description = "List all indexed files")]
    async fn tool_list_files(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        spawn_handler!(self.index().await, handlers::core::list_files)
    }

    #[tool(name = "file_symbols", description = "List symbols in a specific file")]
    async fn tool_file_symbols(
        &self,
        Parameters(p): Parameters<tools::FileSymbolsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let file_path = p.file_path;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::file_symbols(
            rt, &file_path
        ))
    }

    #[tool(
        name = "list_communities",
        description = "List detected code communities"
    )]
    async fn tool_list_communities(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        spawn_handler!(self.index().await, handlers::core::list_communities)
    }

    #[tool(name = "list_frameworks", description = "List detected frameworks")]
    async fn tool_list_frameworks(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        spawn_handler!(self.index().await, handlers::core::list_frameworks)
    }

    #[tool(
        name = "index_capabilities",
        description = "Report available index capabilities"
    )]
    async fn tool_index_capabilities(&self) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        spawn_handler!(self.index().await, handlers::core::index_capabilities)
    }

    #[tool(name = "callers", description = "Find callers of a symbol")]
    async fn tool_callers(
        &self,
        Parameters(p): Parameters<tools::CallersCalleesParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let symbol = p.symbol;
        let limit = p.limit;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::callers(rt, &symbol, limit))
    }

    #[tool(name = "callees", description = "Find callees of a symbol")]
    async fn tool_callees(
        &self,
        Parameters(p): Parameters<tools::CallersCalleesParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let symbol = p.symbol;
        let limit = p.limit;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::callees(rt, &symbol, limit))
    }

    #[tool(
        name = "analyze_impact",
        description = "Impact analysis from changed files or git diff"
    )]
    async fn tool_analyze_impact(
        &self,
        Parameters(p): Parameters<tools::ImpactParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let files = p.files;
        let base_branch = p.base_branch;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::core::analyze_impact(rt, &files, base_branch.as_deref())
        })
    }

    #[tool(
        name = "summarize_file",
        description = "Get a summary of a single file"
    )]
    async fn tool_summarize_file(
        &self,
        Parameters(p): Parameters<tools::SummarizeFileParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let file_path = p.file_path;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::core::summarize_file(
            rt, &file_path
        ))
    }

    // ── context ───────────────────────────────────────────────

    #[tool(
        name = "search_in_context",
        description = "Search code with context envelope, accepting explicit intent"
    )]
    async fn tool_search_in_context(
        &self,
        Parameters(p): Parameters<tools::SearchParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let intent = parse_intent_opt(p.intent.as_deref());
        let query = p.query;
        let top_k = p.top_k;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::context::search_in_context(
            rt, &query, top_k, intent
        ))
    }

    #[tool(
        name = "prepare_edit_region",
        description = "Read a code region with surrounding symbol context"
    )]
    async fn tool_prepare_edit_region(
        &self,
        Parameters(p): Parameters<tools::EditRegionParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let file_path = p.file_path;
        let start_line = p.start_line;
        let end_line = p.end_line;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::context::prepare_edit_region(rt, &file_path, start_line, end_line)
        })
    }

    #[tool(
        name = "expand_code_region",
        description = "Expand a code region by context lines"
    )]
    async fn tool_expand_code_region(
        &self,
        Parameters(p): Parameters<tools::ExpandRegionParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let file_path = p.file_path;
        let start_line = p.start_line;
        let end_line = p.end_line;
        let context_lines = p.context_lines;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::context::expand_code_region(
                rt,
                &file_path,
                start_line,
                end_line,
                context_lines,
            )
        })
    }

    #[tool(
        name = "task_symbols",
        description = "Build context from a natural language task description — extracts relevant symbols and their relationships"
    )]
    async fn tool_task_symbols(
        &self,
        Parameters(p): Parameters<tools::TaskSymbolsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let task = p.task;
        let max_symbols = p.max_symbols;
        let expand_depth = p.expand_depth;
        let intent = p.intent;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::context::task_symbols(rt, &task, max_symbols, expand_depth, intent.as_deref())
        })
    }

    #[tool(
        name = "explore_symbols",
        description = "Batch explore symbols: returns source code, callers, callees, and semantic relations in one call"
    )]
    async fn tool_explore_symbols(
        &self,
        Parameters(p): Parameters<tools::ExploreSymbolsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let project_path = p.project_path.clone();
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::context::explore_symbols(
                rt,
                &p.symbols,
                p.max_callers,
                p.max_callees,
                p.include_source,
                p.include_relations,
            )
        })
    }

    #[tool(
        name = "get_symbol_source",
        description = "Read exact symbol source by name or qualified name with line numbers and ambiguity candidates"
    )]
    async fn tool_get_symbol_source(
        &self,
        Parameters(p): Parameters<tools::GetSymbolSourceParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let symbol = p.symbol;
        let exact = p.exact;
        let include_line_numbers = p.include_line_numbers;
        let max_chars = p.max_chars;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::context::get_symbol_source(
                rt,
                &symbol,
                exact,
                include_line_numbers,
                max_chars,
            )
        })
    }

    // ── graph ─────────────────────────────────────────────────

    #[tool(
        name = "graph_query",
        description = "Execute a graph query (Cypher subset)"
    )]
    async fn tool_graph_query(
        &self,
        Parameters(p): Parameters<tools::GraphQueryParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let query = p.query;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::graph_query(rt, &query))
    }

    #[tool(
        name = "trace_path",
        description = "Trace call path between two symbols"
    )]
    async fn tool_trace_path(
        &self,
        Parameters(p): Parameters<tools::TracePathParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let from = p.from;
        let to = p.to;
        let max_depth = p.max_depth;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::trace_path(
            rt, &from, &to, max_depth
        ))
    }

    #[tool(name = "symbol_refs", description = "Find references to a symbol")]
    async fn tool_symbol_refs(
        &self,
        Parameters(p): Parameters<tools::SymbolRefsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let symbol = p.symbol;
        let limit = p.limit;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::symbol_refs(
            rt, &symbol, limit
        ))
    }

    #[tool(
        name = "find_impacted_tests",
        description = "Find tests impacted by files"
    )]
    async fn tool_find_impacted_tests(
        &self,
        Parameters(p): Parameters<tools::ImpactedTestsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let files = p.files;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_impacted_tests(
            rt, &files
        ))
    }

    #[tool(
        name = "find_async_consumers",
        description = "Find consumers of a topic or queue"
    )]
    async fn tool_find_async_consumers(
        &self,
        Parameters(p): Parameters<tools::FindAsyncConsumersParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let topic = p.topic_or_queue;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_async_consumers(
            rt, &topic
        ))
    }

    #[tool(
        name = "find_service_bindings",
        description = "Find infrastructure bindings for a service or route"
    )]
    async fn tool_find_service_bindings(
        &self,
        Parameters(p): Parameters<tools::FindServiceBindingsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let svc = p.service_or_route;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_service_bindings(
            rt, &svc
        ))
    }

    #[tool(
        name = "list_package_boundaries",
        description = "List cross-package call boundaries"
    )]
    async fn tool_list_package_boundaries(
        &self,
        Parameters(p): Parameters<tools::ListPackageBoundariesParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let limit = p.limit;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::list_package_boundaries(
            rt, limit
        ))
    }

    #[tool(
        name = "list_unresolved_refs",
        description = "List unresolved symbol/call references with resolver candidates"
    )]
    async fn tool_list_unresolved_refs(
        &self,
        Parameters(p): Parameters<tools::ListUnresolvedRefsParams>,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let limit = p.limit;
        let file_path = p.file_path;
        let kind = p.kind;
        let project_path = p.project_path;
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| {
            handlers::graph::list_unresolved_refs(rt, limit, file_path.as_deref(), kind.as_deref())
        })
    }

    #[tool(
        name = "get_dependents",
        description = "Find all files that depend on a given file"
    )]
    async fn tool_get_dependents(
        &self,
        args: JsonObject,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let p = serde_json::Value::Object(args);
        let project_path = project_path_from_value(&p);
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::get_dependents(rt, p))
    }

    #[tool(name = "find_dead_code", description = "Find unused/dead code symbols")]
    async fn tool_find_dead_code(
        &self,
        args: JsonObject,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let p = serde_json::Value::Object(args);
        let project_path = project_path_from_value(&p);
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_dead_code(rt, p))
    }

    #[tool(
        name = "find_references",
        description = "Find references to a symbol across the codebase"
    )]
    async fn tool_find_references(
        &self,
        args: JsonObject,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let p = serde_json::Value::Object(args);
        let project_path = project_path_from_value(&p);
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_references(rt, p))
    }

    #[tool(
        name = "get_architecture",
        description = "Get project architecture from the code index"
    )]
    async fn tool_get_architecture(
        &self,
        args: JsonObject,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let p = serde_json::Value::Object(args);
        let project_path = project_path_from_value(&p);
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::get_architecture(rt, p))
    }

    #[tool(name = "find_route_handlers", description = "Find HTTP route handlers")]
    async fn tool_find_route_handlers(
        &self,
        args: JsonObject,
    ) -> Result<Json<JsonResult>, rmcp::ErrorData> {
        let p = serde_json::Value::Object(args);
        let project_path = project_path_from_value(&p);
        let index = self.index_for_project_path(project_path.as_deref()).await?;
        spawn_handler!(index, move |rt| handlers::graph::find_route_handlers(rt, p))
    }
}

impl CodeCortexMcpServer {
    pub fn new(project_path: Option<&std::path::Path>) -> Self {
        let services = ProjectServices::new(project_path).unwrap_or_else(|e| {
            tracing::warn!("failed to initialize project: {}", e);
            ProjectServices::new(None).expect("empty CodeIndex should not fail")
        });
        let mut initial_cache = HashMap::new();
        if let Some(path) = project_path {
            initial_cache.insert(
                normalize_project_path(&path.to_string_lossy()),
                services.clone(),
            );
        }
        let project = Arc::new(RwLock::new(services));
        let project_cache = Arc::new(RwLock::new(initial_cache));
        let active_domains = Arc::new(Mutex::new(tools::default_active_domains()));
        let tool_router = Self::tool_router();
        Self {
            project,
            project_cache,
            active_domains,
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
}

impl ServerHandler for CodeCortexMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("codecortex", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            r#"CodeCortex code index server — 35+ tools across 4 domains.

## Quick Start
set_project(path) → build_index() → search(query)
Activate "context" or "graph" domains with activate_domain for advanced tools.

## Recommended Workflows
- Architecture overview: get_architecture()
- Locate implementation: search(name) → explore_symbols([name]) or get_symbol_source(symbol)
- Call chain: callers(sym) / callees(sym) / trace_path(from, to)
- Impact analysis: analyze_impact(files) → find_impacted_tests(files)
- Task planning: task_symbols(task="what you want to do")
- Resolver quality: list_unresolved_refs(limit) shows unresolved refs with candidate targets

## Tips
- explore_symbols returns source + callers + callees in one call — prefer over sequential lookups
- Most parameterized tools accept optional project_path for cross-repository comparisons
- Edges with synthesized_by field are dynamic dispatch inferences (EventEmitter, etc.)
- Use find_symbol for exact lookup, search for fuzzy/semantic matching"#
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
        let active = self
            .active_domains
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let tools_list = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| {
                let domain = tools::tool_domain(&tool.name);
                domain == tools::DOMAIN_META || active.contains(domain)
            })
            .collect();
        std::future::ready(Ok(ListToolsResult {
            tools: tools_list,
            ..Default::default()
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResult, rmcp::ErrorData>> + Send + '_
    {
        self.touch_activity();
        let active = self
            .active_domains
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let tool_name = request.name.to_string();
        async move {
            let domain = tools::tool_domain(&tool_name);
            if domain != tools::DOMAIN_META && !active.contains(domain) {
                return Err(rmcp::ErrorData::invalid_request(
                    format!("tool domain '{}' is inactive for '{}'", domain, tool_name),
                    None,
                ));
            }
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
