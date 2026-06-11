//! Svelte / SvelteKit framework resolver.
//!
//! - `enrich_file`: extracts SvelteKit file-based routes (+page.svelte, +server.ts,
//!   +layout.svelte) and HTTP method exports from +server files.
//! - `resolve_cross_file`: resolves SvelteKit file-based routing pairs —
//!   links `+page.svelte` to `+page.server.ts` load/actions in the same
//!   directory, enabling trace to follow page → server data loading paths.

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

/// HTTP method export in +server.ts/+server.js:
/// `export function GET(`, `export async function POST(`, etc.
///
/// Captures: (1) HTTP method name (GET, POST, PUT, DELETE, PATCH, etc.)
static HTTP_METHOD_EXPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^export\s+(?:async\s+)?function\s+(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\s*\("#,
    )
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

    fn resolver_tier(&self) -> &'static str {
        "full"
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
                    resolution_strategy: None,
                    resolution_confidence: None,
                });
            }

            // --- 2. +server.ts HTTP method exports ---
            if kind == "api_endpoint" {
                let mut found_method = false;
                for cap in HTTP_METHOD_EXPORT_RE.captures_iter(source) {
                    let method = cap.get(1).map(|m| m.as_str()).unwrap_or("GET");
                    let offset = cap.get(0).unwrap().start();
                    let line = line_for_offset(source, offset);

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
                        resolution_strategy: None,
                        resolution_confidence: None,
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
                        resolution_strategy: None,
                        resolution_confidence: None,
                    });
                }
            }
        }
    }

    fn resolve_cross_file(
        &self,
        catalog: &crate::resolver::SymbolCatalog,
        outcomes: &mut [(String, ParseOutcome)],
        _ctx: &ProjectFrameworkContext,
    ) {
        // Phase 1: Collect SvelteKit page files and their corresponding server files.
        //
        // For each +page.svelte, find the +page.server.{ts,js} in the same
        // directory. Then look up exported `load` / `actions` symbols from
        // the server file in the catalog and bind them to the page's route
        // edge as handler_symbol_uid.
        //
        // Similarly, link +layout.svelte to +layout.server.{ts,js}.

        // Build a map of directory → server file path from the outcomes list.
        let page_suffixes: &[(&str, &str)] = &[
            ("+page.svelte", "+page.server.ts"),
            ("+page.svelte", "+page.server.js"),
            ("+layout.svelte", "+layout.server.ts"),
            ("+layout.svelte", "+layout.server.js"),
        ];

        // Collect server file paths indexed by directory.
        let mut server_files: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for (file_path, _) in outcomes.iter() {
            let normalized = file_path.replace('\\', "/");
            for &(_, server_suffix) in page_suffixes {
                if normalized.ends_with(server_suffix) {
                    let dir = normalized
                        .rsplit_once('/')
                        .map(|(d, _)| d.to_string())
                        .unwrap_or_default();
                    server_files.entry(dir).or_default().push(file_path.clone());
                }
            }
        }

        // Phase 2: For each page/layout file, resolve the server load symbol.
        //
        // We look up "load" and "actions" function symbols exported from the
        // paired server file. The first matching UID is bound to the page's
        // route edge as handler_symbol_uid, creating a semantic link.
        let server_function_names: &[&str] = &["load", "actions"];

        for (file_path, outcome) in outcomes.iter_mut() {
            let normalized = file_path.replace('\\', "/");

            // Check if this is a page or layout file
            let is_page = normalized.ends_with("+page.svelte");
            let is_layout = normalized.ends_with("+layout.svelte");
            if !is_page && !is_layout {
                continue;
            }

            let dir = normalized
                .rsplit_once('/')
                .map(|(d, _)| d.to_string())
                .unwrap_or_default();

            // Find server files in the same directory
            let paired_servers = match server_files.get(&dir) {
                Some(v) => v.clone(),
                None => continue,
            };

            // Look up load/actions symbols from each server file
            let mut resolved_uid: Option<String> = None;
            let mut server_file_path: Option<String> = None;
            'outer: for srv_file in &paired_servers {
                for &fn_name in server_function_names {
                    let matches = catalog.lookup_all_by_name(fn_name);
                    for (uid, sym_file, _kind) in &matches {
                        if sym_file == srv_file && !uid.is_empty() {
                            resolved_uid = Some(uid.clone());
                            server_file_path = Some(srv_file.clone());
                            break 'outer;
                        }
                    }
                }
            }

            // Bind the resolved UID to the page/layout route edge
            if let Some(uid) = resolved_uid {
                let expected_kind = if is_page { "page" } else { "layout" };
                for edge in &mut outcome.route_edges {
                    if edge.handler_symbol_uid.is_some() {
                        continue;
                    }
                    if edge.route_kind.as_deref() == Some(expected_kind)
                        && edge.framework.as_deref() == Some("sveltekit")
                    {
                        edge.handler_symbol_uid = Some(uid.clone());
                        edge.handler_name = Some(format!(
                            "load@{}",
                            server_file_path.as_deref().unwrap_or("")
                        ));
                    }
                }
            }
        }
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
    fn test_sveltekit_cross_file_page_server_resolution() {
        use cc_model::symbol::{SymbolKind, SymbolRecord};

        // Set up catalog with a `load` function in +page.server.ts
        let mut catalog = crate::resolver::SymbolCatalog::new();
        catalog.add_symbols(&[SymbolRecord {
            symbol_id: "sym_load_users".to_string(),
            symbol_uid: Some("uid_load_users".to_string()),
            name: "load".to_string(),
            file_path: "src/routes/users/+page.server.ts".to_string(),
            kind: SymbolKind::Function,
            start_line: 3,
            end_line: 8,
            start_col: 0,
            end_col: 0,
            container: None,
            qname: None,
            export_name: Some("load".to_string()),
            is_default_export: false,
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

        // Build outcomes for both the page and server file
        let ctx = ProjectFrameworkContext::new();

        // +page.svelte
        let mut page_outcome = ParseOutcome::default();
        SvelteResolver.enrich_file(
            "src/routes/users/+page.svelte",
            "<h1>Users</h1>",
            Language::Svelte,
            &mut page_outcome,
            &ctx,
        );
        assert_eq!(page_outcome.route_edges.len(), 1);
        assert_eq!(
            page_outcome.route_edges[0].route_kind.as_deref(),
            Some("page")
        );
        assert!(page_outcome.route_edges[0].handler_symbol_uid.is_none());

        // +page.server.ts
        let mut server_outcome = ParseOutcome::default();
        SvelteResolver.enrich_file(
            "src/routes/users/+page.server.ts",
            "export async function load({ params }) { return { users: [] }; }",
            Language::TypeScript,
            &mut server_outcome,
            &ctx,
        );

        // Run cross-file resolution
        let mut file_outcomes = vec![
            ("src/routes/users/+page.svelte".to_string(), page_outcome),
            (
                "src/routes/users/+page.server.ts".to_string(),
                server_outcome,
            ),
        ];
        SvelteResolver.resolve_cross_file(&catalog, &mut file_outcomes, &ctx);

        // The page's route edge should now have handler_symbol_uid pointing to load
        let page_edges = &file_outcomes[0].1.route_edges;
        assert_eq!(page_edges.len(), 1);
        assert_eq!(
            page_edges[0].handler_symbol_uid.as_deref(),
            Some("uid_load_users"),
            "page route edge should be linked to server load function"
        );
        assert!(
            page_edges[0]
                .handler_name
                .as_deref()
                .unwrap()
                .starts_with("load@"),
            "handler_name should indicate load from the server file"
        );
    }

    #[test]
    fn test_sveltekit_cross_file_nested_dynamic_route() {
        use cc_model::symbol::{SymbolKind, SymbolRecord};

        // Nested dynamic route: src/routes/users/[id]/+page.svelte + +page.server.ts
        let mut catalog = crate::resolver::SymbolCatalog::new();
        catalog.add_symbols(&[SymbolRecord {
            symbol_id: "sym_load_user_detail".to_string(),
            symbol_uid: Some("uid_load_user_detail".to_string()),
            name: "load".to_string(),
            file_path: "src/routes/users/[id]/+page.server.ts".to_string(),
            kind: SymbolKind::Function,
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 0,
            container: None,
            qname: None,
            export_name: Some("load".to_string()),
            is_default_export: false,
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

        let ctx = ProjectFrameworkContext::new();

        let mut page_outcome = ParseOutcome::default();
        SvelteResolver.enrich_file(
            "src/routes/users/[id]/+page.svelte",
            "<h1>User Detail</h1>",
            Language::Svelte,
            &mut page_outcome,
            &ctx,
        );

        let mut server_outcome = ParseOutcome::default();
        SvelteResolver.enrich_file(
            "src/routes/users/[id]/+page.server.ts",
            "export async function load({ params }) { return { user: {} }; }",
            Language::TypeScript,
            &mut server_outcome,
            &ctx,
        );

        let mut file_outcomes = vec![
            (
                "src/routes/users/[id]/+page.svelte".to_string(),
                page_outcome,
            ),
            (
                "src/routes/users/[id]/+page.server.ts".to_string(),
                server_outcome,
            ),
        ];
        SvelteResolver.resolve_cross_file(&catalog, &mut file_outcomes, &ctx);

        let page_edges = &file_outcomes[0].1.route_edges;
        assert_eq!(page_edges.len(), 1);
        assert_eq!(page_edges[0].route_path, "/users/:id");
        assert_eq!(
            page_edges[0].handler_symbol_uid.as_deref(),
            Some("uid_load_user_detail"),
        );
    }

    #[test]
    fn test_sveltekit_cross_file_no_server_file() {
        // When there is no +page.server.ts, handler_symbol_uid stays None
        let catalog = crate::resolver::SymbolCatalog::new();
        let ctx = ProjectFrameworkContext::new();

        let mut page_outcome = ParseOutcome::default();
        SvelteResolver.enrich_file(
            "src/routes/about/+page.svelte",
            "<h1>About</h1>",
            Language::Svelte,
            &mut page_outcome,
            &ctx,
        );

        let mut file_outcomes = vec![("src/routes/about/+page.svelte".to_string(), page_outcome)];
        SvelteResolver.resolve_cross_file(&catalog, &mut file_outcomes, &ctx);

        let page_edges = &file_outcomes[0].1.route_edges;
        assert_eq!(page_edges.len(), 1);
        assert!(
            page_edges[0].handler_symbol_uid.is_none(),
            "page without server file should have no handler_symbol_uid"
        );
    }

    #[test]
    fn test_sveltekit_resolver_tier() {
        assert_eq!(SvelteResolver.resolver_tier(), "full");
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
