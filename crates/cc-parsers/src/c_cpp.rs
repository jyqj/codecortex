//! C/C++ parser using tree-sitter AST extraction (TreeSitter tier).

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, ImportRecord, ResolutionKind,
    SemanticEdgeRecord, SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Matches `getenv("KEY")` and `std::getenv("KEY")` in C/C++ code.
static C_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:std::)?getenv\(\s*"(\w+)"\s*\)"#).expect("c env access regex")
});

static C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "_Bool",
    "_Complex",
    "_Imaginary",
    // C++ additional keywords
    "class",
    "namespace",
    "template",
    "typename",
    "virtual",
    "override",
    "final",
    "public",
    "private",
    "protected",
    "new",
    "delete",
    "this",
    "throw",
    "try",
    "catch",
    "operator",
    "friend",
    "using",
    "constexpr",
    "nullptr",
    "true",
    "false",
    "bool",
    "dynamic_cast",
    "static_cast",
    "reinterpret_cast",
    "const_cast",
    "typeid",
    "decltype",
    "noexcept",
    // Common builtins
    "sizeof",
    "alignof",
    "offsetof",
    "NULL",
];

pub struct CCppParser {
    language: tree_sitter::Language,
    is_cpp: bool,
    chunker: Chunker,
}

impl CCppParser {
    pub fn new(lang: Language) -> Self {
        let (ts_lang, is_cpp) = match lang {
            Language::Cpp => (tree_sitter_cpp::LANGUAGE.into(), true),
            _ => (tree_sitter_c::LANGUAGE.into(), false),
        };
        Self {
            language: ts_lang,
            is_cpp,
            chunker: Chunker::default(),
        }
    }

    // =========================================================================
    // Symbol extraction
    // =========================================================================

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> Vec<SymbolRecord> {
        let mut symbols = Vec::new();
        let root = tree.root_node();
        self.walk_symbols(&root, source, file_path, None, &mut symbols);
        symbols
    }

    /// Recursively walk AST to extract declarations.
    fn walk_symbols(
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
                "function_definition" => {
                    if let Some(sym) =
                        self.extract_function_definition(&child, source, file_path, container)
                    {
                        symbols.push(sym);
                    }
                }
                "struct_specifier" => {
                    if child.child_by_field_name("body").is_some() {
                        if let Some(sym) = self.extract_struct_or_class(
                            &child,
                            source,
                            file_path,
                            SymbolKind::Class,
                            container,
                        ) {
                            let name = sym.name.clone();
                            symbols.push(sym);
                            // Walk body for nested declarations (methods in C++)
                            if let Some(body) = child.child_by_field_name("body") {
                                self.walk_symbols(&body, source, file_path, Some(&name), symbols);
                            }
                        }
                    }
                }
                "enum_specifier" => {
                    if child.child_by_field_name("body").is_some() {
                        if let Some(sym) = self.extract_enum(&child, source, file_path, container) {
                            symbols.push(sym);
                        }
                    }
                }
                "type_definition" => {
                    if let Some(sym) = self.extract_typedef(&child, source, file_path, container) {
                        symbols.push(sym);
                    }
                }
                // C++ specific
                "class_specifier" if self.is_cpp => {
                    if child.child_by_field_name("body").is_some() {
                        if let Some(sym) = self.extract_struct_or_class(
                            &child,
                            source,
                            file_path,
                            SymbolKind::Class,
                            container,
                        ) {
                            let name = sym.name.clone();
                            symbols.push(sym);
                            if let Some(body) = child.child_by_field_name("body") {
                                self.walk_symbols(&body, source, file_path, Some(&name), symbols);
                            }
                        }
                    }
                }
                "namespace_definition" if self.is_cpp => {
                    if let Some(sym) = self.extract_namespace(&child, source, file_path) {
                        let name = sym.name.clone();
                        symbols.push(sym);
                        if let Some(body) = child.child_by_field_name("body") {
                            self.walk_symbols(&body, source, file_path, Some(&name), symbols);
                        }
                    }
                }
                "template_declaration" if self.is_cpp => {
                    // Unwrap the template and extract the inner declaration
                    self.walk_symbols(&child, source, file_path, container, symbols);
                }
                "declaration" => {
                    // Skip plain declarations (variable decls, forward decls) at the top level
                }
                _ => {}
            }
        }
    }

    /// Extract a `function_definition` node.
    fn extract_function_definition(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let declarator_node = node.child_by_field_name("declarator")?;
        let (name, params_node) = self.unwrap_function_declarator(&declarator_node, source)?;

        // Return type from the `type` field
        let type_node = node.child_by_field_name("type");
        let return_type = type_node
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.to_string());

        let (param_types, param_count) = self.extract_param_types(&params_node, source);

        let kind = if container.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        let params_text = params_node
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");

        let signature = match &return_type {
            Some(rt) => format!("{} {}{}", rt, name, params_text),
            None => format!("{}{}", name, params_text),
        };

        let qname = match container {
            Some(c) => format!("{}::{}", c, name),
            None => name.to_string(),
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
            receiver_type: None,
            param_types,
            return_type,
            param_count: Some(param_count),
            base_types: None,
            implements: None,
        })
    }

    /// Unwrap nested declarators to find the function_declarator and extract name + params.
    /// Returns (name, optional parameter_list node).
    fn unwrap_function_declarator<'a>(
        &self,
        node: &tree_sitter::Node<'a>,
        source: &[u8],
    ) -> Option<(String, Option<tree_sitter::Node<'a>>)> {
        // Walk down through pointer/parenthesized/reference declarators to find the
        // function_declarator. We do an iterative search to avoid lifetime issues.
        let func_decl = self.find_function_declarator(node)?;
        let name_node = func_decl.child_by_field_name("declarator")?;
        let name = self.extract_declarator_name(&name_node, source)?;
        let params = func_decl.child_by_field_name("parameters");
        Some((name, params))
    }

    /// Iteratively search for a function_declarator node within nested declarator wrappers.
    #[allow(clippy::only_used_in_recursion)]
    fn find_function_declarator<'a>(
        &self,
        node: &tree_sitter::Node<'a>,
    ) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == "function_declarator" {
            return Some(*node);
        }
        // Try to unwrap common wrapper kinds
        match node.kind() {
            "pointer_declarator" | "reference_declarator" => {
                let inner = node.child_by_field_name("declarator")?;
                self.find_function_declarator(&inner)
            }
            "parenthesized_declarator" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(found) = self.find_function_declarator(&child) {
                        return Some(found);
                    }
                }
                None
            }
            _ => {
                // Scan children for function_declarator
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "function_declarator" {
                        return Some(child);
                    }
                }
                None
            }
        }
    }

    /// Extract the name from a declarator (identifier, qualified_identifier, etc.).
    fn extract_declarator_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        match node.kind() {
            "identifier" | "field_identifier" | "type_identifier" | "destructor_name" => {
                node.utf8_text(source).ok().map(String::from)
            }
            "qualified_identifier" if self.is_cpp => {
                // e.g., MyClass::method — take the last name component
                let name = node.child_by_field_name("name")?;
                self.extract_declarator_name(&name, source)
            }
            _ => node.utf8_text(source).ok().map(String::from),
        }
    }

    /// Extract a struct/class specifier into a SymbolRecord.
    fn extract_struct_or_class(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        kind: SymbolKind,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        let kind_str = if node.kind() == "class_specifier" {
            "class"
        } else {
            "struct"
        };
        let signature = format!("{} {}", kind_str, name);

        let qname = match container {
            Some(c) => format!("{}::{}", c, name),
            None => name.to_string(),
        };

        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), None);

        let doc = self.extract_doc_comment(node, source);

        // Extract base classes for C++ class_specifier
        let base_types = if self.is_cpp {
            self.extract_base_classes(node, source)
        } else {
            None
        };

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
            implements: None,
        })
    }

    /// Extract an enum specifier.
    fn extract_enum(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        let signature = format!("enum {}", name);
        let qname = match container {
            Some(c) => format!("{}::{}", c, name),
            None => name.to_string(),
        };

        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, &qname, SymbolKind::Enum.as_str(), None);

        let doc = self.extract_doc_comment(node, source);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Enum,
            container: container.map(String::from),
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
            base_types: None,
            implements: None,
        })
    }

    /// Extract a typedef.
    fn extract_typedef(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        // The typedef declarator contains the new name — it's the last named child
        // that is an identifier/type_identifier.
        let declarator = node.child_by_field_name("declarator")?;
        let name = self.extract_declarator_name(&declarator, source)?;

        let type_node = node.child_by_field_name("type");
        let type_text = type_node.and_then(|n| n.utf8_text(source).ok());
        let signature = match type_text {
            Some(tt) => format!("typedef {} {}", tt, name),
            None => format!("typedef {}", name),
        };

        let qname = match container {
            Some(c) => format!("{}::{}", c, name),
            None => name.to_string(),
        };

        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid =
            StableId::symbol_uid(file_path, &qname, SymbolKind::TypeAlias.as_str(), None);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name,
            kind: SymbolKind::TypeAlias,
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc: None,
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
            base_types: None,
            implements: None,
        })
    }

    /// Extract a C++ namespace_definition.
    fn extract_namespace(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;

        let signature = format!("namespace {}", name);
        let qname = name.to_string();

        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid =
            StableId::symbol_uid(file_path, &qname, SymbolKind::Namespace.as_str(), None);

        Some(SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Namespace,
            container: None,
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: Some(signature),
            doc: None,
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
            base_types: None,
            implements: None,
        })
    }

    /// Extract C++ base classes from a class_specifier or struct_specifier.
    /// Returns a comma-separated string of base class names.
    fn extract_base_classes(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut bases = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "base_class_clause" {
                let mut bc_cursor = child.walk();
                for bc_child in child.children(&mut bc_cursor) {
                    // type_identifier nodes within the base_class_clause are base class names
                    if bc_child.kind() == "type_identifier"
                        || bc_child.kind() == "qualified_identifier"
                    {
                        if let Ok(text) = bc_child.utf8_text(source) {
                            bases.push(text.to_string());
                        }
                    }
                }
            }
        }
        if bases.is_empty() {
            None
        } else {
            Some(bases.join(", "))
        }
    }

    // =========================================================================
    // Import extraction (#include)
    // =========================================================================

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
            if child.kind() == "preproc_include" {
                if let Some(imp) = self.extract_include(&child, source, file_path) {
                    imports.push(imp);
                }
            }
        }

        imports
    }

    /// Extract a single `preproc_include` into an ImportRecord.
    fn extract_include(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<ImportRecord> {
        let path_node = node.child_by_field_name("path")?;
        let path_text = path_node.utf8_text(source).ok()?;

        // Determine if system include (<...>) or local include ("...")
        let is_system = path_node.kind() == "system_lib_string";

        // Strip angle brackets or quotes
        let import_path = path_text
            .trim_start_matches('"')
            .trim_end_matches('"')
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string();

        // Extract the file name as the imported name
        let imported_name = import_path.rsplit('/').next().map(String::from);

        Some(ImportRecord {
            file_path: file_path.to_string(),
            import_string: import_path,
            resolved_path: None,
            imported_name,
            alias: None,
            is_namespace: is_system,
            is_default: false,
            is_reexport: false,
        })
    }

    // =========================================================================
    // Call extraction
    // =========================================================================

    fn extract_calls(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let keywords: HashSet<&str> = C_KEYWORDS.iter().copied().collect();
        let mut refs = Vec::new();
        let mut calls = Vec::new();

        // Build name -> (symbol_id, symbol_uid) map
        let mut by_name: HashMap<String, (&str, &str)> = HashMap::new();
        for sym in symbols {
            if let Some(uid) = &sym.symbol_uid {
                by_name
                    .entry(sym.name.clone())
                    .or_insert((sym.symbol_id.as_str(), uid.as_str()));
            }
        }

        let root = tree.root_node();
        self.walk_calls_recursive(
            &root, source, file_path, &keywords, &by_name, symbols, &None, &mut refs, &mut calls,
        );

        (refs, calls)
    }

    /// Recursively walk AST to find call_expression nodes inside function bodies.
    #[allow(clippy::too_many_arguments)]
    fn walk_calls_recursive(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        keywords: &HashSet<&str>,
        by_name: &HashMap<String, (&str, &str)>,
        symbols: &[SymbolRecord],
        current_fn: &Option<&SymbolRecord>,
        refs: &mut Vec<SymbolRefRecord>,
        calls: &mut Vec<CallEdgeRecord>,
    ) {
        // Track when we enter a function body
        let mut active_fn = *current_fn;
        if node.kind() == "function_definition" {
            active_fn = self.find_symbol_for_node(node, source, symbols);
        }

        match node.kind() {
            "call_expression" => {
                self.extract_single_call(
                    node, source, file_path, keywords, by_name, &active_fn, refs, calls,
                );
            }
            "new_expression" if self.is_cpp => {
                self.extract_new_expression(
                    node, source, file_path, keywords, by_name, &active_fn, refs, calls,
                );
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_calls_recursive(
                &child, source, file_path, keywords, by_name, symbols, &active_fn, refs, calls,
            );
        }
    }

    /// Find the SymbolRecord matching a function_definition node.
    fn find_symbol_for_node<'a>(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        symbols: &'a [SymbolRecord],
    ) -> Option<&'a SymbolRecord> {
        let line = node.start_position().row as u32 + 1;
        let declarator = node.child_by_field_name("declarator")?;
        let (name, _) = self.unwrap_function_declarator(&declarator, source)?;
        symbols
            .iter()
            .find(|s| s.name == name && s.start_line == line)
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
                if arg_child.is_named() {
                    cnt += 1;
                }
            }
            cnt
        });

        let (callee, dispatch_kind, receiver_expr) = match func_node.kind() {
            "field_expression" => {
                // obj.method() / obj->method() / obj.method<T>() / obj->method<T>()
                let argument = func_node.child_by_field_name("argument");
                let field = match func_node.child_by_field_name("field") {
                    Some(n) => n,
                    None => return,
                };
                let argument_text = argument.and_then(|n| n.utf8_text(source).ok());
                // A `template_method` field carries the bare name plus a
                // `template_argument_list`; take only the leading identifier so
                // the callee is `method`, not `method<T>`.
                let field_name = self.callee_name_from_field(&field, source);
                match field_name {
                    Some(f) => (f, DispatchKind::Dynamic, argument_text.map(String::from)),
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
            "qualified_identifier" if self.is_cpp => {
                // Namespace::func() / Class::staticMethod() / ns::func<T>()
                match self.callee_name_from_qualified(&func_node, source) {
                    Some(name) => (name, DispatchKind::Direct, None),
                    None => return,
                }
            }
            "template_function" if self.is_cpp => {
                // Bare generic call: foo<T>(). The `name` field holds the callee,
                // which may itself be an identifier or a qualified_identifier.
                let name_node = match func_node.child_by_field_name("name") {
                    Some(n) => n,
                    None => return,
                };
                match name_node.kind() {
                    "identifier" | "field_identifier" => match name_node.utf8_text(source).ok() {
                        Some(t) => (t.to_string(), DispatchKind::Direct, None),
                        None => return,
                    },
                    "qualified_identifier" => {
                        match self.callee_name_from_qualified(&name_node, source) {
                            Some(name) => (name, DispatchKind::Direct, None),
                            None => return,
                        }
                    }
                    _ => return,
                }
            }
            _ => return,
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

    /// Resolve the bare callee name from a `field_expression` field node.
    ///
    /// For a plain `field_identifier` this is the text itself. For a
    /// `template_method` (`method<T>`) it is the inner `field_identifier`,
    /// dropping the template argument suffix.
    fn callee_name_from_field(&self, field: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        match field.kind() {
            "field_identifier" | "identifier" => field.utf8_text(source).ok().map(String::from),
            "template_method" => {
                let name = field
                    .child_by_field_name("name")
                    .or_else(|| field.named_child(0))?;
                name.utf8_text(source).ok().map(String::from)
            }
            _ => field.utf8_text(source).ok().map(String::from),
        }
    }

    /// Resolve the bare callee name from a `qualified_identifier` node, taking the
    /// final name component and stripping any template argument suffix.
    ///
    /// Handles `ns::func`, `Class::method`, and `ns::func<T>` (where the trailing
    /// component is a `template_function`).
    #[allow(clippy::only_used_in_recursion)]
    fn callee_name_from_qualified(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        // The `name` field holds the last component; recurse for nested qualifiers.
        if let Some(name) = node.child_by_field_name("name") {
            match name.kind() {
                "identifier" | "field_identifier" => {
                    return name.utf8_text(source).ok().map(String::from);
                }
                "qualified_identifier" => {
                    return self.callee_name_from_qualified(&name, source);
                }
                "template_function" | "template_method" => {
                    let inner = name
                        .child_by_field_name("name")
                        .or_else(|| name.named_child(0))?;
                    return inner.utf8_text(source).ok().map(String::from);
                }
                _ => {}
            }
        }
        // Fallback: split the raw text and strip a trailing template suffix.
        let text = node.utf8_text(source).ok()?;
        let last = text.rsplit("::").next().unwrap_or(text);
        let stripped = last.split('<').next().unwrap_or(last).trim();
        if stripped.is_empty() {
            None
        } else {
            Some(stripped.to_string())
        }
    }

    /// Extract C++ constructor calls from `new_expression` nodes (`new Foo(...)`).
    ///
    /// Mirrors the Java parser: produces a call edge with
    /// `dispatch_kind = Constructor` and `is_constructor = true`, callee being the
    /// constructed type name (template/qualifier suffix stripped).
    #[allow(clippy::too_many_arguments)]
    fn extract_new_expression(
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
        // The constructed type lives in the `type` field (type_identifier,
        // qualified_identifier, or template_type for `new Foo<T>()`).
        let type_node = match node.child_by_field_name("type") {
            Some(n) => n,
            None => return,
        };
        let type_name = match type_node.kind() {
            "type_identifier" => type_node.utf8_text(source).ok().map(String::from),
            "qualified_identifier" => self.callee_name_from_qualified(&type_node, source),
            "template_type" => type_node
                .child_by_field_name("name")
                .or_else(|| type_node.named_child(0))
                .and_then(|n| n.utf8_text(source).ok())
                .map(String::from),
            _ => type_node
                .utf8_text(source)
                .ok()
                .map(|t| t.split('<').next().unwrap_or(t).trim().to_string()),
        };
        let type_name = match type_name {
            Some(t) if !t.is_empty() && !keywords.contains(t.as_str()) => t,
            _ => return,
        };

        // Argument count comes from the `arguments` field (argument_list); a `new`
        // without parentheses (e.g. `new Foo`) has no arguments node.
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
    // Semantic edge extraction (C++ inheritance)
    // =========================================================================

    fn extract_semantic_edges(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> Vec<SemanticEdgeRecord> {
        if !self.is_cpp {
            return Vec::new();
        }

        let mut edges = Vec::new();
        let root = tree.root_node();
        self.walk_for_inheritance(&root, source, file_path, symbols, &mut edges);
        edges
    }

    /// Walk AST to find class_specifier/struct_specifier with base_class_clause.
    #[allow(clippy::only_used_in_recursion)]
    fn walk_for_inheritance(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &[SymbolRecord],
        edges: &mut Vec<SemanticEdgeRecord>,
    ) {
        match node.kind() {
            "class_specifier" | "struct_specifier" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(class_name) = name_node.utf8_text(source) {
                        let class_sym = symbols.iter().find(|s| s.name == class_name);

                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() == "base_class_clause" {
                                let mut bc_cursor = child.walk();
                                for bc_child in child.children(&mut bc_cursor) {
                                    if bc_child.kind() == "type_identifier"
                                        || bc_child.kind() == "qualified_identifier"
                                    {
                                        if let Ok(base_name) = bc_child.utf8_text(source) {
                                            let line = bc_child.start_position().row as u32 + 1;
                                            edges.push(SemanticEdgeRecord {
                                                edge_id: format!(
                                                    "se-{}:{}:inherits:{}",
                                                    file_path, line, base_name
                                                ),
                                                file_path: file_path.to_string(),
                                                source_symbol: class_name.to_string(),
                                                source_symbol_uid: class_sym
                                                    .and_then(|s| s.symbol_uid.clone()),
                                                target_symbol: base_name.to_string(),
                                                target_symbol_uid: None,
                                                relation_kind: SemanticRelation::Inherits,
                                                line,
                                                confidence: 0.95,
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
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_inheritance(&child, source, file_path, symbols, edges);
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    /// Extract parameter types from a parameter_list node.
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
                        types.push(type_text.to_string());
                    }
                }
                "variadic_parameter" | "parameter_pack_expansion" => {
                    types.push("...".to_string());
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

    /// Extract doc comment (C-style /** ... */ or consecutive // comments) before a node.
    fn extract_doc_comment(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let mut prev = node.prev_sibling();
        // Check for block comment (/** ... */)
        if let Some(sib) = prev {
            if sib.kind() == "comment" {
                let text = sib.utf8_text(source).ok()?;
                if text.starts_with("/**") {
                    let cleaned = text
                        .trim_start_matches("/**")
                        .trim_end_matches("*/")
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !cleaned.is_empty() {
                        return Some(cleaned);
                    }
                }
            }
        }

        // Check for consecutive // comments
        let mut comments = Vec::new();
        while let Some(sib) = prev {
            if sib.kind() == "comment" {
                let text = sib.utf8_text(source).ok()?;
                if text.starts_with("//") {
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

    /// Extract environment variable accesses from C/C++ code.
    ///
    /// Matches `getenv("KEY")` and `std::getenv("KEY")`.
    fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        for cap in C_ENV_ACCESS_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let env_key = cap.get(1).map(|m| m.as_str().to_string());

            let source_uid = crate::dataflow_common::find_enclosing_symbol(symbols, line)
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

    /// Check if a file path looks like a test file.
    fn is_test_file(file_path: &str) -> bool {
        let lower = file_path.to_lowercase();
        lower.ends_with("_test.c")
            || lower.ends_with("_test.cpp")
            || lower.ends_with("_test.cc")
            || lower.ends_with("_test.cxx")
            || lower.contains("/test_")
            || lower.contains("/tests/")
            || lower.contains("_tests.c")
            || lower.contains("_tests.cpp")
    }
}

impl Default for CCppParser {
    fn default() -> Self {
        Self::new(Language::C)
    }
}

impl FileParser for CCppParser {
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
        let mut data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);
        data_flow_edges.extend(crate::dataflow_common::extract_param_return_flow(
            &call_edges,
            file_path,
        ));

        let tier = ParserTier::TreeSitter;
        let confidence = 0.7;
        let lang_name = if self.is_cpp { "cpp" } else { "c" };
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);

        let summary = format!(
            "{} ({}, {} lines, {} symbols)",
            file_path,
            lang_name,
            content.lines().count(),
            symbols.len()
        );
        let is_test = Self::is_test_file(file_path);

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            data_flow_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        if self.is_cpp {
            &[Language::Cpp]
        } else {
            &[Language::C]
        }
    }

    fn tier(&self) -> ParserTier {
        ParserTier::TreeSitter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_c() {
        let parser = CCppParser::new(Language::C);
        let code = r#"
#include <stdio.h>
#include "myheader.h"

typedef int MyInt;

enum Color {
    RED,
    GREEN,
    BLUE
};

struct Point {
    int x;
    int y;
};

void helper(int n) {
    printf("n=%d\n", n);
}

int main(int argc, char** argv) {
    struct Point p;
    p.x = 1;
    helper(42);
    return 0;
}
"#;
        let outcome = parser.parse("main.c", code, Language::C).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();

        assert!(
            names.contains(&"helper"),
            "missing helper, got: {:?}",
            names
        );
        assert!(names.contains(&"main"), "missing main, got: {:?}", names);
        assert!(names.contains(&"Point"), "missing Point, got: {:?}", names);
        assert!(names.contains(&"Color"), "missing Color, got: {:?}", names);
        assert!(names.contains(&"MyInt"), "missing MyInt, got: {:?}", names);

        // Check imports
        assert_eq!(outcome.imports.len(), 2, "should have 2 includes");
        let stdio = outcome
            .imports
            .iter()
            .find(|i| i.import_string == "stdio.h")
            .unwrap();
        assert!(stdio.is_namespace, "stdio.h should be system include");

        let local = outcome
            .imports
            .iter()
            .find(|i| i.import_string == "myheader.h")
            .unwrap();
        assert!(!local.is_namespace, "myheader.h should be local include");

        // Check calls
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );
        let helper_call = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "helper");
        assert!(helper_call.is_some(), "should have call to helper");

        // Check tiers
        assert_eq!(outcome.parser_tier, ParserTier::TreeSitter);
        assert!(!outcome.is_test_file);

        // Test file detection
        let test_outcome = parser.parse("main_test.c", code, Language::C).unwrap();
        assert!(test_outcome.is_test_file);
    }

    #[test]
    fn parse_simple_cpp() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
#include <iostream>
#include <vector>

namespace mylib {

class Base {
public:
    virtual void greet() {
        std::cout << "Base" << std::endl;
    }
};

class Derived : public Base {
public:
    void greet() override {
        std::cout << "Derived" << std::endl;
    }

    int compute(int x, int y) {
        return x + y;
    }
};

} // namespace mylib

int main() {
    mylib::Derived d;
    d.greet();
    d.compute(1, 2);
    return 0;
}
"#;
        let outcome = parser.parse("main.cpp", code, Language::Cpp).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();

        assert!(
            names.contains(&"mylib"),
            "missing namespace mylib, got: {:?}",
            names
        );
        assert!(names.contains(&"Base"), "missing Base, got: {:?}", names);
        assert!(
            names.contains(&"Derived"),
            "missing Derived, got: {:?}",
            names
        );
        assert!(names.contains(&"main"), "missing main, got: {:?}", names);

        // Check includes
        assert!(
            outcome.imports.len() >= 2,
            "should have at least 2 includes"
        );

        // Check inheritance
        assert!(
            !outcome.semantic_edges.is_empty(),
            "should detect inheritance"
        );
        let inherits = outcome
            .semantic_edges
            .iter()
            .find(|e| e.source_symbol == "Derived" && e.target_symbol == "Base");
        assert!(inherits.is_some(), "Derived should inherit from Base");
        assert_eq!(inherits.unwrap().relation_kind, SemanticRelation::Inherits);

        // Check calls
        assert!(!outcome.call_edges.is_empty(), "should have call edges");

        assert_eq!(outcome.parser_tier, ParserTier::TreeSitter);

        // Test file detection
        let test_outcome = parser.parse("main_test.cpp", code, Language::Cpp).unwrap();
        assert!(test_outcome.is_test_file);
    }

    #[test]
    fn parse_c_function_params() {
        let parser = CCppParser::new(Language::C);
        let code = r#"
int add(int a, int b) {
    return a + b;
}
"#;
        let outcome = parser.parse("math.c", code, Language::C).unwrap();
        let add = outcome.symbols.iter().find(|s| s.name == "add").unwrap();
        assert_eq!(add.kind, SymbolKind::Function);
        assert_eq!(add.param_count, Some(2));
        assert_eq!(add.return_type.as_deref(), Some("int"));
    }

    #[test]
    fn parse_cpp_method_in_class() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
class Calculator {
public:
    int add(int a, int b) {
        return a + b;
    }
};
"#;
        let outcome = parser.parse("calc.cpp", code, Language::Cpp).unwrap();
        let add = outcome.symbols.iter().find(|s| s.name == "add");
        assert!(add.is_some(), "should find method add");
        let add = add.unwrap();
        assert_eq!(add.kind, SymbolKind::Method);
        assert_eq!(add.container.as_deref(), Some("Calculator"));
    }

    #[test]
    fn parse_call_dispatch_kinds() {
        let parser = CCppParser::new(Language::C);
        let code = r#"
void helper() {}

struct Obj {
    int value;
};

int main() {
    helper();
    return 0;
}
"#;
        let outcome = parser.parse("calls.c", code, Language::C).unwrap();
        let direct = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "helper");
        assert!(direct.is_some(), "should have direct call to helper");
        assert_eq!(direct.unwrap().dispatch_kind, DispatchKind::Direct);
    }

    #[test]
    fn parse_emits_param_pass_and_return_flow() {
        let parser = CCppParser::new(Language::C);
        // `caller` passes its param into `callee(x)` (param_pass) and the call
        // result flows back (return_flow). Both functions are in-file/resolved.
        let code = r#"
int callee(int v) {
    return v + 1;
}

int caller(int x) {
    return callee(x);
}
"#;
        let outcome = parser.parse("flow.c", code, Language::C).unwrap();

        let param_pass: Vec<_> = outcome
            .data_flow_edges
            .iter()
            .filter(|e| e.flow_kind == "param_pass")
            .collect();
        assert!(
            !param_pass.is_empty(),
            "expected a param_pass data flow edge for callee(x)"
        );

        let return_flow: Vec<_> = outcome
            .data_flow_edges
            .iter()
            .filter(|e| e.flow_kind == "return_flow")
            .collect();
        assert!(
            !return_flow.is_empty(),
            "expected a return_flow data flow edge from callee back to caller"
        );
    }

    #[test]
    fn parse_cpp_new_expression_constructor() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
class Widget {
public:
    Widget(int n) {}
};

void factory() {
    Widget* w = new Widget(5);
    Widget* d = new Widget();
}
"#;
        let outcome = parser.parse("widget.cpp", code, Language::Cpp).unwrap();
        let ctors: Vec<&CallEdgeRecord> = outcome
            .call_edges
            .iter()
            .filter(|c| c.callee_symbol == "Widget" && c.is_constructor)
            .collect();
        assert_eq!(
            ctors.len(),
            2,
            "should detect two new Widget() constructor calls, got {}",
            ctors.len()
        );
        let with_arg = ctors.iter().find(|c| c.arg_count == Some(1)).unwrap();
        assert_eq!(with_arg.dispatch_kind, DispatchKind::Constructor);
        assert_eq!(with_arg.call_kind, "constructor");
        // The constructor call sits inside `factory`.
        assert_eq!(with_arg.caller_symbol.as_deref(), Some("factory"));
        // `new Widget(5)` should resolve to the class symbol in the same file.
        assert!(
            with_arg.callee_symbol_uid.is_some(),
            "constructor should resolve to in-file Widget symbol"
        );
    }

    #[test]
    fn parse_cpp_template_call_strips_args() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
template<typename T> T identity(T x) { return x; }

struct Box {
    template<typename T> T peek() { return T(); }
};

void use() {
    identity<int>(3);
    Box b;
    b.peek<int>();
}
"#;
        let outcome = parser.parse("tmpl.cpp", code, Language::Cpp).unwrap();
        // Bare generic call: callee must be `identity`, not `identity<int>`.
        let bare = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "identity");
        assert!(
            bare.is_some(),
            "template_function call should yield callee `identity`, got {:?}",
            outcome
                .call_edges
                .iter()
                .map(|c| c.callee_symbol.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(bare.unwrap().dispatch_kind, DispatchKind::Direct);
        assert!(
            !outcome
                .call_edges
                .iter()
                .any(|c| c.callee_symbol.contains('<')),
            "no callee should retain a template argument suffix"
        );
        // Member generic call: callee must be `peek`, not `peek<int>`.
        let member = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "peek");
        assert!(member.is_some(), "template_method call should yield `peek`");
        assert_eq!(member.unwrap().dispatch_kind, DispatchKind::Dynamic);
    }

    #[test]
    fn parse_cpp_qualified_call_name() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
namespace util {
    int compute(int x) { return x; }
}

void run() {
    util::compute(7);
}
"#;
        let outcome = parser.parse("ns.cpp", code, Language::Cpp).unwrap();
        // Qualified call `util::compute` should yield callee `compute` and resolve
        // to the in-file definition.
        let call = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "compute");
        assert!(call.is_some(), "should detect util::compute call");
        let call = call.unwrap();
        assert_eq!(call.dispatch_kind, DispatchKind::Direct);
        assert_eq!(call.caller_symbol.as_deref(), Some("run"));
        assert!(
            call.callee_symbol_uid.is_some(),
            "qualified call should resolve to in-file compute symbol"
        );
    }

    #[test]
    fn parse_cpp_arrow_method_call() {
        let parser = CCppParser::new(Language::Cpp);
        let code = r#"
struct Account {
    bool has_funds(int amount) { return true; }
    void withdraw(int amount) {
        this->has_funds(amount);
    }
};
"#;
        let outcome = parser.parse("acct.cpp", code, Language::Cpp).unwrap();
        // `this->has_funds(...)` is a field_expression call via `->`.
        let call = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "has_funds")
            .expect("should detect this->has_funds call");
        assert_eq!(call.dispatch_kind, DispatchKind::Dynamic);
        assert_eq!(call.caller_symbol.as_deref(), Some("withdraw"));
        assert_eq!(call.receiver_expr.as_deref(), Some("this"));
    }

    #[test]
    fn parse_c_function_pointer_member_call() {
        let parser = CCppParser::new(Language::C);
        let code = r#"
struct Ops {
    int (*run)(int);
};

int driver(struct Ops o) {
    return o.run(1);
}
"#;
        let outcome = parser.parse("ops.c", code, Language::C).unwrap();
        // `o.run(1)` is a field_expression call; callee is the field name `run`.
        let call = outcome
            .call_edges
            .iter()
            .find(|c| c.callee_symbol == "run")
            .expect("should detect o.run() function-pointer call");
        assert_eq!(call.dispatch_kind, DispatchKind::Dynamic);
        assert_eq!(call.receiver_expr.as_deref(), Some("o"));
        assert_eq!(call.caller_symbol.as_deref(), Some("driver"));
    }
}
