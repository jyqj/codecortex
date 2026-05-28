//! Hono framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Hono-style APIs
//! - `resolve_cross_file`: resolves sub-router prefix mounting

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

/// Standard route methods: app.get("/path", handler) or app.post("/path", async (c) => { ... })
///
/// Matches:
///   app.get("/users", listUsers)
///   app.post("/items", async (c) => { ... })
///   api.delete("/users/:id", deleteUser)
///
/// Captures: (1) method name, (2) route path
static ROUTE_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:app|[a-zA-Z]\w*)\.(\w+)\(\s*["']([^"']+)["']\s*,"#)
        .expect("hono route method re")
});

/// Sub-router mounting: app.route("/prefix", subApp)
///
/// Captures: (1) prefix path, (2) sub-app identifier
static ROUTE_MOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:app|[a-zA-Z]\w*)\.route\(\s*["']([^"']+)["']\s*,\s*(\w+)"#)
        .expect("hono route mount re")
});

/// Middleware: app.use("/path", middleware) or app.use(middleware)
///
/// Captures: (1) optional path, (2) handler
static USE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:app|[a-zA-Z]\w*)\.use\(\s*(?:["']([^"']+)["']\s*,\s*)?(\w+)"#)
        .expect("hono use re")
});

/// Valid Hono HTTP method names.
const HONO_HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "head", "options", "all",
];

pub struct HonoResolver;

impl HonoResolver {
    fn is_http_method(method: &str) -> bool {
        HONO_HTTP_METHODS.contains(&method.to_lowercase().as_str())
    }
}

impl FrameworkResolver for HonoResolver {
    fn framework_key(&self) -> &str {
        "hono"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        if !(file_path.ends_with(".ts")
            || file_path.ends_with(".js")
            || file_path.ends_with(".mjs")
            || file_path.ends_with(".cjs"))
        {
            return;
        }

        // --- Standard route methods: app.get("/path", ...) ---
        for cap in ROUTE_METHOD_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");

            // Skip non-HTTP methods (use, route, on, etc.)
            if !Self::is_http_method(method_name) {
                continue;
            }

            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name: None,
                method: Some(method_name.to_uppercase()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("hono".to_string()),
                route_kind: Some("http".to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Sub-router mounting: app.route("/prefix", subApp) ---
        for cap in ROUTE_MOUNT_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let sub_app = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: prefix.to_string(),
                handler_name: sub_app,
                method: Some("ROUTE".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("hono".to_string()),
                route_kind: Some("subrouter_mount".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
            });
        }

        // --- Middleware: app.use("/path", middleware) ---
        for cap in USE_RE.captures_iter(source) {
            let prefix = cap.get(1).map(|m| m.as_str());
            let handler_name = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            let route_path = prefix.unwrap_or("/").to_string();
            let route_kind = if prefix.is_some() {
                "middleware_mount"
            } else {
                "middleware"
            };

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path,
                handler_name,
                method: Some("USE".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("hono".to_string()),
                route_kind: Some(route_kind.to_string()),
                confidence: 0.75,
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
        // Resolve sub-router prefix mounting across files.
        //
        // Pattern: app.route("/api", apiRoutes)
        //   → find the file where `apiRoutes` is defined/exported
        //   → prepend "/api" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.

        // Step 1: collect mount points (prefix → sub-app variable name → mounting file)
        struct MountInfo {
            prefix: String,
            sub_app_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("subrouter_mount") {
                    if let Some(ref handler) = edge.handler_name {
                        let prefix = &edge.route_path;
                        if prefix != "/" && !prefix.is_empty() {
                            mounts.push(MountInfo {
                                prefix: prefix.clone(),
                                sub_app_name: handler.clone(),
                                mount_file: file_path.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Step 2: for each mount, find the target file via catalog and prepend prefix
        for mount in &mounts {
            let target_file = match catalog.lookup_symbol(&mount.sub_app_name, &mount.mount_file) {
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

    fn run_hono(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        HonoResolver.enrich_file(file_path, source, Language::TypeScript, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_hono_route_methods() {
        let source = r#"
import { Hono } from "hono";
const app = new Hono();

app.get("/users", (c) => c.json(users));
app.post("/users", async (c) => { });
app.delete("/users/:id", async (c) => { });
"#;
        let routes = run_hono("src/index.ts", source);
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(
            http_routes.len(),
            3,
            "expected 3 http routes, got {}",
            http_routes.len()
        );

        assert!(http_routes
            .iter()
            .any(|r| r.route_path == "/users" && r.method == Some("GET".into())));
        assert!(http_routes
            .iter()
            .any(|r| r.route_path == "/users" && r.method == Some("POST".into())));
        assert!(http_routes
            .iter()
            .any(|r| r.route_path == "/users/:id" && r.method == Some("DELETE".into())));

        assert!(http_routes
            .iter()
            .all(|r| r.framework.as_deref() == Some("hono")));
    }

    #[test]
    fn test_hono_subrouter_and_middleware() {
        let source = r#"
import { Hono } from "hono";
const app = new Hono();

app.route("/api", apiRoutes);
app.use("/api/*", authMiddleware);
app.use(cors);
"#;
        let routes = run_hono("src/app.ts", source);

        // Sub-router mount
        let mounts: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("subrouter_mount"))
            .collect();
        assert_eq!(mounts.len(), 1, "expected 1 subrouter mount");
        assert_eq!(mounts[0].route_path, "/api");
        assert_eq!(mounts[0].handler_name.as_deref(), Some("apiRoutes"));

        // Middleware
        let mw: Vec<_> = routes
            .iter()
            .filter(|r| {
                r.route_kind.as_deref() == Some("middleware_mount")
                    || r.route_kind.as_deref() == Some("middleware")
            })
            .collect();
        assert_eq!(mw.len(), 2, "expected 2 middleware entries");
        assert!(mw.iter().any(
            |r| r.route_path == "/api/*" && r.route_kind.as_deref() == Some("middleware_mount")
        ));
        assert!(mw
            .iter()
            .any(|r| r.route_path == "/" && r.route_kind.as_deref() == Some("middleware")));
    }

    #[test]
    fn test_hono_ignores_non_js_ts_files() {
        let source = r#"app.get("/test", handler);"#;
        let routes = run_hono("test.py", source);
        assert!(routes.is_empty(), "should ignore non-JS/TS files");
    }
}
