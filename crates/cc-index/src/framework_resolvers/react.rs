//! React framework resolver.
//!
//! - `enrich_file`: extracts React Router route definitions (JSX `<Route>`,
//!   route config objects, `lazy()` imports), Next.js file-based routes,
//!   and component export patterns.
//! - `resolve_cross_file`: resolves route → component cross-file links by
//!   looking up handler components in the symbol catalog and binding
//!   `handler_symbol_uid`.

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

/// React Router `<Route path="..." element={<Comp/>} />` or `component={Comp}`.
///
/// Captures: (1) route path, (2) component name
static ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<Route\s+[^>]*path=["'{]"?([^"'\}>]+)"?[^>]*(?:element|component)=\{?<?(\w+)"#)
        .expect("react route re")
});

/// `export default function ComponentName` — default function component export.
///
/// Captures: (1) component name
static EXPORT_DEFAULT_FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^export\s+default\s+function\s+([A-Z]\w*)"#)
        .expect("react export default fn re")
});

/// `export const ComponentName =` — named const export (PascalCase = component).
///
/// Captures: (1) component name
static EXPORT_CONST_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^export\s+(?:const|let)\s+([A-Z][a-zA-Z0-9]*)\s*="#)
        .expect("react export const re")
});

/// Route config object: `{ path: '/...', component: ComponentName }`.
///
/// Captures: (1) route path, (2) component name
static ROUTE_CONFIG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"path:\s*["']([^"']+)["']\s*,\s*component:\s*(\w+)"#)
        .expect("react route config re")
});

/// Route config with lazy component:
/// `{ path: '/...', component: lazy(() => import('./path')) }`
///
/// Captures: (1) route path, (2) import path
static ROUTE_LAZY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"path:\s*["']([^"']+)["']\s*,\s*component:\s*lazy\s*\(\s*\(\)\s*=>\s*import\s*\(\s*["']([^"']+)["']\s*\)"#)
        .expect("react route lazy re")
});

pub struct ReactComponentResolver;

impl ReactComponentResolver {
    /// Detect Next.js file-based routing from the file path.
    ///
    /// Patterns:
    /// - `pages/api/foo.ts`  → API route `/api/foo`
    /// - `pages/foo/bar.tsx` → page route `/foo/bar`
    /// - `app/foo/page.tsx`  → app-router page `/foo`
    /// - `app/foo/route.ts`  → app-router API route `/foo`
    fn detect_nextjs_file_route(file_path: &str) -> Option<(String, &'static str)> {
        // Normalize separators
        let normalized = file_path.replace('\\', "/");

        // --- pages/ directory ---
        if let Some(rest) = Self::strip_after_prefix(&normalized, "pages/") {
            let clean = Self::strip_extension(rest);
            // Remove trailing /index
            let clean = clean.strip_suffix("/index").unwrap_or(clean);
            let route = if clean.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", clean)
            };
            let kind = if clean.starts_with("api/") || clean == "api" {
                "nextjs_api"
            } else {
                "nextjs_page"
            };
            return Some((route, kind));
        }

        // --- app/ directory ---
        if let Some(rest) = Self::strip_after_prefix(&normalized, "app/") {
            let clean = Self::strip_extension(rest);
            // Must end with /page or /route (or be exactly page/route)
            if clean.ends_with("/page") || clean == "page" {
                let dir = clean
                    .strip_suffix("/page")
                    .or_else(|| if clean == "page" { Some("") } else { None })
                    .unwrap_or("");
                let route = if dir.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", dir)
                };
                return Some((route, "nextjs_page"));
            }
            if clean.ends_with("/route") || clean == "route" {
                let dir = clean
                    .strip_suffix("/route")
                    .or_else(|| if clean == "route" { Some("") } else { None })
                    .unwrap_or("");
                let route = if dir.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", dir)
                };
                return Some((route, "nextjs_api"));
            }
        }

        None
    }

    /// Find the suffix after a path prefix like `pages/` regardless of leading dirs.
    fn strip_after_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
        // Direct prefix
        if let Some(rest) = path.strip_prefix(prefix) {
            return Some(rest);
        }
        // After some parent like `src/pages/`
        if let Some(idx) = path.find(&format!("/{}", prefix)) {
            return Some(&path[idx + 1 + prefix.len()..]);
        }
        None
    }

    /// Strip the file extension (.ts, .tsx, .js, .jsx).
    fn strip_extension(path: &str) -> &str {
        for ext in &[".tsx", ".jsx", ".ts", ".js"] {
            if let Some(stripped) = path.strip_suffix(ext) {
                return stripped;
            }
        }
        path
    }
}

impl FrameworkResolver for ReactComponentResolver {
    fn framework_key(&self) -> &str {
        "react"
    }

    fn languages(&self) -> &[Language] {
        &[
            Language::TypeScript,
            Language::JavaScript,
            Language::Tsx,
            Language::Jsx,
        ]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // --- 1. React Router <Route> definitions ---
        for cap in ROUTE_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let component_name = cap.get(2).map(|m| m.as_str().to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name: component_name,
                method: Some("GET".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("react".to_string()),
                route_kind: Some("react_route".to_string()),
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
                resolution_strategy: None,
                resolution_confidence: None,
            });
        }

        // --- 1b. Route config objects: { path: '/...', component: Comp } ---
        // First collect lazy routes so we can skip them in the non-lazy pass.
        let mut lazy_route_paths: Vec<String> = Vec::new();
        for cap in ROUTE_LAZY_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let import_path = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            lazy_route_paths.push(route_path.to_string());

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            // Derive a component name from the import path (last segment, PascalCase)
            let component_name = import_path.rsplit('/').next().map(|s| s.to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name: component_name,
                method: Some("GET".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: Some(format!("lazy(() => import('{}'))", import_path)),
                router_symbol_uid: None,
                framework: Some("react".to_string()),
                route_kind: Some("react_lazy_route".to_string()),
                confidence: 0.78,
                parser_tier: ParserTier::Heuristic,
                resolution_strategy: None,
                resolution_confidence: None,
            });
        }

        for cap in ROUTE_CONFIG_RE.captures_iter(source) {
            let route_path = cap.get(1).map(|m| m.as_str()).unwrap_or("/");
            let component_name = cap.get(2).map(|m| m.as_str().to_string());

            // Skip if this route path was already captured as a lazy route
            if lazy_route_paths.contains(&route_path.to_string()) {
                continue;
            }

            let match_offset = cap.get(0).unwrap().start();
            let line = line_for_offset(source, match_offset);

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, line, 0),
                file_path: file_path.to_string(),
                route_path: route_path.to_string(),
                handler_name: component_name,
                method: Some("GET".to_string()),
                line,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("react".to_string()),
                route_kind: Some("react_route_config".to_string()),
                confidence: 0.78,
                parser_tier: ParserTier::Heuristic,
                resolution_strategy: None,
                resolution_confidence: None,
            });
        }

        // --- 2. Next.js file-based routing ---
        if let Some((route, kind)) = Self::detect_nextjs_file_route(file_path) {
            // Find the first exported component or function as the handler
            let handler_name = EXPORT_DEFAULT_FN_RE
                .captures(source)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            outcome.route_edges.push(RouteEdgeRecord {
                edge_id: StableId::edge_id("route", file_path, 1, 0),
                file_path: file_path.to_string(),
                route_path: route,
                handler_name,
                method: if kind == "nextjs_api" {
                    None // API routes can serve any method
                } else {
                    Some("GET".to_string())
                },
                line: 1,
                start_col: 0,
                end_line: None,
                end_col: 0,
                handler_symbol_id: None,
                handler_symbol_uid: None,
                handler_expr: None,
                router_symbol_uid: None,
                framework: Some("nextjs".to_string()),
                route_kind: Some(kind.to_string()),
                confidence: 0.85,
                parser_tier: ParserTier::Heuristic,
                resolution_strategy: None,
                resolution_confidence: None,
            });
        }

        // --- 3. Component export detection (metadata enrichment) ---
        // We record component exports as route_kind "component_export" for symbol linkage.
        // Only if we haven't already found a Next.js route for this file.
        let has_nextjs_route = outcome
            .route_edges
            .iter()
            .any(|r| r.framework.as_deref() == Some("nextjs"));
        if !has_nextjs_route {
            for cap in EXPORT_DEFAULT_FN_RE.captures_iter(source) {
                let name = cap.get(1).map(|m| m.as_str().to_string());
                let offset = cap.get(0).unwrap().start();
                let line = line_for_offset(source, offset);

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("component", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: String::new(),
                    handler_name: name,
                    method: None,
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: None,
                    router_symbol_uid: None,
                    framework: Some("react".to_string()),
                    route_kind: Some("component_export".to_string()),
                    confidence: 0.70,
                    parser_tier: ParserTier::Heuristic,
                    resolution_strategy: None,
                    resolution_confidence: None,
                });
            }

            for cap in EXPORT_CONST_RE.captures_iter(source) {
                let name = cap.get(1).map(|m| m.as_str().to_string());
                let offset = cap.get(0).unwrap().start();
                let line = line_for_offset(source, offset);

                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("component", file_path, line, 0),
                    file_path: file_path.to_string(),
                    route_path: String::new(),
                    handler_name: name,
                    method: None,
                    line,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: None,
                    router_symbol_uid: None,
                    framework: Some("react".to_string()),
                    route_kind: Some("component_export".to_string()),
                    confidence: 0.65,
                    parser_tier: ParserTier::Heuristic,
                    resolution_strategy: None,
                    resolution_confidence: None,
                });
            }
        }
    }

    fn resolver_tier(&self) -> &'static str {
        "full"
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Resolve handler_symbol_uid for route edges that reference components
        // in other files. Covers react_route, react_route_config, and
        // react_lazy_route kinds.
        //
        // For lazy routes, the handler_name is derived from the import path
        // (e.g. "./pages/UserDetail" → "UserDetail") which matches the
        // default export name in the target file.

        let route_kinds: &[&str] = &["react_route", "react_route_config", "react_lazy_route"];

        for (file_path, outcome) in outcomes.iter_mut() {
            let fp = file_path.clone();
            for edge in &mut outcome.route_edges {
                // Only process React route edges without a resolved UID
                if edge.handler_symbol_uid.is_some() {
                    continue;
                }
                let kind = edge.route_kind.as_deref().unwrap_or("");
                if !route_kinds.contains(&kind) {
                    continue;
                }

                if let Some(ref handler_name) = edge.handler_name {
                    if let Some((uid, _target_file)) = catalog.lookup_symbol(handler_name, &fp) {
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

    fn run_react(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        ReactComponentResolver.enrich_file(file_path, source, Language::Tsx, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_react_router_routes() {
        let source = r#"
import { Route, Routes } from "react-router-dom";

function App() {
    return (
        <Routes>
            <Route path="/users" element={<UserList/>} />
            <Route path="/users/:id" component={UserDetail} />
        </Routes>
    );
}
"#;
        let routes = run_react("src/App.tsx", source);
        let react_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_route"))
            .collect();
        assert_eq!(
            react_routes.len(),
            2,
            "expected 2 react routes, got {}",
            react_routes.len()
        );
        assert!(react_routes
            .iter()
            .any(|r| r.route_path == "/users" && r.handler_name.as_deref() == Some("UserList")));
        assert!(react_routes.iter().any(
            |r| r.route_path == "/users/:id" && r.handler_name.as_deref() == Some("UserDetail")
        ));
    }

    #[test]
    fn test_nextjs_pages_file_routing() {
        let source = r#"
export default function UsersPage() {
    return <div>Users</div>;
}
"#;
        // pages/ directory → page route
        let routes = run_react("pages/users.tsx", source);
        let nextjs_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.framework.as_deref() == Some("nextjs"))
            .collect();
        assert_eq!(nextjs_routes.len(), 1);
        assert_eq!(nextjs_routes[0].route_path, "/users");
        assert_eq!(nextjs_routes[0].route_kind.as_deref(), Some("nextjs_page"));
        assert_eq!(nextjs_routes[0].handler_name.as_deref(), Some("UsersPage"));

        // pages/api/ → API route
        let api_routes = run_react("pages/api/users.ts", "export default function handler() {}");
        let nextjs_api: Vec<_> = api_routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("nextjs_api"))
            .collect();
        assert_eq!(nextjs_api.len(), 1);
        assert_eq!(nextjs_api[0].route_path, "/api/users");
    }

    #[test]
    fn test_nextjs_app_router() {
        let source = r#"
export default function UserPage() {
    return <div>User</div>;
}
"#;
        let routes = run_react("app/users/page.tsx", source);
        let nextjs_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.framework.as_deref() == Some("nextjs"))
            .collect();
        assert_eq!(nextjs_routes.len(), 1);
        assert_eq!(nextjs_routes[0].route_path, "/users");
        assert_eq!(nextjs_routes[0].route_kind.as_deref(), Some("nextjs_page"));

        // app/users/route.ts → API route
        let api_routes = run_react("app/users/route.ts", "export async function GET() {}");
        let nextjs_api: Vec<_> = api_routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("nextjs_api"))
            .collect();
        assert_eq!(nextjs_api.len(), 1);
        assert_eq!(nextjs_api[0].route_path, "/users");
    }

    #[test]
    fn test_route_config_objects() {
        let source = r#"
import UserList from './pages/UserList';
import UserDetail from './pages/UserDetail';

const routes = [
    { path: '/users', component: UserList },
    { path: '/users/:id', component: UserDetail },
];
"#;
        let routes = run_react("src/routes.tsx", source);
        let config_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_route_config"))
            .collect();
        assert_eq!(
            config_routes.len(),
            2,
            "expected 2 route config entries, got {}",
            config_routes.len()
        );
        assert!(config_routes
            .iter()
            .any(|r| r.route_path == "/users" && r.handler_name.as_deref() == Some("UserList")));
        assert!(config_routes.iter().any(
            |r| r.route_path == "/users/:id" && r.handler_name.as_deref() == Some("UserDetail")
        ));
    }

    #[test]
    fn test_lazy_import_routes() {
        let source = r#"
import UserList from './pages/UserList';

const routes = [
    { path: '/users', component: UserList },
    { path: '/users/:id', component: lazy(() => import('./pages/UserDetail')) },
];
"#;
        let routes = run_react("src/routes.tsx", source);

        // The lazy route should be detected as react_lazy_route
        let lazy_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_lazy_route"))
            .collect();
        assert_eq!(
            lazy_routes.len(),
            1,
            "expected 1 lazy route, got {}",
            lazy_routes.len()
        );
        assert_eq!(lazy_routes[0].route_path, "/users/:id");
        assert_eq!(
            lazy_routes[0].handler_name.as_deref(),
            Some("UserDetail"),
            "handler_name should be derived from import path"
        );
        assert!(
            lazy_routes[0]
                .handler_expr
                .as_deref()
                .unwrap()
                .contains("./pages/UserDetail"),
            "handler_expr should contain the import path"
        );

        // The non-lazy route should be detected as react_route_config
        let config_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_route_config"))
            .collect();
        assert_eq!(
            config_routes.len(),
            1,
            "expected 1 non-lazy config route, got {}",
            config_routes.len()
        );
        assert_eq!(config_routes[0].route_path, "/users");
    }

    #[test]
    fn test_cross_file_resolution() {
        use cc_model::symbol::{SymbolKind, SymbolRecord};

        // Set up a catalog with a symbol in another file
        let mut catalog = crate::resolver::SymbolCatalog::new();
        catalog.add_symbols(&[SymbolRecord {
            symbol_id: "sym_userlist".to_string(),
            symbol_uid: Some("uid_userlist".to_string()),
            name: "UserList".to_string(),
            file_path: "src/pages/UserList.tsx".to_string(),
            kind: SymbolKind::Function,
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            container: None,
            qname: None,
            export_name: Some("UserList".to_string()),
            is_default_export: true,
            scope_id: None,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.9,
            parent_symbol_id: None,
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }]);

        // Build route edges in a different file referencing UserList
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        let source = r#"
import { Route, Routes } from "react-router-dom";
import UserList from './pages/UserList';

function App() {
    return (
        <Routes>
            <Route path="/users" element={<UserList/>} />
        </Routes>
    );
}
"#;
        ReactComponentResolver.enrich_file(
            "src/App.tsx",
            source,
            Language::Tsx,
            &mut outcome,
            &ctx,
        );

        // Verify the route was extracted
        let react_routes: Vec<_> = outcome
            .route_edges
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_route"))
            .collect();
        assert_eq!(react_routes.len(), 1);
        assert!(react_routes[0].handler_symbol_uid.is_none());

        // Run cross-file resolution
        let mut file_outcomes = vec![("src/App.tsx".to_string(), outcome)];
        ReactComponentResolver.resolve_cross_file(&catalog, &mut file_outcomes, &ctx);

        // Verify UID was resolved
        let resolved = &file_outcomes[0].1.route_edges;
        let react_routes: Vec<_> = resolved
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("react_route"))
            .collect();
        assert_eq!(react_routes.len(), 1);
        assert_eq!(
            react_routes[0].handler_symbol_uid.as_deref(),
            Some("uid_userlist"),
            "handler_symbol_uid should be resolved from catalog"
        );
    }

    #[test]
    fn test_component_exports() {
        let source = r#"
export default function Dashboard() {
    return <div>Dashboard</div>;
}

export const Sidebar = () => {
    return <nav>Sidebar</nav>;
};
"#;
        let routes = run_react("src/components/Dashboard.tsx", source);
        let component_exports: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("component_export"))
            .collect();
        assert_eq!(
            component_exports.len(),
            2,
            "expected 2 component exports, got {}",
            component_exports.len()
        );
        assert!(component_exports
            .iter()
            .any(|r| r.handler_name.as_deref() == Some("Dashboard")));
        assert!(component_exports
            .iter()
            .any(|r| r.handler_name.as_deref() == Some("Sidebar")));
    }
}
