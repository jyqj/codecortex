//! Axum framework resolver.
//!
//! Enrichment:
//! - `enrich_file`: extract `.route("/path", get(handler))` patterns → route_edges
//! - `resolve_cross_file`: no-op for v1

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
    Regex::new(r"(?m)(get|post|put|delete|patch|head|options|trace)\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)")
        .expect("axum method extractor re")
});

/// Matches `.nest("/prefix", router)` patterns.
///
/// Captures: (1) prefix path, (2) nested router identifier
#[allow(dead_code)]
static AXUM_NEST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.nest\s*\(\s*"([^"]+)"\s*,\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)"#)
        .expect("axum nest re")
});

pub struct AxumResolver;

impl AxumResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }
}

impl FrameworkResolver for AxumResolver {
    fn framework_key(&self) -> &str {
        "axum"
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
            let line = Self::line_for_offset(source, route_offset);

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
    }

    fn resolve_cross_file(
        &self,
        _catalog: &crate::resolver::SymbolCatalog,
        _outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // No-op for v1
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

        assert!(routes.iter().all(|r| r.framework.as_deref() == Some("axum")));
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
}
