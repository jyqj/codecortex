//! actix-web framework resolver.
//!
//! Enrichment:
//! - `enrich_file`: extract attribute macro routes (`#[get("/path")]`) and
//!   `web::resource` patterns → route_edges
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

/// Matches actix-web attribute macro routes:
///   #[get("/path")]
///   #[post("/api/users")]
///   #[put("/items/{id}")]
///
/// Captures: (1) HTTP method, (2) route path
static ACTIX_ATTR_ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)#\[(get|post|put|delete|patch|head)\s*\(\s*"([^"]*)"\s*\)\s*\]"#)
        .expect("actix attr route re")
});

/// Matches the next `async fn name(` or `fn name(` after an attribute macro.
///
/// Captures: (1) function name
static RUST_FN_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(")
        .expect("rust fn name re")
});

/// Matches `web::resource` method-based route registrations:
///   web::resource("/users")
///       .route(web::get().to(list_users))
///       .route(web::post().to(create_user))
///
/// We match the `.route(web::METHOD().to(HANDLER))` part separately, combined
/// with a `web::resource("PATH")` prefix scan.
///
/// Captures: (1) resource path
static ACTIX_RESOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)web::resource\s*\(\s*"([^"]+)"\s*\)"#).expect("actix resource re")
});

/// Matches `web::get().to(handler)` or `web::post().to(handler)` etc.
///
/// Captures: (1) HTTP method, (2) handler identifier
static ACTIX_WEB_METHOD_TO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)web::(get|post|put|delete|patch|head)\s*\(\s*\)\s*\.to\s*\(\s*([A-Za-z_][A-Za-z0-9_:]*)\s*\)",
    )
    .expect("actix web method to re")
});

pub struct ActixResolver;

impl ActixResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Find the next Rust function name after a given byte offset.
    fn find_next_fn_name(source: &str, after_offset: usize) -> Option<String> {
        let remaining = &source[after_offset..];
        RUST_FN_NAME_RE
            .captures(remaining)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }
}

impl FrameworkResolver for ActixResolver {
    fn framework_key(&self) -> &str {
        "actix-web"
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

        // --- Attribute macro routes: #[get("/path")] async fn handler() ---
        for cap in ACTIX_ATTR_ROUTE_RE.captures_iter(source) {
            let method = cap.get(1).map(|m| m.as_str()).unwrap_or("get");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let attr_offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, attr_offset);

            let handler_name = Self::find_next_fn_name(source, attr_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some(method.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("actix-web".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.88,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- web::resource("/path").route(web::get().to(handler)) ---
        for res_cap in ACTIX_RESOURCE_RE.captures_iter(source) {
            let resource_path = res_cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let res_match = res_cap.get(0).unwrap();
            let res_offset = res_match.start();
            let line = Self::line_for_offset(source, res_offset);

            // Scan the region after web::resource(...) for .route(web::METHOD().to(handler))
            // Look ahead up to 500 bytes to find chained .route() calls
            let after = &source[res_match.end()..];
            let lookahead = &after[..after.len().min(500)];

            for method_cap in ACTIX_WEB_METHOD_TO_RE.captures_iter(lookahead) {
                let method = method_cap.get(1).map(|m| m.as_str()).unwrap_or("get");
                let handler = method_cap.get(2).map(|m| m.as_str().to_string());

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: resource_path.to_string(),
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
                    framework: Some("actix-web".to_string()),
                    route_kind: Some("http".to_string()),
                    confidence: 0.82,
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

    fn run_actix(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        ActixResolver.enrich_file(file_path, source, Language::Rust, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_actix_attribute_routes() {
        let source = r#"
use actix_web::{get, post, web, HttpResponse};

#[get("/hello")]
async fn hello() -> HttpResponse {
    HttpResponse::Ok().body("Hello!")
}

#[post("/api/users")]
async fn create_user(body: web::Json<User>) -> HttpResponse {
    HttpResponse::Created().finish()
}
"#;
        let routes = run_actix("src/handlers.rs", source);
        assert_eq!(routes.len(), 2, "expected 2 routes, got {}", routes.len());

        assert!(routes.iter().any(|r| r.route_path == "/hello"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("hello")));
        assert!(routes.iter().any(|r| r.route_path == "/api/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("create_user")));

        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("actix-web")));
    }

    #[test]
    fn test_actix_all_http_verbs() {
        let source = r#"
use actix_web::{get, post, put, delete, patch, head};

#[get("/a")]
async fn a() -> impl Responder { "a" }

#[post("/b")]
async fn b() -> impl Responder { "b" }

#[put("/c")]
async fn c() -> impl Responder { "c" }

#[delete("/d")]
async fn d() -> impl Responder { "d" }

#[patch("/e")]
async fn e() -> impl Responder { "e" }

#[head("/f")]
async fn f() -> impl Responder { "f" }
"#;
        let routes = run_actix("src/verbs.rs", source);
        assert_eq!(routes.len(), 6, "expected 6 routes, got {}", routes.len());

        let methods: Vec<String> = routes.iter().filter_map(|r| r.method.clone()).collect();
        assert!(methods.contains(&"GET".to_string()));
        assert!(methods.contains(&"POST".to_string()));
        assert!(methods.contains(&"PUT".to_string()));
        assert!(methods.contains(&"DELETE".to_string()));
        assert!(methods.contains(&"PATCH".to_string()));
        assert!(methods.contains(&"HEAD".to_string()));
    }

    #[test]
    fn test_actix_resource_routes() {
        let source = r#"
use actix_web::{web, App, HttpServer};

fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/users")
            .route(web::get().to(list_users))
            .route(web::post().to(create_user))
    );
}
"#;
        let routes = run_actix("src/config.rs", source);
        assert_eq!(
            routes.len(),
            2,
            "expected 2 resource routes, got {}",
            routes.len()
        );

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("list_users")));
        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("create_user")));

        // Resource routes should have lower confidence than attribute macros
        assert!(routes.iter().all(|r| r.confidence < 0.85));
    }

    #[test]
    fn test_actix_ignores_non_rs_files() {
        let source = r#"#[get("/test")] async fn test() {}"#;
        let routes = run_actix("test.py", source);
        assert!(routes.is_empty(), "should ignore non-.rs files");
    }
}
