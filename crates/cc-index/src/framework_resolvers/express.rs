//! Express.js framework resolver.
//!
//! - `enrich_file`: extracts route definitions from Express-style APIs
//! - `resolve_cross_file`: resolves sub-router prefix mounting

#[cfg(test)]
use cc_model::edge::RouteEdgeRecord;
use cc_model::parse::ParseOutcome;
use cc_model::{Language, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

use super::{
    line_for_offset, push_route_edge, FrameworkResolver, ProjectFrameworkContext, RouteEdgeSpec,
};

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Standard route methods: app.get("/path", ..., handler) or router.post("/path", ..., handler)
///
/// Matches:
///   app.get("/users", getUsers)
///   router.post("/items", validate, createItem)
///   app.delete("/users/:id", auth, deleteUser)
///
/// Captures: (1) HTTP method, (2) route path, (3) last handler identifier
static ROUTE_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)(?:app|router|[a-zA-Z]\w*)\.(\w+)\(\s*["']([^"']+)["']\s*(?:,\s*[\w.]+)*\s*,\s*(\w+)"#,
    )
    .expect("express route method re")
});

/// app.use middleware mounting: app.use("/prefix", routerName) or app.use(middlewareName)
///
/// Captures: (1) optional path prefix, (2) handler/router identifier
static APP_USE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)app\.use\(\s*(?:["']([^"']+)["']\s*,\s*)?(\w+)"#).expect("express app.use re")
});

/// Valid Express HTTP method names.
const EXPRESS_HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "head", "options", "all",
];

pub struct ExpressResolver;

impl ExpressResolver {
    /// Check if a captured method name is a valid HTTP method.
    fn is_http_method(method: &str) -> bool {
        EXPRESS_HTTP_METHODS.contains(&method.to_lowercase().as_str())
    }
}

impl FrameworkResolver for ExpressResolver {
    fn framework_key(&self) -> &str {
        "express"
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn languages(&self) -> &[Language] {
        &[Language::JavaScript, Language::TypeScript]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // Only process JS/TS files
        if !(file_path.ends_with(".js")
            || file_path.ends_with(".ts")
            || file_path.ends_with(".mjs")
            || file_path.ends_with(".cjs"))
        {
            return;
        }

        // --- Standard route methods: app.get("/path", handler) ---
        for cap in ROUTE_METHOD_RE.captures_iter(source) {
            let method_name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            if !Self::is_http_method(method_name) {
                continue;
            }

            let route_path = cap.get(2).map(|m| m.as_str()).unwrap_or("/");
            let handler_name = cap.get(3).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            push_route_edge(
                outcome,
                file_path,
                line,
                0,
                RouteEdgeSpec {
                    route_path: route_path.to_string(),
                    handler_name,
                    method: Some(method_name.to_uppercase()),
                    framework: "express",
                    route_kind: "http",
                    confidence: 0.85,
                    parser_tier: ParserTier::Heuristic,
                },
            );
        }

        // --- app.use middleware mounting ---
        for cap in APP_USE_RE.captures_iter(source) {
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

            push_route_edge(
                outcome,
                file_path,
                line,
                0,
                RouteEdgeSpec {
                    route_path,
                    handler_name,
                    method: Some("USE".to_string()),
                    framework: "express",
                    route_kind,
                    confidence: 0.75,
                    parser_tier: ParserTier::Heuristic,
                },
            );
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
        // Pattern: app.use("/api", apiRouter)
        //   → find the file where `apiRouter` is defined/exported
        //   → prepend "/api" to all http route_edges in that file
        //
        // Also resolve handler_symbol_uid for route_edges whose handler_name
        // is set but handler_symbol_uid is not.

        // Step 1: collect mount points (prefix → router variable name → mounting file)
        struct MountInfo {
            prefix: String,
            router_name: String,
            mount_file: String,
        }

        let mut mounts: Vec<MountInfo> = Vec::new();
        for (file_path, outcome) in outcomes.iter() {
            for edge in &outcome.route_edges {
                if edge.route_kind.as_deref() == Some("middleware_mount") {
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

        // Step 2: for each mount, find the target file via catalog and prepend prefix
        for mount in &mounts {
            // Look up where the router variable is defined
            let target_file = match catalog.lookup_symbol(&mount.router_name, &mount.mount_file) {
                Some((_, file)) if file != mount.mount_file => file,
                _ => continue,
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

    fn run_express(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        ExpressResolver.enrich_file(file_path, source, Language::JavaScript, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_express_app_get() {
        let source = r#"
const express = require("express");
const app = express();

app.get("/users", getUsers);
"#;
        let routes = run_express("src/app.js", source);
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(
            http_routes.len(),
            1,
            "expected 1 http route, got {}",
            http_routes.len()
        );
        assert_eq!(http_routes[0].route_path, "/users");
        assert_eq!(http_routes[0].method, Some("GET".into()));
        assert_eq!(http_routes[0].handler_name.as_deref(), Some("getUsers"));
        assert_eq!(http_routes[0].framework.as_deref(), Some("express"));
    }

    #[test]
    fn test_express_router_post() {
        let source = r#"
const router = express.Router();

router.post("/items", createItem);
"#;
        let routes = run_express("src/routes/items.ts", source);
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(
            http_routes.len(),
            1,
            "expected 1 http route, got {}",
            http_routes.len()
        );
        assert_eq!(http_routes[0].route_path, "/items");
        assert_eq!(http_routes[0].method, Some("POST".into()));
        assert_eq!(http_routes[0].handler_name.as_deref(), Some("createItem"));
    }

    #[test]
    fn test_express_multiple_routes() {
        let source = r#"
const express = require("express");
const app = express();

app.get("/users", listUsers);
app.post("/users", createUser);
app.delete("/users/:id", auth, deleteUser);
app.put("/users/:id", validate, updateUser);
app.patch("/users/:id/status", patchStatus);
"#;
        let routes = run_express("src/app.js", source);
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert_eq!(
            http_routes.len(),
            5,
            "expected 5 http routes, got {}",
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
            .any(|r| r.route_path == "/users/:id" && r.method == Some("PUT".into())));
        assert!(http_routes
            .iter()
            .any(|r| r.route_path == "/users/:id/status" && r.method == Some("PATCH".into())));
    }

    #[test]
    fn test_express_app_use_middleware() {
        let source = r#"
app.use("/api", apiRouter);
app.use(cors);
"#;
        let routes = run_express("src/app.js", source);
        let middleware_routes: Vec<_> = routes
            .iter()
            .filter(|r| {
                r.route_kind.as_deref() == Some("middleware_mount")
                    || r.route_kind.as_deref() == Some("middleware")
            })
            .collect();
        assert_eq!(
            middleware_routes.len(),
            2,
            "expected 2 middleware entries, got {}",
            middleware_routes.len()
        );

        assert!(
            middleware_routes
                .iter()
                .any(|r| r.route_path == "/api"
                    && r.route_kind.as_deref() == Some("middleware_mount"))
        );
        assert!(middleware_routes
            .iter()
            .any(|r| r.route_path == "/" && r.route_kind.as_deref() == Some("middleware")));
    }

    #[test]
    fn test_express_ignores_non_js_files() {
        let source = r#"app.get("/test", handler);"#;
        let routes = run_express("test.py", source);
        assert!(routes.is_empty(), "should ignore non-JS/TS files");
    }

    #[test]
    fn test_express_ignores_non_http_methods() {
        let source = r#"
app.listen(3000, callback);
app.set("view engine", pug);
"#;
        let routes = run_express("src/app.js", source);
        let http_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("http"))
            .collect();
        assert!(
            http_routes.is_empty(),
            "should not extract non-HTTP methods like listen/set"
        );
    }

    /// Guard for the Phase 4d optimization: `resolve_cross_file` mutates route
    /// edges *in place* (e.g. prepending a mount prefix) without changing the
    /// edge count. The new move-based merge in `phase_resolve` preserves these
    /// mutations; the previous length-only merge would have dropped them. This
    /// test pins the cross-file prefix-propagation behaviour so any regression
    /// in the merge path is caught.
    #[test]
    fn cross_file_prefix_propagation_mutates_in_place() {
        use crate::resolver::SymbolCatalog;
        use cc_model::symbol::{SymbolKind, SymbolRecord};
        use cc_model::ParserTier as Tier;

        let router_symbol = SymbolRecord {
            symbol_id: "sub#apiRouter".into(),
            file_path: "src/routes/api.js".into(),
            name: "apiRouter".into(),
            kind: SymbolKind::Variable,
            container: None,
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: Tier::TreeSitter,
            parser_confidence: 0.9,
            qname: Some("apiRouter".into()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: Some("apiRouter".into()),
            is_default_export: false,
            symbol_uid: Some("uid_api_router".into()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(std::slice::from_ref(&router_symbol));

        // Mount file: app.use("/api", apiRouter) → middleware_mount edge.
        let mount_edge = RouteEdgeRecord {
            edge_id: "mount#1".into(),
            file_path: "src/app.js".into(),
            route_path: "/api".into(),
            handler_name: Some("apiRouter".into()),
            method: None,
            line: 1,
            start_col: 0,
            end_line: None,
            end_col: 0,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: None,
            router_symbol_uid: None,
            framework: Some("express".into()),
            route_kind: Some("middleware_mount".into()),
            confidence: 0.9,
            parser_tier: ParserTier::TreeSitter,
        };
        // Sub-router file: router.get("/users", ...) → http edge with bare path.
        let sub_edge = RouteEdgeRecord {
            edge_id: "sub#1".into(),
            file_path: "src/routes/api.js".into(),
            route_path: "/users".into(),
            handler_name: Some("getUsers".into()),
            method: Some("GET".into()),
            line: 2,
            start_col: 0,
            end_line: None,
            end_col: 0,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: None,
            router_symbol_uid: None,
            framework: Some("express".into()),
            route_kind: Some("http".into()),
            confidence: 0.9,
            parser_tier: ParserTier::TreeSitter,
        };

        let mount_outcome = ParseOutcome {
            route_edges: vec![mount_edge],
            ..Default::default()
        };
        let sub_outcome = ParseOutcome {
            route_edges: vec![sub_edge],
            ..Default::default()
        };

        let mut outcomes = vec![
            ("src/app.js".to_string(), mount_outcome),
            ("src/routes/api.js".to_string(), sub_outcome),
        ];

        let edge_count_before = outcomes[1].1.route_edges.len();

        ExpressResolver.resolve_cross_file(
            &catalog,
            &mut outcomes,
            &ProjectFrameworkContext::new(),
        );

        // Edge count is unchanged (the mutation is in place, not additive).
        assert_eq!(outcomes[1].1.route_edges.len(), edge_count_before);
        // The sub-router's http route now carries the mounted prefix.
        assert_eq!(
            outcomes[1].1.route_edges[0].route_path, "/api/users",
            "mount prefix must be prepended in place to the sub-router route"
        );
    }
}
