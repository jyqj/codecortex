//! Go router framework resolver.
//!
//! Covers gin, chi, gorilla/mux, echo, and fiber.
//!
//! Enrichment:
//! - `enrich_file`: extract `r.GET("/path", handler)` patterns → route_edges
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

/// Matches router method registrations across gin/chi/echo/fiber:
///   r.GET("/path", handler)      — gin, echo
///   r.Get("/path", handler)      — chi, fiber
///   r.POST("/path", handler)     — gin, echo
///   r.Post("/path", handler)     — chi, fiber
///   app.Get("/path", handler)    — fiber
///   e.GET("/path", handler)      — echo
///
/// Captures: (1) method name, (2) path string, (3) handler identifier (optional)
static GO_ROUTER_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)\.\s*(GET|Get|POST|Post|PUT|Put|DELETE|Delete|PATCH|Patch|HEAD|Head|OPTIONS|Options|Any|ANY)\s*\(\s*"([^"]+)"\s*(?:,\s*([A-Za-z_][A-Za-z0-9_.]*))?"#,
    )
    .expect("go router method re")
});

/// Matches gorilla/mux HandleFunc pattern (but NOT http.HandleFunc):
///   r.HandleFunc("/path", handler).Methods("GET")
///   r.HandleFunc("/path", handler)
///
/// Uses a negative lookbehind-like approach: requires a word char before the dot
/// (variable name), which `http` satisfies too, so we filter `http.` in code.
///
/// Captures: (1) receiver, (2) path, (3) handler
static GORILLA_HANDLEFUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)([A-Za-z_][A-Za-z0-9_]*)\.HandleFunc\s*\(\s*"([^"]+)"\s*,\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\)"#,
    )
    .expect("gorilla handlefunc re")
});

/// Matches .Methods("GET") chained after HandleFunc.
/// Scans for the .Methods call within the same line/nearby.
static GORILLA_METHODS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\.Methods\s*\(\s*"([A-Z]+)"\s*\)"#).expect("gorilla methods re")
});

/// Matches Group/Route prefix patterns:
///   r.Group("/prefix")   — gin, echo
///   r.Route("/prefix", ...)  — chi
///
/// Captures: (1) group method name, (2) prefix path
/// Reserved for resolve_cross_file in a future phase.
#[allow(dead_code)]
static GO_GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.\s*(Group|Route)\s*\(\s*"([^"]+)""#).expect("go group re")
});

/// Matches standard net/http HandleFunc:
///   http.HandleFunc("/path", handler)
static STD_HTTP_HANDLEFUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)http\s*\.\s*HandleFunc\s*\(\s*"([^"]+)"\s*,\s*([A-Za-z_][A-Za-z0-9_.]*)"#)
        .expect("std http handlefunc re")
});

pub struct GoRouterResolver;

impl GoRouterResolver {
    /// Normalize a Go router method name to uppercase HTTP verb.
    fn normalize_method(method_name: &str) -> String {
        match method_name {
            "GET" | "Get" => "GET",
            "POST" | "Post" => "POST",
            "PUT" | "Put" => "PUT",
            "DELETE" | "Delete" => "DELETE",
            "PATCH" | "Patch" => "PATCH",
            "HEAD" | "Head" => "HEAD",
            "OPTIONS" | "Options" => "OPTIONS",
            "Any" | "ANY" => "ANY",
            _ => "GET",
        }
        .to_string()
    }

    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Detect which Go router framework this source likely uses.
    fn detect_framework(source: &str) -> &'static str {
        if source.contains("gin-gonic/gin") || source.contains("gin.") {
            "gin"
        } else if source.contains("labstack/echo") || source.contains("echo.") {
            "echo"
        } else if source.contains("gofiber/fiber") || source.contains("fiber.") {
            "fiber"
        } else if source.contains("go-chi/chi") || source.contains("chi.") {
            "chi"
        } else if source.contains("gorilla/mux") || source.contains("mux.") {
            "gorilla"
        } else if source.contains("net/http") {
            "net/http"
        } else {
            "go-router"
        }
    }
}

impl FrameworkResolver for GoRouterResolver {
    fn framework_key(&self) -> &str {
        "gin" // Primary key; indexer also activates for echo/fiber/chi
    }

    fn languages(&self) -> &[Language] {
        &[Language::Go]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process .go files
        if !file_path.ends_with(".go") {
            return;
        }

        let framework = Self::detect_framework(source);

        // --- Standard router method pattern: .GET("/path", handler) ---
        for cap in GO_ROUTER_METHOD_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(3).map(|m| m.as_str().to_string());

            let offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: Some(Self::normalize_method(method_name)),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some(framework.to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Gorilla/mux HandleFunc pattern (skip http.HandleFunc) ---
        for cap in GORILLA_HANDLEFUNC_RE.captures_iter(source) {
            let receiver = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if receiver == "http" {
                continue; // Handled by STD_HTTP_HANDLEFUNC_RE
            }
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(3).map(|m| m.as_str().to_string());

            let full_match = cap.get(0).unwrap();
            let offset = full_match.start();
            let line = Self::line_for_offset(source, offset);

            // Try to find chained .Methods("GET") after the HandleFunc
            let after = &source[full_match.end()..];
            // Only look ahead a small amount (same line or next)
            let lookahead = &after[..after.len().min(100)];
            let method = GORILLA_METHODS_RE
                .captures(lookahead)
                .and_then(|mc| mc.get(1))
                .map(|m| m.as_str().to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method,
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("gorilla".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.82,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Standard net/http HandleFunc ---
        for cap in STD_HTTP_HANDLEFUNC_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(2).map(|m| m.as_str().to_string());

            let offset = cap.get(0).unwrap().start();
            let line = Self::line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name,
                method: None, // net/http HandleFunc doesn't specify method
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("net/http".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
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

    fn run_go(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        GoRouterResolver.enrich_file(file_path, source, Language::Go, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_gin_routes() {
        let source = r#"
package main

import "github.com/gin-gonic/gin"

func main() {
    r := gin.Default()
    r.GET("/users", getUsers)
    r.POST("/users", createUser)
    r.PUT("/users/:id", updateUser)
    r.DELETE("/users/:id", deleteUser)
}
"#;
        let routes = run_go("main.go", source);
        assert_eq!(
            routes.len(),
            4,
            "expected 4 gin routes, got {}",
            routes.len()
        );

        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("GET".into())
            && r.handler_name.as_deref() == Some("getUsers")));
        assert!(routes.iter().any(|r| r.route_path == "/users"
            && r.method == Some("POST".into())
            && r.handler_name.as_deref() == Some("createUser")));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/users/:id" && r.method == Some("PUT".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/users/:id" && r.method == Some("DELETE".into())));

        // All should be gin framework
        assert!(routes.iter().all(|r| r.framework.as_deref() == Some("gin")));
    }

    #[test]
    fn test_chi_routes() {
        let source = r#"
package main

import "github.com/go-chi/chi"

func main() {
    r := chi.NewRouter()
    r.Get("/articles", listArticles)
    r.Post("/articles", createArticle)
}
"#;
        let routes = run_go("main.go", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/articles" && r.method == Some("GET".into())));
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/articles" && r.method == Some("POST".into())));
        assert!(routes.iter().all(|r| r.framework.as_deref() == Some("chi")));
    }

    #[test]
    fn test_echo_routes() {
        let source = r#"
package main

import "github.com/labstack/echo"

func main() {
    e := echo.New()
    e.GET("/hello", helloHandler)
    e.POST("/submit", submitHandler)
}
"#;
        let routes = run_go("server.go", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("echo")));
    }

    #[test]
    fn test_fiber_routes() {
        let source = r#"
package main

import "github.com/gofiber/fiber"

func main() {
    app := fiber.New()
    app.Get("/api/health", healthCheck)
    app.Post("/api/data", handleData)
}
"#;
        let routes = run_go("app.go", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("fiber")));
    }

    #[test]
    fn test_gorilla_handlefunc() {
        let source = r#"
package main

import "github.com/gorilla/mux"

func main() {
    r := mux.NewRouter()
    r.HandleFunc("/products", productsHandler).Methods("GET")
    r.HandleFunc("/products", createProduct).Methods("POST")
    r.HandleFunc("/products/{id}", getProduct)
}
"#;
        let routes = run_go("routes.go", source);
        assert_eq!(
            routes.len(),
            3,
            "expected 3 gorilla routes, got {}",
            routes.len()
        );

        let get_products = routes
            .iter()
            .find(|r| r.route_path == "/products" && r.method == Some("GET".into()));
        assert!(get_products.is_some(), "should find GET /products");
        assert_eq!(
            get_products.unwrap().handler_name.as_deref(),
            Some("productsHandler")
        );

        // Third route has no .Methods() chain → method is None
        let no_method = routes.iter().find(|r| r.route_path == "/products/{id}");
        assert!(no_method.is_some());
        assert_eq!(no_method.unwrap().method, None);
    }

    #[test]
    fn test_net_http_handlefunc() {
        let source = r#"
package main

import "net/http"

func main() {
    http.HandleFunc("/hello", helloHandler)
    http.HandleFunc("/api/status", statusHandler)
}
"#;
        let routes = run_go("main.go", source);
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .any(|r| r.route_path == "/hello" && r.method.is_none()));
        assert!(routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("net/http")));
    }

    #[test]
    fn test_go_ignores_non_go_files() {
        let source = r#"r.GET("/test", handler)"#;
        let routes = run_go("test.rs", source);
        assert!(routes.is_empty(), "should ignore non-.go files");
    }
}
