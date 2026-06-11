//! Route/framework detection for JS/TS — Express, NestJS, Next.js, Koa, etc.

use super::{child_by_kind, node_text, short_text, ExtractCtx, JsTsParser};
use cc_model::edge::{HttpCallEdgeRecord, RouteEdgeRecord};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{ElementKind, ParserTier};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

// Per-framework route-detection confidence: how reliably the detection
// pattern identifies a real route, independent of parser tier.
/// Next.js routes follow file conventions, near-deterministic.
const NEXTJS_ROUTE_CONFIDENCE: f64 = 0.92;
/// Express `app.get(path, ...)` calls are explicit registrations.
const EXPRESS_ROUTE_CONFIDENCE: f64 = 0.90;
/// NestJS routes join a method decorator with the controller prefix.
const NESTJS_ROUTE_CONFIDENCE: f64 = 0.88;
/// `app.use(...)` middleware may not be an actual route.
const MIDDLEWARE_ROUTE_CONFIDENCE: f64 = 0.80;

/// Router objects that can register routes (e.g., `app.get(...)`, `router.post(...)`).
pub(super) const ROUTER_OBJECTS: &[&str] = &[
    "app", "router", "server", "api", "route", "routes", "fastify", "hono",
];

/// HTTP methods for route detection.
pub(super) const ROUTE_METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "options", "head", "all", "use",
];

/// NestJS decorator -> HTTP method mapping.
pub(super) const NESTJS_DECORATORS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("options", "OPTIONS"),
    ("head", "HEAD"),
    ("all", "ALL"),
];

/// Framework map: router object -> framework key.
pub(super) fn framework_for_router_object(obj: &str) -> &'static str {
    match obj.to_lowercase().as_str() {
        "fastify" => "fastify",
        "hono" => "hono",
        _ => "express",
    }
}

// ---------------------------------------------------------------------------
// Next.js file route detection
// ---------------------------------------------------------------------------

/// Detect if a file path is a Next.js route file and extract the route.
pub(super) fn detect_nextjs_file_route(
    file_path: &str,
    symbols: &[SymbolRecord],
) -> Option<RouteEdgeRecord> {
    let normalized = file_path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect::<Vec<_>>();
    let app_idx = parts.iter().position(|&p| p == "app")?;
    let remaining = &parts[app_idx + 1..];
    if remaining.is_empty() {
        return None;
    }
    let filename = *remaining.last()?;
    let is_route_file = matches!(
        filename,
        "route.ts"
            | "route.tsx"
            | "route.js"
            | "route.jsx"
            | "page.ts"
            | "page.tsx"
            | "page.js"
            | "page.jsx"
    );
    if !is_route_file {
        return None;
    }
    let name_no_ext = filename
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(filename);

    let mut route_segments = Vec::new();
    for &segment in &remaining[..remaining.len() - 1] {
        if segment.starts_with('[') && segment.ends_with(']') {
            route_segments.push(format!(":{}", &segment[1..segment.len() - 1]));
        } else if segment.starts_with('(') && segment.ends_with(')') {
            continue; // route group — skip
        } else {
            route_segments.push(segment.to_string());
        }
    }
    let route_path = if route_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", route_segments.join("/"))
    };

    let route_kind = if name_no_ext == "page" {
        "page"
    } else {
        "http"
    };
    let method = if name_no_ext == "page" { "GET" } else { "*" };

    // Find exported function for handler name
    let handler_name = symbols
        .iter()
        .find(|s| s.export_name.is_some() && s.kind == SymbolKind::Function)
        .map(|s| s.name.clone());

    Some(RouteEdgeRecord {
        edge_id: StableId::edge_id("route", file_path, 1, 0),
        file_path: file_path.to_string(),
        route_path,
        handler_name,
        method: Some(method.to_string()),
        line: 1,
        start_col: 0,
        end_line: None,
        end_col: 0,
        handler_symbol_id: None,
        handler_symbol_uid: None,
        handler_expr: None,
        router_symbol_uid: None,
        framework: Some("nextjs".to_string()),
        route_kind: Some(route_kind.to_string()),
        confidence: NEXTJS_ROUTE_CONFIDENCE,
        parser_tier: ParserTier::Semantic,
        resolution_strategy: None,
        resolution_confidence: None,
    })
}

// ---------------------------------------------------------------------------
// Route extraction methods on JsTsParser
// ---------------------------------------------------------------------------

impl JsTsParser {
    /// Extract route from `app.get("/path", handler)` pattern.
    pub(super) fn try_extract_route(
        &self,
        call_node: &tree_sitter::Node,
        obj_text: &str,
        method: &str,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
    ) {
        let args_node = match child_by_kind(call_node, "arguments") {
            Some(a) => a,
            None => return,
        };
        let args: Vec<tree_sitter::Node> = {
            let mut cursor = args_node.walk();
            args_node
                .children(&mut cursor)
                .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
                .collect()
        };
        if args.len() < 2 {
            return;
        }

        let path_node = &args[0];
        if path_node.kind() != "string" {
            return;
        }
        let path = node_text(path_node, source)
            .unwrap_or("")
            .trim_matches(|c| c == '\'' || c == '"')
            .to_string();
        if path.is_empty() {
            return;
        }

        let handler_node = args.last().unwrap();
        let handler_expr = short_text(handler_node, source, 80);
        let handler_name = match handler_node.kind() {
            "identifier" => node_text(handler_node, source).map(String::from),
            "member_expression" => node_text(handler_node, source).map(String::from),
            "call_expression" => {
                // app.post("/login", createLoginHandler())
                handler_node
                    .child(0)
                    .and_then(|f| node_text(&f, source))
                    .map(String::from)
            }
            "function_expression" | "function" => {
                // app.get("/path", function getUserRoute(req, res) { ... })
                // Named function expression: extract the name
                handler_node
                    .child_by_field_name("name")
                    .and_then(|n| node_text(&n, source))
                    .map(String::from)
            }
            _ => None,
        };

        // If the handler is a named function expression, also register it as a symbol
        if matches!(handler_node.kind(), "function_expression" | "function") {
            if let Some(ref hname) = handler_name {
                if !hname.is_empty() {
                    let qname = hname.to_string();
                    let mut sym = self.make_symbol(
                        handler_node,
                        file_path,
                        hname,
                        SymbolKind::Function,
                        &qname,
                        None,
                        None,
                        source,
                    );
                    sym.framework_role = Some("route_handler".to_string());
                    let prev_uid = ctx.current_symbol_uid.take();
                    ctx.current_symbol_uid = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                    // Visit function body for call edges
                    if let Some(body) = handler_node.child_by_field_name("body") {
                        self.visit_expression_tree(&body, source, file_path, ctx, Some(hname));
                    }
                    ctx.current_symbol_uid = prev_uid;
                }
            }
        }

        let framework = framework_for_router_object(obj_text);
        ctx.route_edges.push(RouteEdgeRecord {
            edge_id: StableId::edge_id(
                "route",
                file_path,
                call_node.start_position().row as u32 + 1,
                call_node.start_position().column as u32,
            ),
            file_path: file_path.to_string(),
            route_path: path,
            handler_name,
            method: Some(method.to_uppercase()),
            line: call_node.start_position().row as u32 + 1,
            start_col: call_node.start_position().column as u32,
            end_line: Some(call_node.end_position().row as u32 + 1),
            end_col: call_node.end_position().column as u32,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: Some(handler_expr),
            router_symbol_uid: None,
            framework: Some(framework.to_string()),
            route_kind: Some("http".to_string()),
            confidence: EXPRESS_ROUTE_CONFIDENCE,
            parser_tier: ParserTier::Semantic,
            resolution_strategy: None,
            resolution_confidence: None,
        });
    }

    /// Detect broker calls (producer.send(), pubsub.publish(), etc.) and emit
    /// an HTTP-call edge if a matching async broker method is found.
    /// Returns `true` when a broker was detected (edge may or may not have been
    /// emitted depending on the method kind).
    pub(super) fn try_emit_broker_edge(
        &self,
        node: &tree_sitter::Node,
        obj_text: &str,
        prop_text: &str,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
    ) -> bool {
        let broker_type_from_obj = crate::broker_patterns::match_broker_object(obj_text)
            .map(|m| m.broker_type.to_string());
        let broker_type_from_import = ctx.broker_imports.get(obj_text).cloned();
        let detected_broker = broker_type_from_obj.or(broker_type_from_import);
        if let Some(ref bt) = detected_broker {
            let mk = crate::broker_patterns::method_call_kind(prop_text);
            if mk == "async" {
                let line = node.start_position().row as u32 + 1;
                let col = node.start_position().column as u32;
                let url = self
                    .extract_first_string_arg(node, source)
                    .unwrap_or_default();
                ctx.http_call_edges.push(HttpCallEdgeRecord {
                    edge_id: StableId::edge_id("http_call", file_path, line, col),
                    file_path: file_path.to_string(),
                    caller_symbol_uid: ctx.current_symbol_uid.clone(),
                    url_or_path: url,
                    normalized_path: None,
                    method: None,
                    call_kind: "async".to_string(),
                    line,
                    confidence: ParserTier::TreeSitter.element_confidence(ElementKind::HttpCall),
                    parser_tier: ParserTier::TreeSitter,
                    broker_type: Some(bt.clone()),
                });
            }
        }
        detected_broker.is_some()
    }

    /// Extract middleware from `app.use([path], handler)` pattern.
    pub(super) fn try_extract_middleware(
        &self,
        call_node: &tree_sitter::Node,
        obj_text: &str,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
    ) {
        let args_node = match child_by_kind(call_node, "arguments") {
            Some(a) => a,
            None => return,
        };
        let args: Vec<tree_sitter::Node> = {
            let mut cursor = args_node.walk();
            args_node
                .children(&mut cursor)
                .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
                .collect()
        };
        if args.is_empty() {
            return;
        }

        let mut path = "*".to_string();
        let mut handler_start = 0;

        // Check if first arg is a path string
        if args[0].kind() == "string" {
            path = node_text(&args[0], source)
                .unwrap_or("*")
                .trim_matches(|c| c == '\'' || c == '"')
                .to_string();
            handler_start = 1;
        }

        let handler_name = if handler_start < args.len() {
            let handler_node = args.last().unwrap();
            match handler_node.kind() {
                "identifier" | "member_expression" => {
                    node_text(handler_node, source).map(String::from)
                }
                _ => None,
            }
        } else {
            None
        };

        let handler_expr = if handler_start < args.len() {
            Some(short_text(args.last().unwrap(), source, 80))
        } else {
            None
        };

        let framework = framework_for_router_object(obj_text);
        ctx.route_edges.push(RouteEdgeRecord {
            edge_id: StableId::edge_id(
                "route",
                file_path,
                call_node.start_position().row as u32 + 1,
                call_node.start_position().column as u32,
            ),
            file_path: file_path.to_string(),
            route_path: path,
            handler_name,
            method: Some("USE".to_string()),
            line: call_node.start_position().row as u32 + 1,
            start_col: call_node.start_position().column as u32,
            end_line: Some(call_node.end_position().row as u32 + 1),
            end_col: call_node.end_position().column as u32,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr,
            router_symbol_uid: None,
            framework: Some(framework.to_string()),
            route_kind: Some("middleware".to_string()),
            confidence: MIDDLEWARE_ROUTE_CONFIDENCE,
            parser_tier: ParserTier::Semantic,
            resolution_strategy: None,
            resolution_confidence: None,
        });
    }

    /// Extract NestJS decorator route from `@Get("/path")` on a method.
    pub(super) fn try_extract_nestjs_decorator_route(
        &self,
        decorator_node: &tree_sitter::Node,
        method_node: &tree_sitter::Node,
        _class_name: &str,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
    ) {
        let dec_text = node_text(decorator_node, source)
            .unwrap_or("")
            .trim_start_matches('@')
            .to_string();

        let (dec_name, dec_path) = if let Some(paren_idx) = dec_text.find('(') {
            let name = dec_text[..paren_idx].trim().to_string();
            let rest = &dec_text[paren_idx + 1..];
            let path = rest
                .rfind(')')
                .map(|end| rest[..end].trim().trim_matches(|c| c == '\'' || c == '"'))
                .unwrap_or("")
                .to_string();
            (name, path)
        } else {
            (dec_text.trim().to_string(), String::new())
        };

        let method_upper = NESTJS_DECORATORS
            .iter()
            .find(|(k, _)| *k == dec_name.to_lowercase().as_str())
            .map(|(_, v)| *v);

        let method_upper = match method_upper {
            Some(m) => m,
            None => return,
        };

        // Get method name
        let handler_name = method_node
            .child_by_field_name("name")
            .and_then(|n| node_text(&n, source))
            .map(String::from);

        let route_path = if !dec_path.is_empty() {
            dec_path
        } else if let Some(ref name) = handler_name {
            format!("/{}", name)
        } else {
            "/".to_string()
        };

        ctx.route_edges.push(RouteEdgeRecord {
            edge_id: StableId::edge_id(
                "route",
                file_path,
                decorator_node.start_position().row as u32 + 1,
                decorator_node.start_position().column as u32,
            ),
            file_path: file_path.to_string(),
            route_path,
            handler_name: handler_name.clone(),
            method: Some(method_upper.to_string()),
            line: decorator_node.start_position().row as u32 + 1,
            start_col: decorator_node.start_position().column as u32,
            end_line: Some(method_node.end_position().row as u32 + 1),
            end_col: method_node.end_position().column as u32,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: handler_name,
            router_symbol_uid: None,
            framework: Some("nestjs".to_string()),
            route_kind: Some("http".to_string()),
            confidence: NESTJS_ROUTE_CONFIDENCE,
            parser_tier: ParserTier::Semantic,
            resolution_strategy: None,
            resolution_confidence: None,
        });
    }
}
