//! FastAPI framework resolver.
//!
//! - `enrich_file`: extracts route definitions from FastAPI decorators
//! - `resolve_cross_file`: resolves APIRouter prefix mounting

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::{FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Decorator-style routes: @app.get("/path") or @router.post("/path")
///
/// Matches:
///   @app.get("/users")
///   @router.post("/items")
///   @app.delete("/users/{user_id}")
///
/// Captures: (1) HTTP method, (2) route path
static DECORATOR_ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@\w+\.(get|post|put|delete|patch|head|options)\(\s*["']([^"']+)["']"#)
        .expect("fastapi decorator route re")
});

/// Function definition: `def handler_name(` or `async def handler_name(`
///
/// Captures: (1) function name
static DEF_NAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:async\s+)?def\s+(\w+)\s*\("#).expect("fastapi def name re"));

/// app.include_router(router_name, prefix="/prefix") or app.include_router(router_name)
///
/// Captures: (1) router variable name, (2) optional prefix string
static INCLUDE_ROUTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\w+\.include_router\(\s*(\w+)(?:\s*,\s*prefix\s*=\s*["']([^"']+)["'])?"#)
        .expect("fastapi include_router re")
});

pub struct FastApiResolver;

impl FastApiResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }
}

impl FrameworkResolver for FastApiResolver {
    fn framework_key(&self) -> &str {
        "fastapi"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Python]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process Python files
        if !file_path.ends_with(".py") {
            return;
        }

        // Strategy: scan for decorator matches, then look ahead for the `def` line
        for cap in DECORATOR_ROUTE_RE.captures_iter(source) {
            let http_method = cap.get(1).map(|m| m.as_str()).unwrap_or("get");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let decorator_offset = cap.get(0).unwrap().start();
            let decorator_end = cap.get(0).unwrap().end();
            let line = Self::line_for_offset(source, decorator_offset);

            // Look for the function definition after the decorator
            let handler_name = DEF_NAME_RE
                .captures(&source[decorator_end..])
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some(http_method.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("fastapi".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.88,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- include_router mounting: app.include_router(router, prefix="/api") ---
        for cap in INCLUDE_ROUTER_RE.captures_iter(source) {
            let router_name = cap.get(1).map(|m| m.as_str().to_string());
            let prefix = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let match_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: router_name,
                method: Some("USE".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("fastapi".to_string()),
                route_kind: Some("router_mount".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Resolve APIRouter prefix mounting across files.
        //
        // Pattern: app.include_router(items_router, prefix="/api/v1")
        //   → find the file where `items_router` is defined
        //   → prepend "/api/v1" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for http route_edges.

        struct MountInfo {
            prefix: String,
            router_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("router_mount") {
                    if let Some(ref handler) = edge.handler_name {
                        let prefix = &edge.route_path;
                        if prefix != "/" && !prefix.is_empty() {
                            mounts.push(MountInfo {
                                prefix: prefix.clone(),
                                router_name: handler.clone(),
                                mount_file: file_path.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Prepend mount prefix to routes in target files
        for mount in &mounts {
            let target_file = match catalog.lookup_symbol(&mount.router_name, &mount.mount_file) {
                Some((_, file)) if file != mount.mount_file => file,
                _ => continue,
            };

            for (file_path, outcome) in outcomes.iter_mut() {
                if *file_path != target_file {
                    continue;
                }
                for edge in &mut outcome.route_edges {
                    if edge.route_kind.as_deref() != Some("http") {
                        continue;
                    }
                    if !edge.route_path.starts_with(&mount.prefix) {
                        let combined = if edge.route_path == "/" {
                            mount.prefix.clone()
                        } else {
                            format!("{}{}", mount.prefix, edge.route_path)
                        };
                        edge.route_path = combined;
                    }
                }
            }
        }

        // Resolve handler_symbol_uid for http route_edges
        for (file_path, outcome) in outcomes.iter_mut() {
            let fp = file_path.clone();
            for edge in &mut outcome.route_edges {
                if edge.handler_symbol_uid.is_some() {
                    continue;
                }
                if let Some(ref handler_name) = edge.handler_name {
                    if let Some((uid, _)) = catalog.lookup_symbol(handler_name, &fp) {
                        if !uid.is_empty() {
                            edge.handler_symbol_uid = Some(uid);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_fastapi(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        FastApiResolver.enrich_file(file_path, source, Language::Python, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_fastapi_basic_routes() {
        let source = r#"
from fastapi import FastAPI

app = FastAPI()

@app.get("/users")
async def list_users():
    return []

@app.post("/users")
async def create_user(user: UserCreate):
    return user
"#;
        let routes = run_fastapi("src/main.py", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        let get_route = routes.iter().find(|r| r.method == Some("GET".into()));
        assert!(get_route.is_some(), "should have GET /users");
        assert_eq!(get_route.unwrap().route_path, "/users");
        assert_eq!(
            get_route.unwrap().handler_name.as_deref(),
            Some("list_users")
        );

        let post_route = routes.iter().find(|r| r.method == Some("POST".into()));
        assert!(post_route.is_some(), "should have POST /users");
        assert_eq!(
            post_route.unwrap().handler_name.as_deref(),
            Some("create_user")
        );

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("fastapi")));
    }

    #[test]
    fn test_fastapi_router_with_multiple_methods() {
        let source = r#"
from fastapi import APIRouter

router = APIRouter(prefix="/api/v1")

@router.get("/items")
async def list_items():
    pass

@router.put("/items/{item_id}")
async def update_item(item_id: int):
    pass

@router.delete("/items/{item_id}")
async def delete_item(item_id: int):
    pass

@router.patch("/items/{item_id}")
def partial_update(item_id: int):
    pass
"#;
        let routes = run_fastapi("src/routes/items.py", source);
        assert_eq!(routes.len(), 4, "expected 4 routes, got {}", routes.len());

        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items/{item_id}" && r.method == Some("PUT".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items/{item_id}" && r.method == Some("DELETE".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/items/{item_id}" && r.method == Some("PATCH".into())));
    }

    #[test]
    fn test_fastapi_ignores_non_py_files() {
        let source = r#"@app.get("/test")
async def handler():
    pass"#;
        let routes = run_fastapi("test.js", source);
        assert!(routes.is_empty(), "should ignore non-.py files");
    }

    #[test]
    fn test_fastapi_sync_def() {
        let source = r#"
@app.get("/health")
def health_check():
    return {"status": "ok"}
"#;
        let routes = run_fastapi("src/health.py", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].handler_name.as_deref(), Some("health_check"));
    }
}
