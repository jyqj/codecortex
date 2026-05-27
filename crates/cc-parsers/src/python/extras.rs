//! Extra extraction for Python — HTTP calls, broker detection, env access,
//! type refs, type assignments, dispatch site helpers.

use super::{
    find_enclosing_function, PythonExtractCtx, PythonParser, PY_ENV_ACCESS_RE, PY_RAISE_RE,
    PY_RETURN_TYPE_RE, PY_TYPE_ANNOT_RE,
};
use crate::http_call_helpers::*;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, HttpCallEdgeRecord, ImportRecord, SemanticEdgeRecord,
    SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::SymbolRecord;
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::ParserTier;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// HTTP call extraction
// ---------------------------------------------------------------------------

/// Check if a variable/object name looks like an HTTP client by name heuristic.
/// Catches `session`, `http_client`, `api_client`, etc.
fn is_http_client_name_heuristic(name: &str) -> bool {
    let lower = name.to_lowercase();
    // Only match names that explicitly reference HTTP — avoids false positives
    // on generic names like "request" (Django), "session" (DB), "client" (gRPC), etc.
    lower.starts_with("http")
        || lower.ends_with("_http")
        || lower.ends_with("http_client")
        || (lower.ends_with("_client") && lower.contains("http"))
        || (lower.ends_with("_client") && lower.contains("api"))
}

/// Legacy: superseded by `PythonParser::extract_all()` single-pass DFS. Retained for rollback.
#[allow(dead_code)]
pub(super) fn extract_http_calls(
    root: tree_sitter::Node,
    source: &str,
    file_path: &str,
    imports: &[ImportRecord],
) -> Vec<HttpCallEdgeRecord> {
    let mut results = Vec::new();
    let bytes = source.as_bytes();

    // Build import-based broker mapping: local name -> broker_type
    let mut broker_imports: HashMap<String, String> = HashMap::new();
    for imp in imports {
        if let Some(broker_match) = crate::broker_patterns::match_broker(&imp.import_string) {
            if let Some(ref name) = imp.imported_name {
                if name != "*" {
                    broker_imports.insert(name.clone(), broker_match.broker_type.to_string());
                }
            }
            if let Some(ref alias) = imp.alias {
                broker_imports.insert(alias.clone(), broker_match.broker_type.to_string());
            }
        }
    }

    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if node.kind() == "call" {
            if let Some(edge) = try_extract_http_call(&node, bytes, file_path) {
                results.push(edge);
            } else if let Some(edge) =
                try_extract_broker_call(&node, bytes, file_path, &broker_imports)
            {
                results.push(edge);
            }
        }
        // Push children in reverse to maintain order
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    results
}

/// Try to extract an HttpCallEdgeRecord from a single `call` node.
pub(super) fn try_extract_http_call(
    call_node: &tree_sitter::Node,
    source: &[u8],
    file_path: &str,
) -> Option<HttpCallEdgeRecord> {
    let func_node = call_node.child_by_field_name("function")?;

    // We only handle `attribute` nodes: obj.method(...)
    if func_node.kind() != "attribute" {
        return None;
    }

    let obj_node = func_node.child_by_field_name("object")?;
    let attr_node = func_node.child_by_field_name("attribute")?;

    let obj_text = obj_node.utf8_text(source).ok()?;
    let method_name = attr_node.utf8_text(source).ok()?;

    // Check if obj is a known HTTP client or has a name heuristic match,
    // and the method is an HTTP verb.
    let obj_is_http = is_http_client_object(obj_text) || is_http_client_name_heuristic(obj_text);
    if !obj_is_http || !is_http_verb_method(method_name) {
        return None;
    }

    let args_node = call_node.child_by_field_name("arguments")?;

    // When method_name is "request", the signature is (method_str, url, ...),
    // e.g. requests.request("POST", "/api/x"). The first arg is the HTTP method
    // and the second arg is the URL.
    let lower_method = method_name.to_lowercase();
    let (raw_url, http_method) = if lower_method == "request" {
        let method_arg = extract_nth_string_arg(&args_node, source, 0)?;
        let url_arg = extract_nth_string_arg(&args_node, source, 1)?;
        let m = method_arg.to_uppercase();
        (url_arg, Some(m))
    } else {
        let url = extract_url_from_args(&args_node, source)?;
        (url, infer_http_method(method_name).map(|m| m.to_string()))
    };

    if !looks_like_url_or_path(&raw_url) {
        return None;
    }

    let line = call_node.start_position().row as u32 + 1;
    let col = call_node.start_position().column as u32;

    Some(HttpCallEdgeRecord {
        edge_id: StableId::edge_id("http_call", file_path, line, col),
        file_path: file_path.to_string(),
        caller_symbol_uid: None, // v1: don't track enclosing function
        url_or_path: raw_url.clone(),
        normalized_path: Some(cc_model::route_normalize::normalize_route_path(
            &normalize_template_to_path(&raw_url),
        )),
        method: http_method,
        call_kind: "http".to_string(),
        line,
        confidence: 0.80,
        parser_tier: ParserTier::TreeSitter,
        broker_type: None,
    })
}

/// Try to extract an async broker call from a `call` node.
pub(super) fn try_extract_broker_call(
    call_node: &tree_sitter::Node,
    source: &[u8],
    file_path: &str,
    broker_imports: &HashMap<String, String>,
) -> Option<HttpCallEdgeRecord> {
    let func_node = call_node.child_by_field_name("function")?;

    // obj.method(...) pattern
    if func_node.kind() != "attribute" {
        return None;
    }

    let obj_node = func_node.child_by_field_name("object")?;
    let attr_node = func_node.child_by_field_name("attribute")?;

    let obj_text = obj_node.utf8_text(source).ok()?;
    let method_name = attr_node.utf8_text(source).ok()?;

    // Check if the method indicates a broker operation
    let mk = crate::broker_patterns::method_call_kind(method_name);
    if mk != "async" {
        return None;
    }

    // Try object-name match, then fall back to import-based mapping
    let broker_type = crate::broker_patterns::match_broker_object(obj_text)
        .map(|m| m.broker_type.to_string())
        .or_else(|| broker_imports.get(obj_text).cloned())?;

    let line = call_node.start_position().row as u32 + 1;
    let col = call_node.start_position().column as u32;

    // Try to extract the first string arg as a topic/queue name
    let topic = call_node
        .child_by_field_name("arguments")
        .and_then(|args| extract_nth_string_arg(&args, source, 0))
        .unwrap_or_default();

    Some(HttpCallEdgeRecord {
        edge_id: StableId::edge_id("http_call", file_path, line, col),
        file_path: file_path.to_string(),
        caller_symbol_uid: None,
        url_or_path: topic,
        normalized_path: None,
        method: None,
        call_kind: "async".to_string(),
        line,
        confidence: 0.80,
        parser_tier: ParserTier::TreeSitter,
        broker_type: Some(broker_type),
    })
}

// ---------------------------------------------------------------------------
// EventEmitter / Django signal dispatch site detection (Python)
// ---------------------------------------------------------------------------

/// Method names that register an event listener in Python.
/// Covers pyee EventEmitter and Django signals.
pub(super) fn is_py_event_registration(method: &str) -> bool {
    matches!(
        method,
        "on" | "once" | "add_listener" | "connect" | "subscribe" | "add_handler"
    )
}

/// Method names that dispatch/emit an event in Python.
pub(super) fn is_py_event_dispatch(method: &str) -> bool {
    matches!(
        method,
        "emit" | "send" | "send_robust" | "publish" | "fire" | "trigger"
    )
}

/// Extract the Nth non-keyword, non-punctuation argument node from Python
/// call arguments.
fn py_nth_arg_node<'a>(
    args_node: &tree_sitter::Node<'a>,
    source: &[u8],
    n: usize,
) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = args_node.walk();
    let mut idx = 0usize;
    for child in args_node.children(&mut cursor) {
        // Skip punctuation and keyword arguments
        if matches!(child.kind(), "(" | ")" | ",") || child.kind() == "keyword_argument" {
            continue;
        }
        if idx == n {
            return Some(child);
        }
        idx += 1;
    }
    let _ = source;
    None
}

/// Try to extract an EventEmitter or Django signal dispatch site from a
/// Python `call` node (obj.method(...) pattern).
pub(super) fn try_extract_dispatch_site(
    call_node: &tree_sitter::Node,
    source: &[u8],
    file_path: &str,
    symbols: &[SymbolRecord],
) -> Option<DispatchSiteRecord> {
    let func_node = call_node.child_by_field_name("function")?;
    if func_node.kind() != "attribute" {
        return None;
    }

    let obj_node = func_node.child_by_field_name("object")?;
    let attr_node = func_node.child_by_field_name("attribute")?;

    let obj_text = obj_node.utf8_text(source).ok()?;
    let method_name = attr_node.utf8_text(source).ok()?;

    let args_node = call_node.child_by_field_name("arguments")?;

    let line = call_node.start_position().row as u32 + 1;
    let col = call_node.start_position().column as u32;
    let enclosing = find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

    if is_py_event_registration(method_name) {
        let event_name = extract_nth_string_arg(&args_node, source, 0)?;
        // Handler is typically the second positional arg
        let handler_expr = py_nth_arg_node(&args_node, source, 1)
            .and_then(|n| n.utf8_text(source).ok())
            .filter(|t| {
                // Only keep simple identifiers / dotted names; skip lambdas
                t.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            })
            .map(|s| s.to_string());

        return Some(DispatchSiteRecord {
            site_id: StableId::edge_id("dsite", file_path, line, col),
            file_path: file_path.to_string(),
            line,
            col,
            enclosing_symbol_uid: enclosing,
            receiver_expr: Some(obj_text.to_string()),
            site_kind: DispatchSiteKind::EventOn,
            key: event_name,
            handler_expr,
            handler_symbol_uid: None,
            confidence: 0.85,
        });
    }

    if is_py_event_dispatch(method_name) {
        // For emit/send the first string arg is the event name.
        let event_name = extract_nth_string_arg(&args_node, source, 0)?;

        return Some(DispatchSiteRecord {
            site_id: StableId::edge_id("dsite", file_path, line, col),
            file_path: file_path.to_string(),
            line,
            col,
            enclosing_symbol_uid: enclosing,
            receiver_expr: Some(obj_text.to_string()),
            site_kind: DispatchSiteKind::EventEmit,
            key: event_name,
            handler_expr: None,
            handler_symbol_uid: None,
            confidence: 0.85,
        });
    }

    None
}

// ---------------------------------------------------------------------------
// URL extraction helpers
// ---------------------------------------------------------------------------

/// Extract a URL string from the first positional argument of a call.
fn extract_url_from_args(args_node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    extract_nth_string_arg(args_node, source, 0)
}

/// Extract the string value of the Nth positional (non-keyword) string-like argument.
pub(super) fn extract_nth_string_arg(
    args_node: &tree_sitter::Node,
    source: &[u8],
    n: usize,
) -> Option<String> {
    let mut cursor = args_node.walk();
    let mut found = 0usize;
    for child in args_node.children(&mut cursor) {
        match child.kind() {
            "string" => {
                if found == n {
                    let text = child.utf8_text(source).ok()?;
                    return Some(strip_string_delimiters(text).to_string());
                }
                found += 1;
            }
            "concatenated_string" => {
                if found == n {
                    // Take the first segment only
                    let mut inner_cursor = child.walk();
                    for segment in child.children(&mut inner_cursor) {
                        if segment.kind() == "string" {
                            let text = segment.utf8_text(source).ok()?;
                            return Some(strip_string_delimiters(text).to_string());
                        }
                    }
                }
                found += 1;
            }
            // f-string: tree-sitter-python may use various node kinds depending on version
            "format_string" | "formatted_string" | "f_string" => {
                if found == n {
                    let text = child.utf8_text(source).ok()?;
                    return Some(normalize_template_to_path(strip_string_delimiters(text)));
                }
                found += 1;
            }
            _ => {}
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Methods on PythonParser — data flow, semantic edges, type assigns
// ---------------------------------------------------------------------------

impl PythonParser {
    /// Extract throw/raise edges from Python source.
    pub(super) fn extract_throw_edges(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();

        for cap in PY_RAISE_RE.captures_iter(content) {
            let exception_type = &cap[1];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let (source_name, source_uid) = match find_enclosing_function(symbols, line) {
                Some(sym) => (
                    sym.qname.as_deref().unwrap_or(&sym.name).to_string(),
                    sym.symbol_uid.clone(),
                ),
                None => (file_path.to_string(), None),
            };

            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:throws:{}", file_path, line, exception_type),
                file_path: file_path.to_string(),
                source_symbol: source_name,
                source_symbol_uid: source_uid,
                target_symbol: exception_type.to_string(),
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Throws,
                line,
                confidence: 0.9,
                parser_tier: tier,
            });
        }

        edges
    }

    /// Extract type annotation references from function signatures.
    pub(super) fn extract_type_refs(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        // Parameter type annotations
        for cap in PY_TYPE_ANNOT_RE.captures_iter(content) {
            let _type_name = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let source_uid =
                find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfe", file_path, line, m.start() as u32),
                file_path: file_path.to_string(),
                source_symbol_uid: source_uid,
                target_symbol_uid: None,
                flow_kind: "type_ref".to_string(),
                line,
                confidence: 0.85,
                parser_tier: ParserTier::Semantic,
                env_key: None,
            });
        }

        // Return type annotations
        for cap in PY_RETURN_TYPE_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let source_uid =
                find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfe", file_path, line, m.start() as u32),
                file_path: file_path.to_string(),
                source_symbol_uid: source_uid,
                target_symbol_uid: None,
                flow_kind: "type_ref".to_string(),
                line,
                confidence: 0.85,
                parser_tier: ParserTier::Semantic,
                env_key: None,
            });
        }

        edges
    }

    /// Extract environment variable accesses from code.
    pub(super) fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        for cap in PY_ENV_ACCESS_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let env_key = cap
                .get(1)
                .or(cap.get(2))
                .or(cap.get(3))
                .map(|m| m.as_str().to_string());

            let source_uid =
                find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfe", file_path, line, m.start() as u32),
                file_path: file_path.to_string(),
                source_symbol_uid: source_uid,
                target_symbol_uid: None,
                flow_kind: "env_access".to_string(),
                line,
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
                env_key,
            });
        }

        edges
    }

    /// Extract param_pass and return_flow data flow edges from call edges.
    pub(super) fn extract_param_return_flow(
        &self,
        call_edges: &[CallEdgeRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();
        for ce in call_edges {
            let caller_uid = match &ce.caller_symbol_uid {
                Some(uid) if !uid.is_empty() => uid,
                _ => continue,
            };
            let callee_uid = match &ce.callee_symbol_uid {
                Some(uid) if !uid.is_empty() => uid,
                _ => continue,
            };
            if ce.resolution_kind == cc_model::edge::ResolutionKind::Unresolved {
                continue;
            }
            if ce.arg_count.unwrap_or(0) > 0 {
                edges.push(DataFlowEdgeRecord {
                    edge_id: cc_model::StableId::edge_id("dfp", file_path, ce.line, ce.start_col),
                    file_path: file_path.to_string(),
                    source_symbol_uid: Some(caller_uid.clone()),
                    target_symbol_uid: Some(callee_uid.clone()),
                    flow_kind: "param_pass".to_string(),
                    line: ce.line,
                    confidence: ce.resolution_confidence * 0.9,
                    parser_tier: ce.parser_tier,
                    env_key: None,
                });
            }
            edges.push(DataFlowEdgeRecord {
                edge_id: cc_model::StableId::edge_id("dfr", file_path, ce.line, ce.start_col),
                file_path: file_path.to_string(),
                source_symbol_uid: Some(callee_uid.clone()),
                target_symbol_uid: Some(caller_uid.clone()),
                flow_kind: "return_flow".to_string(),
                line: ce.line,
                confidence: ce.resolution_confidence * 0.8,
                parser_tier: ce.parser_tier,
                env_key: None,
            });
        }
        edges
    }

    // =========================================================================
    // Type assignment extraction
    // =========================================================================

    /// Extract local variable type assignments from the AST.
    pub(super) fn extract_type_assigns(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<TypeAssignRecord> {
        let mut assigns = Vec::new();
        let root = tree.root_node();
        self.walk_for_type_assigns_py(&root, source, file_path, symbols, &mut assigns);
        assigns
    }

    /// Recursively walk AST to find assignment and annotated assignment nodes.
    fn walk_for_type_assigns_py(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        match node.kind() {
            "assignment" => {
                self.extract_py_assignment(node, source, file_path, symbols, assigns);
            }
            "typed_default_parameter" | "typed_parameter" => {
                // Handled in function signatures, not local assigns
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_type_assigns_py(&child, source, file_path, symbols, assigns);
        }
    }

    /// Extract type info from a Python `assignment` node.
    /// Handles both `x = Foo()` and `x: Foo = ...`.
    fn extract_py_assignment(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        let left = match node.child_by_field_name("left") {
            Some(n) => n,
            None => return,
        };
        let right = node.child_by_field_name("right");

        let line = node.start_position().row as u32 + 1;
        let enclosing = find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

        // Get variable name — only handle simple identifiers
        let var_name = if left.kind() == "identifier" {
            match left.utf8_text(source).ok() {
                Some(n) => n.to_string(),
                None => return,
            }
        } else {
            return;
        };

        // Check for type annotation: `x: Foo = ...`
        let type_node = node.child_by_field_name("type");
        if let Some(tn) = type_node {
            let type_text = match tn.utf8_text(source).ok() {
                Some(t) => t.trim().to_string(),
                None => String::new(),
            };
            // Strip generics
            let base = if let Some(idx) = type_text.find('[') {
                &type_text[..idx]
            } else {
                &type_text
            };
            let base = base.trim();
            if !base.is_empty() && base.starts_with(|c: char| c.is_ascii_uppercase()) {
                assigns.push(TypeAssignRecord {
                    file_path: file_path.to_string(),
                    enclosing_symbol_uid: enclosing.clone(),
                    var_name: var_name.clone(),
                    type_name: base.to_string(),
                    line,
                    confidence: 0.95,
                    source: TypeAssignSource::TypeAnnotation,
                });
                return; // Don't also add constructor if we have annotation
            }
        }

        // Check RHS for constructor pattern: `x = Foo(...)` where Foo is capitalized
        if let Some(rhs) = right {
            if rhs.kind() == "call" {
                let func_node = match rhs.child_by_field_name("function") {
                    Some(n) => n,
                    None => return,
                };
                let callee = match func_node.utf8_text(source).ok() {
                    Some(t) => t.to_string(),
                    None => return,
                };
                // Get the leaf name (for dotted calls like `module.Foo()`)
                let leaf = callee.rsplit('.').next().unwrap_or(&callee);
                if leaf.starts_with(|c: char| c.is_ascii_uppercase()) {
                    assigns.push(TypeAssignRecord {
                        file_path: file_path.to_string(),
                        enclosing_symbol_uid: enclosing,
                        var_name,
                        type_name: leaf.to_string(),
                        line,
                        confidence: 0.85,
                        source: TypeAssignSource::Constructor,
                    });
                }
            }
        }
    }

    /// Handle a call expression: check for HTTP calls, broker calls, and
    /// EventEmitter / Django signal dispatch sites.
    pub(super) fn handle_call(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
    ) {
        if let Some(edge) = try_extract_http_call(node, source, file_path) {
            ctx.http_call_edges.push(edge);
        } else if let Some(edge) =
            try_extract_broker_call(node, source, file_path, &ctx.broker_imports)
        {
            ctx.http_call_edges.push(edge);
        }

        // EventEmitter / Django signal dispatch site detection
        if let Some(ds) = try_extract_dispatch_site(node, source, file_path, &ctx.symbols) {
            ctx.dispatch_sites.push(ds);
        }
    }
}
