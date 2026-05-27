//! Python parser using tree-sitter.

use crate::chunker::Chunker;
use crate::http_call_helpers::*;
use crate::traits::FileParser;
use cc_model::diagnostic::DiagnosticRecord;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, HttpCallEdgeRecord, ImportRecord,
    ResolutionKind, RouteEdgeRecord, SemanticEdgeRecord, SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::route_normalize::normalize_route_path;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static PY_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("python call regex"));
static PY_IDENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").expect("python ident regex"));
static PY_CLASS_PARENTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*class\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]+)\)\s*:")
        .expect("python class parents regex")
});

/// Matches `raise ClassName` or `raise ClassName(...)` or `raise ClassName(...) from ...`.
/// Captures the exception class name (must start with uppercase).
/// Skips bare `raise` (re-raise).
static PY_RAISE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*raise\s+([A-Z][A-Za-z0-9_]*)(?:\s*\(|\s*$|\s+from\s)")
        .expect("python raise regex")
});
/// Matches parameter type annotations: `param: TypeName` or `param: TypeName[...]`
static PY_TYPE_ANNOT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\w+)\s*:\s*([A-Z]\w*(?:\[[\w\s,\[\]]+\])?)")
        .expect("python type annotation regex")
});

/// Matches return type annotations: `-> TypeName` or `-> TypeName[...]`
static PY_RETURN_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"->\s*([A-Z]\w*(?:\[[\w\s,\[\]]+\])?)").expect("python return type regex")
});

/// Matches `os.environ["KEY"]`, `os.environ['KEY']`, `os.environ.get("KEY")`, `os.getenv("KEY")`
static PY_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"os\.environ\[["'](\w+)["']\]|os\.environ\.get\(["'](\w+)["']\)|os\.getenv\(["'](\w+)["']\)"#)
        .expect("python env access regex")
});

static PY_KEYWORDS: &[&str] = &[
    "def", "class", "return", "if", "elif", "else", "for", "while", "with", "as", "try", "except",
    "finally", "import", "from", "pass", "True", "False", "None", "and", "or", "not", "in", "is",
    "self",
];

/// HTTP methods recognized in decorator route detection.
const HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "route",
];

pub struct PythonParser {
    language: tree_sitter::Language,
    chunker: Chunker,
}

impl PythonParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_python::LANGUAGE.into(),
            chunker: Chunker::default(),
        }
    }

    /// Legacy: superseded by `extract_all()` single-pass DFS. Retained for rollback.
    #[allow(dead_code)]
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> (Vec<SymbolRecord>, Vec<ImportRecord>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_definition" | "decorated_definition" => {
                    if let Some(sym) = self.extract_function(&child, source, file_path, None) {
                        symbols.push(sym);
                    }
                }
                "class_definition" => {
                    if let Some(sym) = self.extract_class(&child, source, file_path) {
                        let class_name = sym.name.clone();
                        let class_id = sym.symbol_id.clone();
                        symbols.push(sym);
                        // Extract methods
                        if let Some(body) = child.child_by_field_name("body") {
                            let mut body_cursor = body.walk();
                            for member in body.children(&mut body_cursor) {
                                if member.kind() == "function_definition"
                                    || member.kind() == "decorated_definition"
                                {
                                    if let Some(method) = self.extract_function(
                                        &member,
                                        source,
                                        file_path,
                                        Some(&class_name),
                                    ) {
                                        let mut m = method;
                                        m.parent_symbol_id = Some(class_id.clone());
                                        m.kind = SymbolKind::Method;
                                        symbols.push(m);
                                    }
                                }
                            }
                        }
                    }
                }
                "import_statement" => {
                    let extracted = self.extract_import_statement(&child, source, file_path);
                    imports.extend(extracted);
                }
                "import_from_statement" => {
                    let extracted = self.extract_import_from_statement(&child, source, file_path);
                    imports.extend(extracted);
                }
                _ => {}
            }
        }

        (symbols, imports)
    }

    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        // Handle decorated_definition by unwrapping
        let func_node = if node.kind() == "decorated_definition" {
            node.child_by_field_name("definition")?
        } else {
            *node
        };

        let name_node = func_node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let params = func_node
            .child_by_field_name("parameters")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        // Build full signature including return type annotation
        let return_type_text = func_node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok());
        let signature = match return_type_text {
            Some(rt) => format!("def {}{} -> {}", name, params, rt),
            None => format!("def {}{}", name, params),
        };

        // Extract parameter types and count from the inner parameter text (strip parens)
        let params_inner = params
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(params);
        let (param_types, param_count) = extract_python_param_types(params_inner);

        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };

        let kind = if container.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), Some(params));

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc: None,
            parser_tier: ParserTier::Semantic,
            parser_confidence: 0.85,
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: container.map(String::from),
            param_types,
            return_type: return_type_text.map(String::from),
            param_count: Some(param_count),
            base_types: None,
            implements: None,
        })
    }

    fn extract_class(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        // Include superclass list in signature if present
        let superclasses = node
            .child_by_field_name("superclasses")
            .and_then(|n| n.utf8_text(source).ok());
        let signature = match superclasses {
            Some(sc) => format!("class {}{}", name, sc),
            None => format!("class {}", name),
        };

        let qname = name.to_string();
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, "class", None);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Class,
            container: None,
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc: None,
            parser_tier: ParserTier::Semantic,
            parser_confidence: 0.85,
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        })
    }

    /// Extract semantic edges (inheritance) from class declarations.
    fn extract_semantic_edges(
        &self,
        content: &str,
        file_path: &str,
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();

        for cap in PY_CLASS_PARENTS_RE.captures_iter(content) {
            let class_name = &cap[1];
            let parents_str = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            for parent in parents_str.split(',') {
                let parent = parent.trim();
                // Skip keyword arguments like metaclass=ABCMeta
                if parent.contains('=') {
                    continue;
                }
                // Strip anything that isn't a plain identifier (e.g. Generic[T])
                let parent_name = parent.split('[').next().unwrap_or(parent).trim();
                if parent_name.is_empty() {
                    continue;
                }
                // Skip common no-value parents
                if parent_name == "object" || parent_name == "ABC" {
                    continue;
                }
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
        }

        edges
    }

    /// Extract `import x, import x.y.z, import x as alias` statements.
    fn extract_import_statement(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Vec<ImportRecord> {
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                // `import pkg.mod` → dotted_name
                "dotted_name" => {
                    if let Ok(name) = child.utf8_text(source) {
                        imports.push(ImportRecord {
                            file_path: file_path.to_string(),
                            import_string: name.to_string(),
                            resolved_path: None,
                            imported_name: Some("*".to_string()),
                            alias: Some(name.split('.').next().unwrap_or(name).to_string()),
                            is_namespace: true,
                            is_default: false,
                            is_reexport: false,
                        });
                    }
                }
                // `import pkg as alias` → aliased_import
                "aliased_import" => {
                    let name_node = child.child_by_field_name("name");
                    let alias_node = child.child_by_field_name("alias");
                    let name = name_node.and_then(|n| n.utf8_text(source).ok());
                    let alias = alias_node.and_then(|n| n.utf8_text(source).ok());
                    if let Some(name) = name {
                        imports.push(ImportRecord {
                            file_path: file_path.to_string(),
                            import_string: name.to_string(),
                            resolved_path: None,
                            imported_name: Some("*".to_string()),
                            alias: alias.map(|a| a.to_string()),
                            is_namespace: true,
                            is_default: false,
                            is_reexport: false,
                        });
                    }
                }
                _ => {}
            }
        }
        if imports.is_empty() {
            // Fallback: just use the raw text
            if let Ok(text) = node.utf8_text(source) {
                imports.push(ImportRecord {
                    file_path: file_path.to_string(),
                    import_string: text.to_string(),
                    resolved_path: None,
                    imported_name: None,
                    alias: None,
                    is_namespace: true,
                    is_default: false,
                    is_reexport: false,
                });
            }
        }
        imports
    }

    /// Extract `from package.module import name` / `from . import relative` statements.
    fn extract_import_from_statement(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Vec<ImportRecord> {
        let mut imports = Vec::new();

        // Build the module path: collect relative dots + module_name
        let mut module_path = String::new();
        let mut cursor = node.walk();
        let mut found_from = false;
        let mut found_import = false;
        for child in node.children(&mut cursor) {
            let kind = child.kind();
            if kind == "from" {
                found_from = true;
                continue;
            }
            if kind == "import" {
                found_import = true;
                continue;
            }
            if found_from && !found_import {
                // Between "from" and "import": relative dots or module name
                match kind {
                    "relative_import" => {
                        if let Ok(text) = child.utf8_text(source) {
                            module_path.push_str(text);
                        }
                    }
                    "dotted_name" => {
                        if let Ok(text) = child.utf8_text(source) {
                            module_path.push_str(text);
                        }
                    }
                    // Handles bare dots for relative imports like `from . import x`
                    _ if kind == "." || kind == ".." || kind == "..." => {
                        if let Ok(text) = child.utf8_text(source) {
                            module_path.push_str(text);
                        }
                    }
                    _ => {}
                }
            }
            if found_import {
                // After "import": the imported names
                match kind {
                    "dotted_name" => {
                        if let Ok(name) = child.utf8_text(source) {
                            imports.push(ImportRecord {
                                file_path: file_path.to_string(),
                                import_string: module_path.clone(),
                                resolved_path: None,
                                imported_name: Some(name.to_string()),
                                alias: Some(name.to_string()),
                                is_namespace: false,
                                is_default: false,
                                is_reexport: false,
                            });
                        }
                    }
                    "aliased_import" => {
                        let name_node = child.child_by_field_name("name");
                        let alias_node = child.child_by_field_name("alias");
                        let name = name_node.and_then(|n| n.utf8_text(source).ok());
                        let alias = alias_node.and_then(|n| n.utf8_text(source).ok());
                        if let Some(name) = name {
                            imports.push(ImportRecord {
                                file_path: file_path.to_string(),
                                import_string: module_path.clone(),
                                resolved_path: None,
                                imported_name: Some(name.to_string()),
                                alias: alias
                                    .map(|a| a.to_string())
                                    .or_else(|| Some(name.to_string())),
                                is_namespace: false,
                                is_default: false,
                                is_reexport: false,
                            });
                        }
                    }
                    "wildcard_import" => {
                        imports.push(ImportRecord {
                            file_path: file_path.to_string(),
                            import_string: module_path.clone(),
                            resolved_path: None,
                            imported_name: Some("*".to_string()),
                            alias: None,
                            is_namespace: true,
                            is_default: false,
                            is_reexport: false,
                        });
                    }
                    "import_list" => {
                        // from x import (a, b, c) — iterate children of the import_list
                        let mut list_cursor = child.walk();
                        for list_child in child.children(&mut list_cursor) {
                            match list_child.kind() {
                                "dotted_name" => {
                                    if let Ok(name) = list_child.utf8_text(source) {
                                        imports.push(ImportRecord {
                                            file_path: file_path.to_string(),
                                            import_string: module_path.clone(),
                                            resolved_path: None,
                                            imported_name: Some(name.to_string()),
                                            alias: Some(name.to_string()),
                                            is_namespace: false,
                                            is_default: false,
                                            is_reexport: false,
                                        });
                                    }
                                }
                                "aliased_import" => {
                                    let name_node = list_child.child_by_field_name("name");
                                    let alias_node = list_child.child_by_field_name("alias");
                                    let name = name_node.and_then(|n| n.utf8_text(source).ok());
                                    let alias = alias_node.and_then(|n| n.utf8_text(source).ok());
                                    if let Some(name) = name {
                                        imports.push(ImportRecord {
                                            file_path: file_path.to_string(),
                                            import_string: module_path.clone(),
                                            resolved_path: None,
                                            imported_name: Some(name.to_string()),
                                            alias: alias
                                                .map(|a| a.to_string())
                                                .or_else(|| Some(name.to_string())),
                                            is_namespace: false,
                                            is_default: false,
                                            is_reexport: false,
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if imports.is_empty() {
            // Fallback: raw text
            if let Ok(text) = node.utf8_text(source) {
                imports.push(ImportRecord {
                    file_path: file_path.to_string(),
                    import_string: text.to_string(),
                    resolved_path: None,
                    imported_name: None,
                    alias: None,
                    is_namespace: false,
                    is_default: false,
                    is_reexport: false,
                });
            }
        }
        imports
    }

    /// Extract route edges from decorator patterns on a `decorated_definition` node.
    /// Detects patterns like:
    ///   @app.get("/path"), @router.post("/path"), @app.route("/path")
    ///   @api_view(["GET", "POST"])
    fn extract_route_edges(
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

    /// Extract the first string argument from an argument_list node.
    fn extract_first_string_arg(
        &self,
        args_node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        let mut cursor = args_node.walk();
        for child in args_node.children(&mut cursor) {
            if child.kind() == "string" {
                return self.unquote_string(&child, source);
            }
        }
        None
    }

    /// Extract a list of strings from the first list argument, e.g., ["GET", "POST"].
    fn extract_list_strings(
        &self,
        args_node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<Vec<String>> {
        let mut cursor = args_node.walk();
        for child in args_node.children(&mut cursor) {
            if child.kind() == "list" {
                let mut strings = Vec::new();
                let mut list_cursor = child.walk();
                for item in child.children(&mut list_cursor) {
                    if item.kind() == "string" {
                        if let Some(s) = self.unquote_string(&item, source) {
                            strings.push(s);
                        }
                    }
                }
                if !strings.is_empty() {
                    return Some(strings);
                }
            }
        }
        None
    }

    /// Remove quotes from a string node.
    fn unquote_string(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let text = node.utf8_text(source).ok()?;
        // Handle f"...", r"...", b"..." prefixed strings
        let content = text.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        // Remove surrounding quotes (single, double, triple)
        let unquoted = if content.starts_with("\"\"\"") || content.starts_with("'''") {
            &content[3..content.len().saturating_sub(3)]
        } else if content.starts_with('"') || content.starts_with('\'') {
            &content[1..content.len().saturating_sub(1)]
        } else {
            content
        };
        Some(unquoted.to_string())
    }

    /// Detect web framework from decorator function text.
    fn detect_framework(&self, func_text: &str) -> String {
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

    /// Legacy: superseded by `extract_all()` single-pass DFS. Retained for rollback.
    #[allow(dead_code)]
    fn extract_diagnostics(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<DiagnosticRecord> {
        let mut diagnostics = Vec::new();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == "raise_statement" {
                if let Some(diag) = self.extract_raise_diagnostic(&node, source, file_path) {
                    diagnostics.push(diag);
                }
            }
            // Push children in reverse to maintain order
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        diagnostics
    }

    /// Extract a DiagnosticRecord from a `raise_statement` node.
    /// Pattern: `raise SomeError("message")` →
    ///   raise_statement → call → argument_list → string
    fn extract_raise_diagnostic(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<DiagnosticRecord> {
        // The raise_statement's first named child should be a `call` expression
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "call" {
                // Get the exception class name
                let func_node = child.child_by_field_name("function");
                let exc_name = func_node
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("Exception");

                // Get the first string argument from argument_list
                let args_node = child.child_by_field_name("arguments");
                let message =
                    args_node.and_then(|args| self.extract_first_string_arg(&args, source));

                if let Some(msg) = message {
                    let line = node.start_position().row as u32 + 1;
                    let diag_id = format!(
                        "diag:{}:{}:{}",
                        file_path,
                        line,
                        &StableId::edge_id(
                            "diag",
                            file_path,
                            line,
                            node.start_position().column as u32
                        )
                    );
                    return Some(DiagnosticRecord {
                        diagnostic_id: diag_id,
                        file_path: file_path.to_string(),
                        severity: "error".to_string(),
                        message: format!("{}: {}", exc_name, msg),
                        line,
                        end_line: Some(node.end_position().row as u32 + 1),
                        source: "static-raise".to_string(),
                        code: Some(exc_name.to_string()),
                        confidence: 0.7,
                        symbol_uid: None,
                    });
                }
            }
        }
        None
    }

    /// Detect Django `urlpatterns` assignments and extract route edges from them.
    /// Pattern: `urlpatterns = [path("/api/", view_func), ...]`
    fn extract_django_urlpatterns(
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

    fn extract_refs_and_calls(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let keywords: HashSet<&str> = PY_KEYWORDS.iter().copied().collect();
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
                for cap in PY_CALL_RE.captures_iter(line) {
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

                for m in PY_IDENT_RE.find_iter(line) {
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

    /// Extract throw/raise edges from Python source.
    ///
    /// Supports:
    /// - `raise MyError`
    /// - `raise MyError("message")`
    /// - `raise MyError(...) from other`
    ///
    /// Skips:
    /// - bare `raise` (re-raise)
    /// - `raise x` where x starts with lowercase (variable, not a type)
    fn extract_throw_edges(
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

    /// Legacy: superseded by `extract_all()` single-pass DFS. Retained for rollback.
    #[allow(dead_code)]
    fn collect_route_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<RouteEdgeRecord> {
        let mut routes = Vec::new();
        let sym_by_name: HashMap<&str, &SymbolRecord> =
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

    /// Extract type annotation references from function signatures.
    ///
    /// Matches parameter type annotations (`param: TypeName`) and return type
    /// annotations (`-> TypeName`). Each match is attributed to the enclosing
    /// function/method via `source_symbol_uid`.
    fn extract_type_refs(
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
    ///
    /// Matches `os.environ["KEY"]`, `os.environ['KEY']`, `os.environ.get("KEY")`,
    /// and `os.getenv("KEY")`. Each match is attributed to the enclosing function
    /// if inside one.
    fn extract_env_accesses(
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

    fn extract_param_return_flow(
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
    // Single-pass DFS extraction (replaces extract_symbols + collect_route_edges
    // + extract_diagnostics + extract_http_calls)
    // =========================================================================

    /// Perform a single DFS traversal of the tree, collecting symbols, imports,
    /// route edges, HTTP call edges, diagnostics, and semantic edges in one pass.
    fn extract_all(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        imports: &[ImportRecord],
    ) -> PythonExtractCtx {
        let mut ctx = PythonExtractCtx::new(file_path);

        // Pre-build broker import mapping from already-extracted imports
        for imp in imports {
            if let Some(broker_match) = crate::broker_patterns::match_broker(&imp.import_string) {
                if let Some(ref name) = imp.imported_name {
                    if name != "*" {
                        ctx.broker_imports
                            .insert(name.clone(), broker_match.broker_type.to_string());
                    }
                }
                if let Some(ref alias) = imp.alias {
                    ctx.broker_imports
                        .insert(alias.clone(), broker_match.broker_type.to_string());
                }
            }
        }

        let root = tree.root_node();
        self.visit_node_recursive(&root, source, file_path, &mut ctx, None);
        ctx
    }

    /// Recursive visitor for a single node. Dispatches to handle_* methods based
    /// on node kind, then recurses into children.
    fn visit_node_recursive(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
        container: Option<&str>,
    ) {
        match node.kind() {
            // ── Top-level and nested class definitions ──────────────────
            "class_definition" => {
                self.handle_class_def(node, source, file_path, ctx, container);
                return; // children handled inside handle_class_def
            }
            // ── Function / decorated function at module level or nested ─
            "function_definition" => {
                self.handle_function_def(node, source, file_path, ctx, container);
                // Do NOT return — we still recurse into the body for calls,
                // raise statements, etc.
            }
            "decorated_definition" => {
                self.handle_decorated_def(node, source, file_path, ctx, container);
                return; // children handled inside handle_decorated_def
            }
            // ── Import statements ──────────────────────────────────────
            "import_statement" => {
                let extracted = self.extract_import_statement(node, source, file_path);
                // Update broker imports on the fly
                for imp in &extracted {
                    if let Some(broker_match) =
                        crate::broker_patterns::match_broker(&imp.import_string)
                    {
                        if let Some(ref name) = imp.imported_name {
                            if name != "*" {
                                ctx.broker_imports
                                    .insert(name.clone(), broker_match.broker_type.to_string());
                            }
                        }
                        if let Some(ref alias) = imp.alias {
                            ctx.broker_imports
                                .insert(alias.clone(), broker_match.broker_type.to_string());
                        }
                    }
                }
                ctx.imports.extend(extracted);
            }
            "import_from_statement" => {
                let extracted = self.extract_import_from_statement(node, source, file_path);
                for imp in &extracted {
                    if let Some(broker_match) =
                        crate::broker_patterns::match_broker(&imp.import_string)
                    {
                        if let Some(ref name) = imp.imported_name {
                            if name != "*" {
                                ctx.broker_imports
                                    .insert(name.clone(), broker_match.broker_type.to_string());
                            }
                        }
                        if let Some(ref alias) = imp.alias {
                            ctx.broker_imports
                                .insert(alias.clone(), broker_match.broker_type.to_string());
                        }
                    }
                }
                ctx.imports.extend(extracted);
            }
            // ── Call expressions (HTTP calls + broker calls) ───────────
            "call" => {
                self.handle_call(node, source, file_path, ctx);
            }
            // ── Raise statements (diagnostics) ─────────────────────────
            "raise_statement" => {
                if let Some(diag) = self.extract_raise_diagnostic(node, source, file_path) {
                    ctx.diagnostics.push(diag);
                }
            }
            _ => {}
        }

        // Recurse into children
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            self.visit_node_recursive(&child, source, file_path, ctx, container);
        }
    }

    /// Handle a class definition node: extract the class symbol, then recurse
    /// into the class body with updated container context.
    fn handle_class_def(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
        _parent_container: Option<&str>,
    ) {
        if let Some(sym) = self.extract_class(node, source, file_path) {
            let class_name = sym.name.clone();
            let class_id = sym.symbol_id.clone();
            ctx.symbols.push(sym);

            // Recurse into the class body with class_name as container
            if let Some(body) = node.child_by_field_name("body") {
                let mut body_cursor = body.walk();
                for member in body.children(&mut body_cursor) {
                    match member.kind() {
                        "function_definition" | "decorated_definition" => {
                            // Extract method via the same function helper
                            self.handle_member_function(
                                &member,
                                source,
                                file_path,
                                ctx,
                                &class_name,
                                &class_id,
                            );
                        }
                        // Nested class
                        "class_definition" => {
                            self.handle_class_def(
                                &member,
                                source,
                                file_path,
                                ctx,
                                Some(&class_name),
                            );
                        }
                        _ => {
                            // Still recurse for calls, raise, etc. inside class body
                            self.visit_node_recursive(
                                &member,
                                source,
                                file_path,
                                ctx,
                                Some(&class_name),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Handle a method (function_definition or decorated_definition) inside a class body.
    fn handle_member_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
        class_name: &str,
        class_id: &str,
    ) {
        // Extract route edges if decorated
        if node.kind() == "decorated_definition" {
            let func_node = node.child_by_field_name("definition");
            if let Some(func_node) = func_node {
                let name = func_node
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok());
                if let Some(name) = name {
                    let qname = format!("{}.{}", class_name, name);
                    // First extract the method symbol
                    if let Some(method) =
                        self.extract_function(node, source, file_path, Some(class_name))
                    {
                        let mut m = method;
                        m.parent_symbol_id = Some(class_id.to_string());
                        m.kind = SymbolKind::Method;
                        let sid = m.symbol_id.clone();
                        ctx.symbols.push(m);
                        // Now extract route edges from decorators
                        let routes =
                            self.extract_route_edges(node, source, file_path, &qname, Some(&sid));
                        ctx.route_edges.extend(routes);
                    }
                }
            }
            // Recurse into the entire decorated_definition for calls/raise/etc.
            let mut child_cursor = node.walk();
            for child in node.children(&mut child_cursor) {
                // Skip decorator nodes themselves for recursion
                if child.kind() == "decorator" {
                    continue;
                }
                self.visit_node_recursive(&child, source, file_path, ctx, Some(class_name));
            }
        } else {
            // Plain function_definition inside class
            if let Some(method) = self.extract_function(node, source, file_path, Some(class_name)) {
                let mut m = method;
                m.parent_symbol_id = Some(class_id.to_string());
                m.kind = SymbolKind::Method;
                ctx.symbols.push(m);
            }
            // Recurse into function body for calls, raise, etc.
            let mut child_cursor = node.walk();
            for child in node.children(&mut child_cursor) {
                self.visit_node_recursive(&child, source, file_path, ctx, Some(class_name));
            }
        }
    }

    /// Handle a standalone function definition (not inside a class).
    fn handle_function_def(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
        container: Option<&str>,
    ) {
        if let Some(sym) = self.extract_function(node, source, file_path, container) {
            ctx.symbols.push(sym);
        }
        // Note: children are recursed by the caller (visit_node_recursive)
    }

    /// Handle a decorated_definition at module level or outside a class.
    /// Extracts the function symbol + route edges from decorators, then recurses.
    fn handle_decorated_def(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut PythonExtractCtx,
        container: Option<&str>,
    ) {
        // Extract the function symbol
        if let Some(sym) = self.extract_function(node, source, file_path, container) {
            let func_qname = sym.qname.clone().unwrap_or_else(|| sym.name.clone());
            let sid = sym.symbol_id.clone();
            ctx.symbols.push(sym);

            // Extract route edges from decorators
            let routes = self.extract_route_edges(node, source, file_path, &func_qname, Some(&sid));
            ctx.route_edges.extend(routes);
        }

        // Recurse into all children (decorators, definition body) for calls/raise
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            self.visit_node_recursive(&child, source, file_path, ctx, container);
        }
    }

    /// Handle a call expression: check for HTTP calls, broker calls, and
    /// EventEmitter / Django signal dispatch sites.
    fn handle_call(
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

    // =========================================================================
    // Type assignment extraction
    // =========================================================================

    /// Extract local variable type assignments from the AST.
    ///
    /// Patterns recognized:
    /// - `assignment` where RHS is `call` and callee is capitalized → Constructor
    /// - Type-annotated assignment: `x: Foo = ...` → TypeAnnotation
    fn extract_type_assigns(
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
        let enclosing = find_enclosing_function(symbols, line)
            .and_then(|s| s.symbol_uid.clone());

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
        // In tree-sitter Python, type annotation shows as a sibling `type` field
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
}

/// Accumulator for single-pass DFS extraction results.
struct PythonExtractCtx {
    symbols: Vec<SymbolRecord>,
    imports: Vec<ImportRecord>,
    route_edges: Vec<RouteEdgeRecord>,
    http_call_edges: Vec<HttpCallEdgeRecord>,
    dispatch_sites: Vec<DispatchSiteRecord>,
    diagnostics: Vec<DiagnosticRecord>,

    /// Maps imported local name -> broker_type (e.g. "pika" -> "rabbitmq").
    broker_imports: HashMap<String, String>,
}

impl PythonExtractCtx {
    fn new(_file_path: &str) -> Self {
        Self {
            symbols: Vec::new(),
            imports: Vec::new(),
            route_edges: Vec::new(),
            http_call_edges: Vec::new(),
            dispatch_sites: Vec::new(),
            diagnostics: Vec::new(),
            broker_imports: HashMap::new(),
        }
    }
}

/// Find the innermost enclosing function/method for a given line number.
fn find_enclosing_function(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| s.start_line <= line && s.end_line >= line)
        .min_by_key(|s| s.end_line - s.start_line)
}

/// Split a string by `delim`, but skip delimiters inside `[]`, `()`, or `{}`.
fn split_respecting_brackets(s: &str, delim: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            c if c == delim && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Extract parameter types and count from a Python parameter string (contents inside parens).
/// Returns `(param_types, param_count)`. `self`/`cls`, `*args`, `**kwargs` are skipped for count.
fn extract_python_param_types(params_str: &str) -> (Option<String>, u32) {
    let mut types = Vec::new();
    let mut count = 0u32;

    for param in split_respecting_brackets(params_str, ',') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }
        // Skip self, cls, *args, **kwargs, bare *, /
        if param == "self" || param == "cls" || param == "*" || param == "/" {
            continue;
        }
        if param.starts_with("**") || param.starts_with('*') {
            continue;
        }
        count += 1;
        // Extract type annotation: name: Type or name: Type = default
        if let Some(colon_pos) = find_colon_outside_brackets(param) {
            let type_part = param[colon_pos + 1..].trim();
            // Remove default value: split at '=' but respect brackets
            let type_name = split_respecting_brackets(type_part, '=')[0].trim();
            if !type_name.is_empty() {
                types.push(type_name.to_string());
            }
        }
    }

    let param_types = if types.is_empty() {
        None
    } else {
        Some(types.join(", "))
    };
    (param_types, count)
}

/// Find the position of the first `:` that is not inside brackets.
fn find_colon_outside_brackets(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

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
fn extract_http_calls(
    root: tree_sitter::Node,
    source: &str,
    file_path: &str,
    imports: &[ImportRecord],
) -> Vec<HttpCallEdgeRecord> {
    let mut results = Vec::new();
    let bytes = source.as_bytes();

    // Build import-based broker mapping: local name → broker_type
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
fn try_extract_http_call(
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
        normalized_path: Some(normalize_route_path(&normalize_template_to_path(&raw_url))),
        method: http_method,
        call_kind: "http".to_string(),
        line,
        confidence: 0.80,
        parser_tier: ParserTier::TreeSitter,
        broker_type: None,
    })
}

/// Try to extract an async broker call from a `call` node.
/// Detects patterns like `producer.send(...)`, `channel.basic_publish(...)`,
/// `celery_app.send_task(...)`, `task.delay(...)`.
fn try_extract_broker_call(
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
fn is_py_event_registration(method: &str) -> bool {
    matches!(
        method,
        "on" | "once" | "add_listener" | "connect" | "subscribe" | "add_handler"
    )
}

/// Method names that dispatch/emit an event in Python.
fn is_py_event_dispatch(method: &str) -> bool {
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
fn try_extract_dispatch_site(
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
        // Django `signal.send(sender)` doesn't always have a string key;
        // pyee `emitter.emit('event', ...)` does.
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

/// Extract a URL string from the first positional argument of a call.
fn extract_url_from_args(args_node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    extract_nth_string_arg(args_node, source, 0)
}

/// Extract the string value of the Nth positional (non-keyword) string-like argument.
///
/// Skips keyword arguments and non-string arguments, counting only positional
/// string / concatenated_string / f-string nodes.
fn extract_nth_string_arg(
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

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for PythonParser {
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
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.language)
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
                    "tree-sitter parse failed or timed out".to_string()
                } else {
                    "tree-sitter parse failed".to_string()
                },
            })?;

        // ── Pass 1: Single DFS traversal ───────────────────────────────
        // Collects symbols, imports, route_edges, http_call_edges, diagnostics.
        // We pass an empty slice for imports initially; the DFS itself discovers
        // imports and updates broker_imports on the fly.
        let ast_ctx = self.extract_all(&tree, content.as_bytes(), file_path, &[]);

        let symbols = ast_ctx.symbols;
        let imports = ast_ctx.imports;

        // ── Pass 2: Regex-based ref/call resolution (needs full symbol table) ──
        let (symbol_refs, call_edges) = self.extract_refs_and_calls(content, file_path, &symbols);

        // Django urlpatterns are root-level assignments — extract separately since
        // they depend on a pattern (assignment inspection) that doesn't naturally
        // fit the node-kind dispatch in visit_node_recursive.
        let mut route_edges = ast_ctx.route_edges;
        route_edges.extend(self.extract_django_urlpatterns(&tree, content.as_bytes(), file_path));

        let http_call_edges = ast_ctx.http_call_edges;
        let diagnostics = ast_ctx.diagnostics;

        // ── Regex-based semantic edges (inheritance + throws) ──────────
        let mut semantic_edges =
            self.extract_semantic_edges(content, file_path, ParserTier::Semantic);
        semantic_edges.extend(self.extract_throw_edges(
            content,
            file_path,
            &symbols,
            ParserTier::Heuristic,
        ));

        // ── Regex-based data flow edges (type refs + env accesses + param/return) ──
        let mut data_flow_edges = self.extract_type_refs(content, &symbols, file_path);
        data_flow_edges.extend(self.extract_env_accesses(content, &symbols, file_path));
        data_flow_edges.extend(self.extract_param_return_flow(&call_edges, file_path));

        let type_assigns =
            self.extract_type_assigns(&tree, content.as_bytes(), file_path, &symbols);

        let tier = ParserTier::Semantic;
        let confidence = 0.85;
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);

        let summary = format!(
            "{} (python, {} lines, {} symbols, {} routes)",
            file_path,
            content.lines().count(),
            symbols.len(),
            route_edges.len(),
        );
        let is_test = file_path.contains("/tests/")
            || file_path.contains("test_")
            || file_path.contains("_test.py")
            || file_path.ends_with("tests.py");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            route_edges,
            http_call_edges,
            semantic_edges,
            data_flow_edges,
            dispatch_sites: ast_ctx.dispatch_sites,
            diagnostics,
            type_assigns,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::Python]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Semantic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_python() {
        let p = PythonParser::new();
        let code = r#"
import os

def hello(name: str) -> str:
    return f"Hello {name}"

class Greeter:
    def greet(self, name):
        return hello(name)
"#;
        let outcome = p.parse("example.py", code, Language::Python).unwrap();
        assert!(outcome.symbols.len() >= 3); // hello, Greeter, greet
        assert!(!outcome.chunks.is_empty());
        assert_eq!(outcome.parser_tier, ParserTier::Semantic);

        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Greeter"));
        assert!(names.contains(&"greet"));

        // greet should be a Method with parent
        let greet = outcome.symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(greet.kind, SymbolKind::Method);
        assert!(greet.parent_symbol_id.is_some());
    }

    #[test]
    fn symbol_uids_are_stable() {
        let p = PythonParser::new();
        let code = "def foo():\n    pass\n";
        let a = p.parse("f.py", code, Language::Python).unwrap();
        let b = p.parse("f.py", code, Language::Python).unwrap();
        assert_eq!(a.symbols[0].symbol_uid, b.symbols[0].symbol_uid);
    }

    #[test]
    fn extract_fastapi_routes() {
        let p = PythonParser::new();
        let code = r#"
from fastapi import FastAPI

app = FastAPI()

@app.get("/users")
def list_users():
    return []

@app.post("/users")
async def create_user(user: dict):
    return user
"#;
        let outcome = p.parse("api.py", code, Language::Python).unwrap();
        assert!(
            outcome.route_edges.len() >= 2,
            "expected at least 2 routes, got {}",
            outcome.route_edges.len()
        );
        let paths: Vec<&str> = outcome
            .route_edges
            .iter()
            .map(|r| r.route_path.as_str())
            .collect();
        assert!(paths.contains(&"/users"));
        let methods: Vec<Option<&str>> = outcome
            .route_edges
            .iter()
            .map(|r| r.method.as_deref())
            .collect();
        assert!(methods.contains(&Some("GET")));
        assert!(methods.contains(&Some("POST")));
    }

    #[test]
    fn extract_flask_route() {
        let p = PythonParser::new();
        let code = r#"
from flask import Flask
app = Flask(__name__)

@app.route("/hello")
def hello():
    return "Hello"
"#;
        let outcome = p.parse("app.py", code, Language::Python).unwrap();
        assert!(
            !outcome.route_edges.is_empty(),
            "expected route edges for Flask @app.route"
        );
        assert_eq!(outcome.route_edges[0].route_path, "/hello");
        assert_eq!(outcome.route_edges[0].framework.as_deref(), Some("flask"));
    }

    #[test]
    fn extract_raise_diagnostics() {
        let p = PythonParser::new();
        let code = r#"
def validate(x):
    if x < 0:
        raise ValueError("x must be non-negative")
    if x > 100:
        raise RuntimeError("x too large")
"#;
        let outcome = p.parse("validate.py", code, Language::Python).unwrap();
        assert!(
            outcome.diagnostics.len() >= 2,
            "expected at least 2 diagnostics, got {}",
            outcome.diagnostics.len()
        );
        let messages: Vec<&str> = outcome
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(messages
            .iter()
            .any(|m| m.contains("x must be non-negative")));
        assert!(messages.iter().any(|m| m.contains("x too large")));
    }

    #[test]
    fn extract_imports_detailed() {
        let p = PythonParser::new();
        let code = r#"
import os
import sys as system
from pathlib import Path
from collections import OrderedDict as OD
from . import utils
from ..core import Base
from typing import List, Dict
"#;
        let outcome = p.parse("mod.py", code, Language::Python).unwrap();
        // Should have multiple import records
        assert!(
            outcome.imports.len() >= 5,
            "expected at least 5 imports, got {}",
            outcome.imports.len()
        );

        // Check `import os` → namespace import
        let os_imp = outcome.imports.iter().find(|i| i.import_string == "os");
        assert!(os_imp.is_some(), "expected import for 'os'");
        assert!(os_imp.unwrap().is_namespace);

        // Check `import sys as system` → alias
        let sys_imp = outcome.imports.iter().find(|i| i.import_string == "sys");
        assert!(sys_imp.is_some(), "expected import for 'sys'");
        assert_eq!(sys_imp.unwrap().alias.as_deref(), Some("system"));

        // Check `from pathlib import Path`
        let path_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string == "pathlib" && i.imported_name.as_deref() == Some("Path"));
        assert!(
            path_imp.is_some(),
            "expected import for 'Path' from 'pathlib'"
        );
    }

    #[test]
    fn extract_django_urlpatterns() {
        let p = PythonParser::new();
        let code = r#"
from django.urls import path
from . import views

urlpatterns = [
    path("api/users/", views.user_list),
    path("api/items/", views.item_list),
]
"#;
        let outcome = p.parse("urls.py", code, Language::Python).unwrap();
        assert!(
            outcome.route_edges.len() >= 2,
            "expected at least 2 Django routes, got {}",
            outcome.route_edges.len()
        );
        let paths: Vec<&str> = outcome
            .route_edges
            .iter()
            .map(|r| r.route_path.as_str())
            .collect();
        assert!(paths.contains(&"api/users/"));
        assert!(paths.contains(&"api/items/"));
        assert_eq!(outcome.route_edges[0].framework.as_deref(), Some("django"));
    }

    #[test]
    fn extract_http_call_requests() {
        let code = r#"
import requests

def get_users():
    response = requests.get("/api/users")
    return response.json()
"#;
        let p = PythonParser::new();
        let outcome = p.parse("client.py", code, Language::Python).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from requests.get"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.url_or_path, "/api/users");
        assert_eq!(hce.method, Some("GET".to_string()));
    }

    #[test]
    fn extract_http_call_httpx_post() {
        let code = r#"
import httpx

def create_order(data):
    response = httpx.post("/api/orders", json=data)
    return response.json()
"#;
        let p = PythonParser::new();
        let outcome = p.parse("orders.py", code, Language::Python).unwrap();

        assert!(
            !outcome.http_call_edges.is_empty(),
            "should extract HTTP call from httpx.post"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.method, Some("POST".to_string()));
    }

    #[test]
    fn extract_http_call_requests_request() {
        let code = r#"
import requests

def update_item(item_id, data):
    response = requests.request("PUT", f"/api/items/{item_id}", json=data)
    return response.json()
"#;
        let p = PythonParser::new();
        let outcome = p.parse("client.py", code, Language::Python).unwrap();
        assert!(
            !outcome.http_call_edges.is_empty(),
            "should detect requests.request()"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.method, Some("PUT".to_string()));
        // URL should be the second arg, not the first
        assert!(
            hce.url_or_path.contains("/api/items"),
            "url_or_path should contain /api/items, got: {}",
            hce.url_or_path
        );
    }

    #[test]
    fn extract_http_call_httpx_request() {
        let code = r#"
import httpx

resp = httpx.request("POST", "/api/submit", json={"key": "val"})
"#;
        let p = PythonParser::new();
        let outcome = p.parse("sess.py", code, Language::Python).unwrap();
        assert!(
            !outcome.http_call_edges.is_empty(),
            "should detect httpx.request()"
        );
        let hce = &outcome.http_call_edges[0];
        assert_eq!(hce.method, Some("POST".to_string()));
        assert!(hce.url_or_path.contains("/api/submit"));
    }

    #[test]
    fn no_false_positive_dict_get() {
        // dict.get() should NOT trigger HTTP call detection
        let code = r#"
data = {"key": "value"}
result = data.get("/api/users")
"#;
        let p = PythonParser::new();
        let outcome = p.parse("test.py", code, Language::Python).unwrap();
        assert!(
            outcome.http_call_edges.is_empty(),
            "dict.get should not be HTTP call"
        );
    }

    #[test]
    fn function_signature_includes_return_type() {
        let p = PythonParser::new();
        let code = "def greet(name: str) -> str:\n    return f'Hello {name}'\n";
        let outcome = p.parse("sig.py", code, Language::Python).unwrap();
        let sig = outcome.symbols[0].signature.as_deref().unwrap();
        assert!(
            sig.contains("-> str"),
            "signature should include return type, got: {}",
            sig
        );
    }

    #[test]
    fn extract_semantic_edges_inherits() {
        let p = PythonParser::new();
        let code = r#"
class Animal:
    pass

class Dog(Animal):
    pass

class GuideDog(Dog, Serializable):
    pass

class Plain:
    pass

class EmptyParens():
    pass

class WithMeta(Base, metaclass=ABCMeta):
    pass

class SkipObject(object):
    pass

class SkipABC(ABC):
    pass
"#;
        let outcome = p.parse("inherit.py", code, Language::Python).unwrap();
        let edges = &outcome.semantic_edges;

        // Dog -> Animal
        let dog_edges: Vec<_> = edges.iter().filter(|e| e.source_symbol == "Dog").collect();
        assert_eq!(dog_edges.len(), 1);
        assert_eq!(dog_edges[0].target_symbol, "Animal");
        assert_eq!(dog_edges[0].relation_kind, SemanticRelation::Inherits);
        assert!((dog_edges[0].confidence - 0.95).abs() < 0.01);

        // GuideDog -> Dog, GuideDog -> Serializable
        let guide_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.source_symbol == "GuideDog")
            .collect();
        assert_eq!(guide_edges.len(), 2);
        let targets: Vec<&str> = guide_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"Dog"));
        assert!(targets.contains(&"Serializable"));

        // Plain, EmptyParens -> no edges
        assert!(edges.iter().all(|e| e.source_symbol != "Plain"));
        assert!(edges.iter().all(|e| e.source_symbol != "EmptyParens"));

        // WithMeta -> Base only (metaclass=ABCMeta skipped)
        let meta_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.source_symbol == "WithMeta")
            .collect();
        assert_eq!(meta_edges.len(), 1);
        assert_eq!(meta_edges[0].target_symbol, "Base");

        // SkipObject, SkipABC -> no edges (object/ABC filtered)
        assert!(edges.iter().all(|e| e.source_symbol != "SkipObject"));
        assert!(edges.iter().all(|e| e.source_symbol != "SkipABC"));
    }

    #[test]
    fn extract_param_types_typed_function() {
        let p = PythonParser::new();
        let code = r#"
def process(x: int, y: str, z: float) -> bool:
    pass
"#;
        let outcome = p.parse("typed.py", code, Language::Python).unwrap();
        let sym = outcome
            .symbols
            .iter()
            .find(|s| s.name == "process")
            .unwrap();
        assert_eq!(sym.param_types.as_deref(), Some("int, str, float"));
        assert_eq!(sym.return_type.as_deref(), Some("bool"));
        assert_eq!(sym.param_count, Some(3));
        assert!(sym.receiver_type.is_none());
    }

    #[test]
    fn extract_param_types_no_annotations() {
        let p = PythonParser::new();
        let code = r#"
def simple(a, b, c):
    pass
"#;
        let outcome = p.parse("notype.py", code, Language::Python).unwrap();
        let sym = outcome.symbols.iter().find(|s| s.name == "simple").unwrap();
        assert!(sym.param_types.is_none());
        assert!(sym.return_type.is_none());
        assert_eq!(sym.param_count, Some(3));
    }

    #[test]
    fn extract_param_types_method_with_self() {
        let p = PythonParser::new();
        let code = r#"
class Dog:
    def bark(self, volume: int) -> str:
        return "Woof"
"#;
        let outcome = p.parse("dog.py", code, Language::Python).unwrap();
        let bark = outcome.symbols.iter().find(|s| s.name == "bark").unwrap();
        assert_eq!(bark.kind, SymbolKind::Method);
        assert_eq!(bark.receiver_type.as_deref(), Some("Dog"));
        assert_eq!(bark.param_types.as_deref(), Some("int"));
        assert_eq!(bark.return_type.as_deref(), Some("str"));
        // self should not be counted
        assert_eq!(bark.param_count, Some(1));
    }

    #[test]
    fn extract_param_types_with_defaults() {
        let p = PythonParser::new();
        let code = r#"
def greet(name: str = "World", count: int = 1) -> None:
    pass
"#;
        let outcome = p.parse("defaults.py", code, Language::Python).unwrap();
        let sym = outcome.symbols.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(sym.param_types.as_deref(), Some("str, int"));
        assert_eq!(sym.return_type.as_deref(), Some("None"));
        assert_eq!(sym.param_count, Some(2));
    }

    #[test]
    fn extract_param_types_generic_annotations() {
        let p = PythonParser::new();
        let code = r#"
def transform(items: List[int], mapping: Dict[str, Any]) -> Optional[str]:
    pass
"#;
        let outcome = p.parse("generic.py", code, Language::Python).unwrap();
        let sym = outcome
            .symbols
            .iter()
            .find(|s| s.name == "transform")
            .unwrap();
        assert_eq!(
            sym.param_types.as_deref(),
            Some("List[int], Dict[str, Any]")
        );
        assert_eq!(sym.return_type.as_deref(), Some("Optional[str]"));
        assert_eq!(sym.param_count, Some(2));
    }

    #[test]
    fn extract_param_types_skip_args_kwargs() {
        let p = PythonParser::new();
        let code = r#"
def variadic(a: int, *args, **kwargs) -> None:
    pass
"#;
        let outcome = p.parse("variadic.py", code, Language::Python).unwrap();
        let sym = outcome
            .symbols
            .iter()
            .find(|s| s.name == "variadic")
            .unwrap();
        assert_eq!(sym.param_types.as_deref(), Some("int"));
        // *args and **kwargs should not be counted
        assert_eq!(sym.param_count, Some(1));
    }

    #[test]
    fn extract_param_types_classmethod_cls() {
        let p = PythonParser::new();
        let code = r#"
class Factory:
    @classmethod
    def create(cls, name: str) -> "Factory":
        pass
"#;
        let outcome = p.parse("factory.py", code, Language::Python).unwrap();
        let sym = outcome.symbols.iter().find(|s| s.name == "create").unwrap();
        assert_eq!(sym.receiver_type.as_deref(), Some("Factory"));
        assert_eq!(sym.param_types.as_deref(), Some("str"));
        // cls should not be counted
        assert_eq!(sym.param_count, Some(1));
    }

    #[test]
    fn extract_param_types_no_params() {
        let p = PythonParser::new();
        let code = r#"
def noop() -> None:
    pass
"#;
        let outcome = p.parse("noop.py", code, Language::Python).unwrap();
        let sym = outcome.symbols.iter().find(|s| s.name == "noop").unwrap();
        assert!(sym.param_types.is_none());
        assert_eq!(sym.return_type.as_deref(), Some("None"));
        assert_eq!(sym.param_count, Some(0));
    }

    #[test]
    fn split_respecting_brackets_basic() {
        let parts = split_respecting_brackets("a: int, b: Dict[str, int], c: str", ',');
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].trim(), "a: int");
        assert_eq!(parts[1].trim(), "b: Dict[str, int]");
        assert_eq!(parts[2].trim(), "c: str");
    }

    #[test]
    fn extract_param_types_tuple_return() {
        let p = PythonParser::new();
        let code = r#"
def divide(a: int, b: int) -> Tuple[int, int]:
    return a // b, a % b
"#;
        let outcome = p.parse("tuple_ret.py", code, Language::Python).unwrap();
        let sym = outcome.symbols.iter().find(|s| s.name == "divide").unwrap();
        assert_eq!(sym.param_types.as_deref(), Some("int, int"));
        assert_eq!(sym.return_type.as_deref(), Some("Tuple[int, int]"));
        assert_eq!(sym.param_count, Some(2));
    }

    // ── Async broker extraction tests ─────────────────────────────

    #[test]
    fn extract_celery_broker_call() {
        // `import celery` maps alias "celery" → broker_type "celery" in broker_imports.
        // `celery.send_task(...)` — method "send_task" is in ASYNC_METHODS,
        // and broker_imports.get("celery") returns Some("celery").
        //
        // NOTE: The more idiomatic `send_notification.delay(123)` is NOT detected
        // because `send_notification` doesn't match any broker pattern — the parser
        // only resolves obj.method() where obj matches either OBJECT_PATTERNS or
        // broker_imports.
        // TODO: python broker detection limited to obj.method() patterns where obj
        // is a known broker import or has a broker-like name.
        let code = r#"
import celery

celery.send_task('tasks.send_notification', args=[123])
"#;
        let p = PythonParser::new();
        let outcome = p.parse("tasks.py", code, Language::Python).unwrap();

        let broker_edges: Vec<_> = outcome
            .http_call_edges
            .iter()
            .filter(|e| e.call_kind == "async")
            .collect();
        assert!(
            !broker_edges.is_empty(),
            "should detect celery.send_task() as async broker call"
        );
        assert_eq!(broker_edges[0].broker_type.as_deref(), Some("celery"));
    }

    #[test]
    fn extract_rabbitmq_broker_call() {
        // `import pika` maps alias "pika" → broker_type "rabbitmq" in broker_imports.
        // `pika.basic_publish(...)` — method "basic_publish" is in ASYNC_METHODS,
        // and broker_imports.get("pika") returns Some("rabbitmq").
        //
        // NOTE: The more idiomatic `channel.basic_publish(...)` is NOT detected
        // because "channel" is blocked in match_broker_object() as a generic name,
        // and it's not in broker_imports (only "pika" is).
        let code = r#"
import pika

pika.basic_publish(exchange='', routing_key='task_queue', body='Hello')
"#;
        let p = PythonParser::new();
        let outcome = p.parse("publisher.py", code, Language::Python).unwrap();

        let broker_edges: Vec<_> = outcome
            .http_call_edges
            .iter()
            .filter(|e| e.call_kind == "async")
            .collect();
        assert!(
            !broker_edges.is_empty(),
            "should detect pika.basic_publish() as async broker call"
        );
        assert_eq!(broker_edges[0].broker_type.as_deref(), Some("rabbitmq"));
    }

    #[test]
    fn extract_throw_edges_python() {
        let p = PythonParser::new();
        let code = r#"
def foo():
    raise ValueError("bad value")

def bar():
    raise CustomError("msg") from original

def baz():
    try:
        pass
    except:
        raise

class Validator:
    def validate(self):
        raise ValidationError("invalid")

def multi():
    raise TypeError("type")
    raise KeyError("key")
"#;
        let outcome = p.parse("throws.py", code, Language::Python).unwrap();
        let throw_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Throws)
            .collect();

        // foo -> ValueError
        let foo_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "foo")
            .collect();
        assert_eq!(foo_edges.len(), 1);
        assert_eq!(foo_edges[0].target_symbol, "ValueError");
        assert!((foo_edges[0].confidence - 0.9).abs() < 0.01);

        // bar -> CustomError
        let bar_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "bar")
            .collect();
        assert_eq!(bar_edges.len(), 1);
        assert_eq!(bar_edges[0].target_symbol, "CustomError");

        // baz -> no edges (bare raise is skipped)
        let baz_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "baz")
            .collect();
        assert_eq!(baz_edges.len(), 0, "bare raise should be skipped");

        // Validator.validate -> ValidationError
        let validate_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Validator.validate")
            .collect();
        assert_eq!(validate_edges.len(), 1);
        assert_eq!(validate_edges[0].target_symbol, "ValidationError");

        // multi -> TypeError + KeyError (2 edges)
        let multi_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "multi")
            .collect();
        assert_eq!(multi_edges.len(), 2);
        let targets: Vec<&str> = multi_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"TypeError"));
        assert!(targets.contains(&"KeyError"));
    }

    #[test]
    fn test_pyee_event_emitter_dispatch_sites() {
        let code = r#"
from pyee import EventEmitter

ee = EventEmitter()
ee.on('user:created', handle_user)
ee.emit('user:created', data)
"#;
        let p = PythonParser::new();
        let outcome = p.parse("events.py", code, Language::Python).unwrap();
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
        assert_eq!(on_site.handler_expr.as_deref(), Some("handle_user"));

        let emit_site = outcome
            .dispatch_sites
            .iter()
            .find(|s| s.site_kind == DispatchSiteKind::EventEmit)
            .expect("should have an EventEmit dispatch site");
        assert_eq!(emit_site.key, "user:created");
    }

    #[test]
    fn test_django_signal_dispatch_sites() {
        let code = r#"
from django.dispatch import Signal

user_saved = Signal()
user_saved.connect('post_save', handle_save)
user_saved.send('post_save')
"#;
        let p = PythonParser::new();
        let outcome = p.parse("signals.py", code, Language::Python).unwrap();
        assert_eq!(
            outcome.dispatch_sites.len(),
            2,
            "should detect connect (EventOn) + send (EventEmit)"
        );
        assert!(outcome
            .dispatch_sites
            .iter()
            .any(|s| s.site_kind == DispatchSiteKind::EventOn && s.key == "post_save"));
        assert!(outcome
            .dispatch_sites
            .iter()
            .any(|s| s.site_kind == DispatchSiteKind::EventEmit && s.key == "post_save"));
    }
}
