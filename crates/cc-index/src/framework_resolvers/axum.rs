//! Axum framework resolver.
//!
//! Enrichment:
//! - `enrich_file`: extract `.route("/path", get(handler))` patterns → route_edges,
//!   plus `.nest("/prefix", sub_router)` and `.merge(sub_router)` mount points
//! - `resolve_cross_file`: resolves sub-router prefix mounting across files

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::mount_resolution::{resolve_mounts, MountSpec, PrefixJoin, TargetLookup};
use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Matches `.route("/path", method(handler))` with optional method chaining.
///
/// Examples:
///   .route("/users", get(list_users))
///   .route("/users/:id", get(get_user).post(create_user))
///
/// Captures: (1) route path, (2) remainder with method extractors
static AXUM_ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.route\s*\(\s*"([^"]+)"\s*,\s*((?:\w+\s*\([^)]*\)\s*\.?\s*)+)\)"#)
        .expect("axum route re")
});

/// Matches a single method extractor: `get(handler)`, `post(handler)`, etc.
///
/// Handles both standalone and chained: `get(h1).post(h2)`
///
/// Captures: (1) HTTP method, (2) handler identifier
static AXUM_METHOD_EXTRACTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)(get|post|put|delete|patch|head|options|trace)\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)",
    )
    .expect("axum method extractor re")
});

/// Matches `.nest("/prefix", sub_router)` patterns.
///
/// Handles both variable references and function calls:
///   .nest("/api", api_routes())
///   .nest("/admin", admin_router)
///
/// Captures: (1) prefix path, (2) sub-router expression (name only, without parens)
static AXUM_NEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.nest\s*\(\s*"([^"]+)"\s*,\s*([A-Za-z_][A-Za-z0-9_:]*)\s*(?:\(\s*\))?\s*\)"#)
        .expect("axum nest re")
});

/// Matches `.merge(sub_router)` patterns.
///
/// Handles both variable references and function calls:
///   .merge(api_routes())
///   .merge(health_router)
///
/// Captures: (1) sub-router expression (name only, without parens)
static AXUM_MERGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.merge\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)\s*(?:\(\s*\))?\s*\)"#)
        .expect("axum merge re")
});

pub struct AxumResolver;

impl AxumResolver {}

impl FrameworkResolver for AxumResolver {
    fn framework_key(&self) -> &str {
        "axum"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::Rust]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process .rs files
        if !file_path.ends_with(".rs") {
            return;
        }

        // --- .route("/path", get(handler).post(handler2)) ---
        for cap in AXUM_ROUTE_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let extractors_str = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let route_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, route_offset);

            // Parse each method extractor within the route call
            for ext_cap in AXUM_METHOD_EXTRACTOR_RE.captures_iter(extractors_str) {
                let method = ext_cap.get(1).map(|m| m.as_str()).unwrap_or("get");
                let handler = ext_cap.get(2).map(|m| m.as_str().to_string());

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: route_path.to_string(),
                    handler_name: handler,
                    method: Some(method.to_uppercase()),
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: None,
                    router_symbol_uid: None,
                    framework: Some("axum".to_string()),
                    route_kind: Some("http".to_string()),
                    confidence: 0.85,
                    parser_tier: ParserTier::Heuristic,
                });
            }
        }

        // --- .nest("/prefix", sub_router) / .nest("/prefix", sub_router()) ---
        for cap in AXUM_NEST_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let sub_router = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            if sub_router.is_empty() {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: Some(sub_router.to_string()),
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("axum".to_string()),
                route_kind: Some("router_mount".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- .merge(sub_router) / .merge(sub_router()) ---
        for cap in AXUM_MERGE_RE.captures_iter(source) {
            let sub_router = cap.get(1).map(|m| m.as_str()).unwrap_or("");

            if sub_router.is_empty() {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: String::new(), // merge has no prefix
                handler_name: Some(sub_router.to_string()),
                method: None,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("axum".to_string()),
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
        // Resolve sub-router mounting across files.
        //
        // Pattern: .nest("/api", api_routes()) or .merge(health_routes())
        //   → find the file where `api_routes` / `health_routes` is defined
        //   → prepend the mount prefix to all http route_edges in that file
        //     (merge has an empty prefix, so no path modification happens)
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.
        resolve_mounts(
            catalog,
            outcomes,
            &MountSpec {
                mount_kinds: &["router_mount"],
                skip_root_prefix: false,
                framework: None,
                join: PrefixJoin::Plain,
                lookup: TargetLookup::Default,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_axum(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        AxumResolver.enrich_file(file_path, source, Language::Rust, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_axum_basic_routes() {
        let source = r#"
use axum::{Router, routing::{get, post}};

async fn list_users() -> impl IntoResponse { todo!() }
async fn create_user() -> impl IntoResponse { todo!() }

fn app() -> Router {
    Router::new()
        .route("/users", get(list_users))
        .route("/health", get(health_check))
}
"#;
        let routes = run_axum("src/main.rs", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("list_users")));
        assert!(routes.iter().any(|r| r.route_path == "/health"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("health_check")));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("axum")));
    }

    #[test]
    fn test_axum_chained_methods() {
        let source = r#"
use axum::{Router, routing::{get, post, put, delete}};

fn app() -> Router {
    Router::new()
        .route("/users/:id", get(get_user).post(create_user).delete(delete_user))
}
"#;
        let routes = run_axum("src/routes.rs", source);
        assert_eq!(
            routes.len(),
            3,
            "expected 3 chained routes, got {}",
            routes.len()
        );

        assert!(routes.iter().any(|r| r.route_path == "/users/:id"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("get_user")));
        assert!(routes.iter().any(|r| r.route_path == "/users/:id"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("create_user")));
        assert!(routes.iter().any(|r| r.route_path == "/users/:id"
            && r.method == Some("DELETE".into())
            && r.handler_name.as_deref() == Some("delete_user")));
    }

    #[test]
    fn test_axum_all_http_verbs() {
        let source = r#"
use axum::Router;

fn app() -> Router {
    Router::new()
        .route("/a", get(ha))
        .route("/b", post(hb))
        .route("/c", put(hc))
        .route("/d", delete(hd))
        .route("/e", patch(he))
        .route("/f", head(hf))
        .route("/g", options(hg))
}
"#;
        let routes = run_axum("src/app.rs", source);
        assert_eq!(routes.len(), 7, "expected 7 routes, got {}", routes.len());

        let methods: Vec<String> = routes.iter().filter_map(|r| r.method.clone()).collect();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert!(methods.contains(&"PUT".to_string()));
        assert!(methods.contains(&"DELETE".to_string()));
        assert!(methods.contains(&"PATCH".to_string()));
        assert!(methods.contains(&"HEAD".to_string()));
        assert!(methods.contains(&"OPTIONS".to_string()));
    }

    #[test]
    fn test_axum_ignores_non_rs_files() {
        let source = r#".route("/test", get(handler))"#;
        let routes = run_axum("test.go", source);
        assert!(routes.is_empty(), "should ignore non-.rs files");
    }

    #[test]
    fn test_axum_nest_with_function_call() {
        let source = r#"
use axum::Router;

fn app() -> Router {
    Router::new()
        .nest("/api", api_routes())
        .nest("/admin", admin_routes())
}
"#;
        let routes = run_axum("src/main.rs", source);
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("router_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            2,
            "expected 2 router_mount entries, got {}",
            mounts.len()
        );

        assert!(mounts
            .iter()
            .any(|r| r.route_path == "/api" && r.handler_name.as_deref() == Some("api_routes")));
        assert!(
            mounts
                .iter()
                .any(|r| r.route_path == "/admin"
                    && r.handler_name.as_deref() == Some("admin_routes"))
        );
        assert!(mounts
            .iter()
            .all(|r| r.framework.as_deref() == Some("axum")));
        assert!(mounts.iter().all(|r| r.confidence < 0.85));
    }

    #[test]
    fn test_axum_nest_with_variable() {
        let source = r#"
use axum::Router;

fn app() -> Router {
    let admin = admin_router();
    Router::new()
        .nest("/admin", admin)
}
"#;
        let routes = run_axum("src/main.rs", source);
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("router_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            1,
            "expected 1 router_mount entry, got {}",
            mounts.len()
        );
        assert_eq!(mounts[0].route_path, "/admin");
        assert_eq!(mounts[0].handler_name.as_deref(), Some("admin"));
    }

    #[test]
    fn test_axum_merge_with_function_call() {
        let source = r#"
use axum::Router;

fn app() -> Router {
    Router::new()
        .merge(health_routes())
        .merge(metrics_routes())
}
"#;
        let routes = run_axum("src/main.rs", source);
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("router_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            2,
            "expected 2 merge entries, got {}",
            mounts.len()
        );

        // merge has empty prefix
        assert!(mounts.iter().all(|r| r.route_path.is_empty()));
        assert!(mounts
            .iter()
            .any(|r| r.handler_name.as_deref() == Some("health_routes")));
        assert!(mounts
            .iter()
            .any(|r| r.handler_name.as_deref() == Some("metrics_routes")));
    }

    #[test]
    fn test_axum_merge_with_variable() {
        let source = r#"
use axum::Router;

fn app() -> Router {
    Router::new()
        .merge(fallback_router)
}
"#;
        let routes = run_axum("src/main.rs", source);
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("router_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            1,
            "expected 1 merge entry, got {}",
            mounts.len()
        );
        assert_eq!(mounts[0].route_path, "");
        assert_eq!(mounts[0].handler_name.as_deref(), Some("fallback_router"));
    }

    #[test]
    fn test_axum_mixed_routes_nest_merge() {
        let source = r#"
use axum::{Router, routing::get};

fn app() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .nest("/api/v1", api_v1_routes())
        .merge(auth_routes())
}
"#;
        let routes = run_axum("src/main.rs", source);

        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(http_routes.len(), 1, "expected 1 http route");
        assert_eq!(http_routes[0].route_path, "/health");

        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("router_mount"))
            .collect();
        assert_eq!(mounts.len(), 2, "expected 2 router_mount entries");

        // nest has prefix, merge does not
        assert!(mounts.iter().any(
            |r| r.route_path == "/api/v1" && r.handler_name.as_deref() == Some("api_v1_routes")
        ));
        assert!(mounts
            .iter()
            .any(|r| r.route_path.is_empty() && r.handler_name.as_deref() == Some("auth_routes")));
    }
}
