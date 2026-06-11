//! actix-web framework resolver (full tier).
//!
//! Enrichment:
//! - `enrich_file`: extract attribute macro routes (`#[get("/path")]`),
//!   `web::resource` patterns, `web::scope("/prefix")` with nested
//!   `.service(module::fn)` and `.configure(module::configure)` → route_edges
//! - `resolve_cross_file`: resolve scope-mounted cross-file services and
//!   configure delegates; prepend scope prefixes to referenced file routes;
//!   bind handler_symbol_uid via catalog lookup

use cc_model::edge::RouteEdgeRecord;
use cc_model::id::StableId;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::mount_resolution::{resolve_mounts, MountPoint, MountSpec, PrefixJoin, TargetLookup};
use super::{line_for_offset, FrameworkResolver, ProjectFrameworkContext};

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

/// Matches `web::scope("/prefix")` patterns.
///
/// Captures: (1) scope prefix path
static ACTIX_SCOPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)web::scope\s*\(\s*"([^"]*)"\s*\)"#).expect("actix scope re")
});

/// Matches `.service(module::function)` — cross-module service reference.
///
/// Captures: (1) full qualified path including module, e.g. `users::list_users`
static ACTIX_SERVICE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\.service\s*\(\s*([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s*\)")
        .expect("actix service ref re")
});

/// Matches `.configure(module::configure)` — cross-module configure delegate.
///
/// Captures: (1) full qualified path, e.g. `admin::configure`
static ACTIX_CONFIGURE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\.configure\s*\(\s*([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s*\)")
        .expect("actix configure ref re")
});

pub struct ActixResolver;

impl ActixResolver {
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

        // --- Attribute macro routes: #[get("/path")] async fn handler() ---
        for cap in ACTIX_ATTR_ROUTE_RE.captures_iter(source) {
            let method = cap.get(1).map(|m| m.as_str()).unwrap_or("get");
            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let attr_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, attr_offset);

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
            let line = line_for_offset(source, res_offset);

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

        // --- web::scope("/prefix").service(module::fn) / .configure(module::configure) ---
        for scope_cap in ACTIX_SCOPE_RE.captures_iter(source) {
            let scope_prefix = scope_cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if scope_prefix.is_empty() {
                continue;
            }
            let scope_match = scope_cap.get(0).unwrap();
            let scope_offset = scope_match.start();
            let line = line_for_offset(source, scope_offset);

            // Look ahead up to 1000 bytes for .service() and .configure() chains
            let after = &source[scope_match.end()..];
            let lookahead = &after[..after.len().min(1000)];

            // .service(module::function) — cross-module service references
            for svc_cap in ACTIX_SERVICE_REF_RE.captures_iter(lookahead) {
                let qualified_ref = svc_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if qualified_ref.is_empty() {
                    continue;
                }
                // Extract handler name: last segment of `module::func`
                let handler_name = qualified_ref.rsplit("::").next().map(|s| s.to_string());

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: scope_prefix.to_string(),
                    handler_name,
                    method: None,
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: Some(qualified_ref.to_string()),
                    router_symbol_uid: None,
                    framework: Some("actix-web".to_string()),
                    route_kind: Some("scope_mount".to_string()),
                    confidence: 0.80,
                    parser_tier: ParserTier::Heuristic,
                });
            }

            // .configure(module::configure) — cross-module configure delegates
            for cfg_cap in ACTIX_CONFIGURE_REF_RE.captures_iter(lookahead) {
                let qualified_ref = cfg_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                if qualified_ref.is_empty() {
                    continue;
                }
                let handler_name = qualified_ref.rsplit("::").next().map(|s| s.to_string());

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: scope_prefix.to_string(),
                    handler_name,
                    method: None,
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: Some(qualified_ref.to_string()),
                    router_symbol_uid: None,
                    framework: Some("actix-web".to_string()),
                    route_kind: Some("scope_mount".to_string()),
                    confidence: 0.78,
                    parser_tier: ParserTier::Heuristic,
                });
            }
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Resolve scope-mounted cross-file services and configure delegates.
        //
        // Pattern: web::scope("/api").service(users::list_users)
        //   → find the file containing `list_users`
        //   → the route attached to `list_users` gets "/api" prepended
        //
        // Pattern: web::scope("/api").configure(admin::configure)
        //   → find the file containing `configure`
        //   → all http route_edges in that file get "/api" prepended

        // Fallback for qualified paths like "admin::configure": search any
        // same-name symbol whose file path contains the module part.
        let module_fallback = |catalog: &crate::resolver::SymbolCatalog,
                               _outcomes: &[(String, ParseOutcome)],
                               mount: &MountPoint|
         -> Option<String> {
            if !mount.handler_expr.contains("::") {
                return None;
            }
            // Extract the module part: "admin::configure" → "admin"
            let module_part = mount.handler_expr.split("::").next().unwrap_or("");
            if module_part.is_empty() {
                return None;
            }
            catalog
                .lookup_all_by_name(&mount.handler_name)
                .into_iter()
                .find_map(|(_, file, _)| {
                    if file != mount.mount_file && file.contains(module_part) {
                        Some(file)
                    } else {
                        None
                    }
                })
        };

        resolve_mounts(
            catalog,
            outcomes,
            &MountSpec {
                mount_kinds: &["scope_mount"],
                skip_root_prefix: false,
                framework: Some("actix-web"),
                // Actix: "/api" + "/users" -> "/api/users"
                join: PrefixJoin::SlashNormalized,
                lookup: TargetLookup::DefaultWithFallback(&module_fallback),
            },
        );
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

    #[test]
    fn test_actix_scope_with_service_refs() {
        let source = r#"
use actix_web::{web, App, HttpServer};

fn main() {
    App::new()
        .service(web::scope("/api")
            .service(users::list_users)
            .service(users::create_user))
}
"#;
        let routes = run_actix("src/main.rs", source);
        // Should detect 2 scope_mount entries for the service references
        let scope_mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("scope_mount"))
            .collect();
        assert_eq!(
            scope_mounts.len(),
            2,
            "expected 2 scope_mount entries, got {}",
            scope_mounts.len()
        );

        assert!(scope_mounts.iter().any(|r| r.route_path == "/api"
            && r.handler_name.as_deref() == Some("list_users")
            && r.handler_expr.as_deref() == Some("users::list_users")));
        assert!(scope_mounts.iter().any(|r| r.route_path == "/api"
            && r.handler_name.as_deref() == Some("create_user")
            && r.handler_expr.as_deref() == Some("users::create_user")));
    }

    #[test]
    fn test_actix_configure_ref() {
        let source = r#"
use actix_web::{web, App};

fn main() {
    App::new()
        .service(web::scope("/admin")
            .configure(admin::configure))
}
"#;
        let routes = run_actix("src/main.rs", source);
        let scope_mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("scope_mount"))
            .collect();
        assert_eq!(
            scope_mounts.len(),
            1,
            "expected 1 scope_mount for configure, got {}",
            scope_mounts.len()
        );

        let mount = &scope_mounts[0];
        assert_eq!(mount.route_path, "/admin");
        assert_eq!(mount.handler_name.as_deref(), Some("configure"));
        assert_eq!(mount.handler_expr.as_deref(), Some("admin::configure"));
        // configure refs should have slightly lower confidence
        assert!(mount.confidence < 0.80);
    }

    #[test]
    fn test_actix_scope_ignores_local_service() {
        // .service(local_fn) without :: should NOT produce scope_mount
        let source = r#"
App::new()
    .service(web::scope("/api")
        .service(local_handler))
"#;
        let routes = run_actix("src/main.rs", source);
        let scope_mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("scope_mount"))
            .collect();
        assert!(
            scope_mounts.is_empty(),
            "local (non-qualified) service should not produce scope_mount"
        );
    }

    #[test]
    fn test_actix_resolver_tier() {
        assert_eq!(ActixResolver.resolver_tier(), "full");
    }
}
