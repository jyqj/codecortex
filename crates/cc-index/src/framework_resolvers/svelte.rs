//! Svelte / SvelteKit framework resolver.
//!
//! - `enrich_file`: extracts SvelteKit file-based routes (+page.svelte, +server.ts,
//!   +layout.svelte) and HTTP method exports from +server files.
//! - `resolve_cross_file`: no-op

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

/// HTTP method export in +server.ts/+server.js:
/// `export function GET(`, `export async function POST(`, etc.
///
/// Captures: (1) HTTP method name (GET, POST, PUT, DELETE, PATCH, etc.)
static HTTP_METHOD_EXPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^export\s+(?:async\s+)?function\s+(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\s*\("#)
        .expect("svelte http method export re")
});

/// Valid SvelteKit route file suffixes.
const SVELTEKIT_ROUTE_FILES: &[(&str, &str)] = &[
    ("+page.svelte", "page"),
    ("+page.ts", "page_load"),
    ("+page.js", "page_load"),
    ("+page.server.ts", "page_server_load"),
    ("+page.server.js", "page_server_load"),
    ("+layout.svelte", "layout"),
    ("+layout.ts", "layout_load"),
    ("+layout.js", "layout_load"),
    ("+layout.server.ts", "layout_server_load"),
    ("+layout.server.js", "layout_server_load"),
    ("+server.ts", "api_endpoint"),
    ("+server.js", "api_endpoint"),
    ("+error.svelte", "error_boundary"),
];

pub struct SvelteResolver;

impl SvelteResolver {
    /// Compute 1-based line number for a byte offset.
    fn line_for_offset(source: &str, offset: usize) -> u32 {
        source[..offset].matches('\n').count() as u32 + 1
    }

    /// Detect SvelteKit file-based routing from the file path.
    ///
    /// Patterns:
    /// - `src/routes/users/+page.svelte` → /users (page)
    /// - `src/routes/users/+server.ts` → /users (api_endpoint)
    /// - `src/routes/users/[id]/+page.svelte` → /users/:id (page)
    /// - `src/routes/+layout.svelte` → / (layout)
    ///
    /// Returns `(route_path, route_kind)` if the file matches a SvelteKit convention.
    fn detect_sveltekit_file_route(file_path: &str) -> Option<(String, &'static str)> {
        let normalized = file_path.replace('\\', "/");

        let rest = Self::strip_after_prefix(&normalized, "src/routes/")?;

        // Match against known SvelteKit route file suffixes
        for &(suffix, kind) in SVELTEKIT_ROUTE_FILES {
            if rest.ends_with(suffix) {
                let dir = rest.strip_suffix(suffix).unwrap_or("");
                let dir = dir.strip_suffix('/').unwrap_or(dir);

                // Convert [param] segments to :param
                let route = if dir.is_empty() {
                    "/".to_string()
                } else {
                    let converted = dir
                        .split('/')
                        .map(|seg| {
                            if seg.starts_with('[') && seg.ends_with(']') {
                                // Handle [...rest] (catch-all) and [param]
                                let inner = &seg[1..seg.len() - 1];
                                if let Some(rest_param) = inner.strip_prefix("...") {
                                    format!(":{}*", rest_param)
                                } else {
                                    format!(":{}", inner)
                                }
                            } else if seg.starts_with('(') && seg.ends_with(')') {
                                // Route groups like (app) — skip from path
                                String::new()
                            } else {
                                seg.to_string()
                            }
                        })
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("/");
                    if converted.is_empty() {
                        "/".to_string()
                    } else {
                        format!("/{}", converted)
                    }
                };

                return Some((route, kind));
            }
        }

        None
    }

    /// Find the suffix after a path prefix like `src/routes/` regardless of leading dirs.
    fn strip_after_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
        if let Some(rest) = path.strip_prefix(prefix) {
            return Some(rest);
        }
        if let Some(idx) = path.find(&format!("/{}", prefix)) {
            return Some(&path[idx + 1 + prefix.len()..]);
        }
        None
    }
}

impl FrameworkResolver for SvelteResolver {
    fn framework_key(&self) -> &str {
        "sveltekit"
    }

    fn languages(&self) -> &[Language] {
        &[Language::TypeScript, Language::JavaScript, Language::Svelte]
    }

    fn enrich_file(
        &self,
        file_path: &str,
        source: &str,
        _lang: Language,
        outcome: &mut ParseOutcome,
        _ctx: &ProjectFrameworkContext,
    ) {
        // --- 1. SvelteKit file-based routing ---
        if let Some((route, kind)) = Self::detect_sveltekit_file_route(file_path) {
            // For +server files, we detect individual HTTP method exports below.
            // For other route files, emit a single route edge.
            if kind != "api_endpoint" {
                outcome.route_edges.push(RouteEdgeRecord {
                    edge_id: StableId::edge_id("route", file_path, 1, 0),
                    file_path: file_path.to_string(),
                    route_path: route.clone(),
                    handler_name: None,
                    method: Some("GET".to_string()),
                    line: 1,
                    start_col: 0,
                    end_line: None,
                    end_col: 0,
                    handler_symbol_id: None,
                    handler_symbol_uid: None,
                    handler_expr: None,
                    router_symbol_uid: None,
                    framework: Some("sveltekit".to_string()),
                    route_kind: Some(kind.to_string()),
                    confidence: 0.85,
                    parser_tier: ParserTier::Heuristic,
                });
            }

            // --- 2. +server.ts HTTP method exports ---
            if kind == "api_endpoint" {
                let mut found_method = false;
                for cap in HTTP_METHOD_EXPORT_RE.captures_iter(source) {
                    let method = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
                    let offset = cap.get(0).unwrap().start();
                    let line = Self::line_for_offset(source, offset);

                    outcome.route_edges.push(RouteEdgeRecord {
                        edge_id: StableId::edge_id("route", file_path, line, 0),
                        file_path: file_path.to_string(),
                        route_path: route.clone(),
                        handler_name: Some(method.to_string()),
                        method: Some(method.to_string()),
                        line,
                        start_col: 0,
                        end_line: None,
                        end_col: 0,
                        handler_symbol_id: None,
                        handler_symbol_uid: None,
                        handler_expr: None,
                        router_symbol_uid: None,
                        framework: Some("sveltekit".to_string()),
                        route_kind: Some("api_endpoint".to_string()),
                        confidence: 0.85,
                        parser_tier: ParserTier::Heuristic,
                    });
                    found_method = true;
                }

                // If no explicit methods found, emit a generic endpoint
                if !found_method {
                    outcome.route_edges.push(RouteEdgeRecord {
                        edge_id: StableId::edge_id("route", file_path, 1, 0),
                        file_path: file_path.to_string(),
                        route_path: route,
                        handler_name: None,
                        method: None,
                        line: 1,
                        start_col: 0,
                        end_line: None,
                        end_col: 0,
                        handler_symbol_id: None,
                        handler_symbol_uid: None,
                        handler_expr: None,
                        router_symbol_uid: None,
                        framework: Some("sveltekit".to_string()),
                        route_kind: Some("api_endpoint".to_string()),
                        confidence: 0.75,
                        parser_tier: ParserTier::Heuristic,
                    });
                }
            }
        }
    }

    fn resolve_cross_file(
        &self,
        _catalog: &crate::resolver::SymbolCatalog,
        _outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // No-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_svelte(file_path: &str, source: &str) -> Vec<RouteEdgeRecord> {
        let mut outcome = ParseOutcome::default();
        let ctx = ProjectFrameworkContext::new();
        SvelteResolver.enrich_file(file_path, source, Language::TypeScript, &mut outcome, &ctx);
        outcome.route_edges
    }

    #[test]
    fn test_sveltekit_page_routes() {
        let source = r#"<h1>Users</h1>"#;

        // +page.svelte
        let routes = run_svelte("src/routes/users/+page.svelte", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/users");
        assert_eq!(routes[0].route_kind.as_deref(), Some("page"));
        assert_eq!(routes[0].framework.as_deref(), Some("sveltekit"));

        // Dynamic param: [id]
        let routes = run_svelte("src/routes/users/[id]/+page.svelte", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/users/:id");

        // Root layout
        let routes = run_svelte("src/routes/+layout.svelte", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/");
        assert_eq!(routes[0].route_kind.as_deref(), Some("layout"));
    }

    #[test]
    fn test_sveltekit_server_endpoints() {
        let source = r#"
import { json } from "@sveltejs/kit";

export function GET({ url }) {
    return json({ users: [] });
}

export async function POST({ request }) {
    const data = await request.json();
    return json({ created: true });
}
"#;
        let routes = run_svelte("src/routes/api/users/+server.ts", source);
        let api_routes: Vec<_> = routes
            .iter()
            .filter(|r| r.route_kind.as_deref() == Some("api_endpoint"))
            .collect();
        assert_eq!(
            api_routes.len(),
            2,
            "expected 2 API methods, got {}",
            api_routes.len()
        );
        assert!(api_routes
            .iter()
            .any(|r| r.method.as_deref() == Some("GET") && r.route_path == "/api/users"));
        assert!(api_routes
            .iter()
            .any(|r| r.method.as_deref() == Some("POST") && r.route_path == "/api/users"));
    }

    #[test]
    fn test_sveltekit_route_groups_and_catchall() {
        let source = r#"<h1>Page</h1>"#;

        // Route group: (app) directory is stripped
        let routes = run_svelte("src/routes/(app)/dashboard/+page.svelte", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/dashboard");

        // Catch-all: [...rest]
        let routes = run_svelte("src/routes/[...rest]/+page.svelte", source);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_path, "/:rest*");
    }
}
