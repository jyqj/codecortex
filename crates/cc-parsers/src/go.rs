//! Go parser using tree-sitter AST extraction (TreeSitter tier).

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, ImportRecord, ResolutionKind,
    RouteEdgeRecord, SemanticEdgeRecord, SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::{CcResult, ElementKind, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static GO_KEYWORDS: &[&str] = &[
    "func",
    "var",
    "const",
    "type",
    "struct",
    "interface",
    "map",
    "chan",
    "go",
    "select",
    "case",
    "switch",
    "if",
    "else",
    "for",
    "range",
    "return",
    "break",
    "continue",
    "defer",
    "import",
    "package",
    "fallthrough",
    "goto",
    "default",
    "make",
    "new",
    "len",
    "cap",
    "append",
    "copy",
    "delete",
    "close",
    "panic",
    "recover",
    "print",
    "println",
    "nil",
    "true",
    "false",
    "iota",
    "string",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "float32",
    "float64",
    "bool",
    "byte",
    "rune",
    "error",
];

/// Matches `os.Getenv("KEY")` and `os.LookupEnv("KEY")`.
static GO_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"os\.Getenv\("(\w+)"\)|os\.LookupEnv\("(\w+)"\)"#).expect("go env access regex")
});

/// Outbound HTTP via net/http convenience funcs: `http.Get/Head/Post("url")`.
static GO_HTTP_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\bhttp\.(Get|Head|Post)\s*\(\s*"([^"]+)""#).expect("go http func regex")
});

/// Outbound HTTP via `http.NewRequest[WithContext](..., "METHOD", "url", ...)`.
static GO_HTTP_NEWREQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"\bhttp\.NewRequest(?:WithContext)?\s*\([^"]*"(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)"\s*,\s*"([^"]+)""#,
    )
    .expect("go http newrequest regex")
});

pub struct GoParser {
    language: tree_sitter::Language,
    chunker: Chunker,
}

impl GoParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_go::LANGUAGE.into(),
            chunker: Chunker::default(),
        }
    }

    // =========================================================================
    // Symbol extraction
    // =========================================================================

    /// Extract all top-level declarations from the tree-sitter AST.
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<SymbolRecord> {
        let mut symbols = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            match child.kind() {
                "function_declaration" => {
                    if let Some(sym) = self.extract_function(&child, source, file_path) {
                        symbols.push(sym);
                    }
                }
                "method_declaration" => {
                    if let Some(sym) = self.extract_method(&child, source, file_path) {
                        symbols.push(sym);
                    }
                }
                "type_declaration" => {
                    self.extract_type_declaration(&child, source, file_path, &mut symbols);
                }
                _ => {}
            }
        }

        symbols
    }

    /// Extract a `function_declaration` node into a SymbolRecord.
    fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        let params_node = node.child_by_field_name("parameters");
        let params_text = params_node
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        let (param_types, param_count) = self.extract_param_types(&params_node, source);
        let return_type = self.extract_result_type(node, source);

        let signature = match &return_type {
            Some(rt) => format!("func {}{} {}", name, params_text, rt),
            None => format!("func {}{}", name, params_text),
        };

        let qname = name.to_string();
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, "function", Some(&signature));

        let end_line = node
            .child_by_field_name("body")
            .map(|b| b.end_position().row as u32 + 1)
            .unwrap_or(node.end_position().row as u32 + 1);

        let doc = self.extract_doc_comment(node, source);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line: node.start_position().row as u32 + 1,
            end_line,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::Symbol),
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: None,
            param_types,
            return_type,
            param_count: Some(param_count),
            base_types: None,
            implements: None,
        })
    }

    /// Extract a `method_declaration` node into a SymbolRecord.
    fn extract_method(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        // Extract receiver type
        let receiver_node = node.child_by_field_name("receiver")?;
        let receiver_type = self.extract_receiver_type(&receiver_node, source)?;

        let params_node = node.child_by_field_name("parameters");
        let params_text = params_node
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        let (param_types, param_count) = self.extract_param_types(&params_node, source);
        let return_type = self.extract_result_type(node, source);

        let signature = match &return_type {
            Some(rt) => format!("func ({}) {}{} {}", receiver_type, name, params_text, rt),
            None => format!("func ({}) {}{}", receiver_type, name, params_text),
        };

        let qname = format!("{}.{}", receiver_type, name);
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, "method", Some(&signature));

        let end_line = node
            .child_by_field_name("body")
            .map(|b| b.end_position().row as u32 + 1)
            .unwrap_or(node.end_position().row as u32 + 1);

        let doc = self.extract_doc_comment(node, source);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Method,
            container: Some(receiver_type.clone()),
            start_line: node.start_position().row as u32 + 1,
            end_line,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::Symbol),
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: Some(receiver_type),
            param_types,
            return_type,
            param_count: Some(param_count),
            base_types: None,
            implements: None,
        })
    }

    /// Extract receiver type name from a method receiver parameter_list.
    /// Handles both `(r RecvType)` and `(r *RecvType)`.
    fn extract_receiver_type(
        &self,
        receiver_node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        // receiver is a parameter_list containing one parameter_declaration
        let mut cursor = receiver_node.walk();
        for child in receiver_node.children(&mut cursor) {
            if child.kind() == "parameter_declaration" {
                let type_node = child.child_by_field_name("type")?;
                let type_text = type_node.utf8_text(source).ok()?;
                // Strip pointer prefix `*`
                let cleaned = type_text.trim_start_matches('*');
                return Some(cleaned.to_string());
            }
        }
        None
    }

    /// Extract `type_declaration` children into SymbolRecords.
    fn extract_type_declaration(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &mut Vec<SymbolRecord>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_spec" => {
                    if let Some(sym) = self.extract_type_spec(&child, source, file_path) {
                        symbols.push(sym);
                    }
                }
                "type_alias" => {
                    if let Some(sym) = self.extract_type_alias(&child, source, file_path) {
                        symbols.push(sym);
                    }
                }
                _ => {}
            }
        }
    }

    /// Extract a `type_spec` (e.g., `type Foo struct { ... }` or `type Bar interface { ... }`).
    fn extract_type_spec(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let type_node = node.child_by_field_name("type")?;

        let kind = match type_node.kind() {
            "struct_type" => SymbolKind::Class,
            "interface_type" => SymbolKind::Interface,
            _ => SymbolKind::TypeAlias,
        };

        let kind_str = match kind {
            SymbolKind::Class => "struct",
            SymbolKind::Interface => "interface",
            _ => "type",
        };

        let signature = format!("type {} {}", name, kind_str);
        let qname = name.to_string();
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), None);

        let doc = self.extract_doc_comment(node, source);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: None,
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::Symbol),
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

    /// Extract a `type_alias` (e.g., `type ID = string`).
    fn extract_type_alias(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let type_node = node.child_by_field_name("type");
        let type_text = type_node.and_then(|n| n.utf8_text(source).ok());

        let signature = match type_text {
            Some(tt) => format!("type {} = {}", name, tt),
            None => format!("type {}", name),
        };

        let qname = name.to_string();
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, "type_alias", None);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::TypeAlias,
            container: None,
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::Symbol),
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

    // =========================================================================
    // Import extraction
    // =========================================================================

    /// Extract imports from `import_declaration` nodes.
    fn extract_imports(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<ImportRecord> {
        let mut imports = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            if child.kind() == "import_declaration" {
                self.extract_import_declaration(&child, source, file_path, &mut imports);
            }
        }

        imports
    }

    /// Extract import specs from a single `import_declaration`.
    fn extract_import_declaration(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        imports: &mut Vec<ImportRecord>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_spec" => {
                    if let Some(imp) = self.extract_import_spec(&child, source, file_path) {
                        imports.push(imp);
                    }
                }
                "import_spec_list" => {
                    let mut list_cursor = child.walk();
                    for spec in child.children(&mut list_cursor) {
                        if spec.kind() == "import_spec" {
                            if let Some(imp) = self.extract_import_spec(&spec, source, file_path) {
                                imports.push(imp);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Extract a single `import_spec` into an ImportRecord.
    fn extract_import_spec(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<ImportRecord> {
        let path_node = node.child_by_field_name("path")?;
        let path_text = path_node.utf8_text(source).ok()?;
        // Remove quotes from path
        let import_path = path_text
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('`')
            .trim_end_matches('`');

        let alias_node = node.child_by_field_name("name");
        let alias = alias_node
            .and_then(|n| n.utf8_text(source).ok())
            .map(String::from);

        // Last segment of path is the imported package name
        let imported_name = import_path.rsplit('/').next().map(String::from);

        Some(ImportRecord {
            file_path: file_path.to_string(),
            import_string: import_path.to_string(),
            resolved_path: None,
            imported_name,
            alias,
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        })
    }

    // =========================================================================
    // Call extraction (AST-based)
    // =========================================================================

    /// Walk function/method bodies for `call_expression` nodes to extract calls.
    fn extract_calls(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let keywords: HashSet<&str> = GO_KEYWORDS.iter().copied().collect();
        let mut refs = Vec::new();
        let mut calls = Vec::new();

        // Build name -> (symbol_id, symbol_uid) map for resolution
        let mut by_name: HashMap<String, (&str, &str)> = HashMap::new();
        for sym in symbols {
            if let Some(uid) = &sym.symbol_uid {
                by_name
                    .entry(sym.name.clone())
                    .or_insert((sym.symbol_id.as_str(), uid.as_str()));
            }
        }

        let root = tree.root_node();
        let mut root_cursor = root.walk();

        for child in root.children(&mut root_cursor) {
            match child.kind() {
                "function_declaration" | "method_declaration" => {
                    // Find the corresponding symbol
                    let caller_sym = self.find_symbol_for_decl(&child, source, symbols);
                    if let Some(body) = child.child_by_field_name("body") {
                        self.walk_calls(
                            &body,
                            source,
                            file_path,
                            &keywords,
                            &by_name,
                            &caller_sym,
                            &mut refs,
                            &mut calls,
                        );
                    }
                }
                _ => {}
            }
        }

        (refs, calls)
    }

    /// Find the SymbolRecord matching a function/method declaration node.
    fn find_symbol_for_decl<'a>(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        symbols: &'a [SymbolRecord],
    ) -> Option<&'a SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let line = node.start_position().row as u32 + 1;
        symbols
            .iter()
            .find(|s| s.name == name && s.start_line == line)
    }

    /// Recursively walk AST nodes to find `call_expression` nodes.
    #[allow(clippy::too_many_arguments)]
    fn walk_calls(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        caller_sym: &Option<&SymbolRecord>,
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        if node.kind() == "call_expression" {
            self.extract_single_call(
                node, source, file_path, keywords, by_name, caller_sym, refs, calls,
            );
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_calls(
                &child, source, file_path, keywords, by_name, caller_sym, refs, calls,
            );
        }
    }

    /// Extract a single call from a `call_expression` node.
    #[allow(clippy::too_many_arguments)]
    fn extract_single_call(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        caller_sym: &Option<&SymbolRecord>,
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        let func_node = match node.child_by_field_name("function") {
            Some(n) => n,
            None => return,
        };

        // Count arguments
        let arg_count = node.child_by_field_name("arguments").map(|args| {
            let mut cnt = 0u32;
            let mut arg_cursor = args.walk();
            for arg_child in args.named_children(&mut arg_cursor) {
                // Skip non-expression children like commas
                if arg_child.is_named() {
                    cnt += 1;
                }
            }
            cnt
        });

        let (callee, dispatch_kind, receiver_expr) = match func_node.kind() {
            "selector_expression" => {
                // obj.Method(...)
                let operand = func_node.child_by_field_name("operand");
                let field = func_node.child_by_field_name("field");
                let operand_text = operand.and_then(|n| n.utf8_text(source).ok());
                let field_text = field.and_then(|n| n.utf8_text(source).ok());
                match field_text {
                    Some(f) => (
                        f.to_string(),
                        DispatchKind::Dynamic,
                        operand_text.map(String::from),
                    ),
                    None => return,
                }
            }
            "identifier" => {
                let text = match func_node.utf8_text(source).ok() {
                    Some(t) => t.to_string(),
                    None => return,
                };
                (text, DispatchKind::Direct, None)
            }
            _ => {
                // Parenthesized or complex expression — skip
                return;
            }
        };

        if keywords.contains(callee.as_str()) {
            return;
        }

        let line_no = func_node.start_position().row as u32 + 1;
        let start_col = func_node.start_position().column as u32;
        let end_col = func_node.end_position().column as u32;

        let target = by_name.get(&callee);
        let ref_id = StableId::ref_id(file_path, &callee, line_no, start_col);

        refs.push(SymbolRefRecord {
            ref_id: ref_id.clone(),
            file_path: file_path.to_string(),
            symbol_name: callee.clone(),
            container: caller_sym.and_then(|s| s.qname.clone()),
            ref_kind: "call".into(),
            line: line_no,
            column: start_col,
            target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
            target_file_path: target.map(|_| file_path.to_string()),
            target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
            ref_name: Some(callee.clone()),
            scope_id: caller_sym.and_then(|s| s.scope_id.clone()),
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
            ref_end_col: Some(end_col),
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::CallRef),
        });

        calls.push(CallEdgeRecord {
            edge_id: StableId::edge_id("call", file_path, line_no, start_col),
            file_path: file_path.to_string(),
            caller_symbol: caller_sym.map(|s| s.name.clone()),
            callee_symbol: callee,
            line: line_no,
            start_col,
            end_line: Some(line_no),
            end_col,
            target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
            target_file_path: target.map(|_| file_path.to_string()),
            caller_symbol_id: caller_sym.map(|s| s.symbol_id.clone()),
            caller_symbol_uid: caller_sym.and_then(|s| s.symbol_uid.clone()),
            callee_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
            callee_ref_id: Some(ref_id),
            dispatch_kind,
            call_kind: match dispatch_kind {
                DispatchKind::Direct => "direct",
                DispatchKind::Dynamic => "dynamic",
                _ => "unknown",
            }
            .into(),
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
            receiver_expr,
            arg_count,
            is_optional_chain: false,
            is_awaited: false,
            is_constructor: false,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: ParserTier::TreeSitter.element_confidence(ElementKind::CallEdge),
            synthesized_by: None,
            synthesis_key: None,
            registered_file: None,
            registered_line: None,
        });
    }

    // =========================================================================
    // Semantic edge extraction (embedded struct fields)
    // =========================================================================

    /// Extract embedded struct field relationships from `struct_type` bodies.
    fn extract_semantic_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();
        let root = tree.root_node();
        let mut root_cursor = root.walk();

        for child in root.children(&mut root_cursor) {
            if child.kind() == "type_declaration" {
                let mut decl_cursor = child.walk();
                for spec in child.children(&mut decl_cursor) {
                    if spec.kind() == "type_spec" {
                        self.extract_embedded_field_edges(
                            &spec, source, file_path, symbols, &mut edges,
                        );
                    }
                }
            }
        }

        edges
    }

    /// For a given `type_spec` node, if it's a struct, find embedded types.
    fn extract_embedded_field_edges(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let struct_name = match name_node.utf8_text(source).ok() {
            Some(n) => n,
            None => return,
        };

        let type_node = match node.child_by_field_name("type") {
            Some(n) => n,
            None => return,
        };
        if type_node.kind() != "struct_type" {
            return;
        }

        // Find the corresponding symbol for uid
        let struct_sym = symbols.iter().find(|s| s.name == struct_name);

        // Walk field_declaration_list
        let mut type_cursor = type_node.walk();
        for field_list_child in type_node.children(&mut type_cursor) {
            if field_list_child.kind() == "field_declaration_list" {
                let mut fl_cursor = field_list_child.walk();
                for field in field_list_child.children(&mut fl_cursor) {
                    if field.kind() == "field_declaration" {
                        // Embedded type: field_declaration with no `name` field
                        let has_name = field.child_by_field_name("name").is_some();
                        if !has_name {
                            if let Some(type_field) = field.child_by_field_name("type") {
                                let embedded_text = type_field
                                    .utf8_text(source)
                                    .ok()
                                    .map(|t| t.trim_start_matches('*').to_string());
                                if let Some(embedded) = embedded_text {
                                    if embedded == struct_name || embedded.is_empty() {
                                        continue;
                                    }
                                    let line = field.start_position().row as u32 + 1;
                                    edges.push(SemanticEdgeRecord {
                                        edge_id: format!(
                                            "se-{}:{}:inherits:{}",
                                            file_path, line, embedded
                                        ),
                                        file_path: file_path.to_string(),
                                        source_symbol: struct_name.to_string(),
                                        source_symbol_uid: struct_sym
                                            .and_then(|s| s.symbol_uid.clone()),
                                        target_symbol: embedded,
                                        target_symbol_uid: None,
                                        relation_kind: SemanticRelation::Inherits,
                                        line,
                                        confidence: ParserTier::TreeSitter
                                            .element_confidence(ElementKind::SemanticEdge),
                                        parser_tier: ParserTier::TreeSitter,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // Type assignment extraction
    // =========================================================================

    /// Extract local variable type assignments from function/method bodies.
    ///
    /// Patterns recognized:
    /// - `short_var_declaration` (`:=`):
    ///   - RHS is `call_expression` with callee starting with `New` or uppercase → FactoryCall
    ///   - RHS is `composite_literal` with type → Constructor
    ///   - RHS is `unary_expression` with `&` + `composite_literal` → Constructor (pointer)
    /// - `var_declaration` with type annotation → TypeAnnotation
    fn extract_type_assigns(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<TypeAssignRecord> {
        let mut assigns = Vec::new();
        let root = tree.root_node();
        let mut root_cursor = root.walk();

        for child in root.children(&mut root_cursor) {
            match child.kind() {
                "function_declaration" | "method_declaration" => {
                    let caller_sym = self.find_symbol_for_decl(&child, source, symbols);
                    let enclosing_uid = caller_sym.and_then(|s| s.symbol_uid.clone());
                    if let Some(body) = child.child_by_field_name("body") {
                        self.walk_type_assigns(
                            &body,
                            source,
                            file_path,
                            &enclosing_uid,
                            &mut assigns,
                        );
                    }
                }
                _ => {}
            }
        }

        assigns
    }

    /// Recursively walk AST nodes to find variable declarations with type information.
    fn walk_type_assigns(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        enclosing_uid: &Option<String>,
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        match node.kind() {
            "short_var_declaration" => {
                self.extract_short_var_assign(node, source, file_path, enclosing_uid, assigns);
            }
            "var_declaration" => {
                self.extract_var_decl_assign(node, source, file_path, enclosing_uid, assigns);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_type_assigns(&child, source, file_path, enclosing_uid, assigns);
        }
    }

    /// Extract type info from `short_var_declaration` (`:=`).
    fn extract_short_var_assign(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        enclosing_uid: &Option<String>,
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        // short_var_declaration: left := right
        let left = match node.child_by_field_name("left") {
            Some(n) => n,
            None => return,
        };
        let right = match node.child_by_field_name("right") {
            Some(n) => n,
            None => return,
        };

        // Get the first variable name from the left side (expression_list)
        let var_name = self.first_identifier_text(&left, source);
        let var_name = match var_name {
            Some(n) => n,
            None => return,
        };

        let line = node.start_position().row as u32 + 1;

        // Get the first expression from the right side (expression_list)
        let rhs = self.first_named_child(&right);
        let rhs = match rhs {
            Some(n) => n,
            None => return,
        };

        // Check RHS patterns
        if let Some(record) =
            self.infer_type_from_rhs(&rhs, source, file_path, &var_name, line, enclosing_uid)
        {
            assigns.push(record);
        }
    }

    /// Extract type info from `var_declaration` with type annotation.
    fn extract_var_decl_assign(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        enclosing_uid: &Option<String>,
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        // var_declaration contains var_spec children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "var_spec" {
                let name_node = child.child_by_field_name("name");
                let type_node = child.child_by_field_name("type");

                if let (Some(name_n), Some(type_n)) = (name_node, type_node) {
                    let var_name = match name_n.utf8_text(source).ok() {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    let type_text = match type_n.utf8_text(source).ok() {
                        Some(t) => t.trim_start_matches('*').trim().to_string(),
                        None => continue,
                    };
                    if type_text.is_empty() {
                        continue;
                    }
                    let line = child.start_position().row as u32 + 1;
                    assigns.push(TypeAssignRecord {
                        file_path: file_path.to_string(),
                        enclosing_symbol_uid: enclosing_uid.clone(),
                        var_name,
                        type_name: type_text,
                        line,
                        confidence: 0.95,
                        source: TypeAssignSource::TypeAnnotation,
                    });
                }
            }
        }
    }

    /// Infer the type of a variable from its RHS expression.
    fn infer_type_from_rhs(
        &self,
        rhs: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        var_name: &str,
        line: u32,
        enclosing_uid: &Option<String>,
    ) -> Option<TypeAssignRecord> {
        match rhs.kind() {
            "composite_literal" => {
                // Foo{...} or pkg.Foo{...}
                let type_node = rhs.child_by_field_name("type")?;
                let type_text = type_node.utf8_text(source).ok()?;
                let type_name = type_text.trim_start_matches('*').trim().to_string();
                if type_name.is_empty() {
                    return None;
                }
                Some(TypeAssignRecord {
                    file_path: file_path.to_string(),
                    enclosing_symbol_uid: enclosing_uid.clone(),
                    var_name: var_name.to_string(),
                    type_name,
                    line,
                    confidence: 0.95,
                    source: TypeAssignSource::Constructor,
                })
            }
            "unary_expression" => {
                // &Foo{...} (address-of composite literal)
                let operator = rhs.child_by_field_name("operator")?;
                let op_text = operator.utf8_text(source).ok()?;
                if op_text != "&" {
                    return None;
                }
                let operand = rhs.child_by_field_name("operand")?;
                if operand.kind() == "composite_literal" {
                    let type_node = operand.child_by_field_name("type")?;
                    let type_text = type_node.utf8_text(source).ok()?;
                    let type_name = type_text.trim().to_string();
                    if type_name.is_empty() {
                        return None;
                    }
                    return Some(TypeAssignRecord {
                        file_path: file_path.to_string(),
                        enclosing_symbol_uid: enclosing_uid.clone(),
                        var_name: var_name.to_string(),
                        type_name,
                        line,
                        confidence: 0.95,
                        source: TypeAssignSource::Constructor,
                    });
                }
                None
            }
            "call_expression" => {
                // NewFoo() or pkg.NewFoo()
                let func_node = rhs.child_by_field_name("function")?;
                let callee_text = func_node.utf8_text(source).ok()?;

                // Extract the leaf function name
                let leaf = callee_text.rsplit('.').next().unwrap_or(callee_text);

                // Pattern: NewXxx() → type = Xxx
                if leaf.starts_with("New") && leaf.len() > 3 {
                    let type_name = leaf[3..].to_string();
                    if !type_name.is_empty()
                        && type_name.starts_with(|c: char| c.is_ascii_uppercase())
                    {
                        return Some(TypeAssignRecord {
                            file_path: file_path.to_string(),
                            enclosing_symbol_uid: enclosing_uid.clone(),
                            var_name: var_name.to_string(),
                            type_name,
                            line,
                            confidence: 0.85,
                            source: TypeAssignSource::FactoryCall,
                        });
                    }
                }

                // Pattern: Xxx() where Xxx starts with uppercase (constructor-like)
                if leaf.starts_with(|c: char| c.is_ascii_uppercase()) && leaf != "New" {
                    return Some(TypeAssignRecord {
                        file_path: file_path.to_string(),
                        enclosing_symbol_uid: enclosing_uid.clone(),
                        var_name: var_name.to_string(),
                        type_name: leaf.to_string(),
                        line,
                        confidence: 0.70,
                        source: TypeAssignSource::FactoryCall,
                    });
                }

                None
            }
            _ => None,
        }
    }

    /// Get the text of the first `identifier` child in a node.
    fn first_identifier_text(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return child.utf8_text(source).ok().map(|s| s.to_string());
            }
        }
        // If node itself is an identifier
        if node.kind() == "identifier" {
            return node.utf8_text(source).ok().map(|s| s.to_string());
        }
        None
    }

    /// Get the first named child of a node (useful for expression_list).
    fn first_named_child<'a>(&self, node: &tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == "expression_list" {
            node.named_child(0)
        } else {
            Some(*node)
        }
    }

    // =========================================================================
    // Route extraction (Go HTTP frameworks)
    // =========================================================================

    /// HTTP method names recognized for route registration patterns.
    const ROUTE_METHODS: &'static [&'static str] = &[
        "GET",
        "POST",
        "PUT",
        "DELETE",
        "PATCH",
        "HEAD",
        "OPTIONS",
        "Handle",
        "HandleFunc",
        "Any",
        "Group",
    ];

    /// Detect framework from import paths present in the file.
    fn detect_go_framework(imports: &[ImportRecord]) -> Option<&'static str> {
        for imp in imports {
            let path = imp.import_string.as_str();
            if path.contains("gin-gonic/gin") {
                return Some("gin");
            }
            if path.contains("labstack/echo") {
                return Some("echo");
            }
            if path.contains("go-chi/chi") {
                return Some("chi");
            }
            if path.contains("gofiber/fiber") {
                return Some("fiber");
            }
            if path.contains("gorilla/mux") {
                return Some("gorilla");
            }
        }
        // Check for net/http (stdlib)
        for imp in imports {
            if imp.import_string == "net/http" {
                return Some("net_http");
            }
        }
        None
    }

    /// Extract route edges from function/method bodies.
    ///
    /// Looks for patterns like:
    ///   - `r.GET("/path", handler)` — Gin/Chi/Echo style
    ///   - `http.HandleFunc("/path", handler)` — stdlib net/http
    ///   - `router.Handle("/path", handler)` — generic
    fn extract_route_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        imports: &[ImportRecord],
        symbols: &[SymbolRecord],
    ) -> Vec<RouteEdgeRecord> {
        let mut routes = Vec::new();
        let framework = Self::detect_go_framework(imports);
        let root = tree.root_node();
        let mut root_cursor = root.walk();

        for child in root.children(&mut root_cursor) {
            match child.kind() {
                "function_declaration" | "method_declaration" => {
                    let caller_sym = self.find_symbol_for_decl(&child, source, symbols);
                    if let Some(body) = child.child_by_field_name("body") {
                        self.walk_route_calls(
                            &body,
                            source,
                            file_path,
                            framework,
                            &caller_sym,
                            &mut routes,
                        );
                    }
                }
                _ => {}
            }
        }

        routes
    }

    /// Recursively walk AST nodes to find route registration call expressions.
    fn walk_route_calls(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        framework: Option<&str>,
        caller_sym: &Option<&SymbolRecord>,
        routes: &mut Vec<RouteEdgeRecord>,
    ) {
        if node.kind() == "call_expression" {
            self.try_extract_route(node, source, file_path, framework, caller_sym, routes);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_route_calls(&child, source, file_path, framework, caller_sym, routes);
        }
    }

    /// Attempt to extract a route from a single call_expression node.
    ///
    /// Matches patterns:
    ///   selector_expression: `obj.GET("/path", handler)` or `obj.HandleFunc("/path", handler)`
    ///   identifier: `HandleFunc(...)` (less common standalone)
    fn try_extract_route(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        framework: Option<&str>,
        caller_sym: &Option<&SymbolRecord>,
        routes: &mut Vec<RouteEdgeRecord>,
    ) {
        let func_node = match node.child_by_field_name("function") {
            Some(n) => n,
            None => return,
        };

        let (method_name, _receiver_text) = match func_node.kind() {
            "selector_expression" => {
                let field = func_node.child_by_field_name("field");
                let operand = func_node.child_by_field_name("operand");
                let field_text = match field.and_then(|n| n.utf8_text(source).ok()) {
                    Some(t) => t.to_string(),
                    None => return,
                };
                let operand_text = operand
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(String::from);
                (field_text, operand_text)
            }
            _ => return, // Only selector expressions for route registration
        };

        // Check if the method name matches a route registration pattern
        let is_route_method = Self::ROUTE_METHODS
            .iter()
            .any(|m| m.eq_ignore_ascii_case(&method_name));
        // Also check lowercase versions common in some frameworks
        let method_lower = method_name.to_lowercase();
        let is_lowercase_route = matches!(
            method_lower.as_str(),
            "get"
                | "post"
                | "put"
                | "delete"
                | "patch"
                | "head"
                | "options"
                | "handle"
                | "handlefunc"
                | "any"
                | "group"
        );

        if !is_route_method && !is_lowercase_route {
            return;
        }

        // Extract arguments
        let args_node = match node.child_by_field_name("arguments") {
            Some(n) => n,
            None => return,
        };

        // First argument should be the route path (a string literal)
        let mut arg_cursor = args_node.walk();
        let mut args: Vec<tree_sitter::Node> = Vec::new();
        for arg in args_node.named_children(&mut arg_cursor) {
            args.push(arg);
        }

        if args.is_empty() {
            return;
        }

        // Extract route path from first argument (must be a string literal)
        let route_path = match args[0].kind() {
            "interpreted_string_literal" | "raw_string_literal" => {
                args[0].utf8_text(source).ok().map(|s| {
                    s.trim_start_matches('"')
                        .trim_end_matches('"')
                        .trim_start_matches('`')
                        .trim_end_matches('`')
                        .to_string()
                })
            }
            _ => None,
        };

        let route_path = match route_path {
            Some(p) if !p.is_empty() => p,
            _ => return,
        };

        // Extract handler name from second argument if present
        let handler_name = if args.len() > 1 {
            args[1].utf8_text(source).ok().map(|s| s.to_string())
        } else {
            None
        };

        // Determine HTTP method from the call method name
        let http_method = match method_lower.as_str() {
            "get" => Some("GET".to_string()),
            "post" => Some("POST".to_string()),
            "put" => Some("PUT".to_string()),
            "delete" => Some("DELETE".to_string()),
            "patch" => Some("PATCH".to_string()),
            "head" => Some("HEAD".to_string()),
            "options" => Some("OPTIONS".to_string()),
            "any" => None, // matches all methods
            "handle" | "handlefunc" | "group" => None,
            _ => None,
        };

        let line = node.start_position().row as u32 + 1;
        let start_col = node.start_position().column as u32;
        let end_line = node.end_position().row as u32 + 1;
        let end_col = node.end_position().column as u32;

        routes.push(RouteEdgeRecord {
            edge_id: StableId::edge_id("route", file_path, line, start_col),
            file_path: file_path.to_string(),
            route_path,
            handler_name: handler_name.clone(),
            method: http_method,
            line,
            start_col,
            end_line: Some(end_line),
            end_col,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: handler_name,
            router_symbol_uid: caller_sym.and_then(|s| s.symbol_uid.clone()),
            framework: framework.map(String::from),
            route_kind: Some("http".to_string()),
            confidence: ParserTier::TreeSitter.element_confidence(ElementKind::Route),
            parser_tier: ParserTier::TreeSitter,
            resolution_strategy: None,
            resolution_confidence: None,
        });
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Extract parameter types from a `parameter_list` node.
    /// Returns (comma-separated types string, param count).
    fn extract_param_types(
        &self,
        params_node: &Option<tree_sitter::Node>,
        source: &[u8],
    ) -> (Option<String>, u32) {
        let params_node = match params_node {
            Some(n) => *n,
            None => return (None, 0),
        };

        let mut types = Vec::new();
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            match child.kind() {
                "parameter_declaration" => {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        let type_text = type_node.utf8_text(source).unwrap_or("");
                        // Count how many names this declaration has (e.g., `a, b int` = 2)
                        let mut name_count = 0u32;
                        let mut nc = child.walk();
                        for ch in child.children(&mut nc) {
                            if ch.kind() == "identifier" {
                                name_count += 1;
                            }
                        }
                        // If no names, it's a single unnamed param
                        let count = if name_count == 0 { 1 } else { name_count };
                        for _ in 0..count {
                            types.push(type_text.to_string());
                        }
                    }
                }
                "variadic_parameter_declaration" => {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        let type_text = type_node.utf8_text(source).unwrap_or("");
                        types.push(format!("...{}", type_text));
                    }
                }
                _ => {}
            }
        }

        let count = types.len() as u32;
        if types.is_empty() {
            (None, 0)
        } else {
            (Some(types.join(", ")), count)
        }
    }

    /// Extract the result (return) type from a function/method declaration.
    fn extract_result_type(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let result_node = node.child_by_field_name("result")?;
        let text = result_node.utf8_text(source).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Extract doc comment immediately preceding a node.
    /// Go doc comments are `//` comments on consecutive lines right before the declaration.
    fn extract_doc_comment(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut comments = Vec::new();
        let mut prev = node.prev_sibling();
        while let Some(sib) = prev {
            if sib.kind() == "comment" {
                let text = sib.utf8_text(source).ok()?;
                // Check it's on the line immediately before
                if sib.end_position().row + 1 >= node.start_position().row
                    || (!comments.is_empty()
                        && sib.end_position().row + 1
                            >= comments
                                .last()
                                .map(|_| sib.end_position().row + 1)
                                .unwrap_or(0))
                {
                    comments.push(text.trim_start_matches("//").trim().to_string());
                    prev = sib.prev_sibling();
                    continue;
                }
            }
            break;
        }
        if comments.is_empty() {
            None
        } else {
            comments.reverse();
            Some(comments.join("\n"))
        }
    }

    // =========================================================================
    // Environment variable access extraction
    // =========================================================================

    /// Extract environment variable accesses from Go code.
    ///
    /// Matches `os.Getenv("KEY")` and `os.LookupEnv("KEY")`.
    fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        crate::dataflow_common::extract_env_accesses_with(
            content,
            file_path,
            &GO_ENV_ACCESS_RE,
            &[1, 2],
            |line| {
                crate::dataflow_common::find_enclosing_symbol(symbols, line)
                    .and_then(|s| s.symbol_uid.clone())
            },
        )
    }
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for GoParser {
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
        let tree =
            crate::parse_common::parse_tree(&self.language, content, file_path, timeout_micros)?;

        let symbols = self.extract_symbols(&tree, content.as_bytes(), file_path);
        let imports = self.extract_imports(&tree, content.as_bytes(), file_path);
        let (symbol_refs, call_edges) =
            self.extract_calls(&tree, content.as_bytes(), file_path, &symbols);
        let semantic_edges =
            self.extract_semantic_edges(&tree, content.as_bytes(), file_path, &symbols);
        let type_assigns =
            self.extract_type_assigns(&tree, content.as_bytes(), file_path, &symbols);
        let route_edges =
            self.extract_route_edges(&tree, content.as_bytes(), file_path, &imports, &symbols);
        let mut data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);
        data_flow_edges.extend(crate::dataflow_common::extract_param_return_flow(
            &call_edges,
            file_path,
        ));

        let http_call_edges = crate::http_call_helpers::extract_http_calls_with(
            content,
            file_path,
            &[
                crate::http_call_helpers::HttpCallSpec {
                    re: &GO_HTTP_FUNC_RE,
                    url_group: 2,
                    method_group: Some(1),
                    fixed_method: None,
                },
                crate::http_call_helpers::HttpCallSpec {
                    re: &GO_HTTP_NEWREQ_RE,
                    url_group: 2,
                    method_group: Some(1),
                    fixed_method: None,
                },
            ],
        );

        let tier = ParserTier::TreeSitter;
        let confidence = tier.default_confidence();
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);

        let summary = format!(
            "{} (go, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );
        let is_test = file_path.ends_with("_test.go");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            type_assigns,
            route_edges,
            data_flow_edges,
            http_call_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::Go]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::TreeSitter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_go() {
        let parser = GoParser::new();
        let code = r#"package main

import (
    "fmt"
    "os"
)

type Greeter struct {
    Name string
}

type Reader interface {
    Read(p []byte) (n int, err error)
}

type ID = string

func (g *Greeter) Greet() string {
    return fmt.Sprintf("hello %s", g.Name)
}

func main() {
    g := Greeter{Name: "world"}
    fmt.Println(g.Greet())
}
"#;
        let outcome = parser.parse("main.go", code, Language::Go).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Greeter"),
            "missing Greeter, got: {:?}",
            names
        );
        assert!(
            names.contains(&"Reader"),
            "missing Reader, got: {:?}",
            names
        );
        assert!(names.contains(&"Greet"), "missing Greet, got: {:?}", names);
        assert!(names.contains(&"main"), "missing main, got: {:?}", names);
        assert!(names.contains(&"ID"), "missing ID, got: {:?}", names);
        assert!(!outcome.imports.is_empty(), "imports should not be empty");
        assert!(!outcome.chunks.is_empty(), "chunks should not be empty");
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );
        assert_eq!(outcome.parser_tier, ParserTier::TreeSitter);
        assert!(!outcome.is_test_file);

        // Test file detection
        let test_outcome = parser.parse("main_test.go", code, Language::Go).unwrap();
        assert!(test_outcome.is_test_file);
    }

    #[test]
    fn parse_method_receiver() {
        let parser = GoParser::new();
        let code = r#"package main

type Server struct {}

func (s *Server) Start(addr string, port int) error {
    return nil
}
"#;
        let outcome = parser.parse("server.go", code, Language::Go).unwrap();
        let method = outcome.symbols.iter().find(|s| s.name == "Start").unwrap();
        assert_eq!(method.kind, SymbolKind::Method);
        assert_eq!(method.receiver_type.as_deref(), Some("Server"));
        assert_eq!(method.container.as_deref(), Some("Server"));
        assert_eq!(method.qname.as_deref(), Some("Server.Start"));
        assert_eq!(method.param_types.as_deref(), Some("string, int"));
        assert_eq!(method.param_count, Some(2));
        assert_eq!(method.return_type.as_deref(), Some("error"));
    }

    #[test]
    fn parse_imports() {
        let parser = GoParser::new();
        let code = r#"package main

import "fmt"

import (
    "os"
    myalias "encoding/json"
)

func main() {}
"#;
        let outcome = parser.parse("main.go", code, Language::Go).unwrap();
        assert_eq!(outcome.imports.len(), 3);

        let fmt_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string == "fmt")
            .unwrap();
        assert_eq!(fmt_imp.imported_name.as_deref(), Some("fmt"));

        let json_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string == "encoding/json")
            .unwrap();
        assert_eq!(json_imp.imported_name.as_deref(), Some("json"));
        assert_eq!(json_imp.alias.as_deref(), Some("myalias"));
    }

    #[test]
    fn parse_struct_embedded_field() {
        let parser = GoParser::new();
        let code = r#"package main

type Base struct {
    ID int
}

type Derived struct {
    Base
    Name string
}
"#;
        let outcome = parser.parse("embed.go", code, Language::Go).unwrap();
        assert!(
            !outcome.semantic_edges.is_empty(),
            "should detect embedded field"
        );
        let edge = &outcome.semantic_edges[0];
        assert_eq!(edge.source_symbol, "Derived");
        assert_eq!(edge.target_symbol, "Base");
        assert_eq!(edge.relation_kind, SemanticRelation::Inherits);
    }

    #[test]
    fn parse_call_dispatch_kinds() {
        let parser = GoParser::new();
        let code = r#"package main

import "fmt"

func helper() {}

func main() {
    helper()
    fmt.Println("hi")
}
"#;
        let outcome = parser.parse("calls.go", code, Language::Go).unwrap();
        let direct = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "helper");
        assert!(direct.is_some(), "should have direct call to helper");
        assert_eq!(direct.unwrap().dispatch_kind, DispatchKind::Direct);

        let dynamic = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "Println");
        assert!(dynamic.is_some(), "should have dynamic call to Println");
        assert_eq!(dynamic.unwrap().dispatch_kind, DispatchKind::Dynamic);
        assert_eq!(dynamic.unwrap().receiver_expr.as_deref(), Some("fmt"));
    }

    #[test]
    fn parse_gin_route_edges() {
        let parser = GoParser::new();
        let code = r#"package main

import "github.com/gin-gonic/gin"

func setupRouter() {
    r := gin.Default()
    r.GET("/api/users", getUsers)
    r.POST("/api/users", createUser)
    r.DELETE("/api/users/:id", deleteUser)
}

func getUsers(c *gin.Context) {}
func createUser(c *gin.Context) {}
func deleteUser(c *gin.Context) {}
"#;
        let outcome = parser.parse("routes.go", code, Language::Go).unwrap();
        assert!(
            outcome.route_edges.len() >= 3,
            "should detect at least 3 route edges, got {}",
            outcome.route_edges.len()
        );

        let get_route = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/api/users" && r.method.as_deref() == Some("GET"));
        assert!(get_route.is_some(), "should detect GET /api/users route");
        assert_eq!(
            get_route.unwrap().framework.as_deref(),
            Some("gin"),
            "should detect gin framework"
        );
        assert_eq!(
            get_route.unwrap().handler_name.as_deref(),
            Some("getUsers"),
            "should extract handler name"
        );

        let delete_route = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/api/users/:id");
        assert!(
            delete_route.is_some(),
            "should detect DELETE /api/users/:id route"
        );
        assert_eq!(delete_route.unwrap().method.as_deref(), Some("DELETE"));
    }

    #[test]
    fn parse_net_http_route_edges() {
        let parser = GoParser::new();
        let code = r#"package main

import "net/http"

func main() {
    http.HandleFunc("/health", healthHandler)
    http.HandleFunc("/api/data", dataHandler)
}

func healthHandler(w http.ResponseWriter, r *http.Request) {}
func dataHandler(w http.ResponseWriter, r *http.Request) {}
"#;
        let outcome = parser.parse("server.go", code, Language::Go).unwrap();
        assert!(
            outcome.route_edges.len() >= 2,
            "should detect at least 2 route edges from net/http, got {}",
            outcome.route_edges.len()
        );

        let health = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/health");
        assert!(health.is_some(), "should detect /health route");
        assert_eq!(
            health.unwrap().framework.as_deref(),
            Some("net_http"),
            "should detect net_http framework"
        );
    }

    #[test]
    fn parse_echo_route_edges() {
        let parser = GoParser::new();
        let code = r#"package main

import "github.com/labstack/echo/v4"

func main() {
    e := echo.New()
    e.GET("/users", getUsers)
    e.POST("/users", createUser)
    e.PUT("/users/:id", updateUser)
}

func getUsers(c echo.Context) error { return nil }
func createUser(c echo.Context) error { return nil }
func updateUser(c echo.Context) error { return nil }
"#;
        let outcome = parser.parse("echo_server.go", code, Language::Go).unwrap();
        assert!(
            outcome.route_edges.len() >= 3,
            "should detect at least 3 echo routes, got {}",
            outcome.route_edges.len()
        );

        let put_route = outcome
            .route_edges
            .iter()
            .find(|r| r.route_path == "/users/:id");
        assert!(put_route.is_some(), "should detect PUT /users/:id route");
        assert_eq!(put_route.unwrap().method.as_deref(), Some("PUT"));
        assert_eq!(put_route.unwrap().framework.as_deref(), Some("echo"));
    }

    #[test]
    fn test_go_env_access_getenv() {
        let parser = GoParser::new();
        let code = r#"package main

import "os"

func loadConfig() {
    dbUrl := os.Getenv("DATABASE_URL")
    _ = dbUrl
}
"#;
        let outcome = parser.parse("config.go", code, Language::Go).unwrap();
        assert_eq!(
            outcome.data_flow_edges.len(),
            1,
            "expected 1 data_flow_edge for Getenv, got {}",
            outcome.data_flow_edges.len()
        );
        let edge = &outcome.data_flow_edges[0];
        assert_eq!(edge.flow_kind, "env_access");
        assert_eq!(edge.env_key.as_deref(), Some("DATABASE_URL"));
        assert_eq!(edge.parser_tier, ParserTier::Heuristic);
        // The enclosing function is loadConfig
        assert!(
            edge.source_symbol_uid.is_some(),
            "should resolve enclosing function"
        );
    }

    #[test]
    fn test_go_env_access_lookupenv() {
        let parser = GoParser::new();
        let code = r#"package main

import "os"

func initAPI() {
    key, ok := os.LookupEnv("API_KEY")
    if !ok {
        panic("missing API_KEY")
    }
    _ = key
}
"#;
        let outcome = parser.parse("api.go", code, Language::Go).unwrap();
        assert_eq!(
            outcome.data_flow_edges.len(),
            1,
            "expected 1 data_flow_edge for LookupEnv, got {}",
            outcome.data_flow_edges.len()
        );
        let edge = &outcome.data_flow_edges[0];
        assert_eq!(edge.flow_kind, "env_access");
        assert_eq!(edge.env_key.as_deref(), Some("API_KEY"));
        assert!(
            edge.source_symbol_uid.is_some(),
            "should resolve enclosing function"
        );
    }

    #[test]
    fn extract_outbound_http_calls() {
        let parser = GoParser::new();
        let code = r#"
package main

import "net/http"

func fetchUsers() {
    http.Get("https://api.example.com/users")
    req, _ := http.NewRequest("POST", "/api/orders/123", nil)
    _ = req
    m := map[string]int{}
    _ = m["x"]
}
"#;
        let outcome = parser.parse("client.go", code, Language::Go).unwrap();
        assert!(
            outcome.http_call_edges.len() >= 2,
            "expected http.Get + http.NewRequest edges, got {}",
            outcome.http_call_edges.len()
        );
        let methods: Vec<_> = outcome
            .http_call_edges
            .iter()
            .filter_map(|e| e.method.clone())
            .collect();
        assert!(
            methods.contains(&"GET".to_string()),
            "methods: {:?}",
            methods
        );
        assert!(
            methods.contains(&"POST".to_string()),
            "methods: {:?}",
            methods
        );
    }
}
