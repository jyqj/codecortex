//! Extra extraction for JS/TS — literals, semantic edges, type refs, env access,
//! type assignments, and EventEmitter dispatch site helpers.

use super::{
    child_by_kind, js_find_enclosing_function, node_text, ExtractCtx, JsTsParser, JS_CALL_RE,
    JS_DECORATOR_RE, JS_ENV_ACCESS_RE, JS_EXTENDS_RE, JS_IDENT_RE, JS_IMPLEMENTS_RE, JS_KEYWORDS,
    JS_RETURN_TYPE_RE, JS_TYPE_ANNOT_RE,
};
use cc_model::diagnostic::LiteralRecord;
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, ResolutionKind, SemanticEdgeRecord,
    SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::ParserTier;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Literal classification
// ---------------------------------------------------------------------------

// Literal classification regexes
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s'""]+"#).expect("url regex"));
static ENV_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9_]{2,}$").expect("env key regex"));
static SQL_TABLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:from|join|update|into|table)\s+([A-Za-z_]\w*)").expect("sql regex")
});
static ERROR_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(error|failed|failure|invalid|unauthorized|forbidden|timeout|not found)\b")
        .expect("err regex")
});
static TOPIC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[-.](?:topic|events)$").expect("topic regex"));
static QUEUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:[-.]queue|[-.]fifo|^queue[-.]|^fifo[-.])").expect("queue regex")
});
static CONFIG_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9]*(?:\.[a-z][a-z0-9]*){1,}$").expect("config key regex")
});

/// Classify a string literal into a category, or None if uninteresting.
pub(super) fn classify_literal(value: &str) -> Option<&'static str> {
    if value.is_empty() || value.len() < 3 {
        return None;
    }
    if value.starts_with('/') {
        return Some("route");
    }
    if URL_RE.is_match(value) {
        return Some("url");
    }
    // topic/queue before env_key (priority: url > topic/queue > env_key > config_key)
    if TOPIC_RE.is_match(value) {
        return Some("topic");
    }
    if QUEUE_RE.is_match(value) {
        return Some("queue");
    }
    if ENV_KEY_RE.is_match(value) {
        return Some("env_key");
    }
    if CONFIG_KEY_RE.is_match(value) {
        return Some("config_key");
    }
    if SQL_TABLE_RE.is_match(value) {
        return Some("sql");
    }
    if ERROR_STRING_RE.is_match(value) {
        return Some("error_string");
    }
    let lower = value.to_lowercase();
    if ["trace", "logger", "log", "event"]
        .iter()
        .any(|k| lower.contains(k))
    {
        return Some("log_key");
    }
    None
}

// ---------------------------------------------------------------------------
// EventEmitter dispatch site detection helpers
// ---------------------------------------------------------------------------

pub(super) fn is_event_registration(method: &str) -> bool {
    matches!(
        method,
        "on" | "once" | "addEventListener" | "addListener" | "subscribe" | "prependListener"
    )
}

pub(super) fn is_event_dispatch(method: &str) -> bool {
    matches!(
        method,
        "emit" | "trigger" | "dispatchEvent" | "publish" | "fire" | "send"
    )
}

/// Extract the Nth non-punctuation argument node from a call expression's `arguments`.
pub(super) fn nth_arg_node<'a>(
    call_node: &tree_sitter::Node<'a>,
    source: &[u8],
    n: usize,
) -> Option<tree_sitter::Node<'a>> {
    let args = child_by_kind(call_node, "arguments")?;
    let mut cursor = args.walk();
    let mut idx = 0usize;
    for child in args.children(&mut cursor) {
        if matches!(child.kind(), "(" | ")" | ",") {
            continue;
        }
        if idx == n {
            return Some(child);
        }
        idx += 1;
    }
    let _ = source; // kept in signature for consistency
    None
}

/// Extract handler expression text from the second (or last) argument of a
/// `.on('event', handler)` style call. Returns `None` for inline closures.
pub(super) fn extract_handler_expr(call_node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Try the second argument first; if it doesn't exist fall back to last.
    let handler_node = nth_arg_node(call_node, source, 1)?;
    match handler_node.kind() {
        "identifier" | "member_expression" => {
            node_text(&handler_node, source).map(|s| s.to_string())
        }
        // Inline arrow / function — not a named handler reference.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Methods on JsTsParser — literals, semantic edges, data flow, type assigns
// ---------------------------------------------------------------------------

impl JsTsParser {
    // -------------------------------------------------------------------
    // Literal extraction
    // -------------------------------------------------------------------

    pub(super) fn add_literal(
        &self,
        value: &str,
        node: &tree_sitter::Node,
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
    ) {
        if let Some(kind) = classify_literal(value) {
            let key_path = if kind == "config_key" {
                Some(value.to_string())
            } else {
                None
            };
            ctx.literals.push(LiteralRecord {
                literal_id: StableId::edge_id(
                    "lit",
                    file_path,
                    node.start_position().row as u32 + 1,
                    node.start_position().column as u32,
                ),
                file_path: file_path.to_string(),
                literal: value.to_string(),
                literal_kind: kind.to_string(),
                line: node.start_position().row as u32 + 1,
                container: container.map(String::from),
                confidence: 0.92 * 0.85,
                enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                key_path,
            });
        }
    }

    // -------------------------------------------------------------------
    // Semantic edges (inheritance, implements, decorators)
    // -------------------------------------------------------------------

    pub(super) fn extract_semantic_edges(
        &self,
        content: &str,
        file_path: &str,
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let lines: Vec<&str> = content.lines().collect();
        let mut edges = Vec::new();

        // class Foo extends Bar
        for cap in JS_EXTENDS_RE.captures_iter(content) {
            let class_name = &cap[1];
            let parent_name = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:inherits:{}", file_path, line, parent_name),
                file_path: file_path.to_string(),
                source_symbol: class_name.to_string(),
                source_symbol_uid: None,
                target_symbol: parent_name.to_string(),
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Inherits,
                line,
                confidence: 0.95,
                parser_tier: tier,
            });
        }

        // class Foo implements Bar, Baz (TypeScript)
        for cap in JS_IMPLEMENTS_RE.captures_iter(content) {
            let class_name = &cap[1];
            let impl_list = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            for iface in impl_list.split(',') {
                let iface = iface.trim();
                let iface_name = iface.split('<').next().unwrap_or(iface).trim();
                if iface_name.is_empty() {
                    continue;
                }
                edges.push(SemanticEdgeRecord {
                    edge_id: format!("se-{}:{}:implements:{}", file_path, line, iface_name),
                    file_path: file_path.to_string(),
                    source_symbol: class_name.to_string(),
                    source_symbol_uid: None,
                    target_symbol: iface_name.to_string(),
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Implements,
                    line,
                    confidence: 0.95,
                    parser_tier: tier,
                });
            }
        }

        // @decorator
        for cap in JS_DECORATOR_RE.captures_iter(content) {
            let dec_name = &cap[1];
            let m = cap.get(0).unwrap();
            let line_idx = content[..m.start()].matches('\n').count();
            let line = line_idx as u32 + 1;
            // Find the next class/function
            let mut target_name = String::new();
            for next_line in lines.iter().skip(line_idx + 1) {
                let trimmed = next_line.trim();
                if trimmed.is_empty() || trimmed.starts_with('@') {
                    continue;
                }
                if let Some(c) = JS_EXTENDS_RE.captures(next_line) {
                    target_name = c[1].to_string();
                } else if trimmed.contains("class ") {
                    // Simple class extraction
                    let after = trimmed.split_once("class ").map(|(_, r)| r).unwrap_or("");
                    if let Some(name) = after
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .next()
                    {
                        if !name.is_empty() {
                            target_name = name.to_string();
                        }
                    }
                } else if trimmed.contains("function ") {
                    let after = trimmed
                        .split_once("function ")
                        .map(|(_, r)| r)
                        .unwrap_or("");
                    if let Some(name) = after
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .next()
                    {
                        if !name.is_empty() {
                            target_name = name.to_string();
                        }
                    }
                }
                break;
            }
            if target_name.is_empty() {
                continue;
            }
            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:decorates:{}", file_path, line, dec_name),
                file_path: file_path.to_string(),
                source_symbol: dec_name.to_string(),
                source_symbol_uid: None,
                target_symbol: target_name,
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Decorates,
                line,
                confidence: 0.95,
                parser_tier: tier,
            });
        }

        edges
    }

    // -------------------------------------------------------------------
    // Type refs (data flow edges)
    // -------------------------------------------------------------------

    /// Extract type annotation references from TypeScript code.
    ///
    /// Matches parameter/variable type annotations (`: TypeName`) and return
    /// type annotations (`): TypeName`). Uses regex-based post-processing.
    pub(super) fn extract_type_refs(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        // Parameter / variable type annotations
        for cap in JS_TYPE_ANNOT_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let source_uid =
                js_find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

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

        // Return type annotations — deduplicate with parameter annotations
        // by checking that the `)` prefix makes it a distinct match location
        for cap in JS_RETURN_TYPE_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let col = m.start() as u32;

            // Skip if already covered by JS_TYPE_ANNOT_RE at same position
            if edges.iter().any(|e| {
                e.line == line && e.edge_id == StableId::edge_id("dfe", file_path, line, col)
            }) {
                continue;
            }

            let source_uid =
                js_find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfe", file_path, line, col),
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

    // -------------------------------------------------------------------
    // Environment variable access
    // -------------------------------------------------------------------

    /// Extract environment variable accesses from JS/TS code.
    ///
    /// Matches `process.env.KEY`, `process.env["KEY"]`, and
    /// `Deno.env.get("KEY")`.
    pub(super) fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        for cap in JS_ENV_ACCESS_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let env_key = cap
                .get(1)
                .or(cap.get(2))
                .or(cap.get(3))
                .map(|m| m.as_str().to_string());

            let source_uid =
                js_find_enclosing_function(symbols, line).and_then(|s| s.symbol_uid.clone());

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
        self.walk_for_type_assigns_jsts(&root, source, file_path, symbols, &mut assigns);
        assigns
    }

    /// Recursively walk AST to find `variable_declarator` nodes.
    fn walk_for_type_assigns_jsts(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        if node.kind() == "variable_declarator" {
            self.extract_var_declarator_type_assign(node, source, file_path, symbols, assigns);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_type_assigns_jsts(&child, source, file_path, symbols, assigns);
        }
    }

    /// Extract type info from a `variable_declarator` node.
    fn extract_var_declarator_type_assign(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let var_name = match name_node.utf8_text(source).ok() {
            Some(n) => n.to_string(),
            None => return,
        };

        let line = node.start_position().row as u32 + 1;
        let enclosing =
            Self::find_enclosing_symbol(symbols, line).and_then(|s| s.symbol_uid.clone());

        // Check for TypeScript type annotation on the name node
        // e.g., `const x: Foo = ...`
        let type_annotation = self.find_ts_type_annotation(&name_node, source);

        // Check for `new_expression` in the value
        let value_node = node.child_by_field_name("value");
        let new_type = value_node.and_then(|v| self.find_new_expression_type(&v, source));

        if let Some(new_type_name) = new_type {
            assigns.push(TypeAssignRecord {
                file_path: file_path.to_string(),
                enclosing_symbol_uid: enclosing.clone(),
                var_name: var_name.clone(),
                type_name: new_type_name,
                line,
                confidence: 0.95,
                source: TypeAssignSource::Constructor,
            });
        } else if let Some(type_name) = type_annotation {
            assigns.push(TypeAssignRecord {
                file_path: file_path.to_string(),
                enclosing_symbol_uid: enclosing,
                var_name,
                type_name,
                line,
                confidence: 0.95,
                source: TypeAssignSource::TypeAnnotation,
            });
        }
    }

    /// Find TypeScript type annotation child of a node.
    /// Looks for `type_annotation` child containing `: TypeName`.
    pub(super) fn find_ts_type_annotation(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        let mut cursor = node.walk();
        // Check siblings — in TS, type_annotation is often a sibling or child
        // of the name node within the declarator
        let parent = node.parent()?;
        let mut pcursor = parent.walk();
        for child in parent.children(&mut pcursor) {
            if child.kind() == "type_annotation" {
                // Get the type text — skip the leading ":"
                let text = child.utf8_text(source).ok()?;
                let trimmed = text.trim().trim_start_matches(':').trim();
                // Strip generics for base type
                let base = if let Some(idx) = trimmed.find('<') {
                    &trimmed[..idx]
                } else {
                    trimmed
                };
                let base = base.trim();
                if !base.is_empty() && base.starts_with(|c: char| c.is_ascii_uppercase()) {
                    return Some(base.to_string());
                }
            }
        }
        // Also check direct children
        for child in node.children(&mut cursor) {
            if child.kind() == "type_annotation" {
                let text = child.utf8_text(source).ok()?;
                let trimmed = text.trim().trim_start_matches(':').trim();
                let base = if let Some(idx) = trimmed.find('<') {
                    &trimmed[..idx]
                } else {
                    trimmed
                };
                let base = base.trim();
                if !base.is_empty() && base.starts_with(|c: char| c.is_ascii_uppercase()) {
                    return Some(base.to_string());
                }
            }
        }
        None
    }

    /// Find the type name from a `new_expression` (`new Foo(...)`).
    #[allow(clippy::only_used_in_recursion)]
    pub(super) fn find_new_expression_type(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        if node.kind() == "new_expression" {
            let constructor = node.child_by_field_name("constructor")?;
            let text = constructor.utf8_text(source).ok()?;
            let base = if let Some(idx) = text.find('<') {
                &text[..idx]
            } else {
                text
            };
            let base = base.trim();
            if !base.is_empty() {
                return Some(base.to_string());
            }
        }
        // Check if the node contains a new_expression (e.g., `await new Foo()`)
        if node.kind() == "await_expression" {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if let Some(t) = self.find_new_expression_type(&child, source) {
                    return Some(t);
                }
            }
        }
        None
    }

    // -------------------------------------------------------------------
    // Regex-based ref/call extraction
    // -------------------------------------------------------------------

    /// Build refs and calls from regex scanning (legacy, for intra-file resolution).
    pub(super) fn extract_refs_and_calls(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let keywords: HashSet<&str> = JS_KEYWORDS.iter().copied().collect();
        let mut refs = Vec::new();
        let mut calls = Vec::new();

        let mut by_name: HashMap<String, (&str, &str)> = HashMap::new();
        for sym in symbols {
            if let Some(uid) = &sym.symbol_uid {
                by_name
                    .entry(sym.name.clone())
                    .or_insert((sym.symbol_id.as_str(), uid.as_str()));
            }
        }

        for sym in symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        {
            let start = sym.start_line.saturating_sub(1) as usize;
            let end = (sym.end_line as usize).min(lines.len());
            for (offset, line) in lines[start..end].iter().enumerate() {
                let line_no = (start + offset + 1) as u32;
                let mut call_starts = HashSet::new();
                for cap in JS_CALL_RE.captures_iter(line) {
                    let Some(m) = cap.get(1) else { continue };
                    let callee = m.as_str();
                    if keywords.contains(callee) {
                        continue;
                    }
                    let start_col = m.start() as u32;
                    call_starts.insert(start_col);
                    let target = by_name.get(callee);
                    let ref_id = StableId::ref_id(file_path, callee, line_no, start_col);
                    refs.push(SymbolRefRecord {
                        ref_id: ref_id.clone(),
                        file_path: file_path.to_string(),
                        symbol_name: callee.to_string(),
                        container: sym.qname.clone(),
                        ref_kind: "call".into(),
                        line: line_no,
                        column: start_col,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        ref_name: Some(callee.to_string()),
                        scope_id: sym.scope_id.clone(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        ref_end_line: Some(line_no),
                        ref_end_col: Some(m.end() as u32),
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.7,
                    });
                    calls.push(CallEdgeRecord {
                        edge_id: StableId::edge_id("call", file_path, line_no, start_col),
                        file_path: file_path.to_string(),
                        caller_symbol: Some(sym.name.clone()),
                        callee_symbol: callee.to_string(),
                        line: line_no,
                        start_col,
                        end_line: Some(line_no),
                        end_col: m.end() as u32,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        caller_symbol_id: Some(sym.symbol_id.clone()),
                        caller_symbol_uid: sym.symbol_uid.clone(),
                        callee_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        callee_ref_id: Some(ref_id),
                        dispatch_kind: DispatchKind::Direct,
                        call_kind: "direct".into(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        receiver_expr: None,
                        arg_count: None,
                        is_optional_chain: false,
                        is_awaited: false,
                        is_constructor: false,
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.7,
                        synthesized_by: None,
                        synthesis_key: None,
                        registered_file: None,
                        registered_line: None,
                    });
                }

                for m in JS_IDENT_RE.find_iter(line) {
                    let ident = m.as_str();
                    if keywords.contains(ident)
                        || (line_no == sym.start_line && ident == sym.name)
                        || call_starts.contains(&(m.start() as u32))
                    {
                        continue;
                    }
                    let target = by_name.get(ident);
                    refs.push(SymbolRefRecord {
                        ref_id: StableId::ref_id(file_path, ident, line_no, m.start() as u32),
                        file_path: file_path.to_string(),
                        symbol_name: ident.to_string(),
                        container: sym.qname.clone(),
                        ref_kind: "identifier".into(),
                        line: line_no,
                        column: m.start() as u32,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        ref_name: Some(ident.to_string()),
                        scope_id: sym.scope_id.clone(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        ref_end_line: Some(line_no),
                        ref_end_col: Some(m.end() as u32),
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.6,
                    });
                }
            }
        }

        (refs, calls)
    }
}
