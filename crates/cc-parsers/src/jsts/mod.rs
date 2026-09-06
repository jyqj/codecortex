//! JS/TS parser using tree-sitter with semantic extraction.
//!
//! Ported from Python `jsts_semantic.py` — includes route extraction,
//! middleware detection, NestJS decorators, framework role classification,
//! Next.js file routes, and literal indexing.
//!
//! Split by responsibility: `visitor` (AST traversal), `symbols` (symbol
//! extraction), `imports_exports` (import/export records), `calls`
//! (call-edge extraction), `routes` (route/framework detection), and
//! `extras` (literals, semantic edges, data flow, type assigns). This file
//! keeps the shared statics, AST cursor helpers, core types, and the
//! outward-facing `FileParser` impl.

mod calls;
mod extras;
mod imports_exports;
mod routes;
mod symbols;
mod visitor;

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::diagnostic::LiteralRecord;
use cc_model::dispatch_site::DispatchSiteRecord;
use cc_model::edge::{CallEdgeRecord, HttpCallEdgeRecord, ImportRecord, RouteEdgeRecord};
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};

/// AST-based call extraction carries receiver/scope context, so it is more
/// precise than the regex fallback in extras.rs (which uses the tier baseline).
const AST_CALL_CONFIDENCE: f64 = 0.85;

/// React state setter detected only by the `setXxx(...)` name pattern.
const STATE_SETTER_NAME_CONFIDENCE: f64 = 0.75;

/// React state setter bound by explicit `const [x, setX] = useState(...)`.
const STATE_SETTER_BINDING_CONFIDENCE: f64 = 0.90;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

// Re-import items from submodules for internal use
use routes::detect_nextjs_file_route;

// ---------------------------------------------------------------------------
// Static data
// ---------------------------------------------------------------------------

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
        format!("{}...", &text[..text.floor_char_boundary(max_len)])
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

    /// Find the innermost enclosing symbol for a given line number.
    pub(crate) fn find_enclosing_symbol(
        symbols: &[SymbolRecord],
        line: u32,
    ) -> Option<&SymbolRecord> {
        crate::dataflow_common::find_enclosing_symbol(symbols, line)
    }
}

/// Find the innermost enclosing function/method for a given line number.
fn js_find_enclosing_function(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    crate::dataflow_common::find_enclosing_symbol(symbols, line)
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
    /// Local binding name → index into `imports`, for ES `import_statement`
    /// records. Used by `apply_pending_exports` to mark two-step forwarding
    /// (`import { x } from './b'; export { x };`) as a re-export.
    import_bindings: HashMap<String, usize>,
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
            import_bindings: HashMap::new(),
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
        self.parse_with_timeout(file_path, content, language, None)
    }

    fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        let ts_lang = self.ts_language_for(language);
        let tree = crate::parse_common::parse_tree(ts_lang, content, file_path, timeout_micros)?;

        let ast_ctx = self.extract_all(&tree, content.as_bytes(), file_path);
        let (symbol_refs, all_call_edges) =
            crate::ast_facts::extract(&tree, content, file_path, &ast_ctx.symbols, language);

        let mut route_edges = ast_ctx.route_edges;
        if let Some(nextjs_route) = detect_nextjs_file_route(file_path, &ast_ctx.symbols) {
            route_edges.push(nextjs_route);
        }

        let tier = ParserTier::Semantic;
        let confidence = tier.default_confidence();
        let semantic_edges = self.extract_semantic_edges(content, file_path, tier);

        // Extract data flow edges (type refs + env accesses + param/return flow)
        let mut data_flow_edges = self.extract_type_refs(content, &ast_ctx.symbols, file_path);
        data_flow_edges.extend(self.extract_env_accesses(content, &ast_ctx.symbols, file_path));
        data_flow_edges.extend(crate::dataflow_common::extract_param_return_flow(
            &all_call_edges,
            file_path,
        ));

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
        let is_test = crate::parse_common::is_test_file(file_path, language);

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
mod tests;
