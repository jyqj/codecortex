//! AST-authoritative calls and identifier uses for JS/TS and Python.
//!
//! Syntax existence and target resolution are separate contracts: only actual
//! call nodes create call facts. Local bindings are resolved in lexical scopes;
//! opaque/shadowed or ambiguous bindings are terminal misses, not invitations
//! to attach an unrelated globally unique name. This is not a type checker.

use cc_model::edge::{CallEdgeRecord, DispatchKind, ResolutionKind};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolRecord, SymbolRefRecord};
use cc_model::{ElementKind, Language, ParserTier};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Symbol(usize),
    Opaque,
    External,
}

struct Scope {
    parent: Option<usize>,
    owner: Option<usize>,
    class: bool,
    bindings: HashMap<String, Vec<Binding>>,
}

struct Facts<'a> {
    source: &'a [u8],
    file: &'a str,
    symbols: &'a [SymbolRecord],
    python: bool,
    scopes: Vec<Scope>,
    enter: HashMap<usize, usize>,
    definitions: HashSet<usize>,
    refs: Vec<SymbolRefRecord>,
    calls: Vec<CallEdgeRecord>,
}

pub(crate) fn extract(
    tree: &tree_sitter::Tree,
    content: &str,
    file: &str,
    symbols: &[SymbolRecord],
    language: Language,
) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
    let mut f = Facts {
        source: content.as_bytes(),
        file,
        symbols,
        python: language == Language::Python,
        scopes: vec![Scope {
            parent: None,
            owner: None,
            class: false,
            bindings: HashMap::new(),
        }],
        enter: HashMap::new(),
        definitions: HashSet::new(),
        refs: Vec::new(),
        calls: Vec::new(),
    };
    f.collect(tree.root_node(), 0);
    f.emit(tree.root_node(), 0);
    (f.refs, f.calls)
}

fn callable(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "generator_function_declaration"
            | "function_definition"
            | "function_expression"
            | "generator_function"
            | "method_definition"
            | "arrow_function"
            | "lambda"
    )
}

impl Facts<'_> {
    fn text(&self, node: Node<'_>) -> &str {
        node.utf8_text(self.source).unwrap_or("")
    }

    fn bind(&mut self, scope: usize, name: Node<'_>, binding: Binding) {
        self.definitions.insert(name.id());
        let text = self.text(name).to_string();
        if !text.is_empty() {
            let bindings = self.scopes[scope].bindings.entry(text).or_default();
            if !bindings.contains(&binding) {
                bindings.push(binding);
            }
        }
    }

    // Bind patterns, not their annotations/default-value expressions.
    fn pattern(&mut self, node: Node<'_>, scope: usize, binding: Binding) {
        match node.kind() {
            "identifier" | "shorthand_property_identifier_pattern" => {
                self.bind(scope, node, binding)
            }
            "type_annotation" | "type" => {}
            "assignment_pattern"
            | "assignment"
            | "default_parameter"
            | "typed_default_parameter" => {
                if let Some(n) = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("name"))
                {
                    self.pattern(n, scope, binding);
                }
            }
            "pair_pattern" => {
                if let Some(n) = node.child_by_field_name("value") {
                    self.pattern(n, scope, binding);
                }
            }
            "typed_parameter" | "required_parameter" | "optional_parameter" => {
                if let Some(n) = node
                    .child_by_field_name("pattern")
                    .or_else(|| node.child_by_field_name("name"))
                    .or_else(|| node.named_child(0))
                {
                    self.pattern(n, scope, binding);
                }
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.pattern(child, scope, binding);
                }
            }
        }
    }

    fn definition_symbol(&self, node: Node<'_>, name: Option<&str>) -> Option<usize> {
        let start = (
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let end = (
            node.end_position().row as u32 + 1,
            node.end_position().column as u32,
        );
        self.symbols
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                name.is_none_or(|n| s.name == n)
                    && (s.start_line, s.start_col) <= start
                    && (s.end_line, s.end_col) == end
            })
            .min_by_key(|(_, s)| {
                (
                    s.end_line - s.start_line,
                    s.end_col.saturating_sub(s.start_col),
                )
            })
            .map(|(i, _)| i)
    }

    fn new_scope(
        &mut self,
        node: Node<'_>,
        parent: usize,
        owner: Option<usize>,
        class: bool,
    ) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            parent: Some(parent),
            owner,
            class,
            bindings: HashMap::new(),
        });
        self.enter.insert(node.id(), id);
        id
    }

    fn collect(&mut self, node: Node<'_>, mut scope: usize) {
        if node.is_error() || node.is_missing() {
            return;
        }
        let kind = node.kind();
        if callable(kind) {
            let name = node.child_by_field_name("name");
            let sym = self.definition_symbol(node, name.map(|n| self.text(n)));
            let binding = sym.map(Binding::Symbol).unwrap_or(Binding::Opaque);
            let declaration = matches!(
                kind,
                "function_declaration" | "generator_function_declaration" | "function_definition"
            );
            if let Some(n) = name {
                self.definitions.insert(n.id());
                if declaration {
                    self.bind(scope, n, binding);
                }
            }
            let parent = scope;
            scope = self.new_scope(node, parent, sym, false);
            if !declaration && kind != "method_definition" {
                if let Some(n) = name {
                    self.bind(scope, n, binding);
                }
            }
            if let Some(params) = node
                .child_by_field_name("parameters")
                .or_else(|| node.child_by_field_name("parameter"))
            {
                self.pattern(params, scope, Binding::Opaque);
            }
            // Python defaults/annotations execute in the enclosing scope, not
            // the function body; decorators are outside function_definition.
            if self.python {
                self.enter.remove(&node.id());
                if let Some(body) = node.child_by_field_name("body") {
                    self.enter.insert(body.id(), scope);
                }
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.collect(
                        child,
                        if Some(child) == node.child_by_field_name("body") {
                            scope
                        } else {
                            parent
                        },
                    );
                }
                return;
            }
        } else if matches!(kind, "class_declaration" | "class_definition") {
            if let Some(name) = node.child_by_field_name("name") {
                let binding = self
                    .definition_symbol(node, Some(self.text(name)))
                    .map(Binding::Symbol)
                    .unwrap_or(Binding::Opaque);
                self.bind(scope, name, binding);
            }
            scope = self.new_scope(node, scope, None, true);
        } else if !self.python && kind == "statement_block" {
            // JS let/const bindings are block-scoped. A block inherits the
            // callable owner, but does not invent a new caller identity.
            let owner = self.scopes[scope].owner;
            scope = self.new_scope(node, scope, owner, false);
        } else if kind == "variable_declarator" {
            if let Some(name) = node.child_by_field_name("name") {
                let value = node.child_by_field_name("value");
                let commonjs = value.is_some_and(|n| {
                    n.kind() == "call_expression"
                        && n.child_by_field_name("function")
                            .is_some_and(|f| self.text(f) == "require")
                });
                let binding = if commonjs {
                    Binding::External
                } else {
                    value
                        .filter(|n| callable(n.kind()))
                        .and_then(|n| self.definition_symbol(n, Some(self.text(name))))
                        .map(Binding::Symbol)
                        .unwrap_or(Binding::Opaque)
                };
                // `var` is function scoped, unlike `let` and `const`.
                let mut target_scope = scope;
                if node
                    .parent()
                    .is_some_and(|p| p.kind() == "variable_declaration")
                {
                    while let Some(parent) = self.scopes[target_scope].parent {
                        if self.scopes[parent].owner != self.scopes[target_scope].owner {
                            break;
                        }
                        target_scope = parent;
                    }
                }
                self.pattern(name, target_scope, binding);
            }
        } else if self.python && matches!(kind, "assignment" | "for_statement" | "named_expression")
        {
            if let Some(left) = node
                .child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name"))
            {
                // Attribute and subscript assignments don't bind a local name.
                if !matches!(left.kind(), "attribute" | "subscript") {
                    self.pattern(left, scope, Binding::Opaque);
                }
            }
        } else if matches!(kind, "import_statement" | "import_from_statement") {
            self.collect_import(node, scope);
            return;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.collect(child, scope);
        }
    }

    fn collect_import(&mut self, node: Node<'_>, scope: usize) {
        // Bind only the LOCAL side of an import. In `f as g`, f is not a
        // local declaration and must not shadow a same-file f.
        let mut cursor = node.walk();
        if self.python {
            for child in node.named_children(&mut cursor) {
                if Some(child) == node.child_by_field_name("module_name") {
                    continue;
                }
                if child.kind() == "aliased_import" {
                    if let Some(alias) = child.child_by_field_name("alias") {
                        self.bind(scope, alias, Binding::External);
                    }
                } else if matches!(child.kind(), "dotted_name" | "identifier") {
                    let local = if child.kind() == "dotted_name" {
                        child.named_child(0).unwrap_or(child)
                    } else {
                        child
                    };
                    self.bind(scope, local, Binding::External);
                }
            }
        } else {
            for clause in node
                .named_children(&mut cursor)
                .filter(|n| n.kind() == "import_clause")
            {
                let mut c = clause.walk();
                for part in clause.named_children(&mut c) {
                    match part.kind() {
                        "identifier" => self.bind(scope, part, Binding::External),
                        "namespace_import" => {
                            if let Some(id) = part.named_child(0) {
                                self.bind(scope, id, Binding::External);
                            }
                        }
                        "named_imports" => {
                            let mut sc = part.walk();
                            for spec in part.named_children(&mut sc) {
                                if let Some(local) = spec
                                    .child_by_field_name("alias")
                                    .or_else(|| spec.child_by_field_name("name"))
                                {
                                    self.bind(scope, local, Binding::External);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn resolve(&self, name: &str, mut scope: usize) -> (Option<usize>, &'static str) {
        loop {
            let s = &self.scopes[scope];
            // Class attributes are not lexical variables captured by methods.
            if !s.class {
                if let Some(bindings) = s.bindings.get(name) {
                    return match bindings.as_slice() {
                        [Binding::Symbol(i)] => (Some(*i), "parser_exact"),
                        [Binding::External] => (None, "unresolved"),
                        [Binding::Opaque] => (None, "parser_shadowed"),
                        _ => (None, "parser_ambiguous"),
                    };
                }
            }
            match s.parent {
                Some(p) => scope = p,
                None => return (None, "unresolved"),
            }
        }
    }

    fn reference(
        &mut self,
        node: Node<'_>,
        name: String,
        scope: usize,
        call: bool,
        dynamic: bool,
    ) -> SymbolRefRecord {
        let (target, strategy) = if dynamic {
            (None, "parser_dynamic")
        } else {
            self.resolve(&name, scope)
        };
        let target = target.map(|i| &self.symbols[i]);
        let owner = self.scopes[scope].owner.map(|i| &self.symbols[i]);
        let line = node.start_position().row as u32 + 1;
        let column = node.start_position().column as u32;
        SymbolRefRecord {
            ref_id: StableId::ref_id(self.file, &name, line, column),
            file_path: self.file.to_string(),
            symbol_name: name.clone(),
            container: owner.and_then(|s| s.qname.clone()),
            ref_kind: if call { "call" } else { "identifier" }.into(),
            line,
            column,
            target_symbol_id: target.map(|s| s.symbol_id.clone()),
            target_file_path: target.map(|s| s.file_path.clone()),
            target_symbol_uid: target.and_then(|s| s.symbol_uid.clone()),
            ref_name: Some(name),
            scope_id: owner.and_then(|s| s.scope_id.clone()),
            resolution_kind: if target.is_some() {
                ResolutionKind::Exact
            } else {
                ResolutionKind::Unresolved
            },
            resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
            resolution_strategy: strategy.into(),
            ref_end_line: Some(node.end_position().row as u32 + 1),
            ref_end_col: Some(node.end_position().column as u32),
            parser_tier: ParserTier::Semantic,
            parser_confidence: ParserTier::Semantic.element_confidence(if call {
                ElementKind::CallRef
            } else {
                ElementKind::IdentifierRef
            }),
        }
    }

    fn emit(&mut self, node: Node<'_>, mut scope: usize) {
        if node.is_error() || node.is_missing() {
            return;
        }
        if let Some(s) = self.enter.get(&node.id()) {
            scope = *s;
        }
        if matches!(node.kind(), "import_statement" | "import_from_statement") {
            return;
        }
        let mut skip_callee = None;
        if matches!(node.kind(), "call" | "call_expression" | "new_expression") && !node.has_error()
        {
            if let Some(callee) = node
                .child_by_field_name("function")
                .or_else(|| node.child_by_field_name("constructor"))
            {
                if matches!(
                    callee.kind(),
                    "identifier"
                        | "member_expression"
                        | "attribute"
                        | "subscript_expression"
                        | "subscript"
                ) {
                    let name = self.text(callee).to_string();
                    let dynamic = matches!(callee.kind(), "subscript_expression" | "subscript");
                    let r = self.reference(callee, name.clone(), scope, true, dynamic);
                    let owner = self.scopes[scope].owner.map(|i| &self.symbols[i]);
                    let receiver = callee
                        .child_by_field_name("object")
                        .or_else(|| callee.child_by_field_name("value"))
                        .map(|n| self.text(n).to_string());
                    let optional = self.text(callee).contains("?.");
                    let constructor = node.kind() == "new_expression";
                    let awaited = node
                        .parent()
                        .is_some_and(|p| matches!(p.kind(), "await_expression" | "await"));
                    self.calls.push(CallEdgeRecord {
                        edge_id: StableId::edge_id(
                            "call",
                            self.file,
                            node.start_position().row as u32 + 1,
                            node.start_position().column as u32,
                        ),
                        file_path: self.file.into(),
                        caller_symbol: owner.map(|s| s.name.clone()),
                        callee_symbol: name,
                        line: node.start_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_line: Some(node.end_position().row as u32 + 1),
                        end_col: node.end_position().column as u32,
                        target_symbol_id: r.target_symbol_id.clone(),
                        target_file_path: r.target_file_path.clone(),
                        caller_symbol_id: owner.map(|s| s.symbol_id.clone()),
                        caller_symbol_uid: owner.and_then(|s| s.symbol_uid.clone()),
                        callee_symbol_uid: r.target_symbol_uid.clone(),
                        callee_ref_id: Some(r.ref_id.clone()),
                        dispatch_kind: if optional {
                            DispatchKind::OptionalChain
                        } else if receiver.is_some() || dynamic {
                            DispatchKind::Dynamic
                        } else {
                            DispatchKind::Direct
                        },
                        call_kind: if constructor {
                            "constructor"
                        } else if receiver.is_some() {
                            "member"
                        } else {
                            "direct"
                        }
                        .into(),
                        resolution_kind: r.resolution_kind,
                        resolution_confidence: r.resolution_confidence,
                        resolution_strategy: r.resolution_strategy.clone(),
                        receiver_expr: receiver,
                        arg_count: node
                            .child_by_field_name("arguments")
                            .map(|n| n.named_child_count() as u32),
                        is_optional_chain: optional,
                        is_awaited: awaited,
                        is_constructor: constructor,
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.85,
                        synthesized_by: None,
                        synthesis_key: None,
                        registered_file: None,
                        registered_line: None,
                    });
                    self.refs.push(r);
                    skip_callee = Some(callee.id());
                }
            }
        } else if node.kind() == "identifier" && !self.definitions.contains(&node.id()) {
            let property = node.parent().is_some_and(|p| {
                (matches!(p.kind(), "attribute" | "member_expression")
                    && p.child_by_field_name("attribute")
                        .or_else(|| p.child_by_field_name("property"))
                        == Some(node))
                    || (p.kind() == "pair" && p.child_by_field_name("key") == Some(node))
            });
            if !property {
                let r = self.reference(node, self.text(node).to_string(), scope, false, false);
                self.refs.push(r);
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            // A direct callee already has its call ref. Still descend into a
            // member receiver: factory().run() contains an independent call.
            if Some(child.id()) == skip_callee && child.kind() == "identifier" {
                continue;
            }
            self.emit(child, scope);
        }
    }
}
