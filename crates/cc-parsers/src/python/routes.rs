//! Route/framework detection for Python — Django, Flask, FastAPI, DRF patterns.

use super::PythonParser;
use cc_model::edge::RouteEdgeRecord;
use cc_model::ParserTier;

/// HTTP methods recognized in decorator route detection.
pub(super) const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "route",
];

// ---------------------------------------------------------------------------
// Route extraction methods on PythonParser
// ---------------------------------------------------------------------------

impl PythonParser {
    /// Extract route edges from decorator patterns on a `decorated_definition` node.
    /// Detects patterns like:
    ///   @app.get("/path"), @router.post("/path"), @app.route("/path")
    ///   @api_view(["GET", "POST"])
    pub(super) fn extract_route_edges(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        func_qname: &str,
        handler_symbol_id: Option<&str>,
    ) -> Vec<RouteEdgeRecord> {
        let mut routes = Vec::new();
        if node.kind() != "decorated_definition" {
            return routes;
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "decorator" {
                continue;
            }
            // A decorator node has a child which is the expression after @
            // It could be a `call` (e.g., @app.get("/path")) or just an expression
            let expr = if child.named_child_count() > 0 {
                child.named_child(0)
            } else {
                continue;
            };
            let Some(expr) = expr else { continue };

            if expr.kind() == "call" {
                // The function part of the call
                let func_part = expr.child_by_field_name("function");
                let args_part = expr.child_by_field_name("arguments");

                if let Some(func_part) = func_part {
                    let func_text = func_part.utf8_text(source).unwrap_or("");

                    // Check for attribute pattern: obj.method where method is HTTP method
                    if func_part.kind() == "attribute" {
                        let attr_node = func_part.child_by_field_name("attribute");
                        if let Some(attr_node) = attr_node {
                            let method = attr_node.utf8_text(source).unwrap_or("");
                            if HTTP_METHODS.contains(&method) {
                                // Extract first string argument as route path
                                let route_path = args_part
                                    .and_then(|args| self.extract_first_string_arg(&args, source));
                                if let Some(path) = route_path {
                                    let framework = self.detect_framework(func_text);
                                    let http_method = if method == "route" {
                                        None
                                    } else {
                                        Some(method.to_uppercase())
                                    };
                                    routes.push(RouteEdgeRecord {
                                        edge_id: format!(
                                            "route:{}:{}:{}",
                                            file_path,
                                            func_qname,
                                            child.start_position().row + 1
                                        ),
                                        file_path: file_path.to_string(),
                                        route_path: path,
                                        handler_name: Some(func_qname.to_string()),
                                        method: http_method,
                                        line: child.start_position().row as u32 + 1,
                                        start_col: child.start_position().column as u32,
                                        end_line: Some(child.end_position().row as u32 + 1),
                                        end_col: child.end_position().column as u32,
                                        handler_symbol_id: handler_symbol_id.map(|s| s.to_string()),
                                        handler_symbol_uid: None,
                                        handler_expr: None,
                                        router_symbol_uid: None,
                                        framework: Some(framework),
                                        route_kind: Some("http".to_string()),
                                        confidence: 0.85,
                                        parser_tier: ParserTier::Semantic,
                                    });
                                }
                            }
                        }
                    }

                    // Check for @api_view(["GET", "POST"]) pattern (DRF)
                    if func_text == "api_view" {
                        let methods =
                            args_part.and_then(|args| self.extract_list_strings(&args, source));
                        let method_str = methods.map(|m| m.join(","));
                        routes.push(RouteEdgeRecord {
                            edge_id: format!(
                                "route:{}:{}:{}",
                                file_path,
                                func_qname,
                                child.start_position().row + 1
                            ),
                            file_path: file_path.to_string(),
                            route_path: String::new(), // DRF route path comes from urlpatterns
                            handler_name: Some(func_qname.to_string()),
                            method: method_str,
                            line: child.start_position().row as u32 + 1,
                            start_col: child.start_position().column as u32,
                            end_line: Some(child.end_position().row as u32 + 1),
                            end_col: child.end_position().column as u32,
                            handler_symbol_id: handler_symbol_id.map(|s| s.to_string()),
                            handler_symbol_uid: None,
                            handler_expr: None,
                            router_symbol_uid: None,
                            framework: Some("drf".to_string()),
                            route_kind: Some("http".to_string()),
                            confidence: 0.75,
                            parser_tier: ParserTier::Semantic,
                        });
                    }
                }
            }
        }
        routes
    }

    /// Detect web framework from decorator function text.
    pub(super) fn detect_framework(&self, func_text: &str) -> String {
        let obj = func_text.split('.').next().unwrap_or("");
        match obj {
            "app" => {
                // Could be Flask or FastAPI; lean towards FastAPI as it's more common
                // with `.get()/.post()` pattern; Flask typically uses `.route()`
                if func_text.contains("route") {
                    "flask".to_string()
                } else {
                    "fastapi".to_string()
                }
            }
            "router" | "api_router" => "fastapi".to_string(),
            "blueprint" | "bp" => "flask".to_string(),
            _ => "python-web".to_string(),
        }
    }

    /// Detect Django `urlpatterns` assignments and extract route edges from them.
    /// Pattern: `urlpatterns = [path("/api/", view_func), ...]`
    pub(super) fn extract_django_urlpatterns(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<RouteEdgeRecord> {
        let mut routes = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            // Look for `urlpatterns = [...]` or `urlpatterns += [...]`
            if child.kind() == "expression_statement" {
                let expr = child.named_child(0);
                let Some(expr) = expr else { continue };
                if expr.kind() != "assignment" && expr.kind() != "augmented_assignment" {
                    continue;
                }
                let left = expr.child_by_field_name("left");
                let right = expr.child_by_field_name("right");
                let left_text = left.and_then(|n| n.utf8_text(source).ok()).unwrap_or("");
                if left_text != "urlpatterns" {
                    continue;
                }
                if let Some(right) = right {
                    if right.kind() == "list" {
                        self.extract_django_url_list(&right, source, file_path, &mut routes);
                    }
                }
            }
        }
        routes
    }

    /// Parse individual `path(...)` / `re_path(...)` / `url(...)` calls in a urlpatterns list.
    fn extract_django_url_list(
        &self,
        list_node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        routes: &mut Vec<RouteEdgeRecord>,
    ) {
        let mut cursor = list_node.walk();
        for child in list_node.children(&mut cursor) {
            if child.kind() == "call" {
                let func_node = child.child_by_field_name("function");
                let func_text = func_node
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("");
                if !matches!(func_text, "path" | "re_path" | "url") {
                    continue;
                }
                let args_node = child.child_by_field_name("arguments");
                let Some(args_node) = args_node else {
                    continue;
                };

                // First arg = route path, second arg = handler
                let mut args_cursor = args_node.walk();
                let named_children: Vec<_> = args_node.named_children(&mut args_cursor).collect();
                let route_path = named_children
                    .first()
                    .filter(|n| n.kind() == "string")
                    .and_then(|n| self.unquote_string(n, source));
                let handler_name = named_children
                    .get(1)
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(|s| s.to_string());

                if let Some(path) = route_path {
                    routes.push(RouteEdgeRecord {
                        edge_id: format!(
                            "route:{}:{}:{}",
                            file_path,
                            path,
                            child.start_position().row + 1
                        ),
                        file_path: file_path.to_string(),
                        route_path: path,
                        handler_name,
                        method: None, // Django URL dispatch is method-agnostic at this level
                        line: child.start_position().row as u32 + 1,
                        start_col: child.start_position().column as u32,
                        end_line: Some(child.end_position().row as u32 + 1),
                        end_col: child.end_position().column as u32,
                        handler_symbol_id: None,
                        handler_symbol_uid: None,
                        handler_expr: None,
                        router_symbol_uid: None,
                        framework: Some("django".to_string()),
                        route_kind: Some("http".to_string()),
                        confidence: 0.8,
                        parser_tier: ParserTier::Semantic,
                    });
                }
            }
        }
    }

    /// Legacy: superseded by `extract_all()` single-pass DFS. Retained for rollback.
    #[allow(dead_code)]
    pub(super) fn collect_route_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[cc_model::symbol::SymbolRecord],
    ) -> Vec<RouteEdgeRecord> {
        use std::collections::HashMap;
        let mut routes = Vec::new();
        let sym_by_name: HashMap<&str, &cc_model::symbol::SymbolRecord> =
            symbols.iter().map(|s| (s.name.as_str(), s)).collect();

        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == "decorated_definition" {
                // Get the function name from the definition child
                let func_node = node.child_by_field_name("definition");
                if let Some(func_node) = func_node {
                    let name = func_node
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source).ok());
                    if let Some(name) = name {
                        let sym = sym_by_name.get(name);
                        let qname = sym.and_then(|s| s.qname.as_deref()).unwrap_or(name);
                        let sid = sym.map(|s| s.symbol_id.as_str());
                        routes
                            .extend(self.extract_route_edges(&node, source, file_path, qname, sid));
                    }
                }
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        routes
    }
}
