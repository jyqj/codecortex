//! JS/TS parser using tree-sitter with semantic extraction.
//!
//! Ported from Python `jsts_semantic.py` — includes route extraction,
//! middleware detection, NestJS decorators, framework role classification,
//! Next.js file routes, and literal indexing.

mod extras;
mod routes;

use crate::chunker::Chunker;
use crate::http_call_helpers::*;
use crate::traits::FileParser;
use cc_model::diagnostic::LiteralRecord;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{
    CallEdgeRecord, DispatchKind, HttpCallEdgeRecord, ImportRecord, ResolutionKind, RouteEdgeRecord,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// Re-import items from submodules for internal use
#[cfg(test)]
use extras::classify_literal;
use extras::{extract_handler_expr, is_event_dispatch, is_event_registration};
use routes::{detect_nextjs_file_route, NESTJS_DECORATORS, ROUTER_OBJECTS, ROUTE_METHODS};

// ---------------------------------------------------------------------------
// Static data
// ---------------------------------------------------------------------------

static JS_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("js call regex"));
static JS_IDENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").expect("js ident regex"));

/// Matches `class Foo extends Bar` — captures class name and parent.
static JS_EXTENDS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\s+extends\s+([A-Za-z_][A-Za-z0-9_.]*)")
        .expect("js extends re")
});

/// Matches `class Foo implements Bar, Baz` (TypeScript) — captures class name and implements list.
static JS_IMPLEMENTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\s+(?:extends\s+[A-Za-z_][A-Za-z0-9_.]*\s+)?implements\s+([A-Za-z_][A-Za-z0-9_.,\s<>]*)")
        .expect("js implements re")
});

/// Matches `@decorator` on its own line (used in TS).
static JS_DECORATOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*@([A-Za-z_][A-Za-z0-9_.]*)").expect("js decorator re"));

/// Matches TypeScript parameter/variable type annotations: `param: TypeName` or `: TypeName<...>`
static JS_TYPE_ANNOT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r":\s*([A-Z]\w*(?:<[\w\s,<>]+>)?)").expect("js type annotation regex")
});

/// Matches TypeScript return type annotations: `): TypeName` or `): TypeName<...>`
static JS_RETURN_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\)\s*:\s*([A-Z]\w*(?:<[\w\s,<>]+>)?)").expect("js return type regex")
});

/// Matches `process.env.KEY` or `process.env["KEY"]` or `process.env['KEY']`
static JS_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"process\.env\.(\w+)|process\.env\[["'](\w+)["']\]|Deno\.env\.get\(["'](\w+)["']\)"#,
    )
    .expect("js env access regex")
});

static JS_KEYWORDS: &[&str] = &[
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "break",
    "continue",
    "throw",
    "try",
    "catch",
    "finally",
    "const",
    "let",
    "var",
    "class",
    "new",
    "import",
    "export",
    "default",
    "extends",
    "async",
    "await",
    "typeof",
    "instanceof",
    "this",
    "super",
    "delete",
    "void",
    "in",
    "of",
    "with",
    "debugger",
    "yield",
    "do",
    "true",
    "false",
    "null",
    "undefined",
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Classify framework role for a symbol based on naming conventions.
fn classify_framework_role(
    name: &str,
    kind: SymbolKind,
    container: Option<&str>,
) -> Option<String> {
    let lower = name.to_lowercase();

    // React hooks: useXxx (starts with "use", 4+ chars, 4th char uppercase)
    if name.starts_with("use") && name.len() > 3 {
        if let Some(ch) = name.chars().nth(3) {
            if ch.is_uppercase() {
                return Some("hook".to_string());
            }
        }
    }

    // React components: PascalCase top-level functions
    if kind == SymbolKind::Function && container.is_none() {
        if let Some(first) = name.chars().next() {
            if first.is_uppercase() && !name.chars().all(|c| c.is_uppercase() || c == '_') {
                return Some("component".to_string());
            }
        }
    }

    // Middleware patterns
    if matches!(kind, SymbolKind::Function | SymbolKind::Method) && lower.contains("middleware") {
        return Some("middleware".to_string());
    }

    // Controller pattern (NestJS)
    if kind == SymbolKind::Class && lower.contains("controller") {
        return Some("controller".to_string());
    }

    // Service pattern (NestJS)
    if kind == SymbolKind::Class && lower.contains("service") {
        return Some("service".to_string());
    }

    None
}

/// Get text from a tree-sitter node.
fn node_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    node.utf8_text(source).ok()
}

/// Extract the HTTP method from a fetch() call's second argument (options object).
/// e.g. `fetch("/api", { method: "POST" })` → Some("POST")
/// Returns None if no method is specified; caller should default to "GET".
fn extract_fetch_method(call_node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let args = call_node.child_by_field_name("arguments")?;
    // Find the second non-punctuation argument
    let mut arg_index = 0u32;
    let mut second_arg = None;
    for i in 0..args.child_count() {
        let child = args.child(i)?;
        let kind = child.kind();
        if kind != "," && kind != "(" && kind != ")" {
            arg_index += 1;
            if arg_index == 2 {
                second_arg = Some(child);
                break;
            }
        }
    }
    let obj = second_arg?;
    if obj.kind() != "object" {
        return None;
    }
    // Look for a "method" property in the object literal
    for i in 0..obj.named_child_count() {
        let prop = obj.named_child(i)?;
        if prop.kind() == "pair" {
            let key = prop.child_by_field_name("key")?;
            let key_text = node_text(&key, source)?;
            if key_text == "method" {
                let value = prop.child_by_field_name("value")?;
                let method_text = node_text(&value, source)?;
                let cleaned = method_text.trim_matches(|c| c == '"' || c == '\'');
                if !cleaned.is_empty() {
                    return Some(cleaned.to_uppercase());
                }
            }
        }
    }
    None
}

/// Get text from a node, truncated to max_len.
fn short_text(node: &tree_sitter::Node, source: &[u8], max_len: usize) -> String {
    let text = node.utf8_text(source).unwrap_or("");
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}

/// Find first child with the given kind.
fn child_by_kind<'a>(node: &tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    result
}

/// Count non-punctuation arguments in an `arguments` node.
fn count_args(call_node: &tree_sitter::Node) -> u32 {
    let args = match child_by_kind(call_node, "arguments") {
        Some(a) => a,
        None => return 0,
    };
    let mut cursor = args.walk();
    args.children(&mut cursor)
        .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
        .count() as u32
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct JsTsParser {
    js_lang: tree_sitter::Language,
    ts_lang: tree_sitter::Language,
    tsx_lang: tree_sitter::Language,
    chunker: Chunker,
}

impl JsTsParser {
    pub fn new() -> Self {
        Self {
            js_lang: tree_sitter_javascript::LANGUAGE.into(),
            ts_lang: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tsx_lang: tree_sitter_typescript::LANGUAGE_TSX.into(),
            chunker: Chunker::default(),
        }
    }

    fn ts_language_for(&self, lang: Language) -> &tree_sitter::Language {
        match lang {
            Language::JavaScript | Language::Jsx => &self.js_lang,
            Language::Tsx => &self.tsx_lang,
            _ => &self.ts_lang,
        }
    }

    // -------------------------------------------------------------------
    // Top-level AST extraction
    // -------------------------------------------------------------------

    fn extract_all(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> ExtractResult {
        let mut ctx = ExtractCtx::new(file_path);
        let root = tree.root_node();
        self.visit_node(&root, source, file_path, &mut ctx, None, false, false);
        // Apply pending exports
        self.apply_pending_exports(&mut ctx);
        ctx
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_node(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        in_export: bool,
        is_default_export: bool,
    ) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                let prev_uid = ctx.current_symbol_uid.take();
                let mut func_name: Option<String> = None;
                if let Some(mut sym) = self.extract_function(node, source, file_path, container) {
                    // Framework role
                    if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, container);
                    }
                    if in_export {
                        sym.export_name = Some(sym.name.clone());
                        sym.is_default_export = is_default_export;
                    }
                    func_name = Some(sym.name.clone());
                    ctx.current_symbol_uid = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit function body for calls/routes — use function name as container
                let body_container = func_name.as_deref().or(container);
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit_expression_tree(&body, source, file_path, ctx, body_container);
                }
                // Children visited via generic walk below; uid restored after walk
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(&child, source, file_path, ctx, body_container, false, false);
                }
                ctx.current_symbol_uid = prev_uid;
                return;
            }
            "class_declaration" => {
                if let Some(mut sym) = self.extract_class(node, source, file_path) {
                    if sym.framework_role.is_none() {
                        sym.framework_role = classify_framework_role(&sym.name, sym.kind, None);
                    }
                    if in_export {
                        sym.export_name = Some(sym.name.clone());
                        sym.is_default_export = is_default_export;
                    }
                    let name = sym.name.clone();
                    ctx.symbols.push(sym);
                    if let Some(body) = node.child_by_field_name("body") {
                        self.visit_class_body(&body, source, file_path, ctx, &name);
                    }
                    return; // already visited children
                }
            }
            "method_definition" => {
                let prev_uid = ctx.current_symbol_uid.take();
                let mut method_name: Option<String> = None;
                if let Some(mut sym) = self.extract_method(node, source, file_path, container) {
                    if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, container);
                    }
                    method_name = Some(sym.name.clone());
                    ctx.current_symbol_uid = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit method body for calls/routes — use method name as container
                let body_container = method_name.as_deref().or(container);
                if let Some(body) = child_by_kind(node, "statement_block") {
                    self.visit_expression_tree(&body, source, file_path, ctx, body_container);
                }
                // Children visited via generic walk below; uid restored after walk
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(&child, source, file_path, ctx, body_container, false, false);
                }
                ctx.current_symbol_uid = prev_uid;
                return;
            }
            "lexical_declaration" | "variable_declaration" => {
                self.extract_variable_declarations(
                    node,
                    source,
                    file_path,
                    ctx,
                    container,
                    in_export,
                    is_default_export,
                );
            }
            "export_statement" => {
                let (export_name, is_default) = self.visit_export(node, source, ctx, file_path);
                let _ = export_name; // used via pending_exports
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let k = child.kind();
                    if !matches!(
                        k,
                        "export"
                            | "default"
                            | "string"
                            | "export_clause"
                            | ";"
                            | "{"
                            | "}"
                            | ","
                            | "from"
                            | "*"
                    ) {
                        self.visit_node(
                            &child, source, file_path, ctx, container, true, is_default,
                        );
                    }
                }
                return;
            }
            "import_statement" => {
                if let Some(imp) = self.extract_import(node, source, file_path) {
                    // Check if import path matches a known broker pattern
                    if let Some(broker_match) =
                        crate::broker_patterns::match_broker(&imp.import_string)
                    {
                        // Register all imported names as broker-associated
                        if let Some(ref name) = imp.imported_name {
                            // For `import X from "kafkajs"`, X is broker-associated
                            ctx.broker_imports
                                .insert(name.clone(), broker_match.broker_type.to_string());
                        }
                        if let Some(ref alias) = imp.alias {
                            ctx.broker_imports
                                .insert(alias.clone(), broker_match.broker_type.to_string());
                        }
                    }
                    ctx.imports.push(imp);
                }
            }
            "expression_statement" => {
                // Visit expression children for call/route extraction
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
                return;
            }
            _ => {}
        }

        // Visit children (except for class which we handle above)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(&child, source, file_path, ctx, container, false, false);
        }
    }

    // -------------------------------------------------------------------
    // Class body with decorator support
    // -------------------------------------------------------------------

    fn visit_class_body(
        &self,
        body: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        class_name: &str,
    ) {
        let mut pending_decorators: Vec<tree_sitter::Node> = Vec::new();
        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            if member.kind() == "decorator" {
                pending_decorators.push(member);
                continue;
            }

            let decorators = std::mem::take(&mut pending_decorators);

            if member.kind() == "method_definition" {
                let mut sym_uid_for_body: Option<String> = None;
                if let Some(mut sym) =
                    self.extract_method(&member, source, file_path, Some(class_name))
                {
                    // Check decorators for NestJS route handlers
                    let mut fw_role: Option<String> = None;
                    for dec in &decorators {
                        if let Some(dec_text) = node_text(dec, source) {
                            let dec_clean = dec_text.trim_start_matches('@');
                            let dec_name = dec_clean
                                .split('(')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_lowercase();
                            if NESTJS_DECORATORS
                                .iter()
                                .any(|(k, _)| *k == dec_name.as_str())
                            {
                                fw_role = Some("route_handler".to_string());
                                self.try_extract_nestjs_decorator_route(
                                    dec, &member, class_name, source, file_path, ctx,
                                );
                            }
                        }
                    }
                    if fw_role.is_some() {
                        sym.framework_role = fw_role;
                    } else if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, Some(class_name));
                    }
                    sym_uid_for_body = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit method body
                if let Some(block) = child_by_kind(&member, "statement_block") {
                    let prev_uid = ctx.current_symbol_uid.take();
                    ctx.current_symbol_uid = sym_uid_for_body;
                    self.visit_expression_tree(&block, source, file_path, ctx, Some(class_name));
                    ctx.current_symbol_uid = prev_uid;
                }
            } else if member.kind() == "field_definition" {
                // field_definition may contain arrow functions
                if let Some(name_node) = member.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        // Check for arrow function value
                        let mut has_fn = false;
                        let mut inner_cursor = member.walk();
                        for child in member.children(&mut inner_cursor) {
                            if matches!(child.kind(), "arrow_function" | "function_expression") {
                                has_fn = true;
                                let params = child
                                    .child_by_field_name("parameters")
                                    .and_then(|n| n.utf8_text(source).ok())
                                    .unwrap_or("()");
                                let qname = format!("{}.{}", class_name, name);
                                let mut sym = self.make_symbol(
                                    &member,
                                    file_path,
                                    name,
                                    SymbolKind::Method,
                                    &qname,
                                    Some(class_name),
                                    Some(&format!("{}{}", name, params)),
                                    source,
                                );
                                sym.framework_role = classify_framework_role(
                                    name,
                                    SymbolKind::Method,
                                    Some(class_name),
                                );
                                let field_sym_uid = sym.symbol_uid.clone();
                                ctx.symbols.push(sym);
                                // Visit body
                                if let Some(block) = child_by_kind(&child, "statement_block") {
                                    let prev_uid = ctx.current_symbol_uid.take();
                                    ctx.current_symbol_uid = field_sym_uid;
                                    self.visit_expression_tree(
                                        &block,
                                        source,
                                        file_path,
                                        ctx,
                                        Some(class_name),
                                    );
                                    ctx.current_symbol_uid = prev_uid;
                                }
                                break;
                            }
                        }
                        if !has_fn {
                            // Non-function field — still visit value for calls
                            let mut inner_cursor2 = member.walk();
                            for child in member.children(&mut inner_cursor2) {
                                if child.kind() != "property_identifier"
                                    && child.kind() != "="
                                    && child.kind() != ";"
                                {
                                    self.visit_expression_tree(
                                        &child,
                                        source,
                                        file_path,
                                        ctx,
                                        Some(class_name),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Expression tree walking — extracts calls, routes, literals
    // -------------------------------------------------------------------

    fn visit_expression_tree(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
    ) {
        match node.kind() {
            "call_expression" => {
                self.visit_call_expression(node, source, file_path, ctx, container, false);
                // Also visit arguments for nested refs
                if let Some(args) = child_by_kind(node, "arguments") {
                    let mut cursor = args.walk();
                    for child in args.children(&mut cursor) {
                        if !matches!(child.kind(), "(" | ")" | ",") {
                            self.visit_expression_tree(&child, source, file_path, ctx, container);
                        }
                    }
                }
            }
            "new_expression" => {
                self.visit_new_expression(node, source, file_path, ctx, container);
            }
            "await_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "call_expression" {
                        self.visit_call_expression(&child, source, file_path, ctx, container, true);
                    } else if child.kind() != "await" {
                        self.visit_expression_tree(&child, source, file_path, ctx, container);
                    }
                }
            }
            "string" => {
                if let Some(text) = node_text(node, source) {
                    let value =
                        text.trim_matches(|c: char| c == '\'' || c == '"' || c == '\u{0060}');
                    if (3..=160).contains(&value.len()) {
                        self.add_literal(value, node, file_path, ctx, container);
                    }
                }
            }
            "template_string" => {
                if let Some(text) = node_text(node, source) {
                    let value = text.trim_matches('\u{0060}');
                    if (3..=160).contains(&value.len()) {
                        self.add_literal(value, node, file_path, ctx, container);
                    }
                }
            }
            // JSX tag extraction: <ComponentName /> or <ComponentName>...</ComponentName>
            "jsx_opening_element" | "jsx_self_closing_element" => {
                if let Some(tag_node) = node.child_by_field_name("name") {
                    let tag_text = node_text(&tag_node, source).unwrap_or("");
                    // Only PascalCase = user components (not div, span, etc.)
                    if !tag_text.is_empty()
                        && tag_text.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        let line = node.start_position().row as u32 + 1;
                        let col = node.start_position().column as u32;
                        ctx.dispatch_sites.push(DispatchSiteRecord {
                            site_id: StableId::edge_id("dsite", file_path, line, col),
                            file_path: file_path.to_string(),
                            line,
                            col,
                            enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                            receiver_expr: None,
                            site_kind: DispatchSiteKind::JsxTag,
                            key: tag_text.to_string(),
                            handler_expr: None,
                            handler_symbol_uid: None,
                            confidence: 0.85,
                        });
                    }
                }
                // Recurse into children for nested JSX expressions
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            // JSX element containers: recurse into children to find nested JSX tags
            "jsx_element" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            // Named function expressions passed as callbacks: set up proper caller context
            "function_expression" | "function" => {
                let func_name = node
                    .child_by_field_name("name")
                    .and_then(|n| node_text(&n, source))
                    .map(|s| s.to_string());
                if let Some(ref fname) = func_name {
                    if !fname.is_empty() {
                        // Check if this symbol was already registered (e.g., by try_extract_route)
                        let already_registered = ctx
                            .symbols
                            .iter()
                            .any(|s| s.name == *fname && s.file_path == file_path);
                        if !already_registered {
                            let qname = fname.to_string();
                            let sym = self.make_symbol(
                                node,
                                file_path,
                                fname,
                                SymbolKind::Function,
                                &qname,
                                container,
                                None,
                                source,
                            );
                            let prev_uid = ctx.current_symbol_uid.take();
                            ctx.current_symbol_uid = sym.symbol_uid.clone();
                            ctx.symbols.push(sym);
                            // Visit body with the function as container
                            if let Some(body) = node.child_by_field_name("body") {
                                self.visit_expression_tree(
                                    &body,
                                    source,
                                    file_path,
                                    ctx,
                                    Some(fname),
                                );
                            }
                            ctx.current_symbol_uid = prev_uid;
                            return; // Already handled children
                        } else {
                            // Already registered — just set the UID context and visit body
                            let existing_uid = ctx
                                .symbols
                                .iter()
                                .rev()
                                .find(|s| s.name == *fname && s.file_path == file_path)
                                .and_then(|s| s.symbol_uid.clone());
                            let prev_uid = ctx.current_symbol_uid.take();
                            ctx.current_symbol_uid = existing_uid;
                            if let Some(body) = node.child_by_field_name("body") {
                                self.visit_expression_tree(
                                    &body,
                                    source,
                                    file_path,
                                    ctx,
                                    Some(fname),
                                );
                            }
                            ctx.current_symbol_uid = prev_uid;
                            return; // Already handled children
                        }
                    }
                }
                // Anonymous function: just recurse
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            _ => {
                // Recurse into all children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Call expression analysis
    // -------------------------------------------------------------------

    fn visit_call_expression(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        is_awaited: bool,
    ) {
        let func_node = match node.child(0) {
            Some(n) => n,
            None => return,
        };

        let is_optional = func_node.kind() == "optional_chain_expression"
            || func_node.kind().contains("optional");

        if func_node.kind() == "member_expression" {
            // obj.method() pattern
            let obj_node = func_node.child(0);
            let prop_node = {
                let mut found = None;
                let mut cursor = func_node.walk();
                for ch in func_node.children(&mut cursor) {
                    if matches!(ch.kind(), "property_identifier" | "identifier")
                        && Some(ch.id()) != obj_node.as_ref().map(|n| n.id())
                    {
                        found = Some(ch);
                        break;
                    }
                }
                found
            };

            if let (Some(obj), Some(prop)) = (obj_node, prop_node) {
                let obj_text = node_text(&obj, source).unwrap_or("");
                let prop_text = node_text(&prop, source).unwrap_or("");
                let callee = format!("{}.{}", obj_text, prop_text);

                let obj_lower = obj_text.to_lowercase();
                let prop_lower = prop_text.to_lowercase();
                let is_router = ROUTER_OBJECTS.contains(&obj_lower.as_str());

                // Middleware detection: app.use(handler)
                if is_router && prop_lower == "use" {
                    self.try_extract_middleware(node, obj_text, source, file_path, ctx);
                }
                // Route detection: app.get("/path", handler)
                else if is_router && ROUTE_METHODS.contains(&prop_lower.as_str()) {
                    self.try_extract_route(node, obj_text, prop_text, source, file_path, ctx);
                }
                // Broker detection: producer.send(), pubsub.publish(), etc.
                // Try object-name match first, then fall back to import-based mapping.
                else if self
                    .try_emit_broker_edge(node, obj_text, prop_text, source, file_path, ctx)
                {
                    // Broker edge already emitted inside try_emit_broker_edge.
                }
                // HTTP client call detection: axios.get("/api/users"), client.post(url)
                else if is_http_client_object(obj_text) && is_http_verb_method(prop_text) {
                    if let Some(url) = self.extract_first_string_arg(node, source) {
                        if looks_like_url_or_path(&url) {
                            let line = node.start_position().row as u32 + 1;
                            let col = node.start_position().column as u32;
                            let normalized = normalize_template_to_path(&url);
                            ctx.http_call_edges.push(HttpCallEdgeRecord {
                                edge_id: StableId::edge_id("http_call", file_path, line, col),
                                file_path: file_path.to_string(),
                                caller_symbol_uid: ctx.current_symbol_uid.clone(),
                                url_or_path: url.to_string(),
                                normalized_path: Some(
                                    cc_model::route_normalize::normalize_route_path(&normalized),
                                ),
                                method: infer_http_method(prop_text).map(|m| m.to_string()),
                                call_kind: "http".to_string(),
                                line,
                                confidence: 0.85,
                                parser_tier: ParserTier::TreeSitter,
                                broker_type: None,
                            });
                        }
                    }
                }

                // EventEmitter registration: emitter.on('event', handler)
                if is_event_registration(prop_text) {
                    if let Some(event_name) = self.extract_first_string_arg(node, source) {
                        let handler_expr = extract_handler_expr(node, source);
                        let line = node.start_position().row as u32 + 1;
                        let col = node.start_position().column as u32;
                        ctx.dispatch_sites.push(DispatchSiteRecord {
                            site_id: StableId::edge_id("dsite", file_path, line, col),
                            file_path: file_path.to_string(),
                            line,
                            col,
                            enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                            receiver_expr: Some(obj_text.to_string()),
                            site_kind: DispatchSiteKind::EventOn,
                            key: event_name,
                            handler_expr,
                            handler_symbol_uid: None,
                            confidence: 0.85,
                        });
                    }
                }

                // EventEmitter dispatch: emitter.emit('event', data)
                if is_event_dispatch(prop_text) {
                    if let Some(event_name) = self.extract_first_string_arg(node, source) {
                        let line = node.start_position().row as u32 + 1;
                        let col = node.start_position().column as u32;
                        ctx.dispatch_sites.push(DispatchSiteRecord {
                            site_id: StableId::edge_id("dsite", file_path, line, col),
                            file_path: file_path.to_string(),
                            line,
                            col,
                            enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                            receiver_expr: Some(obj_text.to_string()),
                            site_kind: DispatchSiteKind::EventEmit,
                            key: event_name,
                            handler_expr: None,
                            handler_symbol_uid: None,
                            confidence: 0.85,
                        });
                    }
                }

                // Class component this.setState() detection
                if obj_text == "this" && prop_text == "setState" {
                    let line = node.start_position().row as u32 + 1;
                    let col = node.start_position().column as u32;
                    ctx.dispatch_sites.push(DispatchSiteRecord {
                        site_id: StableId::edge_id("dsite", file_path, line, col),
                        file_path: file_path.to_string(),
                        line,
                        col,
                        enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                        receiver_expr: Some("this".to_string()),
                        site_kind: DispatchSiteKind::StateSetterCall,
                        key: "setState".to_string(),
                        handler_expr: None,
                        handler_symbol_uid: None,
                        confidence: 0.85,
                    });
                }

                // Always emit call edge for member expressions
                let arg_count = count_args(node);
                let dispatch = if is_optional {
                    DispatchKind::OptionalChain
                } else {
                    DispatchKind::Dynamic // member dispatch
                };
                ctx.call_edges.push(CallEdgeRecord {
                    edge_id: StableId::edge_id(
                        "call",
                        file_path,
                        node.start_position().row as u32 + 1,
                        node.start_position().column as u32,
                    ),
                    file_path: file_path.to_string(),
                    caller_symbol: container.map(String::from),
                    callee_symbol: callee,
                    line: node.start_position().row as u32 + 1,
                    start_col: node.start_position().column as u32,
                    end_line: Some(node.end_position().row as u32 + 1),
                    end_col: node.end_position().column as u32,
                    target_symbol_id: None,
                    target_file_path: None,
                    caller_symbol_id: None,
                    caller_symbol_uid: ctx.current_symbol_uid.clone(),
                    callee_symbol_uid: None,
                    callee_ref_id: None,
                    dispatch_kind: dispatch,
                    call_kind: "member".into(),
                    resolution_kind: ResolutionKind::Unresolved,
                    resolution_confidence: 0.0,
                    resolution_strategy: "unresolved".into(),
                    receiver_expr: Some(obj_text.to_string()),
                    arg_count: Some(arg_count),
                    is_optional_chain: is_optional,
                    is_awaited,
                    is_constructor: false,
                    parser_tier: ParserTier::Semantic,
                    parser_confidence: 0.85,
                    synthesized_by: None,
                    synthesis_key: None,
                    registered_file: None,
                    registered_line: None,
                });
            }
            return;
        }

        if func_node.kind() == "identifier" {
            let callee = node_text(&func_node, source).unwrap_or("");
            if JS_KEYWORDS.contains(&callee) {
                return;
            }

            // Standalone HTTP call detection: fetch("/api/users")
            if is_standalone_http_call(callee) {
                if let Some(url) = self.extract_first_string_arg(node, source) {
                    if looks_like_url_or_path(&url) {
                        let line = node.start_position().row as u32 + 1;
                        let col = node.start_position().column as u32;
                        let normalized = normalize_template_to_path(&url);
                        ctx.http_call_edges.push(HttpCallEdgeRecord {
                            edge_id: StableId::edge_id("http_call", file_path, line, col),
                            file_path: file_path.to_string(),
                            caller_symbol_uid: ctx.current_symbol_uid.clone(),
                            url_or_path: url.to_string(),
                            normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                                &normalized,
                            )),
                            method: Some(
                                extract_fetch_method(node, source)
                                    .unwrap_or_else(|| "GET".to_string()),
                            ),
                            call_kind: "http".to_string(),
                            line,
                            confidence: 0.85,
                            parser_tier: ParserTier::TreeSitter,
                            broker_type: None,
                        });
                    }
                }
            }

            // React state setter call detection: setCount(...), setName(...), etc.
            // Heuristic: identifier starting with "set" followed by an uppercase letter.
            let callee_bytes = callee.as_bytes();
            if callee_bytes.len() > 3
                && callee_bytes.starts_with(b"set")
                && callee_bytes[3].is_ascii_uppercase()
            {
                let line = node.start_position().row as u32 + 1;
                let col = node.start_position().column as u32;
                ctx.dispatch_sites.push(DispatchSiteRecord {
                    site_id: StableId::edge_id("dsite", file_path, line, col),
                    file_path: file_path.to_string(),
                    line,
                    col,
                    enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                    receiver_expr: None,
                    site_kind: DispatchSiteKind::StateSetterCall,
                    key: callee.to_string(),
                    handler_expr: None,
                    handler_symbol_uid: None,
                    confidence: 0.75,
                });
            }

            let arg_count = count_args(node);
            ctx.call_edges.push(CallEdgeRecord {
                edge_id: StableId::edge_id(
                    "call",
                    file_path,
                    node.start_position().row as u32 + 1,
                    node.start_position().column as u32,
                ),
                file_path: file_path.to_string(),
                caller_symbol: container.map(String::from),
                callee_symbol: callee.to_string(),
                line: node.start_position().row as u32 + 1,
                start_col: node.start_position().column as u32,
                end_line: Some(node.end_position().row as u32 + 1),
                end_col: node.end_position().column as u32,
                target_symbol_id: None,
                target_file_path: None,
                caller_symbol_id: None,
                caller_symbol_uid: ctx.current_symbol_uid.clone(),
                callee_symbol_uid: None,
                callee_ref_id: None,
                dispatch_kind: DispatchKind::Direct,
                call_kind: "direct".into(),
                resolution_kind: ResolutionKind::Unresolved,
                resolution_confidence: 0.0,
                resolution_strategy: "unresolved".into(),
                receiver_expr: None,
                arg_count: Some(arg_count),
                is_optional_chain: false,
                is_awaited,
                is_constructor: false,
                parser_tier: ParserTier::Semantic,
                parser_confidence: 0.85,
                synthesized_by: None,
                synthesis_key: None,
                registered_file: None,
                registered_line: None,
            });
            return;
        }

        // Nested call: foo()()
        if func_node.kind() == "call_expression" {
            self.visit_call_expression(&func_node, source, file_path, ctx, container, is_awaited);
        }
    }

    /// Extract the first string/template argument from a call expression.
    /// Returns the stripped string value, or None if the first arg is not a literal.
    fn extract_first_string_arg(
        &self,
        call_node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        let args_node = child_by_kind(call_node, "arguments")?;
        let mut cursor = args_node.walk();
        let first_arg = args_node
            .children(&mut cursor)
            .find(|c| !matches!(c.kind(), "(" | ")" | ","))?;
        match first_arg.kind() {
            "string" => {
                let text = node_text(&first_arg, source)?;
                Some(strip_string_delimiters(text).to_string())
            }
            "template_string" => {
                let text = node_text(&first_arg, source)?;
                let stripped = strip_string_delimiters(text);
                Some(normalize_template_to_path(stripped))
            }
            _ => None,
        }
    }

    /// Handle: new Foo(args)
    fn visit_new_expression(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
    ) {
        // children: "new" <constructor> <arguments>
        let constructor_node = if node.child_count() >= 2 {
            let first = node.child(0).unwrap();
            if first.kind() == "new" {
                node.child(1)
            } else {
                Some(first)
            }
        } else {
            None
        };

        if let Some(con) = constructor_node {
            let name = node_text(&con, source).unwrap_or("");
            if !name.is_empty() && !JS_KEYWORDS.contains(&name) {
                let arg_count = count_args(node);
                ctx.call_edges.push(CallEdgeRecord {
                    edge_id: StableId::edge_id(
                        "call",
                        file_path,
                        node.start_position().row as u32 + 1,
                        node.start_position().column as u32,
                    ),
                    file_path: file_path.to_string(),
                    caller_symbol: container.map(String::from),
                    callee_symbol: name.to_string(),
                    line: node.start_position().row as u32 + 1,
                    start_col: node.start_position().column as u32,
                    end_line: Some(node.end_position().row as u32 + 1),
                    end_col: node.end_position().column as u32,
                    target_symbol_id: None,
                    target_file_path: None,
                    caller_symbol_id: None,
                    caller_symbol_uid: ctx.current_symbol_uid.clone(),
                    callee_symbol_uid: None,
                    callee_ref_id: None,
                    dispatch_kind: DispatchKind::Constructor,
                    call_kind: "constructor".into(),
                    resolution_kind: ResolutionKind::Unresolved,
                    resolution_confidence: 0.0,
                    resolution_strategy: "unresolved".into(),
                    receiver_expr: None,
                    arg_count: Some(arg_count),
                    is_optional_chain: false,
                    is_awaited: false,
                    is_constructor: true,
                    parser_tier: ParserTier::Semantic,
                    parser_confidence: 0.85,
                    synthesized_by: None,
                    synthesis_key: None,
                    registered_file: None,
                    registered_line: None,
                });
            }
        }
    }

    // -------------------------------------------------------------------
    // Export handling
    // -------------------------------------------------------------------

    /// Process export statement; returns (export_name, is_default).
    fn visit_export(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        ctx: &mut ExtractCtx,
        file_path: &str,
    ) -> (Option<String>, bool) {
        let mut is_default = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "default" {
                is_default = true;
            }
        }

        // Check for re-export: export { ... } from "..."
        let source_node = {
            let mut cursor2 = node.walk();
            let mut found = None;
            for child in node.children(&mut cursor2) {
                if child.kind() == "string" {
                    found = Some(child);
                    break;
                }
            }
            found
        };

        if let Some(src_node) = source_node {
            let src = node_text(&src_node, source)
                .unwrap_or("")
                .trim_matches(|c| c == '\'' || c == '"')
                .to_string();

            // export * from "..."
            let mut cursor3 = node.walk();
            let has_star = node.children(&mut cursor3).any(|c| c.kind() == "*");
            if has_star {
                ctx.imports.push(ImportRecord {
                    file_path: file_path.to_string(),
                    import_string: src,
                    resolved_path: None,
                    imported_name: None,
                    alias: None,
                    is_namespace: true,
                    is_default: false,
                    is_reexport: true,
                });
                return (None, false);
            }

            // export { a as b } from "..."
            let mut cursor4 = node.walk();
            for child in node.children(&mut cursor4) {
                if child.kind() == "export_clause" {
                    let mut spec_cursor = child.walk();
                    for spec in child.children(&mut spec_cursor) {
                        if spec.kind() == "export_specifier" {
                            let names: Vec<tree_sitter::Node> = {
                                let mut sc = spec.walk();
                                spec.children(&mut sc)
                                    .filter(|c| {
                                        matches!(c.kind(), "identifier" | "property_identifier")
                                    })
                                    .collect()
                            };
                            let (imported, exported) = if names.len() >= 2 {
                                (
                                    node_text(&names[0], source).unwrap_or("").to_string(),
                                    node_text(&names[1], source).unwrap_or("").to_string(),
                                )
                            } else if names.len() == 1 {
                                let n = node_text(&names[0], source).unwrap_or("").to_string();
                                (n.clone(), n)
                            } else {
                                continue;
                            };
                            ctx.imports.push(ImportRecord {
                                file_path: file_path.to_string(),
                                import_string: src.clone(),
                                resolved_path: None,
                                imported_name: Some(imported),
                                alias: Some(exported),
                                is_namespace: false,
                                is_default: false,
                                is_reexport: true,
                            });
                        }
                    }
                }
            }
            return (None, false);
        }

        // Local export clause: export { foo, bar as baz }
        let mut cursor5 = node.walk();
        for child in node.children(&mut cursor5) {
            if child.kind() == "export_clause" {
                let mut spec_cursor = child.walk();
                for spec in child.children(&mut spec_cursor) {
                    if spec.kind() == "export_specifier" {
                        let names: Vec<tree_sitter::Node> = {
                            let mut sc = spec.walk();
                            spec.children(&mut sc)
                                .filter(|c| {
                                    matches!(c.kind(), "identifier" | "property_identifier")
                                })
                                .collect()
                        };
                        let (local_name, exported_name) = if names.len() >= 2 {
                            (
                                node_text(&names[0], source).unwrap_or("").to_string(),
                                Some(node_text(&names[1], source).unwrap_or("").to_string()),
                            )
                        } else if names.len() == 1 {
                            let n = node_text(&names[0], source).unwrap_or("").to_string();
                            (n.clone(), Some(n))
                        } else {
                            continue;
                        };
                        ctx.pending_exports.push(PendingExport {
                            local_name,
                            export_name: exported_name,
                            is_default: false,
                        });
                    }
                }
                return (None, false);
            }
        }

        // export default <identifier>
        if is_default {
            let mut cursor6 = node.walk();
            for child in node.children(&mut cursor6) {
                if child.kind() == "identifier" {
                    let local_name = node_text(&child, source).unwrap_or("").to_string();
                    ctx.pending_exports.push(PendingExport {
                        local_name,
                        export_name: None,
                        is_default: true,
                    });
                    break;
                }
            }
        }

        (None, is_default)
    }

    /// Apply deferred export bindings to symbols.
    fn apply_pending_exports(&self, ctx: &mut ExtractCtx) {
        if ctx.pending_exports.is_empty() {
            return;
        }
        // Build name→index lookup (top-level only)
        let mut name_to_idx: HashMap<String, usize> = HashMap::new();
        for (idx, sym) in ctx.symbols.iter().enumerate() {
            if sym.container.is_none() {
                name_to_idx.entry(sym.name.clone()).or_insert(idx);
            }
        }

        for pe in &ctx.pending_exports {
            if let Some(&idx) = name_to_idx.get(&pe.local_name) {
                if pe.is_default {
                    ctx.symbols[idx].is_default_export = true;
                    if ctx.symbols[idx].export_name.is_none() {
                        ctx.symbols[idx].export_name = Some(ctx.symbols[idx].name.clone());
                    }
                } else if let Some(ref en) = pe.export_name {
                    ctx.symbols[idx].export_name = Some(en.clone());
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Symbol extraction (unchanged core logic, enhanced with framework_role)
    // -------------------------------------------------------------------

    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let params_node = node.child_by_field_name("parameters");
        let params = params_node
            .as_ref()
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");
        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };
        let kind = if container.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        // Extract TS type annotations if available
        let (param_types, param_count) = Self::extract_ts_param_info(params_node.as_ref(), source);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.trim().trim_start_matches(':').trim().to_string())
            .filter(|s| !s.is_empty());

        let mut sym = self.make_symbol(
            node,
            file_path,
            name,
            kind,
            &qname,
            container,
            Some(&format!("function {}{}", name, params)),
            source,
        );
        sym.receiver_type = container.map(String::from);
        sym.param_types = param_types;
        sym.return_type = return_type;
        sym.param_count = param_count;
        Some(sym)
    }

    fn extract_class(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        Some(self.make_symbol(
            node,
            file_path,
            name,
            SymbolKind::Class,
            name,
            None,
            Some(&format!("class {}", name)),
            source,
        ))
    }

    fn extract_method(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };

        // Extract param info for methods
        let params_node = node.child_by_field_name("parameters");
        let (param_types, param_count) = Self::extract_ts_param_info(params_node.as_ref(), source);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.trim().trim_start_matches(':').trim().to_string())
            .filter(|s| !s.is_empty());

        let mut sym = self.make_symbol(
            node,
            file_path,
            name,
            SymbolKind::Method,
            &qname,
            container,
            None,
            source,
        );
        sym.receiver_type = container.map(String::from);
        sym.param_types = param_types;
        sym.return_type = return_type;
        sym.param_count = param_count;
        Some(sym)
    }

    /// Extract TypeScript parameter type annotations from a function's parameters node.
    /// For plain JS this will typically return (None, Some(count)).
    fn extract_ts_param_info(
        params_node: Option<&tree_sitter::Node>,
        source: &[u8],
    ) -> (Option<String>, Option<u32>) {
        let params = match params_node {
            Some(n) => n,
            None => return (None, Some(0)),
        };
        let mut types = Vec::new();
        let mut count = 0u32;
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            match child.kind() {
                "required_parameter" | "optional_parameter" | "rest_parameter" => {
                    count += 1;
                    // Try to get the type annotation
                    if let Some(type_ann) = child.child_by_field_name("type") {
                        if let Ok(t) = type_ann.utf8_text(source) {
                            let t = t.trim().trim_start_matches(':').trim();
                            if !t.is_empty() {
                                types.push(t.to_string());
                            }
                        }
                    }
                }
                // Plain JS parameters (identifier nodes)
                "identifier" | "assignment_pattern" | "object_pattern" | "array_pattern" => {
                    count += 1;
                }
                _ => {}
            }
        }
        let pt = if types.is_empty() {
            None
        } else {
            Some(types.join(", "))
        };
        (pt, Some(count))
    }

    #[allow(clippy::too_many_arguments)]
    fn extract_variable_declarations(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        in_export: bool,
        is_default_export: bool,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                // CommonJS require() import extraction:
                // const X = require('mod')  or  const { A, B } = require('./mod')
                if let Some(val_node) = child.child_by_field_name("value") {
                    if val_node.kind() == "call_expression" {
                        let callee = val_node.child(0).and_then(|c| node_text(&c, source));
                        if callee == Some("require") {
                            if let Some(src) = self.extract_first_string_arg(&val_node, source) {
                                if let Some(name_node) = child.child_by_field_name("name") {
                                    if name_node.kind() == "object_pattern" {
                                        // const { A, B } = require('./mod')
                                        let mut oc = name_node.walk();
                                        for prop in name_node.named_children(&mut oc) {
                                            if prop.kind()
                                                == "shorthand_property_identifier_pattern"
                                                || prop.kind() == "shorthand_property_identifier"
                                            {
                                                if let Some(iname) = node_text(&prop, source) {
                                                    ctx.imports.push(ImportRecord {
                                                        file_path: file_path.to_string(),
                                                        import_string: src.clone(),
                                                        resolved_path: None,
                                                        imported_name: Some(iname.to_string()),
                                                        alias: None,
                                                        is_namespace: false,
                                                        is_default: false,
                                                        is_reexport: false,
                                                    });
                                                }
                                            } else if prop.kind() == "pair_pattern"
                                                || prop.kind() == "pair"
                                            {
                                                // const { A: alias } = require(...)
                                                let key = prop
                                                    .child_by_field_name("key")
                                                    .and_then(|k| node_text(&k, source));
                                                let value = prop
                                                    .child_by_field_name("value")
                                                    .and_then(|v| node_text(&v, source));
                                                if let (Some(k), Some(v)) = (key, value) {
                                                    ctx.imports.push(ImportRecord {
                                                        file_path: file_path.to_string(),
                                                        import_string: src.clone(),
                                                        resolved_path: None,
                                                        imported_name: Some(k.to_string()),
                                                        alias: Some(v.to_string()),
                                                        is_namespace: false,
                                                        is_default: false,
                                                        is_reexport: false,
                                                    });
                                                }
                                            }
                                        }
                                    } else {
                                        // const X = require('mod') — default/namespace import
                                        if let Some(iname) = node_text(&name_node, source) {
                                            ctx.imports.push(ImportRecord {
                                                file_path: file_path.to_string(),
                                                import_string: src.clone(),
                                                resolved_path: None,
                                                imported_name: Some(iname.to_string()),
                                                alias: None,
                                                is_namespace: true,
                                                is_default: true,
                                                is_reexport: false,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(name_node) = child.child_by_field_name("name") {
                    // useState setter detection: const [state, setState] = useState(...)
                    if name_node.kind() == "array_pattern" {
                        let value_node = child.child_by_field_name("value");
                        if let Some(ref val) = value_node {
                            if val.kind() == "call_expression" {
                                let callee_text = val
                                    .child(0)
                                    .and_then(|c| node_text(&c, source))
                                    .unwrap_or("");
                                if callee_text == "useState" {
                                    // Extract the setter name (second element of array pattern)
                                    let mut ap_cursor = name_node.walk();
                                    let elements: Vec<_> =
                                        name_node.named_children(&mut ap_cursor).collect();
                                    if elements.len() >= 2 {
                                        if let Some(setter_name) = node_text(&elements[1], source) {
                                            let line = child.start_position().row as u32 + 1;
                                            let col = child.start_position().column as u32;
                                            ctx.dispatch_sites.push(DispatchSiteRecord {
                                                site_id: StableId::edge_id(
                                                    "dsite", file_path, line, col,
                                                ),
                                                file_path: file_path.to_string(),
                                                line,
                                                col,
                                                enclosing_symbol_uid: ctx
                                                    .current_symbol_uid
                                                    .clone(),
                                                receiver_expr: None,
                                                site_kind: DispatchSiteKind::StateSetterBinding,
                                                key: setter_name.to_string(),
                                                handler_expr: None,
                                                handler_symbol_uid: None,
                                                confidence: 0.90,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        // Visit RHS for calls/routes even for destructured vars
                        if let Some(val) = value_node {
                            self.visit_expression_tree(&val, source, file_path, ctx, container);
                        }
                    } else if let Ok(name) = name_node.utf8_text(source) {
                        // Check if RHS is arrow function or function expression
                        let value_node = child.child_by_field_name("value");
                        let kind = value_node
                            .as_ref()
                            .map(|v| match v.kind() {
                                "arrow_function" | "function" | "function_expression" => {
                                    SymbolKind::Function
                                }
                                _ => SymbolKind::Variable,
                            })
                            .unwrap_or(SymbolKind::Variable);

                        let qname = match container {
                            Some(c) => format!("{}.{}", c, name),
                            None => name.to_string(),
                        };

                        let mut sym = self.make_symbol(
                            node, file_path, name, kind, &qname, container, None, source,
                        );
                        sym.framework_role = classify_framework_role(name, kind, container);
                        if in_export {
                            sym.export_name = Some(name.to_string());
                            sym.is_default_export = is_default_export;
                        }
                        ctx.symbols.push(sym);

                        // Visit RHS for calls/routes
                        if let Some(val) = value_node {
                            self.visit_expression_tree(&val, source, file_path, ctx, container);
                        }
                    }
                }
            }
        }
    }

    fn extract_import(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<ImportRecord> {
        let text = node.utf8_text(source).ok()?;
        // Extract source path from import
        let src_node = node.child_by_field_name("source")?;
        let src = src_node
            .utf8_text(source)
            .ok()?
            .trim_matches(|c| c == '"' || c == '\'');
        Some(ImportRecord {
            file_path: file_path.to_string(),
            import_string: src.to_string(),
            resolved_path: None,
            imported_name: Some(text.to_string()),
            alias: None,
            is_namespace: text.contains('*'),
            is_default: text.contains("default"),
            is_reexport: false,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn make_symbol(
        &self,
        node: &tree_sitter::Node,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        qname: &str,
        container: Option<&str>,
        signature: Option<&str>,
        _source: &[u8],
    ) -> SymbolRecord {
        let sid = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let uid = StableId::symbol_uid(file_path, qname, kind.as_str(), signature);
        SymbolRecord {
            symbol_id: sid,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: signature.map(String::from),
            doc: None,
            parser_tier: ParserTier::Semantic,
            parser_confidence: 0.85,
            qname: Some(qname.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    /// Find the innermost enclosing symbol for a given line number.
    pub(crate) fn find_enclosing_symbol(
        symbols: &[SymbolRecord],
        line: u32,
    ) -> Option<&SymbolRecord> {
        symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .filter(|s| s.start_line <= line && s.end_line >= line)
            .min_by_key(|s| s.end_line - s.start_line)
    }
}

/// Find the innermost enclosing function/method for a given line number.
fn js_find_enclosing_function(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| s.start_line <= line && s.end_line >= line)
        .min_by_key(|s| s.end_line - s.start_line)
}

// ---------------------------------------------------------------------------
// Extraction context
// ---------------------------------------------------------------------------

struct PendingExport {
    local_name: String,
    export_name: Option<String>,
    is_default: bool,
}

/// Accumulator for extraction results.
type ExtractResult = ExtractCtx;

struct ExtractCtx {
    symbols: Vec<SymbolRecord>,
    imports: Vec<ImportRecord>,
    route_edges: Vec<RouteEdgeRecord>,
    call_edges: Vec<CallEdgeRecord>,
    http_call_edges: Vec<HttpCallEdgeRecord>,
    dispatch_sites: Vec<DispatchSiteRecord>,
    literals: Vec<LiteralRecord>,
    pending_exports: Vec<PendingExport>,
    /// Maps imported local name → broker_type (e.g. "KafkaProducer" → "kafka").
    /// Built from import paths that match known broker patterns.
    broker_imports: HashMap<String, String>,
    /// UID of the symbol currently being traversed (set during visit_node).
    current_symbol_uid: Option<String>,
}

impl ExtractCtx {
    fn new(_file_path: &str) -> Self {
        Self {
            symbols: Vec::new(),
            imports: Vec::new(),
            route_edges: Vec::new(),
            call_edges: Vec::new(),
            http_call_edges: Vec::new(),
            dispatch_sites: Vec::new(),
            literals: Vec::new(),
            pending_exports: Vec::new(),
            broker_imports: HashMap::new(),
            current_symbol_uid: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Trait impls
// ---------------------------------------------------------------------------

impl Default for JsTsParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for JsTsParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let ts_lang = self.ts_language_for(language);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(ts_lang)
            .map_err(|e| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: e.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "parse failed".to_string(),
            })?;

        // Phase 1: AST-based extraction (symbols, routes, calls from AST, literals)
        let ast_ctx = self.extract_all(&tree, content.as_bytes(), file_path);

        // Phase 2: Regex-based ref/call extraction (for intra-file resolution)
        let (symbol_refs, regex_call_edges) =
            self.extract_refs_and_calls(content, file_path, &ast_ctx.symbols);

        // Merge call edges: AST-based calls first, then regex-based
        let mut all_call_edges = ast_ctx.call_edges;
        // Deduplicate by (line, start_col) — prefer AST-based
        let existing: HashSet<(u32, u32)> = all_call_edges
            .iter()
            .map(|c| (c.line, c.start_col))
            .collect();
        for ce in regex_call_edges {
            if !existing.contains(&(ce.line, ce.start_col)) {
                all_call_edges.push(ce);
            }
        }

        // Next.js file route detection
        let mut route_edges = ast_ctx.route_edges;
        if let Some(nextjs_route) = detect_nextjs_file_route(file_path, &ast_ctx.symbols) {
            route_edges.push(nextjs_route);
        }

        let tier = ParserTier::Semantic;
        let confidence = 0.85;
        let semantic_edges = self.extract_semantic_edges(content, file_path, tier);

        // Extract data flow edges (type refs + env accesses + param/return flow)
        let mut data_flow_edges = self.extract_type_refs(content, &ast_ctx.symbols, file_path);
        data_flow_edges.extend(self.extract_env_accesses(content, &ast_ctx.symbols, file_path));
        data_flow_edges.extend(self.extract_param_return_flow(&all_call_edges, file_path));

        let type_assigns =
            self.extract_type_assigns(&tree, content.as_bytes(), file_path, &ast_ctx.symbols);

        let chunks = self.chunker.chunk_with_symbols(
            file_path,
            content,
            language,
            &ast_ctx.symbols,
            tier,
            confidence,
        );

        let line_count = content.lines().count();
        let lang_str = language.as_str();
        let summary = format!(
            "{} ({}, {} lines, {} symbols, {} routes)",
            file_path,
            lang_str,
            line_count,
            ast_ctx.symbols.len(),
            route_edges.len(),
        );

        let is_test = file_path.contains(".test.")
            || file_path.contains(".spec.")
            || file_path.contains("__tests__");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols: ast_ctx.symbols,
            imports: ast_ctx.imports,
            symbol_refs,
            call_edges: all_call_edges,
            route_edges,
            literal_index: ast_ctx.literals,
            semantic_edges,
            data_flow_edges,
            dispatch_sites: ast_ctx.dispatch_sites,
            type_assigns,
            parser_tier: tier,
            parser_confidence: confidence,
            http_call_edges: ast_ctx.http_call_edges,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        let ts_lang = self.ts_language_for(language);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(ts_lang)
            .map_err(|e| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: e.to_string(),
            })?;
        if let Some(timeout) = timeout_micros {
            parser.set_timeout_micros(timeout);
        }

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: if timeout_micros.is_some() {
                    "parse failed or timed out".to_string()
                } else {
                    "parse failed".to_string()
                },
            })?;

        let ast_ctx = self.extract_all(&tree, content.as_bytes(), file_path);
        let (symbol_refs, regex_call_edges) =
            self.extract_refs_and_calls(content, file_path, &ast_ctx.symbols);

        let mut all_call_edges = ast_ctx.call_edges;
        let existing: HashSet<(u32, u32)> = all_call_edges
            .iter()
            .map(|c| (c.line, c.start_col))
            .collect();
        for ce in regex_call_edges {
            if !existing.contains(&(ce.line, ce.start_col)) {
                all_call_edges.push(ce);
            }
        }

        let mut route_edges = ast_ctx.route_edges;
        if let Some(nextjs_route) = detect_nextjs_file_route(file_path, &ast_ctx.symbols) {
            route_edges.push(nextjs_route);
        }

        let tier = ParserTier::Semantic;
        let confidence = 0.85;
        let semantic_edges = self.extract_semantic_edges(content, file_path, tier);

        // Extract data flow edges (type refs + env accesses + param/return flow)
        let mut data_flow_edges = self.extract_type_refs(content, &ast_ctx.symbols, file_path);
        data_flow_edges.extend(self.extract_env_accesses(content, &ast_ctx.symbols, file_path));
        data_flow_edges.extend(self.extract_param_return_flow(&all_call_edges, file_path));

        let type_assigns =
            self.extract_type_assigns(&tree, content.as_bytes(), file_path, &ast_ctx.symbols);

        let chunks = self.chunker.chunk_with_symbols(
            file_path,
            content,
            language,
            &ast_ctx.symbols,
            tier,
            confidence,
        );

        let summary = format!(
            "{} ({}, {} lines, {} symbols, {} routes)",
            file_path,
            language.as_str(),
            content.lines().count(),
            ast_ctx.symbols.len(),
            route_edges.len(),
        );
        let is_test = file_path.contains(".test.")
            || file_path.contains(".spec.")
            || file_path.contains("__tests__");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols: ast_ctx.symbols,
            imports: ast_ctx.imports,
            symbol_refs,
            call_edges: all_call_edges,
            route_edges,
            literal_index: ast_ctx.literals,
            semantic_edges,
            data_flow_edges,
            dispatch_sites: ast_ctx.dispatch_sites,
            type_assigns,
            parser_tier: tier,
            parser_confidence: confidence,
            http_call_edges: ast_ctx.http_call_edges,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[
            Language::JavaScript,
            Language::TypeScript,
            Language::Tsx,
            Language::Jsx,
        ]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Semantic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_js() {
        let p = JsTsParser::new();
        let code = r#"
function greet(name) {
    return "Hello " + name;
}

class Greeter {
    constructor(prefix) {
        this.prefix = prefix;
    }
    greet(name) {
        return this.prefix + name;
    }
}
"#;
        let outcome = p.parse("app.js", code, Language::JavaScript).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"Greeter"));
        assert!(!outcome.chunks.is_empty());
    }

    #[test]
    fn parse_typescript_arrow() {
        let p = JsTsParser::new();
        let code = "const add = (a: number, b: number): number => a + b;\n";
        let outcome = p.parse("math.ts", code, Language::TypeScript).unwrap();
        assert!(!outcome.symbols.is_empty());
        let add = outcome.symbols.iter().find(|s| s.name == "add");
        assert!(add.is_some());
    }

    #[test]
    fn extract_express_route() {
        let p = JsTsParser::new();
        let code = r#"
const express = require("express");
const app = express();

app.get("/api/users", getUsers);
app.post("/api/users", createUser);
app.use("/api", authMiddleware);
"#;
        let outcome = p.parse("server.js", code, Language::JavaScript).unwrap();
        assert!(
            outcome.route_edges.len() >= 3,
            "expected >= 3 routes, got {}",
            outcome.route_edges.len()
        );

        let get_route = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/api/users" && r.method.as_deref() == Some("GET"));
        assert!(get_route.is_some(), "missing GET /api/users route");
        assert_eq!(get_route.unwrap().handler_name.as_deref(), Some("getUsers"));

        let post_route = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/api/users" && r.method.as_deref() == Some("POST"));
        assert!(post_route.is_some(), "missing POST /api/users route");

        let middleware = outcome
            .route_edges
            .iter()
            .find(|r| r.route_kind.as_deref() == Some("middleware"));
        assert!(middleware.is_some(), "missing middleware");
        assert_eq!(middleware.unwrap().method.as_deref(), Some("USE"));
    }

    #[test]
    fn extract_framework_roles() {
        let p = JsTsParser::new();
        let code = r#"
function useAuth() { return {}; }
function UserProfile() { return null; }
function processData() { return null; }
class UserController {}
class AuthService {}
"#;
        let outcome = p.parse("app.tsx", code, Language::Tsx).unwrap();

        let hook = outcome.symbols.iter().find(|s| s.name == "useAuth");
        assert!(hook.is_some());
        assert_eq!(
            hook.unwrap().framework_role.as_deref(),
            Some("hook"),
            "useAuth should be classified as hook"
        );

        let component = outcome.symbols.iter().find(|s| s.name == "UserProfile");
        assert!(component.is_some());
        assert_eq!(
            component.unwrap().framework_role.as_deref(),
            Some("component"),
            "UserProfile should be classified as component"
        );

        let controller = outcome.symbols.iter().find(|s| s.name == "UserController");
        assert!(controller.is_some());
        assert_eq!(
            controller.unwrap().framework_role.as_deref(),
            Some("controller"),
            "UserController should be classified as controller"
        );

        let service = outcome.symbols.iter().find(|s| s.name == "AuthService");
        assert!(service.is_some());
        assert_eq!(
            service.unwrap().framework_role.as_deref(),
            Some("service"),
            "AuthService should be classified as service"
        );

        let plain = outcome.symbols.iter().find(|s| s.name == "processData");
        assert!(plain.is_some());
        assert_eq!(
            plain.unwrap().framework_role,
            None,
            "processData should have no framework_role"
        );
    }

    #[test]
    fn detect_nextjs_route() {
        let p = JsTsParser::new();
        let code = r#"
export async function GET(request) {
    return Response.json({ ok: true });
}
"#;
        let outcome = p
            .parse("app/api/users/route.ts", code, Language::TypeScript)
            .unwrap();
        let route = outcome
            .route_edges
            .iter()
            .find(|r| r.framework.as_deref() == Some("nextjs"));
        assert!(route.is_some(), "missing Next.js route");
        assert_eq!(route.unwrap().route_path, "/api/users");
    }

    #[test]
    fn extract_literals() {
        let p = JsTsParser::new();
        let code = r#"
const url = "https://api.example.com/v1/users";
const key = "DATABASE_URL";
const query = "SELECT * FROM users WHERE id = 1";
"#;
        let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
        assert!(
            !outcome.literal_index.is_empty(),
            "expected some literals to be extracted"
        );
        let url_lit = outcome
            .literal_index
            .iter()
            .find(|l| l.literal_kind == "url");
        assert!(url_lit.is_some(), "missing url literal");
        let env_lit = outcome
            .literal_index
            .iter()
            .find(|l| l.literal_kind == "env_key");
        assert!(env_lit.is_some(), "missing env_key literal");
    }

    #[test]
    fn extract_call_edges_with_dispatch_kind() {
        let p = JsTsParser::new();
        let code = r#"
function main() {
    doSomething();
    obj.method();
    const x = new MyClass();
}
"#;
        let outcome = p.parse("main.ts", code, Language::TypeScript).unwrap();

        let direct = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "doSomething" && c.dispatch_kind == DispatchKind::Direct);
        assert!(direct.is_some(), "missing direct call edge");

        let member = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "obj.method" && c.call_kind == "member");
        assert!(member.is_some(), "missing member call edge");

        let constructor = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "MyClass" && c.is_constructor);
        assert!(constructor.is_some(), "missing constructor call edge");
    }

    #[test]
    fn extract_http_call_fetch() {
        // 代码片段包含 fetch 调用
        let code = r#"
async function loadUsers() {
    const response = await fetch("/api/users");
    return response.json();
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("app.js", code, Language::JavaScript).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from fetch"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.url_or_path, "/api/users");
        assert_eq!(hce.method, Some("GET".to_string())); // fetch defaults to GET
        assert_eq!(hce.call_kind, "http");
        assert!(hce.normalized_path.is_some());
        assert_eq!(hce.normalized_path.as_deref(), Some("/api/users"));
    }

    #[test]
    fn extract_http_call_axios() {
        let code = r#"
import axios from 'axios';
async function createOrder(data) {
    const res = await axios.post("/api/orders", data);
    return res.data;
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("orders.ts", code, Language::TypeScript).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from axios.post"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.url_or_path, "/api/orders");
        assert_eq!(hce.method, Some("POST".to_string()));
    }

    #[test]
    fn extract_http_call_template_string() {
        let code = r#"
async function getUser(id) {
    return fetch(`/api/users/${id}`);
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("users.js", code, Language::JavaScript).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from fetch with template string"
        );
        let hce = &outcome.http_call_edges[0];
        // Template variables should be normalized to *
        assert!(
            hce.normalized_path.as_deref().unwrap().contains('*'),
            "normalized_path should contain * for template variable, got: {:?}",
            hce.normalized_path
        );
    }

    #[test]
    fn no_false_positive_console_log() {
        // console.log should NOT trigger HTTP call detection
        let code = r#"console.log("/api/test");"#;
        let p = JsTsParser::new();
        let outcome = p.parse("test.js", code, Language::JavaScript).unwrap();
        assert!(
            outcome.http_call_edges.is_empty(),
            "console.log should not be detected as HTTP call"
        );
    }

    #[test]
    fn pending_exports_applied() {
        let p = JsTsParser::new();
        let code = r#"
function foo() { return 1; }
function bar() { return 2; }
export { foo, bar as baz };
export default foo;
"#;
        let outcome = p.parse("mod.ts", code, Language::TypeScript).unwrap();

        let foo = outcome.symbols.iter().find(|s| s.name == "foo");
        assert!(foo.is_some());
        // foo should be both default export and named export
        assert!(
            foo.unwrap().is_default_export,
            "foo should be default export"
        );
    }

    #[test]
    fn extract_fetch_post_method() {
        let code = r#"
async function createOrder(data) {
    const res = await fetch("/api/orders", { method: "POST", body: JSON.stringify(data) });
    return res.json();
}
"#;
        let parser = JsTsParser::new();
        let outcome = parser
            .parse("orders.js", code, Language::JavaScript)
            .unwrap();
        assert!(!outcome.http_call_edges.is_empty());
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.method, Some("POST".to_string()));
    }

    #[test]
    fn extract_fetch_default_get() {
        let code = r#"const data = await fetch("/api/users");"#;
        let parser = JsTsParser::new();
        let outcome = parser.parse("app.js", code, Language::JavaScript).unwrap();
        assert!(!outcome.http_call_edges.is_empty());
        assert_eq!(outcome.http_call_edges[0].method, Some("GET".to_string()));
    }

    #[test]
    fn classify_literal_env_key() {
        assert_eq!(classify_literal("DATABASE_URL"), Some("env_key"));
        assert_eq!(classify_literal("API_KEY"), Some("env_key"));
        assert_eq!(classify_literal("NODE_ENV"), Some("env_key"));
    }

    #[test]
    fn classify_literal_config_key() {
        assert_eq!(classify_literal("app.database.host"), Some("config_key"));
        assert_eq!(
            classify_literal("redis.connection.timeout"),
            Some("config_key")
        );
    }

    #[test]
    fn classify_literal_topic() {
        assert_eq!(classify_literal("orders-topic"), Some("topic"));
        assert_eq!(classify_literal("user-events"), Some("topic"));
        assert_eq!(classify_literal("payment.events"), Some("topic"));
    }

    #[test]
    fn classify_literal_queue() {
        assert_eq!(classify_literal("orders-queue"), Some("queue"));
        assert_eq!(classify_literal("tasks-fifo"), Some("queue"));
        assert_eq!(classify_literal("queue.orders"), Some("queue"));
    }

    #[test]
    fn classify_literal_priority() {
        // URL takes priority over everything
        assert_eq!(classify_literal("https://api.example.com"), Some("url"));
        // Route takes priority
        assert_eq!(classify_literal("/api/users"), Some("route"));
    }

    #[test]
    fn literal_enclosing_symbol_uid() {
        let p = JsTsParser::new();
        let code = r#"
function loadConfig() {
    const url = "https://api.example.com/v1";
    const key = "DATABASE_URL";
}
"#;
        let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
        // All literals inside loadConfig should have enclosing_symbol_uid set
        for lit in &outcome.literal_index {
            assert!(
                lit.enclosing_symbol_uid.is_some(),
                "literal '{}' should have enclosing_symbol_uid",
                lit.literal
            );
        }
    }

    #[test]
    fn literal_config_key_has_key_path() {
        let p = JsTsParser::new();
        let code = r#"
function getConfig() {
    return "app.database.host";
}
"#;
        let outcome = p.parse("config.ts", code, Language::TypeScript).unwrap();
        let config_lit = outcome
            .literal_index
            .iter()
            .find(|l| l.literal_kind == "config_key");
        assert!(config_lit.is_some(), "missing config_key literal");
        assert_eq!(
            config_lit.unwrap().key_path.as_deref(),
            Some("app.database.host")
        );
    }

    // ── Async broker extraction tests ─────────────────────────────

    #[test]
    fn extract_kafka_broker_call() {
        // Object name "kafka" matches OBJECT_PATTERNS → broker_type = "kafka".
        // Method "send" is in ASYNC_METHODS → call_kind = "async".
        let code = r#"
const kafka = require('kafkajs');
async function publishOrder(order) {
    await kafka.send({ topic: 'orders', messages: [{ value: JSON.stringify(order) }] });
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("producer.js", code, Language::JavaScript).unwrap();

        let broker_edges: Vec<_> = outcome
            .http_call_edges
            .iter()
            .filter(|e| e.call_kind == "async")
            .collect();
        assert!(
            !broker_edges.is_empty(),
            "should detect kafka.send() as async broker call"
        );
        assert_eq!(broker_edges[0].call_kind, "async");
        assert_eq!(broker_edges[0].broker_type.as_deref(), Some("kafka"));
    }

    #[test]
    fn extract_bullmq_broker_call() {
        // Object name "bullQueue" contains "bull" → matches OBJECT_PATTERNS → broker_type = "bullmq".
        // Method "dispatch" is in ASYNC_METHODS → call_kind = "async".
        // BullMQ's `queue.add()` is detected because "add" is in ASYNC_METHODS.
        let code = r#"
import { Queue } from 'bullmq';
const bullQueue = new Queue('notifications');
await bullQueue.dispatch('send-email', { to: 'user@example.com' });
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("queue.ts", code, Language::TypeScript).unwrap();

        let broker_edges: Vec<_> = outcome
            .http_call_edges
            .iter()
            .filter(|e| e.call_kind == "async")
            .collect();
        assert!(
            !broker_edges.is_empty(),
            "should detect bullQueue.dispatch() as async broker call"
        );
        assert_eq!(broker_edges[0].broker_type.as_deref(), Some("bullmq"));
    }

    #[test]
    fn http_call_not_misclassified_as_broker() {
        // Plain HTTP calls via axios should produce call_kind = "http", broker_type = None.
        let code = r#"
import axios from 'axios';
async function getUsers() {
    const res = await axios.get('/api/users');
    return res.data;
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("client.ts", code, Language::TypeScript).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from axios.get"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.call_kind, "http");
        assert_eq!(
            hce.broker_type, None,
            "plain HTTP call should have no broker_type"
        );
    }

    #[test]
    fn test_event_emitter_dispatch_sites() {
        let source = r#"
const emitter = new EventEmitter();
emitter.on('user:created', handleUser);
emitter.emit('user:created', data);
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
        assert_eq!(
            outcome.dispatch_sites.len(),
            2,
            "should extract 2 dispatch sites (on + emit), got: {:?}",
            outcome.dispatch_sites
        );
        let on_site = outcome
            .dispatch_sites
            .iter()
            .find(|s| s.site_kind == DispatchSiteKind::EventOn)
            .expect("should have an EventOn dispatch site");
        assert_eq!(on_site.key, "user:created");
        assert_eq!(on_site.handler_expr.as_deref(), Some("handleUser"));
        assert_eq!(on_site.receiver_expr.as_deref(), Some("emitter"));

        let emit_site = outcome
            .dispatch_sites
            .iter()
            .find(|s| s.site_kind == DispatchSiteKind::EventEmit)
            .expect("should have an EventEmit dispatch site");
        assert_eq!(emit_site.key, "user:created");
        assert!(emit_site.handler_expr.is_none());
        assert_eq!(emit_site.receiver_expr.as_deref(), Some("emitter"));
    }

    #[test]
    fn test_event_listener_variants() {
        let source = r#"
window.addEventListener('click', handler);
bus.once('ready', onReady);
bus.subscribe('data', processData);
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
        assert_eq!(
            outcome.dispatch_sites.len(),
            3,
            "should detect addEventListener, once, subscribe"
        );
        assert!(outcome
            .dispatch_sites
            .iter()
            .all(|s| s.site_kind == DispatchSiteKind::EventOn));
        let keys: Vec<&str> = outcome
            .dispatch_sites
            .iter()
            .map(|s| s.key.as_str())
            .collect();
        assert!(keys.contains(&"click"));
        assert!(keys.contains(&"ready"));
        assert!(keys.contains(&"data"));
    }

    #[test]
    fn test_event_dispatch_variants() {
        let source = r#"
emitter.emit('start', payload);
bus.trigger('update');
el.dispatchEvent('custom');
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("test.js", source, Language::JavaScript).unwrap();
        assert_eq!(
            outcome.dispatch_sites.len(),
            3,
            "should detect emit, trigger, dispatchEvent"
        );
        assert!(outcome
            .dispatch_sites
            .iter()
            .all(|s| s.site_kind == DispatchSiteKind::EventEmit));
    }

    #[test]
    fn test_jsx_tag_dispatch_sites() {
        let source = r#"
function App() {
    return (
        <div>
            <UserProfile name="test" />
            <Header />
            <span>text</span>
        </div>
    );
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("app.tsx", source, Language::Tsx).unwrap();
        let jsx_sites: Vec<_> = outcome
            .dispatch_sites
            .iter()
            .filter(|s| s.site_kind == DispatchSiteKind::JsxTag)
            .collect();
        assert_eq!(
            jsx_sites.len(),
            2,
            "should extract 2 JSX tag sites (UserProfile + Header, NOT div/span), got: {:?}",
            jsx_sites
        );
        assert!(jsx_sites.iter().any(|s| s.key == "UserProfile"));
        assert!(jsx_sites.iter().any(|s| s.key == "Header"));
    }

    #[test]
    fn test_jsx_member_expression_tag() {
        let source = r#"
function App() {
    return <Router.Switch><Route path="/" /></Router.Switch>;
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("app.tsx", source, Language::Tsx).unwrap();
        let jsx_sites: Vec<_> = outcome
            .dispatch_sites
            .iter()
            .filter(|s| s.site_kind == DispatchSiteKind::JsxTag)
            .collect();
        // Router.Switch and Route are PascalCase
        assert!(
            jsx_sites.iter().any(|s| s.key.contains("Router")),
            "should detect Router.Switch, got: {:?}",
            jsx_sites
        );
        assert!(
            jsx_sites.iter().any(|s| s.key == "Route"),
            "should detect Route, got: {:?}",
            jsx_sites
        );
    }

    #[test]
    fn test_use_state_setter_dispatch_sites() {
        let source = r#"
function Counter() {
    const [count, setCount] = useState(0);
    const [name, setName] = useState("");
    const handleClick = () => setCount(count + 1);
    return <button onClick={handleClick}>{count}</button>;
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("counter.tsx", source, Language::Tsx).unwrap();

        // Check bindings (StateSetterBinding from useState destructuring)
        let binding_sites: Vec<_> = outcome
            .dispatch_sites
            .iter()
            .filter(|s| s.site_kind == DispatchSiteKind::StateSetterBinding)
            .collect();
        assert!(
            binding_sites.len() >= 2,
            "should detect at least 2 useState bindings (setCount + setName), got: {:?}",
            binding_sites
        );
        assert!(
            binding_sites.iter().any(|s| s.key == "setCount"),
            "should detect setCount binding, got: {:?}",
            binding_sites
        );
        assert!(
            binding_sites.iter().any(|s| s.key == "setName"),
            "should detect setName binding, got: {:?}",
            binding_sites
        );

        // Check calls (StateSetterCall from setCount(...) invocation)
        let call_sites: Vec<_> = outcome
            .dispatch_sites
            .iter()
            .filter(|s| s.site_kind == DispatchSiteKind::StateSetterCall)
            .collect();
        assert!(
            call_sites.iter().any(|s| s.key == "setCount"),
            "should detect setCount call site, got: {:?}",
            call_sites
        );
    }

    #[test]
    fn test_class_set_state_dispatch_sites() {
        let source = r#"
class Counter extends React.Component {
    handleClick() {
        this.setState({ count: this.state.count + 1 });
    }
    render() {
        return <button onClick={this.handleClick}>{this.state.count}</button>;
    }
}
"#;
        let p = JsTsParser::new();
        let outcome = p.parse("counter.tsx", source, Language::Tsx).unwrap();
        let setter_sites: Vec<_> = outcome
            .dispatch_sites
            .iter()
            .filter(|s| s.site_kind == DispatchSiteKind::StateSetterCall)
            .collect();
        assert!(
            setter_sites.iter().any(|s| s.key == "setState"),
            "should detect this.setState as StateSetterCall, got: {:?}",
            setter_sites
        );
        assert_eq!(
            setter_sites
                .iter()
                .find(|s| s.key == "setState")
                .unwrap()
                .receiver_expr
                .as_deref(),
            Some("this")
        );
    }
}
