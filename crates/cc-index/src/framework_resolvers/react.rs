//! React framework resolver.
//!
//! - `enrich_file`: extracts React Router route definitions, Next.js file-based
//!   routes, and component export patterns.
//! - `resolve_cross_file`: no-op (JSX component references handled by dispatch_synthesis)

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

/// React Router `<Route path="..." element={<Comp/>} />` or `component={Comp}`.
///
/// Captures: (1) route path, (2) component name
static ROUTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"<Route\s+[^>]*path=["'{]"?([^"'\}>]+)"?[^>]*(?:element|component)=\{?<?(\w+)"#,
    )
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

pub struct ReactComponentResolver;

impl ReactComponentResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

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
            let line = Self::line_for_offset(source, match_offset);

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
                let line = Self::line_for_offset(source, offset);

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
                });
            }

            for cap in EXPORT_CONST_RE.captures_iter(source) {
                let name = cap.get(1).map(|m| m.as_str().to_string());
                let offset = cap.get(0).unwrap().start();
                let line = Self::line_for_offset(source, offset);

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
        // No-op: JSX component references are already handled by dispatch_synthesis.
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
        assert_eq!(
            nextjs_routes[0].handler_name.as_deref(),
            Some("UsersPage")
        );

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
