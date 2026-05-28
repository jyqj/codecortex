//! Go router framework resolver (full tier).
//!
//! Covers gin, chi, gorilla/mux, echo, fiber, and net/http.
//!
//! Enrichment:
//! - `enrich_file`: extract `r.GET("/path", handler)` patterns → route_edges,
//!   plus group/mount/register-function patterns → group_mount edges
//! - `resolve_cross_file`: resolve cross-file group/mount prefixes and handler UIDs

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

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
static GO_GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.\s*(Group|Route)\s*\(\s*"([^"]+)""#).expect("go group re")
});

/// Matches chi Mount pattern:
///   r.Mount("/prefix", handler)
///   r.Mount("/api", apiRouter())
///
/// Captures: (1) prefix path, (2) handler/sub-router expression
static GO_MOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\.\s*Mount\s*\(\s*"([^"]+)"\s*,\s*([A-Za-z_][A-Za-z0-9_.]*(?:\(\s*\))?)"#)
        .expect("go mount re")
});

/// Matches cross-module router registration function calls:
///   routes.RegisterUserRoutes(api)
///   handlers.SetupRoutes(rg)
///   RegisterUserRoutes(apiGroup)
///
/// Captures: (1) optional package prefix, (2) function name, (3) argument
static GO_REGISTER_CALL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(?:([A-Za-z_][A-Za-z0-9_]*)\.)?([A-Z][A-Za-z0-9_]*(?:Routes|Router|Handlers|API|Endpoints))\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)"#,
    )
    .expect("go register call re")
});

/// Matches group variable assignment to capture prefix→variable binding:
///   api := r.Group("/api")
///   v1 := api.Group("/v1")
///
/// Captures: (1) variable name, (2) prefix path
static GO_GROUP_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)([A-Za-z_][A-Za-z0-9_]*)\s*:?=\s*\w+\.\s*Group\s*\(\s*"([^"]+)""#)
        .expect("go group assign re")
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

    fn resolver_tier(&self) -> &'static str {
        "full"
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
            let line = line_for_offset(source, offset);

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
            let line = line_for_offset(source, offset);

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
            let line = line_for_offset(source, offset);

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

        // --- Chi Mount pattern: .Mount("/prefix", subRouter) ---
        for cap in GO_MOUNT_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let handler_expr_raw = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            // Strip trailing "()" for function-call sub-routers like apiRouter()
            let handler_name = handler_expr_raw
                .trim_end_matches("()")
                .trim_end_matches('(');

            let offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: Some(handler_name.to_string()),
                method: Some("MOUNT".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: Some(handler_expr_raw.to_string()),
                router_symbol_uid: None,
                framework: Some(framework.to_string()),
                route_kind: Some("group_mount".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Group assignment with cross-file register calls ---
        // First, collect group variable → prefix mappings
        let mut group_prefixes: Vec<(String, String)> = Vec::new();
        for cap in GO_GROUP_ASSIGN_RE.captures_iter(source) {
            let var_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let prefix = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            if !var_name.is_empty() && !prefix.is_empty() {
                group_prefixes.push((var_name.to_string(), prefix.to_string()));
            }
        }

        // --- Cross-module register function calls: routes.RegisterUserRoutes(api) ---
        for cap in GO_REGISTER_CALL_RE.captures_iter(source) {
            let pkg_prefix = cap.get(1).map(|m| m.as_str());
            let func_name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let arg_name = cap.get(3).map(|m| m.as_str()).unwrap_or("");

            let offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, offset);

            // Find the group prefix for the argument variable
            let prefix = group_prefixes
                .iter()
                .find(|(var, _)| var == arg_name)
                .map(|(_, p)| p.as_str())
                .unwrap_or("");

            // Build the full qualified function name for catalog lookup
            let qualified_name = match pkg_prefix {
                Some(pkg) => format!("{}.{}", pkg, func_name),
                None => func_name.to_string(),
            };

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: Some(qualified_name),
                method: Some("MOUNT".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some(framework.to_string()),
                route_kind: Some("group_mount".to_string()),
                confidence: 0.78,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Standalone Group calls without assignment (emit for in-file grouping) ---
        // These are groups used inline without cross-file calls; less common but useful
        // for the resolve phase to know about group prefix nesting.
        for cap in GO_GROUP_RE.captures_iter(source) {
            let _method = cap.get(1).map(|m| m.as_str()).unwrap_or("Group");
            let prefix = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // Skip if we already captured this via GO_GROUP_ASSIGN_RE
            if group_prefixes.iter().any(|(_, p)| p == prefix) {
                continue;
            }

            let offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: None,
                method: Some("GROUP".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some(framework.to_string()),
                route_kind: Some("group_mount".to_string()),
                confidence: 0.70,
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
        // Resolve cross-file group/mount prefix propagation for Go routers.
        //
        // Patterns:
        //   1. r.Mount("/api", apiRouter)       → chi mount
        //   2. routes.RegisterUserRoutes(api)    → register-function with group arg
        //   3. api := r.Group("/api"); routes.RegisterXxx(api)
        //
        // For each group_mount edge, look up the handler in the catalog to find
        // the target file, then prepend the mount prefix to all http route_edges
        // in that target file.

        // Step 1: collect mount points
        struct MountInfo {
            prefix: String,
            handler_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("group_mount") {
                    if let Some(ref handler) = edge.handler_name {
                        let prefix = &edge.route_path;
                        if !prefix.is_empty() {
                            mounts.push(MountInfo {
                                prefix: prefix.clone(),
                                handler_name: handler.clone(),
                                mount_file: file_path.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Step 2: for each mount, resolve the target file and prepend prefix
        for mount in &mounts {
            // For qualified names like "routes.RegisterUserRoutes", try:
            //   1. The full qualified name
            //   2. Just the function name (after the dot)
            let lookup_names: Vec<&str> = if mount.handler_name.contains('.') {
                let parts: Vec<&str> = mount.handler_name.splitn(2, '.').collect();
                vec![parts[1], &mount.handler_name]
            } else {
                vec![&mount.handler_name]
            };

            let mut target_file = None;
            for name in &lookup_names {
                if let Some((_, file)) = catalog.lookup_symbol(name, &mount.mount_file) {
                    if file != mount.mount_file {
                        target_file = Some(file);
                        break;
                    }
                }
            }

            let target_file = match target_file {
                Some(f) => f,
                None => continue,
            };

            // Prepend the mount prefix to all http routes in the target file
            for (file_path, outcome) in outcomes.iter_mut() {
                if *file_path != target_file {
                    continue;
                }
                for edge in &mut outcome.route_edges {
                    if edge.route_kind.as_deref() != Some("http") {
                        continue;
                    }
                    // Only prepend once (avoid double-prefixing on repeated runs)
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

        // Step 3: resolve handler_symbol_uid for http route_edges
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

    // -----------------------------------------------------------------------
    // Cross-file extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_gin_group_and_register_call() {
        let source = r#"
package main

import (
    "github.com/gin-gonic/gin"
    "myapp/routes"
)

func main() {
    r := gin.Default()
    api := r.Group("/api")
    routes.RegisterUserRoutes(api)
}
"#;
        let routes = run_go("main.go", source);

        // Should have a group_mount edge for routes.RegisterUserRoutes
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_mount"))
            .collect();
        assert!(
            !mounts.is_empty(),
            "should detect group_mount from register call"
        );

        let register_mount = mounts
            .iter()
            .find(|r| r.handler_name.as_deref() == Some("routes.RegisterUserRoutes"));
        assert!(
            register_mount.is_some(),
            "should capture routes.RegisterUserRoutes"
        );
        assert_eq!(
            register_mount.unwrap().route_path,
            "/api",
            "should resolve group prefix /api from variable 'api'"
        );
    }

    #[test]
    fn test_chi_mount_extraction() {
        let source = r#"
package main

import "github.com/go-chi/chi"

func main() {
    r := chi.NewRouter()
    r.Mount("/api", apiRouter())
    r.Mount("/admin", adminRouter)
}
"#;
        let routes = run_go("main.go", source);

        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_mount"))
            .collect();
        assert_eq!(
            mounts.len(),
            2,
            "expected 2 mount edges, got {}",
            mounts.len()
        );

        let api_mount = mounts.iter().find(|r| r.route_path == "/api");
        assert!(api_mount.is_some(), "should find /api mount");
        // apiRouter() should strip trailing () for handler_name
        assert_eq!(
            api_mount.unwrap().handler_name.as_deref(),
            Some("apiRouter")
        );

        let admin_mount = mounts.iter().find(|r| r.route_path == "/admin");
        assert!(admin_mount.is_some(), "should find /admin mount");
        assert_eq!(
            admin_mount.unwrap().handler_name.as_deref(),
            Some("adminRouter")
        );
    }

    #[test]
    fn test_echo_group_extraction() {
        let source = r#"
package main

import "github.com/labstack/echo"

func main() {
    e := echo.New()
    g := e.Group("/api")
    g.GET("/users", listUsers)
    g.POST("/users", createUser)
}
"#;
        let routes = run_go("main.go", source);

        // Should have http routes for /users
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(http_routes.len(), 2, "expected 2 http routes");

        // The group assignment should be detected
        let group_mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("group_mount"))
            .collect();
        // No standalone group (it was assigned to 'g'), no register call,
        // so group_mount edges come from standalone Group regex minus assigned ones.
        // The group was captured by GO_GROUP_ASSIGN_RE, so GO_GROUP_RE skips it.
        // This is expected: the group is used in-file, not cross-file.
        assert!(
            group_mounts.is_empty() || group_mounts.iter().all(|r| r.route_path == "/api"),
            "group mount if present should have /api prefix"
        );
    }

    #[test]
    fn test_cross_file_resolve_chi_mount() {
        use cc_model::symbol::{SymbolKind, SymbolRecord};
        use cc_model::ParserTier;

        // Simulate main.go: r.Mount("/api", apiRouter())
        let main_source = r#"
package main

import "github.com/go-chi/chi"

func main() {
    r := chi.NewRouter()
    r.Mount("/api", apiRouter())
}
"#;
        // Simulate api.go: defines apiRouter and has routes
        let api_source = r#"
package main

import "github.com/go-chi/chi"

func apiRouter() chi.Router {
    r := chi.NewRouter()
    r.Get("/users", listUsers)
    r.Post("/users", createUser)
    return r
}
"#;

        let ctx = ProjectFrameworkContext::new();

        // Parse both files
        let mut main_outcome = ParseOutcome::default();
        GoRouterResolver.enrich_file(
            "main.go",
            main_source,
            Language::Go,
            &mut main_outcome,
            &ctx,
        );

        let mut api_outcome = ParseOutcome::default();
        GoRouterResolver.enrich_file("api.go", api_source, Language::Go, &mut api_outcome, &ctx);

        // Build a catalog with apiRouter symbol in api.go
        let mut catalog = crate::resolver::SymbolCatalog::new();
        catalog.add_symbols(&[SymbolRecord {
            symbol_id: "api_router_fn".to_string(),
            file_path: "api.go".to_string(),
            name: "apiRouter".to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line: 6,
            end_line: 11,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.9,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some("uid:apiRouter".to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }]);

        let mut outcomes: Vec<(String, ParseOutcome)> = vec![
            ("main.go".to_string(), main_outcome),
            ("api.go".to_string(), api_outcome),
        ];

        GoRouterResolver.resolve_cross_file(&catalog, &mut outcomes, &ctx);

        // After resolution, api.go routes should have /api prefix prepended
        let api_routes: Vec<_> = outcomes[1]
            .1
            .route_edges
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(api_routes.len(), 2, "expected 2 http routes in api.go");
        assert!(
            api_routes
                .iter()
                .any(|r| r.route_path == "/api/users" && r.method == Some("GET".into())),
            "GET /users should become /api/users after prefix resolution"
        );
        assert!(
            api_routes
                .iter()
                .any(|r| r.route_path == "/api/users" && r.method == Some("POST".into())),
            "POST /users should become /api/users after prefix resolution"
        );
    }

    #[test]
    fn test_cross_file_resolve_gin_register() {
        use cc_model::symbol::{SymbolKind, SymbolRecord};
        use cc_model::ParserTier;

        // main.go: api := r.Group("/api"); routes.RegisterUserRoutes(api)
        let main_source = r#"
package main

import (
    "github.com/gin-gonic/gin"
    "myapp/routes"
)

func main() {
    r := gin.Default()
    api := r.Group("/api")
    routes.RegisterUserRoutes(api)
}
"#;
        // routes/users.go: defines RegisterUserRoutes with sub-routes
        let users_source = r#"
package routes

import "github.com/gin-gonic/gin"

func RegisterUserRoutes(rg *gin.RouterGroup) {
    rg.GET("/users", listUsers)
    rg.POST("/users", createUser)
    rg.DELETE("/users/:id", deleteUser)
}
"#;

        let ctx = ProjectFrameworkContext::new();

        let mut main_outcome = ParseOutcome::default();
        GoRouterResolver.enrich_file(
            "main.go",
            main_source,
            Language::Go,
            &mut main_outcome,
            &ctx,
        );

        let mut users_outcome = ParseOutcome::default();
        GoRouterResolver.enrich_file(
            "routes/users.go",
            users_source,
            Language::Go,
            &mut users_outcome,
            &ctx,
        );

        // Build catalog with RegisterUserRoutes in routes/users.go
        let mut catalog = crate::resolver::SymbolCatalog::new();
        catalog.add_symbols(&[SymbolRecord {
            symbol_id: "register_user_routes_fn".to_string(),
            file_path: "routes/users.go".to_string(),
            name: "RegisterUserRoutes".to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line: 5,
            end_line: 9,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.9,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some("uid:RegisterUserRoutes".to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }]);

        let mut outcomes: Vec<(String, ParseOutcome)> = vec![
            ("main.go".to_string(), main_outcome),
            ("routes/users.go".to_string(), users_outcome),
        ];

        GoRouterResolver.resolve_cross_file(&catalog, &mut outcomes, &ctx);

        // After resolution, routes/users.go routes should have /api prefix
        let user_routes: Vec<_> = outcomes[1]
            .1
            .route_edges
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(
            user_routes.len(),
            3,
            "expected 3 http routes in routes/users.go"
        );
        assert!(
            user_routes.iter().all(|r| r.route_path.starts_with("/api")),
            "all routes should have /api prefix after cross-file resolution"
        );
        assert!(
            user_routes
                .iter()
                .any(|r| r.route_path == "/api/users" && r.method == Some("GET".into())),
            "should have GET /api/users"
        );
        assert!(
            user_routes
                .iter()
                .any(|r| r.route_path == "/api/users/:id" && r.method == Some("DELETE".into())),
            "should have DELETE /api/users/:id"
        );
    }

    #[test]
    fn test_resolver_tier_is_full() {
        let resolver = GoRouterResolver;
        assert_eq!(resolver.resolver_tier(), "full");
    }
}
