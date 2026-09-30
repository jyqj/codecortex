//! AST-backed call sites and lexical binding evidence. Never scan string or
//! comment text as code. Binding evidence is conservative, not runtime execution.
use cc_model::{
    CallEdgeRecord, ElementKind, ParserTier, ResolutionKind, StableId, SymbolRecord,
    SymbolRefRecord,
};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};
const UNSUPPORTED: &str = cc_model::resolution::PARSER_UNSUPPORTED_BINDING;

#[derive(Default)]
struct Scope<'s> {
    definitions: HashMap<String, Option<&'s SymbolRecord>>,
    dynamic: HashSet<String>,
    class: bool,
}
struct Extractor<'a> {
    source: &'a [u8],
    path: &'a str,
    symbols: HashMap<(u32, String), &'a SymbolRecord>,
    scopes: Vec<Scope<'a>>,
    refs: Vec<SymbolRefRecord>,
    calls: Vec<CallEdgeRecord>,
}
fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}
fn names(node: Node<'_>, source: &[u8], output: &mut HashSet<String>) {
    if node.kind() == "identifier" {
        output.insert(text(node, source).into());
        return;
    }
    if matches!(node.kind(), "attribute" | "subscript" | "type") {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        names(child, source, output);
    }
}
impl<'a> Extractor<'a> {
    fn bindings(&self, node: Node<'_>, scope: &mut Scope<'a>) {
        match node.kind() {
            "function_definition" | "class_definition" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let name = text(name, self.source).to_string();
                    let sym = self
                        .symbols
                        .get(&(node.start_position().row as u32 + 1, name.clone()))
                        .copied();
                    scope
                        .definitions
                        .entry(name)
                        .and_modify(|s| *s = None)
                        .or_insert(sym);
                }
                return;
            }
            "decorated_definition" => {
                if let Some(definition) = node.child_by_field_name("definition") {
                    if let Some(name) = definition.child_by_field_name("name") {
                        scope.dynamic.insert(text(name, self.source).into());
                    }
                }
                return;
            }
            "lambda" => return,
            "assignment" | "augmented_assignment" | "named_expression" => {
                if let Some(left) = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("name"))
                {
                    names(left, self.source, &mut scope.dynamic);
                }
            }
            "for_statement" | "for_in_clause" => {
                if let Some(left) = node.child_by_field_name("left") {
                    names(left, self.source, &mut scope.dynamic);
                }
            }
            "global_statement" | "nonlocal_statement" => {
                names(node, self.source, &mut scope.dynamic)
            }
            // Import routes are resolved by the shared import resolver. Do not
            // call imported names parser-exact solely from the symbol catalog.
            "import_statement" | "import_from_statement" => return,
            "as_pattern" => {
                if let Some(alias) = node.child_by_field_name("alias") {
                    names(alias, self.source, &mut scope.dynamic);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.bindings(child, scope);
        }
    }
    fn parameter_names(&self, node: Node<'_>, out: &mut HashSet<String>) {
        match node.kind() {
            "identifier" => {
                out.insert(text(node, self.source).into());
            }
            "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
                if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| node.named_child(0))
                {
                    names(name, self.source, out);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.parameter_names(child, out);
                }
            }
        }
    }
    fn header_expressions(&mut self, node: Node<'_>, owner: Option<&'a SymbolRecord>) {
        if matches!(node.kind(), "default_parameter" | "typed_default_parameter") {
            if let Some(value) = node.child_by_field_name("value") {
                self.walk(value, owner);
            }
        }
        if let Some(annotation) = node.child_by_field_name("type") {
            self.walk(annotation, owner);
        }
        if matches!(node.kind(), "parameters" | "lambda_parameters") {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                self.header_expressions(child, owner);
            }
        }
    }
    fn lookup(&self, name: &str) -> (Option<&'a SymbolRecord>, bool) {
        // A method's bare name lookup does not traverse its class namespace.
        for scope in self.scopes.iter().rev().filter(|s| !s.class) {
            if scope.dynamic.contains(name) {
                return (None, true);
            }
            if let Some(sym) = scope.definitions.get(name) {
                return (*sym, sym.is_none());
            }
        }
        (None, false)
    }
    fn reference(
        &mut self,
        node: Node<'_>,
        owner: Option<&'a SymbolRecord>,
        name: &str,
        kind: &str,
        allow_binding: bool,
    ) -> SymbolRefRecord {
        let (target, blocked) = if allow_binding {
            self.lookup(name)
        } else {
            (None, false)
        };
        let start = node.start_position();
        let end = node.end_position();
        SymbolRefRecord {
            ref_id: StableId::ref_id(self.path, name, start.row as u32 + 1, start.column as u32),
            file_path: self.path.into(),
            symbol_name: name.into(),
            container: owner.and_then(|s| s.qname.clone()),
            ref_kind: kind.into(),
            line: start.row as u32 + 1,
            column: start.column as u32,
            target_symbol_id: target.map(|s| s.symbol_id.clone()),
            target_file_path: target.map(|_| self.path.into()),
            target_symbol_uid: target.and_then(|s| s.symbol_uid.clone()),
            ref_name: Some(name.into()),
            scope_id: owner.and_then(|s| s.scope_id.clone()),
            resolution_kind: if target.is_some() {
                ResolutionKind::Exact
            } else {
                ResolutionKind::Unresolved
            },
            resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
            resolution_strategy: if target.is_some() {
                "parser_exact"
            } else if blocked {
                UNSUPPORTED
            } else {
                "unresolved"
            }
            .into(),
            ref_end_line: Some(end.row as u32 + 1),
            ref_end_col: Some(end.column as u32),
            parser_tier: ParserTier::Semantic,
            parser_confidence: ParserTier::Semantic.element_confidence(if kind == "call" {
                ElementKind::CallRef
            } else {
                ElementKind::IdentifierRef
            }),
        }
    }
    fn walk(&mut self, node: Node<'_>, owner: Option<&'a SymbolRecord>) {
        match node.kind() {
            "comment" | "string_content" | "string_start" | "string_end" => return,
            "function_definition" | "class_definition" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|n| text(n, self.source))
                    .unwrap_or("");
                let symbol = self
                    .symbols
                    .get(&(node.start_position().row as u32 + 1, name.into()))
                    .copied();
                let is_class = node.kind() == "class_definition";
                let mut scope = Scope {
                    class: is_class,
                    ..Default::default()
                };
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.header_expressions(params, owner);
                    self.parameter_names(params, &mut scope.dynamic);
                }
                if let Some(annotation) = node.child_by_field_name("return_type") {
                    self.walk(annotation, owner);
                }
                if let Some(bases) = node.child_by_field_name("superclasses") {
                    self.walk(bases, owner);
                }
                if let Some(body) = node.child_by_field_name("body") {
                    self.bindings(body, &mut scope);
                    self.scopes.push(scope);
                    self.walk(body, if is_class { None } else { symbol });
                    self.scopes.pop();
                }
                return;
            }
            "lambda" => {
                let mut scope = Scope::default();
                if let Some(params) = node.child_by_field_name("parameters") {
                    self.header_expressions(params, owner);
                    self.parameter_names(params, &mut scope.dynamic);
                }
                if let Some(annotation) = node.child_by_field_name("return_type") {
                    self.walk(annotation, owner);
                }
                if let Some(bases) = node.child_by_field_name("superclasses") {
                    self.walk(bases, owner);
                }
                self.scopes.push(scope);
                if let Some(body) = node.child_by_field_name("body") {
                    self.walk(body, None);
                }
                self.scopes.pop();
                return;
            }
            "import_statement"
            | "import_from_statement"
            | "global_statement"
            | "nonlocal_statement" => return,
            "call" => {
                if let Some(function) = node.child_by_field_name("function") {
                    let dynamic = !matches!(function.kind(), "identifier" | "attribute");
                    let token = if function.kind() == "attribute" {
                        function
                            .child_by_field_name("attribute")
                            .unwrap_or(function)
                    } else if dynamic {
                        node.child_by_field_name("arguments").unwrap_or(function)
                    } else {
                        function
                    };
                    {
                        let name = text(function, self.source);
                        let mut r = self.reference(
                            token,
                            owner,
                            name,
                            "call",
                            function.kind() == "identifier",
                        );
                        if dynamic {
                            r.resolution_strategy = UNSUPPORTED.into();
                        }
                        let receiver = function
                            .child_by_field_name("object")
                            .map(|n| text(n, self.source).to_string());
                        self.calls.push(CallEdgeRecord {
                            edge_id: StableId::edge_id("call", self.path, r.line, r.column),
                            file_path: self.path.into(),
                            caller_symbol: owner.map(|s| s.name.clone()),
                            callee_symbol: name.into(),
                            line: r.line,
                            start_col: r.column,
                            end_line: r.ref_end_line,
                            end_col: r.ref_end_col.unwrap_or(r.column),
                            target_symbol_id: r.target_symbol_id.clone(),
                            target_file_path: r.target_file_path.clone(),
                            caller_symbol_id: owner.map(|s| s.symbol_id.clone()),
                            caller_symbol_uid: owner.and_then(|s| s.symbol_uid.clone()),
                            callee_symbol_uid: r.target_symbol_uid.clone(),
                            callee_ref_id: Some(r.ref_id.clone()),
                            resolution_kind: r.resolution_kind,
                            resolution_confidence: r.resolution_confidence,
                            resolution_strategy: r.resolution_strategy.clone(),
                            call_kind: "direct".into(),
                            receiver_expr: receiver,
                            arg_count: node
                                .child_by_field_name("arguments")
                                .map(|n| n.named_child_count() as u32),
                            parser_tier: ParserTier::Semantic,
                            parser_confidence: ParserTier::Semantic
                                .element_confidence(ElementKind::CallEdge),
                            ..Default::default()
                        });
                        self.refs.push(r);
                    }
                    // Nested callable expressions still contain actual calls.
                    if function.kind() != "identifier" {
                        self.walk(function, owner);
                    }
                }
                if let Some(args) = node.child_by_field_name("arguments") {
                    self.walk(args, owner);
                }
                return;
            }
            "attribute" => {
                if let Some(object) = node.child_by_field_name("object") {
                    self.walk(object, owner);
                }
                return;
            }
            "assignment" | "augmented_assignment" | "named_expression" => {
                if let Some(left) = node.child_by_field_name("left") {
                    if matches!(left.kind(), "attribute" | "subscript") {
                        self.walk(left, owner);
                    }
                }
                if let Some(right) = node
                    .child_by_field_name("right")
                    .or_else(|| node.child_by_field_name("value"))
                {
                    self.walk(right, owner);
                }
                return;
            }
            "keyword_argument" => {
                if let Some(value) = node.child_by_field_name("value") {
                    self.walk(value, owner);
                }
                return;
            }
            "identifier" => {
                if owner.is_some() {
                    let name = text(node, self.source);
                    let r = self.reference(node, owner, name, "identifier", true);
                    self.refs.push(r);
                }
                return;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, owner);
        }
    }
}
pub(super) fn extract(
    tree: &Tree,
    source: &[u8],
    path: &str,
    symbols: &[SymbolRecord],
) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
    let mut e = Extractor {
        source,
        path,
        symbols: symbols
            .iter()
            .map(|s| ((s.start_line, s.name.clone()), s))
            .collect(),
        scopes: Vec::new(),
        refs: Vec::new(),
        calls: Vec::new(),
    };
    let mut root = Scope::default();
    e.bindings(tree.root_node(), &mut root);
    e.scopes.push(root);
    e.walk(tree.root_node(), None);
    e.refs.sort_by_key(|r| (r.line, r.column, r.ref_id.clone()));
    e.calls
        .sort_by_key(|c| (c.line, c.start_col, c.edge_id.clone()));
    (e.refs, e.calls)
}
