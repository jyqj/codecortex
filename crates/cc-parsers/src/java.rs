//! Java parser using tree-sitter AST extraction (TreeSitter tier).

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, ImportRecord, ResolutionKind,
    SemanticEdgeRecord, SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::type_assign::{TypeAssignRecord, TypeAssignSource};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
    "var",
    "yield",
    "record",
    "sealed",
    "permits",
    "String",
    "Object",
    "System",
    "Override",
];

/// Matches `System.getenv("KEY")` and `System.getProperty("KEY")`.
static JAVA_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"System\.getenv\("(\w+)"\)|System\.getProperty\("(\w+)"\)"#)
        .expect("java env access regex")
});

pub struct JavaParser {
    language: tree_sitter::Language,
    chunker: Chunker,
}

impl JavaParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_java::LANGUAGE.into(),
            chunker: Chunker::default(),
        }
    }

    // =========================================================================
    // Symbol extraction
    // =========================================================================

    /// Recursively extract declarations from the tree-sitter AST.
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<SymbolRecord> {
        let mut symbols = Vec::new();
        let root = tree.root_node();
        self.walk_for_symbols(&root, source, file_path, None, &mut symbols);
        symbols
    }

    /// Walk AST nodes recursively to find class/interface/enum/method/constructor declarations.
    fn walk_for_symbols(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
        symbols: &mut Vec<SymbolRecord>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "class_declaration" => {
                    if let Some(sym) =
                        self.extract_class_like(&child, source, file_path, SymbolKind::Class)
                    {
                        let name = sym.name.clone();
                        symbols.push(sym);
                        // Recurse into class body
                        if let Some(body) = child.child_by_field_name("body") {
                            self.walk_for_symbols(&body, source, file_path, Some(&name), symbols);
                        }
                    }
                }
                "interface_declaration" => {
                    if let Some(sym) =
                        self.extract_class_like(&child, source, file_path, SymbolKind::Interface)
                    {
                        let name = sym.name.clone();
                        symbols.push(sym);
                        if let Some(body) = child.child_by_field_name("body") {
                            self.walk_for_symbols(&body, source, file_path, Some(&name), symbols);
                        }
                    }
                }
                "enum_declaration" => {
                    if let Some(sym) =
                        self.extract_class_like(&child, source, file_path, SymbolKind::Enum)
                    {
                        let name = sym.name.clone();
                        symbols.push(sym);
                        if let Some(body) = child.child_by_field_name("body") {
                            self.walk_for_symbols(&body, source, file_path, Some(&name), symbols);
                        }
                    }
                }
                "method_declaration" => {
                    if let Some(sym) = self.extract_method(&child, source, file_path, container) {
                        symbols.push(sym);
                    }
                }
                "constructor_declaration" => {
                    if let Some(sym) =
                        self.extract_constructor(&child, source, file_path, container)
                    {
                        symbols.push(sym);
                    }
                }
                // Recurse into class body, enum body, etc. that we haven't specifically handled
                "class_body" | "interface_body" | "enum_body" | "enum_body_declarations" => {
                    self.walk_for_symbols(&child, source, file_path, container, symbols);
                }
                _ => {}
            }
        }
    }

    /// Extract a class, interface, or enum declaration.
    fn extract_class_like(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        kind: SymbolKind,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        // Collect modifiers
        let modifiers = self.collect_modifiers(node, source);

        // Build signature
        let kind_str = match kind {
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            _ => "class",
        };
        let signature = if modifiers.is_empty() {
            format!("{} {}", kind_str, name)
        } else {
            format!("{} {} {}", modifiers.join(" "), kind_str, name)
        };

        // Extract superclass
        let base_types = self.extract_superclass(node, source);
        // Extract implements
        let implements = self.extract_interfaces(node, source);

        let qname = name.to_string();
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), Some(&signature));

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
            parser_confidence: 0.7,
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
            base_types,
            implements,
        })
    }

    /// Extract a `method_declaration` node into a SymbolRecord.
    fn extract_method(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        // Return type: the `type` field (before name)
        let return_type = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.to_string())
            .and_then(|s| if s == "void" { None } else { Some(s) });

        let params_node = node.child_by_field_name("parameters");
        let (param_types, param_count) = self.extract_param_types(&params_node, source);

        let params_text = params_node
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        let ret_text = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("void");

        let signature = format!("{} {}{}", ret_text, name, params_text);

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
        let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), Some(&signature));

        let end_line = node
            .child_by_field_name("body")
            .map(|b| b.end_position().row as u32 + 1)
            .unwrap_or(node.end_position().row as u32 + 1);

        let doc = self.extract_doc_comment(node, source);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.7,
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: container.map(String::from),
            param_types,
            return_type,
            param_count: Some(param_count),
            base_types: None,
            implements: None,
        })
    }

    /// Extract a `constructor_declaration` node into a SymbolRecord.
    fn extract_constructor(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        let params_node = node.child_by_field_name("parameters");
        let (param_types, param_count) = self.extract_param_types(&params_node, source);

        let params_text = params_node
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        let signature = format!("{}{}", name, params_text);

        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };

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
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.7,
            qname: Some(qname),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: container.map(String::from),
            param_types,
            return_type: None,
            param_count: Some(param_count),
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
                if let Some(imp) = self.extract_single_import(&child, source, file_path) {
                    imports.push(imp);
                }
            }
        }

        imports
    }

    /// Extract a single `import_declaration` into an ImportRecord.
    fn extract_single_import(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<ImportRecord> {
        // The full text: `import static foo.bar.Baz;` or `import foo.bar.Baz;`
        let full_text = node.utf8_text(source).ok()?;
        let is_static = full_text.contains("static ");

        // Extract the path: walk children to find scoped_identifier or identifier
        let import_path = self.find_import_path(node, source)?;

        // Last segment is the imported name
        let imported_name = import_path.rsplit('.').next().map(String::from);

        Some(ImportRecord {
            file_path: file_path.to_string(),
            import_string: if is_static {
                format!("static {}", import_path)
            } else {
                import_path
            },
            resolved_path: None,
            imported_name,
            alias: None,
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        })
    }

    /// Recursively find the import path from an import_declaration's children.
    #[allow(clippy::only_used_in_recursion)]
    fn find_import_path(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "scoped_identifier" | "identifier" => {
                    return child.utf8_text(source).ok().map(|s| s.to_string());
                }
                "asterisk" => {
                    // import foo.bar.* — the scoped_identifier is a sibling
                    // handled by parent already having scoped_identifier
                }
                _ => {
                    // Recurse
                    if let Some(path) = self.find_import_path(&child, source) {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    // =========================================================================
    // Call extraction (AST-based)
    // =========================================================================

    /// Walk all method/constructor bodies for call expressions.
    fn extract_calls(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let keywords: HashSet<&str> = JAVA_KEYWORDS.iter().copied().collect();
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
        self.walk_for_calls(
            &root, source, file_path, &keywords, &by_name, symbols, &mut refs, &mut calls,
        );

        (refs, calls)
    }

    /// Recursively walk AST to find method_invocation and object_creation_expression nodes.
    #[allow(clippy::too_many_arguments)]
    fn walk_for_calls(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        symbols: &[SymbolRecord],
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        match node.kind() {
            "method_invocation" => {
                self.extract_method_invocation(
                    node, source, file_path, keywords, by_name, symbols, refs, calls,
                );
            }
            "object_creation_expression" => {
                self.extract_object_creation(
                    node, source, file_path, keywords, by_name, symbols, refs, calls,
                );
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_calls(
                &child, source, file_path, keywords, by_name, symbols, refs, calls,
            );
        }
    }

    /// Extract a single `method_invocation` call.
    #[allow(clippy::too_many_arguments)]
    fn extract_method_invocation(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        symbols: &[SymbolRecord],
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        let name_node = match node.child_by_field_name("name") {
            Some(n) => n,
            None => return,
        };
        let callee = match name_node.utf8_text(source).ok() {
            Some(t) => t.to_string(),
            None => return,
        };

        if keywords.contains(callee.as_str()) {
            return;
        }

        // Check for receiver (object field)
        let object_node = node.child_by_field_name("object");
        let receiver_expr = object_node
            .and_then(|n| n.utf8_text(source).ok())
            .map(String::from);

        let dispatch_kind = if receiver_expr.is_some() {
            DispatchKind::Dynamic
        } else {
            DispatchKind::Direct
        };

        // Count arguments
        let arg_count = node.child_by_field_name("arguments").map(|args| {
            let mut cnt = 0u32;
            let mut arg_cursor = args.walk();
            for arg_child in args.named_children(&mut arg_cursor) {
                if arg_child.is_named() {
                    cnt += 1;
                }
            }
            cnt
        });

        let line_no = name_node.start_position().row as u32 + 1;
        let start_col = name_node.start_position().column as u32;
        let end_col = name_node.end_position().column as u32;

        // Find enclosing method
        let caller_sym = self.find_enclosing_method(symbols, line_no);

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
            parser_confidence: 0.7,
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
            parser_confidence: 0.7,
            synthesized_by: None,
            synthesis_key: None,
            registered_file: None,
            registered_line: None,
        });
    }

    /// Extract an `object_creation_expression` (`new Foo(...)`) as a constructor call.
    #[allow(clippy::too_many_arguments)]
    fn extract_object_creation(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        symbols: &[SymbolRecord],
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        let type_node = match node.child_by_field_name("type") {
            Some(n) => n,
            None => return,
        };
        let type_name = match type_node.utf8_text(source).ok() {
            Some(t) => {
                // Strip generics: `ArrayList<String>` -> `ArrayList`
                t.split('<').next().unwrap_or(t).to_string()
            }
            None => return,
        };

        if keywords.contains(type_name.as_str()) {
            return;
        }

        // Count arguments
        let arg_count = node.child_by_field_name("arguments").map(|args| {
            let mut cnt = 0u32;
            let mut arg_cursor = args.walk();
            for arg_child in args.named_children(&mut arg_cursor) {
                if arg_child.is_named() {
                    cnt += 1;
                }
            }
            cnt
        });

        let line_no = node.start_position().row as u32 + 1;
        let start_col = node.start_position().column as u32;
        let end_col = type_node.end_position().column as u32;

        let caller_sym = self.find_enclosing_method(symbols, line_no);
        let target = by_name.get(&type_name);
        let ref_id = StableId::ref_id(file_path, &type_name, line_no, start_col);

        refs.push(SymbolRefRecord {
            ref_id: ref_id.clone(),
            file_path: file_path.to_string(),
            symbol_name: type_name.clone(),
            container: caller_sym.and_then(|s| s.qname.clone()),
            ref_kind: "call".into(),
            line: line_no,
            column: start_col,
            target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
            target_file_path: target.map(|_| file_path.to_string()),
            target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
            ref_name: Some(type_name.clone()),
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
            parser_confidence: 0.7,
        });

        calls.push(CallEdgeRecord {
            edge_id: StableId::edge_id("call", file_path, line_no, start_col),
            file_path: file_path.to_string(),
            caller_symbol: caller_sym.map(|s| s.name.clone()),
            callee_symbol: type_name,
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
            dispatch_kind: DispatchKind::Constructor,
            call_kind: "constructor".into(),
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
            arg_count,
            is_optional_chain: false,
            is_awaited: false,
            is_constructor: true,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.7,
            synthesized_by: None,
            synthesis_key: None,
            registered_file: None,
            registered_line: None,
        });
    }

    // =========================================================================
    // Semantic edge extraction
    // =========================================================================

    /// Extract semantic edges: extends, implements, annotations, throws.
    fn extract_semantic_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();
        let root = tree.root_node();
        self.walk_for_semantic_edges(&root, source, file_path, symbols, &mut edges);
        edges
    }

    /// Recursively walk AST for semantic edges.
    fn walk_for_semantic_edges(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        match node.kind() {
            "class_declaration" => {
                self.extract_class_edges(node, source, file_path, symbols, edges);
            }
            "interface_declaration" => {
                // Interface extends
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(source) {
                        let sym = symbols.iter().find(|s| s.name == name);
                        // extends_interfaces field for interface
                        if let Some(extends_node) = node.child_by_field_name("extends_interfaces") {
                            self.extract_type_list_edges(
                                &extends_node,
                                source,
                                file_path,
                                name,
                                sym,
                                SemanticRelation::Inherits,
                                edges,
                            );
                        }
                    }
                }
            }
            "method_declaration" | "constructor_declaration" => {
                self.extract_method_edges(node, source, file_path, symbols, edges);
            }
            "throw_statement" => {
                self.extract_throw_statement_edge(node, source, file_path, symbols, edges);
            }
            _ => {}
        }

        // Check for annotations on this node
        self.extract_annotation_edges(node, source, file_path, edges);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_semantic_edges(&child, source, file_path, symbols, edges);
        }
    }

    /// Extract extends/implements from a class declaration.
    fn extract_class_edges(
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
        let class_name = match name_node.utf8_text(source).ok() {
            Some(n) => n,
            None => return,
        };

        let sym = symbols.iter().find(|s| s.name == class_name);

        // superclass field
        if let Some(superclass_node) = node.child_by_field_name("superclass") {
            // superclass node is a `superclass` node containing a type_identifier
            let parent_name = self.extract_type_name(&superclass_node, source);
            if let Some(parent) = parent_name {
                let line = superclass_node.start_position().row as u32 + 1;
                edges.push(SemanticEdgeRecord {
                    edge_id: format!("se-{}:{}:inherits:{}", file_path, line, parent),
                    file_path: file_path.to_string(),
                    source_symbol: class_name.to_string(),
                    source_symbol_uid: sym.and_then(|s| s.symbol_uid.clone()),
                    target_symbol: parent,
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Inherits,
                    line,
                    confidence: 0.95,
                    parser_tier: ParserTier::TreeSitter,
                });
            }
        }

        // interfaces field
        if let Some(interfaces_node) = node.child_by_field_name("interfaces") {
            self.extract_type_list_edges(
                &interfaces_node,
                source,
                file_path,
                class_name,
                sym,
                SemanticRelation::Implements,
                edges,
            );
        }
    }

    /// Extract edges from method: throws clause.
    fn extract_method_edges(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        let line = node.start_position().row as u32 + 1;

        // Find corresponding symbol
        let name_node = node.child_by_field_name("name");
        let method_name = name_node.and_then(|n| n.utf8_text(source).ok());

        let method_sym = match method_name {
            Some(name) => symbols
                .iter()
                .find(|s| s.name == name && s.start_line == line),
            None => None,
        };

        let source_name = method_sym
            .and_then(|s| s.qname.as_deref())
            .unwrap_or_else(|| method_name.unwrap_or("unknown"))
            .to_string();

        let source_uid = method_sym.and_then(|s| s.symbol_uid.clone());

        // Walk children looking for `throws` type_list
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "throws" {
                // throws node contains type_identifier children
                let mut throws_cursor = child.walk();
                for throws_child in child.children(&mut throws_cursor) {
                    if throws_child.kind() == "type_identifier"
                        || throws_child.kind() == "scoped_type_identifier"
                    {
                        if let Ok(exc_name) = throws_child.utf8_text(source) {
                            let throws_line = throws_child.start_position().row as u32 + 1;
                            edges.push(SemanticEdgeRecord {
                                edge_id: format!(
                                    "se-{}:{}:throws:{}",
                                    file_path, throws_line, exc_name
                                ),
                                file_path: file_path.to_string(),
                                source_symbol: source_name.clone(),
                                source_symbol_uid: source_uid.clone(),
                                target_symbol: exc_name.to_string(),
                                target_symbol_uid: None,
                                relation_kind: SemanticRelation::Throws,
                                line: throws_line,
                                confidence: 0.95,
                                parser_tier: ParserTier::TreeSitter,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Extract a `throw new ExceptionType(...)` statement as a Throws edge.
    fn extract_throw_statement_edge(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        let line = node.start_position().row as u32 + 1;

        // Find the object_creation_expression inside throw
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "object_creation_expression" {
                if let Some(type_node) = child.child_by_field_name("type") {
                    if let Ok(type_name) = type_node.utf8_text(source) {
                        let exc_name = type_name.split('<').next().unwrap_or(type_name);

                        let enclosing = self.find_enclosing_method(symbols, line);
                        let source_name = enclosing
                            .and_then(|s| s.qname.as_deref())
                            .unwrap_or(file_path)
                            .to_string();
                        let source_uid = enclosing.and_then(|s| s.symbol_uid.clone());

                        edges.push(SemanticEdgeRecord {
                            edge_id: format!("se-{}:{}:throws:{}", file_path, line, exc_name),
                            file_path: file_path.to_string(),
                            source_symbol: source_name,
                            source_symbol_uid: source_uid,
                            target_symbol: exc_name.to_string(),
                            target_symbol_uid: None,
                            relation_kind: SemanticRelation::Throws,
                            line,
                            confidence: 0.9,
                            parser_tier: ParserTier::TreeSitter,
                        });
                    }
                }
            }
        }
    }

    /// Extract annotation edges from a declaration node.
    /// Looks for `marker_annotation` or `annotation` siblings/children preceding the node.
    fn extract_annotation_edges(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        // Only process declarations that can be annotated
        let is_declaration = matches!(
            node.kind(),
            "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "constructor_declaration"
        );
        if !is_declaration {
            return;
        }

        // Get the target (annotated) name
        let target_name = match node.child_by_field_name("name") {
            Some(n) => match n.utf8_text(source).ok() {
                Some(t) => t.to_string(),
                None => return,
            },
            None => return,
        };

        // Annotations appear as named children before the declaration in its parent,
        // or as direct children of the declaration node itself.
        // In tree-sitter-java, annotations (modifiers) are children of the declaration node.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "modifiers" => {
                    let mut mod_cursor = child.walk();
                    for mod_child in child.children(&mut mod_cursor) {
                        self.process_annotation(&mod_child, source, file_path, &target_name, edges);
                    }
                }
                "marker_annotation" | "annotation" => {
                    self.process_annotation(&child, source, file_path, &target_name, edges);
                }
                _ => {}
            }
        }
    }

    /// Process a single annotation node and add a Decorates edge if appropriate.
    fn process_annotation(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        target_name: &str,
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        if node.kind() != "marker_annotation" && node.kind() != "annotation" {
            return;
        }

        let ann_name_node = node.child_by_field_name("name");
        let ann_name = match ann_name_node {
            Some(n) => match n.utf8_text(source).ok() {
                Some(t) => t,
                None => return,
            },
            None => return,
        };

        // Skip common non-semantic annotations
        if ann_name == "Override" || ann_name == "SuppressWarnings" || ann_name == "Deprecated" {
            return;
        }

        let line = node.start_position().row as u32 + 1;
        edges.push(SemanticEdgeRecord {
            edge_id: format!("se-{}:{}:decorates:{}", file_path, line, ann_name),
            file_path: file_path.to_string(),
            source_symbol: ann_name.to_string(),
            source_symbol_uid: None,
            target_symbol: target_name.to_string(),
            target_symbol_uid: None,
            relation_kind: SemanticRelation::Decorates,
            line,
            confidence: 0.95,
            parser_tier: ParserTier::TreeSitter,
        });
    }

    /// Extract type names from a type_list node (used for implements/extends lists).
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::only_used_in_recursion)]
    fn extract_type_list_edges(
        &self,
        list_node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        class_name: &str,
        sym: Option<&SymbolRecord>,
        relation: SemanticRelation,
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        let mut cursor = list_node.walk();
        for child in list_node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "scoped_type_identifier" | "generic_type" => {
                    let type_name = if child.kind() == "generic_type" {
                        // Extract base type from generic: `Comparable<T>` -> `Comparable`
                        child
                            .named_child(0)
                            .and_then(|n| n.utf8_text(source).ok())
                            .map(|s| s.to_string())
                    } else {
                        child.utf8_text(source).ok().map(|s| s.to_string())
                    };

                    if let Some(name) = type_name {
                        let line = child.start_position().row as u32 + 1;
                        edges.push(SemanticEdgeRecord {
                            edge_id: format!(
                                "se-{}:{}:{}:{}",
                                file_path,
                                line,
                                relation.as_str(),
                                name
                            ),
                            file_path: file_path.to_string(),
                            source_symbol: class_name.to_string(),
                            source_symbol_uid: sym.and_then(|s| s.symbol_uid.clone()),
                            target_symbol: name,
                            target_symbol_uid: None,
                            relation_kind: relation,
                            line,
                            confidence: 0.95,
                            parser_tier: ParserTier::TreeSitter,
                        });
                    }
                }
                "type_list" => {
                    // Recurse into type_list
                    self.extract_type_list_edges(
                        &child, source, file_path, class_name, sym, relation, edges,
                    );
                }
                _ => {}
            }
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Extract the type name from a node, handling various type kinds.
    fn extract_type_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        // Walk children to find the actual type identifier
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "scoped_type_identifier" => {
                    return child.utf8_text(source).ok().map(|s| s.to_string());
                }
                "generic_type" => {
                    // Get the base type
                    return child
                        .named_child(0)
                        .and_then(|n| n.utf8_text(source).ok())
                        .map(|s| s.to_string());
                }
                _ => {}
            }
        }
        // Fallback: try the node itself
        node.utf8_text(source)
            .ok()
            .map(|s| s.split('<').next().unwrap_or(s).trim().to_string())
    }

    /// Extract superclass name from a class_declaration.
    fn extract_superclass(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let superclass_node = node.child_by_field_name("superclass")?;
        let name = self.extract_type_name(&superclass_node, source)?;
        Some(name)
    }

    /// Extract interface names from a class_declaration's `interfaces` field.
    fn extract_interfaces(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let interfaces_node = node.child_by_field_name("interfaces")?;
        let mut names = Vec::new();
        let mut cursor = interfaces_node.walk();
        for child in interfaces_node.children(&mut cursor) {
            match child.kind() {
                "type_identifier" | "scoped_type_identifier" => {
                    if let Ok(name) = child.utf8_text(source) {
                        names.push(name.to_string());
                    }
                }
                "generic_type" => {
                    if let Some(name) = child.named_child(0).and_then(|n| n.utf8_text(source).ok())
                    {
                        names.push(name.to_string());
                    }
                }
                "type_list" => {
                    let mut tl_cursor = child.walk();
                    for tl_child in child.children(&mut tl_cursor) {
                        match tl_child.kind() {
                            "type_identifier" | "scoped_type_identifier" => {
                                if let Ok(name) = tl_child.utf8_text(source) {
                                    names.push(name.to_string());
                                }
                            }
                            "generic_type" => {
                                if let Some(name) = tl_child
                                    .named_child(0)
                                    .and_then(|n| n.utf8_text(source).ok())
                                {
                                    names.push(name.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if names.is_empty() {
            None
        } else {
            Some(names.join(", "))
        }
    }

    /// Collect modifier keywords from a declaration node.
    fn collect_modifiers(&self, node: &tree_sitter::Node, source: &[u8]) -> Vec<String> {
        let mut modifiers = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "modifiers" {
                let mut mod_cursor = child.walk();
                for mod_child in child.children(&mut mod_cursor) {
                    match mod_child.kind() {
                        "public" | "private" | "protected" | "static" | "final" | "abstract"
                        | "synchronized" | "native" | "strictfp" | "default" => {
                            if let Ok(text) = mod_child.utf8_text(source) {
                                modifiers.push(text.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        modifiers
    }

    /// Extract parameter types from a `formal_parameters` node.
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
                "formal_parameter" | "spread_parameter" => {
                    if let Some(type_node) = child.child_by_field_name("type") {
                        let type_text = type_node.utf8_text(source).unwrap_or("");
                        if child.kind() == "spread_parameter" {
                            types.push(format!("{}...", type_text));
                        } else {
                            types.push(type_text.to_string());
                        }
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

    /// Extract doc comment (Javadoc) immediately preceding a node.
    /// Javadoc comments start with `/**` and are block_comment nodes.
    fn extract_doc_comment(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let prev = node.prev_sibling()?;
        // In tree-sitter-java, Javadoc comments may be block_comment or comment nodes
        if prev.kind() == "block_comment" || prev.kind() == "comment" {
            let text = prev.utf8_text(source).ok()?;
            if text.starts_with("/**") {
                // Clean up the Javadoc comment
                let cleaned: Vec<&str> = text
                    .lines()
                    .map(|l| {
                        l.trim()
                            .trim_start_matches("/**")
                            .trim_start_matches("*/")
                            .trim_start_matches('*')
                            .trim()
                    })
                    .filter(|l| !l.is_empty())
                    .collect();
                if cleaned.is_empty() {
                    return None;
                }
                return Some(cleaned.join("\n"));
            }
        }
        None
    }

    /// Find the innermost enclosing function/method for a given line number.
    fn find_enclosing_method<'a>(
        &self,
        symbols: &'a [SymbolRecord],
        line: u32,
    ) -> Option<&'a SymbolRecord> {
        symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .filter(|s| s.start_line <= line && s.end_line >= line)
            .min_by_key(|s| s.end_line - s.start_line)
    }

    // =========================================================================
    // Type assignment extraction
    // =========================================================================

    /// Extract local variable type assignments from the AST.
    ///
    /// Patterns recognized:
    /// - `local_variable_declaration`: `Type varName = ...`
    ///   - TypeAnnotation from the declared type
    ///   - Constructor if initializer is `object_creation_expression` (`new Foo(...)`)
    fn extract_type_assigns(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<TypeAssignRecord> {
        let mut assigns = Vec::new();
        let root = tree.root_node();
        self.walk_for_type_assigns(&root, source, file_path, symbols, &mut assigns);
        assigns
    }

    /// Recursively walk AST to find `local_variable_declaration` nodes.
    fn walk_for_type_assigns(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        if node.kind() == "local_variable_declaration" {
            self.extract_local_var_type_assign(node, source, file_path, symbols, assigns);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_type_assigns(&child, source, file_path, symbols, assigns);
        }
    }

    /// Extract type info from a `local_variable_declaration` node.
    /// In Java: `Type varName = expr;` or `var varName = new Type(...);`
    fn extract_local_var_type_assign(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        assigns: &mut Vec<TypeAssignRecord>,
    ) {
        // Get the type node
        let type_node = match node.child_by_field_name("type") {
            Some(n) => n,
            None => return,
        };
        let type_text = match type_node.utf8_text(source).ok() {
            Some(t) => t.to_string(),
            None => return,
        };

        let line = node.start_position().row as u32 + 1;
        let enclosing = self
            .find_enclosing_method(symbols, line)
            .and_then(|s| s.symbol_uid.clone());

        // Find variable_declarator children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() != "variable_declarator" {
                continue;
            }
            let name_node = match child.child_by_field_name("name") {
                Some(n) => n,
                None => continue,
            };
            let var_name = match name_node.utf8_text(source).ok() {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Check if the type is `var` (Java 10+ local type inference)
            let is_var = type_text == "var";

            // Check initializer for constructor pattern
            let initializer = child.child_by_field_name("value");
            let has_new_expr = initializer
                .map(|init| self.find_object_creation_type(&init, source))
                .unwrap_or(None);

            if let Some(new_type) = has_new_expr {
                // `new Foo(...)` — Constructor
                assigns.push(TypeAssignRecord {
                    file_path: file_path.to_string(),
                    enclosing_symbol_uid: enclosing.clone(),
                    var_name: var_name.clone(),
                    type_name: new_type,
                    line,
                    confidence: 0.95,
                    source: TypeAssignSource::Constructor,
                });
            } else if !is_var && !type_text.is_empty() {
                // Explicit type annotation: `Foo x = ...`
                // Strip generics for the base type
                let base_type = if let Some(idx) = type_text.find('<') {
                    type_text[..idx].to_string()
                } else {
                    type_text.clone()
                };
                if !base_type.is_empty() {
                    assigns.push(TypeAssignRecord {
                        file_path: file_path.to_string(),
                        enclosing_symbol_uid: enclosing.clone(),
                        var_name,
                        type_name: base_type,
                        line,
                        confidence: 0.95,
                        source: TypeAssignSource::TypeAnnotation,
                    });
                }
            }
        }
    }

    /// Find the type name from an `object_creation_expression` (`new Foo(...)`).
    #[allow(clippy::only_used_in_recursion)]
    fn find_object_creation_type(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        if node.kind() == "object_creation_expression" {
            let type_node = node.child_by_field_name("type")?;
            let type_text = type_node.utf8_text(source).ok()?;
            // Strip generics
            let base = if let Some(idx) = type_text.find('<') {
                &type_text[..idx]
            } else {
                type_text
            };
            return Some(base.trim().to_string());
        }
        // Check if node wraps an object_creation_expression (e.g., cast)
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "object_creation_expression" {
                return self.find_object_creation_type(&child, source);
            }
        }
        None
    }

    // =========================================================================
    // Environment variable access extraction
    // =========================================================================

    /// Extract environment variable accesses from Java code.
    ///
    /// Matches `System.getenv("KEY")` and `System.getProperty("KEY")`.
    fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        for cap in JAVA_ENV_ACCESS_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let env_key = cap.get(1).or(cap.get(2)).map(|m| m.as_str().to_string());

            let source_uid = self
                .find_enclosing_method(symbols, line)
                .and_then(|s| s.symbol_uid.clone());

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
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for JavaParser {
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
        let mut data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);
        data_flow_edges.extend(crate::dataflow_common::extract_param_return_flow(
            &call_edges,
            file_path,
        ));

        let tier = ParserTier::TreeSitter;
        let confidence = 0.7;
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);

        let summary = format!(
            "{} (java, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );

        let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
        let is_test = file_name.starts_with("Test") || file_name.ends_with("Test.java");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            type_assigns,
            data_flow_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::Java]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::TreeSitter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_java() {
        let p = JavaParser::new();
        let code = r#"package com.example;

import java.util.List;
import java.io.IOException;

public class Greeter {
    private String name;

    public Greeter(String name) {
        this.name = name;
    }

    public String greet() {
        return format("hello %s", name);
    }

    private String format(String fmt, String arg) {
        return String.format(fmt, arg);
    }
}

interface Speaker {
    void speak();
}

enum Color {
    RED, GREEN, BLUE
}
"#;
        let outcome = p.parse("Greeter.java", code, Language::Java).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Greeter"),
            "missing Greeter, got: {:?}",
            names
        );
        assert!(
            names.contains(&"Speaker"),
            "missing Speaker, got: {:?}",
            names
        );
        assert!(names.contains(&"Color"), "missing Color, got: {:?}", names);
        assert!(names.contains(&"greet"), "missing greet, got: {:?}", names);
        assert!(
            names.contains(&"format"),
            "missing format, got: {:?}",
            names
        );
        assert!(!outcome.imports.is_empty(), "imports should not be empty");
        assert!(!outcome.chunks.is_empty(), "chunks should not be empty");
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );
        assert_eq!(outcome.parser_tier, ParserTier::TreeSitter);
        assert!(!outcome.is_test_file);

        // Test file detection
        let test_outcome = p.parse("GreeterTest.java", code, Language::Java).unwrap();
        assert!(test_outcome.is_test_file);

        let test_outcome2 = p.parse("TestGreeter.java", code, Language::Java).unwrap();
        assert!(test_outcome2.is_test_file);
    }

    #[test]
    fn parse_class_with_extends_and_implements() {
        let p = JavaParser::new();
        let code = r#"
public class Dog extends Animal implements Runnable, Comparable<Dog> {
    public void run() {}
    public int compareTo(Dog other) { return 0; }
}
"#;
        let outcome = p.parse("Dog.java", code, Language::Java).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Dog"), "missing Dog, got: {:?}", names);
        assert!(names.contains(&"run"), "missing run, got: {:?}", names);
        assert!(
            names.contains(&"compareTo"),
            "missing compareTo, got: {:?}",
            names
        );

        // Check Dog symbol
        let dog = outcome.symbols.iter().find(|s| s.name == "Dog").unwrap();
        assert_eq!(dog.kind, SymbolKind::Class);
        assert!(
            dog.base_types.as_deref() == Some("Animal"),
            "expected base_types = Animal, got: {:?}",
            dog.base_types
        );

        // Check semantic edges
        let inherits: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Inherits)
            .collect();
        assert!(
            inherits
                .iter()
                .any(|e| e.source_symbol == "Dog" && e.target_symbol == "Animal"),
            "missing Dog -> Animal inherits, got: {:?}",
            inherits
        );

        let implements: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Implements)
            .collect();
        assert!(
            implements
                .iter()
                .any(|e| e.source_symbol == "Dog" && e.target_symbol == "Runnable"),
            "missing Dog -> Runnable implements, got: {:?}",
            implements
        );
        assert!(
            implements
                .iter()
                .any(|e| e.source_symbol == "Dog" && e.target_symbol == "Comparable"),
            "missing Dog -> Comparable implements, got: {:?}",
            implements
        );
    }

    #[test]
    fn parse_method_details() {
        let p = JavaParser::new();
        let code = r#"
public class Service {
    public String process(String input, int count) {
        return input.repeat(count);
    }
}
"#;
        let outcome = p.parse("Service.java", code, Language::Java).unwrap();
        let method = outcome
            .symbols
            .iter()
            .find(|s| s.name == "process")
            .unwrap();
        assert_eq!(method.kind, SymbolKind::Method);
        assert_eq!(method.container.as_deref(), Some("Service"));
        assert_eq!(method.qname.as_deref(), Some("Service.process"));
        assert_eq!(method.return_type.as_deref(), Some("String"));
        assert_eq!(method.param_types.as_deref(), Some("String, int"));
        assert_eq!(method.param_count, Some(2));
        assert_eq!(method.parser_tier, ParserTier::TreeSitter);
    }

    #[test]
    fn parse_imports() {
        let p = JavaParser::new();
        let code = r#"
import java.util.List;
import java.io.*;
import static java.lang.Math.PI;

public class Foo {}
"#;
        let outcome = p.parse("Foo.java", code, Language::Java).unwrap();
        assert!(
            outcome.imports.len() >= 2,
            "expected at least 2 imports, got: {}",
            outcome.imports.len()
        );

        let list_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string.contains("java.util.List"));
        assert!(list_imp.is_some(), "missing java.util.List import");

        let static_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string.contains("static"));
        assert!(static_imp.is_some(), "missing static import");
    }

    #[test]
    fn parse_call_dispatch_kinds() {
        let p = JavaParser::new();
        let code = r#"
public class Caller {
    public void doWork() {
        helper();
        obj.process();
    }

    private void helper() {}
}
"#;
        let outcome = p.parse("Caller.java", code, Language::Java).unwrap();

        let direct = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "helper");
        assert!(direct.is_some(), "should have call to helper");
        assert_eq!(direct.unwrap().dispatch_kind, DispatchKind::Direct);

        let dynamic = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "process");
        assert!(dynamic.is_some(), "should have call to process");
        assert_eq!(dynamic.unwrap().dispatch_kind, DispatchKind::Dynamic);
        assert_eq!(dynamic.unwrap().receiver_expr.as_deref(), Some("obj"));
    }

    #[test]
    fn parse_constructor_calls() {
        let p = JavaParser::new();
        let code = r#"
public class Factory {
    public Object create() {
        return new ArrayList();
    }
}
"#;
        let outcome = p.parse("Factory.java", code, Language::Java).unwrap();
        let ctor = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "ArrayList");
        assert!(ctor.is_some(), "should have constructor call to ArrayList");
        assert!(ctor.unwrap().is_constructor);
        assert_eq!(ctor.unwrap().dispatch_kind, DispatchKind::Constructor);
    }

    #[test]
    fn parse_annotations() {
        let p = JavaParser::new();
        let code = r#"
import org.springframework.web.bind.annotation.RestController;

@RestController
public class MyController {
    @Override
    public String toString() { return ""; }

    @GetMapping("/hello")
    public String hello() { return "hello"; }
}
"#;
        let outcome = p.parse("MyController.java", code, Language::Java).unwrap();
        let decorates: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Decorates)
            .collect();

        // @Override should be skipped
        assert!(
            !decorates.iter().any(|e| e.source_symbol == "Override"),
            "Override should be skipped"
        );
        // @RestController should be captured
        assert!(
            decorates
                .iter()
                .any(|e| e.source_symbol == "RestController"),
            "missing @RestController decoration, got: {:?}",
            decorates
        );
        // @GetMapping should be captured
        assert!(
            decorates.iter().any(|e| e.source_symbol == "GetMapping"),
            "missing @GetMapping decoration, got: {:?}",
            decorates
        );
    }

    #[test]
    fn extract_throw_edges_java() {
        let p = JavaParser::new();
        let code = r#"package com.example;

import java.io.IOException;
import java.text.ParseException;

public class Service {
    public void read() throws IOException, ParseException {
        // reading
    }

    public void process() {
        throw new IllegalArgumentException("bad arg");
    }

    public void complex() throws RuntimeException {
        if (true) {
            throw new IllegalStateException("state");
        }
        throw new NullPointerException("null");
    }
}
"#;
        let outcome = p.parse("Service.java", code, Language::Java).unwrap();
        let throw_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Throws)
            .collect();

        // read() throws IOException, ParseException -> 2 edges from declaration
        let read_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.read")
            .collect();
        assert_eq!(
            read_edges.len(),
            2,
            "expected 2 throws edges for read(), got {:?}",
            read_edges
        );
        let read_targets: Vec<&str> = read_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(read_targets.contains(&"IOException"));
        assert!(read_targets.contains(&"ParseException"));
        assert!((read_edges[0].confidence - 0.95).abs() < 0.01);

        // process() -> throw new IllegalArgumentException -> 1 edge
        let process_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.process")
            .collect();
        assert_eq!(process_edges.len(), 1);
        assert_eq!(process_edges[0].target_symbol, "IllegalArgumentException");
        assert!((process_edges[0].confidence - 0.9).abs() < 0.01);

        // complex() -> throws RuntimeException + throw new IllegalStateException + throw new NullPointerException
        let complex_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.complex")
            .collect();
        assert_eq!(
            complex_edges.len(),
            3,
            "expected 3 throws edges for complex(), got {:?}",
            complex_edges
        );
        let complex_targets: Vec<&str> = complex_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(complex_targets.contains(&"RuntimeException"));
        assert!(complex_targets.contains(&"IllegalStateException"));
        assert!(complex_targets.contains(&"NullPointerException"));
    }

    #[test]
    fn parse_enum_and_interface() {
        let p = JavaParser::new();
        let code = r#"
public interface Readable {
    String read();
}

public enum Status {
    ACTIVE,
    INACTIVE;

    public boolean isActive() {
        return this == ACTIVE;
    }
}
"#;
        let outcome = p.parse("Types.java", code, Language::Java).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Readable"),
            "missing Readable, got: {:?}",
            names
        );
        assert!(
            names.contains(&"Status"),
            "missing Status, got: {:?}",
            names
        );

        let readable = outcome
            .symbols
            .iter()
            .find(|s| s.name == "Readable")
            .unwrap();
        assert_eq!(readable.kind, SymbolKind::Interface);

        let status = outcome.symbols.iter().find(|s| s.name == "Status").unwrap();
        assert_eq!(status.kind, SymbolKind::Enum);

        // Method inside enum
        assert!(
            names.contains(&"isActive"),
            "missing isActive in enum, got: {:?}",
            names
        );
    }

    #[test]
    fn parser_tier_is_tree_sitter() {
        let p = JavaParser::new();
        assert_eq!(p.tier(), ParserTier::TreeSitter);
    }

    #[test]
    fn test_java_env_access_getenv() {
        let p = JavaParser::new();
        let code = r#"
public class Config {
    public String getDbHost() {
        return System.getenv("DB_HOST");
    }
}
"#;
        let outcome = p.parse("Config.java", code, Language::Java).unwrap();
        assert_eq!(
            outcome.data_flow_edges.len(),
            1,
            "expected 1 data_flow_edge for getenv, got {}",
            outcome.data_flow_edges.len()
        );
        let edge = &outcome.data_flow_edges[0];
        assert_eq!(edge.flow_kind, "env_access");
        assert_eq!(edge.env_key.as_deref(), Some("DB_HOST"));
        assert_eq!(edge.parser_tier, ParserTier::Heuristic);
        assert!(
            edge.source_symbol_uid.is_some(),
            "should resolve enclosing method"
        );
    }

    #[test]
    fn test_java_env_access_getproperty() {
        let p = JavaParser::new();
        let code = r#"
public class AppConfig {
    public String getAppName() {
        return System.getProperty("app_name");
    }
}
"#;
        let outcome = p.parse("AppConfig.java", code, Language::Java).unwrap();
        assert_eq!(
            outcome.data_flow_edges.len(),
            1,
            "expected 1 data_flow_edge for getProperty, got {}",
            outcome.data_flow_edges.len()
        );
        let edge = &outcome.data_flow_edges[0];
        assert_eq!(edge.flow_kind, "env_access");
        assert_eq!(edge.env_key.as_deref(), Some("app_name"));
        assert!(
            edge.source_symbol_uid.is_some(),
            "should resolve enclosing method"
        );
    }
}
