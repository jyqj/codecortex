//! Compact owned spans from a borrowed, already-parsed tree. Never invokes Parser.
use cc_model::{
    source::*,
    symbol::{SymbolKind, SymbolRecord},
};
use std::collections::BTreeMap;
use tree_sitter::{Node, Tree};
const MAX_NODES: usize = 250_000;
const MAX_BOUNDARIES: usize = 50_000;
const MAX_DEPTH: usize = 512;
fn symbol_kind(kind: &str) -> Option<SymbolKind> {
    Some(match kind {
        "lexical_declaration" | "variable_declaration" | "const_item" | "static_item" => {
            SymbolKind::Variable
        }
        "type_alias_declaration" | "type_item" => SymbolKind::TypeAlias,
        "function_definition"
        | "function_declaration"
        | "function_item"
        | "generator_function_declaration" => SymbolKind::Function,
        "method_definition" | "method_declaration" | "constructor_declaration" => {
            SymbolKind::Method
        }
        "class_definition" | "class_declaration" | "class_specifier" | "impl_item" => {
            SymbolKind::Class
        }
        "struct_item" | "struct_specifier" | "type_declaration" => SymbolKind::Class,
        "interface_declaration" | "trait_item" => SymbolKind::Interface,
        "enum_item" | "enum_declaration" | "enum_specifier" => SymbolKind::Enum,
        "mod_item" => SymbolKind::Module,
        "namespace_definition" => SymbolKind::Namespace,
        _ => return None,
    })
}
/// Parser labels may refine a declaration, but cannot change its AST family.
/// Python and Rust share callable node kinds between functions and methods;
/// the nearest lexical declaration supplies that distinction.
fn declaration_kind(node: Node<'_>) -> Option<SymbolKind> {
    let base = symbol_kind(node.kind())?;
    if matches!(node.kind(), "function_definition" | "function_item") {
        let mut parent = node.parent();
        while let Some(p) = parent {
            if matches!(
                p.kind(),
                "class_definition"
                    | "class_specifier"
                    | "struct_specifier"
                    | "impl_item"
                    | "trait_item"
            ) {
                return Some(SymbolKind::Method);
            }
            if symbol_kind(p.kind()).is_some() {
                break;
            }
            parent = p.parent();
        }
    }
    if matches!(node.kind(), "lexical_declaration" | "variable_declaration") && body(node).is_some()
    {
        return Some(SymbolKind::Function);
    }
    if node.kind() == "type_declaration" {
        let mut cursor = node.walk();
        let spec = node
            .named_children(&mut cursor)
            .find(|n| matches!(n.kind(), "type_spec" | "type_alias"));
        if let Some(spec) = spec {
            return Some(match spec.child_by_field_name("type").map(|n| n.kind()) {
                Some("struct_type") => SymbolKind::Class,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            });
        }
    }
    Some(base)
}
/// Follow only declaration-name edges, never return types or parameter children.
/// Unknown C/C++ declarators deliberately have no publishable name.
fn declarator_name(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..64 {
        node = match node.kind() {
            "identifier" | "field_identifier" | "destructor_name" | "operator_name" => {
                return Some(node);
            }
            "function_declarator" | "pointer_declarator" => {
                node.child_by_field_name("declarator")?
            }
            "qualified_identifier" | "template_function" | "template_method" => {
                node.child_by_field_name("name")?
            }
            "reference_declarator" => node.named_child(0)?,
            "parenthesized_declarator" => {
                let mut cursor = node.walk();
                let mut children = node
                    .named_children(&mut cursor)
                    .filter(|n| n.kind() != "ms_call_modifier");
                let child = children.next()?;
                if children.next().is_some() {
                    return None;
                }
                child
            }
            _ => return None,
        };
    }
    None
}
fn declaration_name(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "function_definition" {
        if let Some(declarator) = node.child_by_field_name("declarator") {
            return declarator_name(declarator);
        }
    }
    node.child_by_field_name("name")
        .or_else(|| {
            // Rust impl owners use their type; a callable's return type is never its name.
            (node.kind() == "impl_item")
                .then(|| node.child_by_field_name("type"))
                .flatten()
        })
        .or_else(|| {
            node.named_child(0)
                .filter(|n| matches!(n.kind(), "variable_declarator" | "type_spec" | "type_alias"))
                .and_then(|n| n.child_by_field_name("name"))
        })
}
fn compatible_hint(node: Node<'_>, hint: &SymbolRecord, source: &[u8]) -> bool {
    if node.kind() == "function_definition"
        && node.child_by_field_name("declarator").is_some()
        && declaration_name(node).is_none()
    {
        return false;
    }
    if declaration_name(node)
        .and_then(|n| n.utf8_text(source).ok())
        .is_some_and(|name| name != hint.name)
    {
        return false;
    }
    let expected = declaration_kind(node);
    expected == Some(hint.kind)
        // JS/TS function declarations inherit their parser's class container.
        || (matches!(node.kind(), "function_declaration" | "generator_function_declaration")
            && matches!(hint.kind, SymbolKind::Function | SymbolKind::Method))
        // A C++ out-of-class member is expressed through a qualified declarator.
        || (node.kind() == "function_definition" && hint.kind == SymbolKind::Method
            && node.child_by_field_name("declarator")
                .and_then(|n| n.child_by_field_name("declarator"))
                .is_some_and(|n| n.kind() == "qualified_identifier"))
        || (matches!(node.kind(), "const_item" | "static_item") && hint.kind == SymbolKind::Constant)
}
fn classify(kind: &str) -> Option<BoundaryKind> {
    // The declaration child owns the wrapper span; do not hide its identity
    // behind a same-span export statement chunk.
    if kind == "export_statement" {
        return None;
    }
    if symbol_kind(kind).is_some() {
        return Some(BoundaryKind::Symbol);
    }
    if matches!(
        kind,
        "block"
            | "statement_block"
            | "compound_statement"
            | "class_body"
            | "declaration_list"
            | "field_declaration_list"
            | "enum_body"
    ) {
        return Some(BoundaryKind::Block);
    }
    if matches!(
        kind,
        "if_statement"
            | "if_expression"
            | "switch_statement"
            | "switch_expression"
            | "match_statement"
            | "match_expression"
            | "for_statement"
            | "for_in_statement"
            | "for_expression"
            | "while_statement"
            | "while_expression"
            | "loop_expression"
            | "do_statement"
            | "try_statement"
            | "try_expression"
            | "with_statement"
            | "synchronized_statement"
            | "select_statement"
            | "type_switch_statement"
            | "expression_switch_statement"
    ) {
        return Some(BoundaryKind::Control);
    }
    if kind.ends_with("statement")
        || matches!(
            kind,
            "let_declaration"
                | "lexical_declaration"
                | "variable_declaration"
                | "const_declaration"
                | "var_declaration"
                | "field_declaration"
                | "attribute_item"
        )
    {
        return Some(BoundaryKind::Statement);
    }
    if matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "documentation_comment"
    ) {
        return Some(BoundaryKind::Comment);
    }
    None
}
fn is_doc(text: &str) -> bool {
    let t = text.trim_start();
    (t.starts_with("///") && !t.starts_with("////"))
        || (t.starts_with("/**") && !t.starts_with("/***"))
        || t.starts_with('#')
}
fn body(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body").or_else(|| {
        if !matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
            return None;
        }
        node.named_child(0)
            .filter(|n| n.kind() == "variable_declarator")
            .and_then(|n| n.child_by_field_name("value"))
            .filter(|n| {
                matches!(
                    n.kind(),
                    "arrow_function" | "function_expression" | "generator_function"
                )
            })
            .and_then(|n| n.child_by_field_name("body"))
    })
}
fn body_documentation(node: Node<'_>, source: &SourceSnapshot<'_>) -> Option<ByteSpan> {
    // Python's literal first statement. f-/b-strings and arbitrary expressions
    // are not docstrings; no import, execution or expression evaluation occurs.
    if !matches!(node.kind(), "function_definition" | "class_definition") {
        return None;
    }
    let statement = body(node)?.named_child(0)?;
    if statement.kind() != "expression_statement" {
        return None;
    }
    let literal = statement.named_child(0)?;
    if literal.kind() != "string" {
        return None;
    }
    let text = literal.utf8_text(source.bytes()).ok()?;
    let quote = text.find(['\'', '"'])?;
    if !matches!(&text[..quote], "" | "r" | "R" | "u" | "U") {
        return None;
    }
    Some(ByteSpan {
        start: statement.start_byte(),
        end: statement.end_byte(),
    })
}
fn associated_comment(node: Node<'_>, source: &SourceSnapshot<'_>) -> Option<ByteSpan> {
    let mut current = node;
    let mut start = node.start_byte();
    let mut found = false;
    for _ in 0..256 {
        let Some(prev) = current.prev_named_sibling() else {
            break;
        };
        let gap = source
            .slice(ByteSpan {
                start: prev.end_byte(),
                end: start,
            })
            .ok()?;
        if !gap.trim().is_empty() || gap.bytes().filter(|b| *b == b'\n').count() > 1 {
            break;
        }
        let text = source
            .slice(ByteSpan {
                start: prev.start_byte(),
                end: prev.end_byte(),
            })
            .ok()?;
        if classify(prev.kind()) != Some(BoundaryKind::Comment) || !is_doc(text) {
            break;
        }
        found = true;
        start = prev.start_byte();
        current = prev;
    }
    found.then_some(ByteSpan {
        start,
        end: node.start_byte(),
    })
}
pub fn extract(
    tree: &Tree,
    source: &SourceSnapshot<'_>,
    symbols: &[SymbolRecord],
) -> SourceStructure {
    let mut result = SourceStructure {
        source: source.identity().clone(),
        capability: "existing_tree_sitter_ast".into(),
        complete: !tree.root_node().has_error(),
        reasons: vec![],
        visited_nodes: 0,
        boundaries: vec![],
    };
    if !result.complete {
        result
            .reasons
            .push("syntax_error_partial_boundaries".into());
    }
    let names: BTreeMap<_, _> = symbols
        .iter()
        .map(|s| ((s.start_line, s.start_col), s))
        .collect();
    let mut cursor = tree.walk();
    let mut parents: Vec<Option<u32>> = vec![None];
    loop {
        result.visited_nodes += 1;
        if result.visited_nodes > MAX_NODES
            || result.boundaries.len() >= MAX_BOUNDARIES
            || parents.len() > MAX_DEPTH
        {
            result.complete = false;
            result.reasons.push("boundary_traversal_budget".into());
            break;
        }
        let node = cursor.node();
        let mut parent = *parents.last().unwrap();
        if node.is_named() && !node.is_missing() {
            if let Some(kind) = classify(node.kind()) {
                let mut owner = node;
                if kind != BoundaryKind::Comment {
                    if let Some(p) = node
                        .parent()
                        .filter(|p| matches!(p.kind(), "export_statement" | "decorated_definition"))
                    {
                        owner = p;
                    }
                }
                let span = ByteSpan {
                    start: owner.start_byte(),
                    end: owner.end_byte(),
                };
                if !span.is_empty() && source.slice(span).is_ok() {
                    let hint = names
                        .get(&(
                            (owner.start_position().row + 1) as u32,
                            owner.start_position().column as u32,
                        ))
                        .filter(|s| compatible_hint(node, s, source.bytes()))
                        .or_else(|| {
                            names
                                .get(&(
                                    (node.start_position().row + 1) as u32,
                                    node.start_position().column as u32,
                                ))
                                .filter(|s| compatible_hint(node, s, source.bytes()))
                        });
                    let name = if kind == BoundaryKind::Symbol {
                        hint.map(|s| s.name.clone()).or_else(|| {
                            declaration_name(node)
                                .and_then(|n| n.utf8_text(source.bytes()).ok())
                                .map(|s| s.chars().take(256).collect())
                        })
                    } else {
                        None
                    };
                    let signature = if kind == BoundaryKind::Symbol {
                        Some(ByteSpan {
                            start: span.start,
                            end: body(node)
                                .map_or(span.end, |b| b.start_byte())
                                .max(span.start),
                        })
                    } else {
                        None
                    };
                    let leading_comment = if kind == BoundaryKind::Symbol {
                        associated_comment(owner, source)
                    } else {
                        None
                    };
                    let record = SyntaxBoundary {
                        span,
                        parent,
                        kind,
                        name,
                        symbol_kind: hint.map(|s| s.kind).or_else(|| declaration_kind(node)),
                        signature,
                        leading_comment,
                        documentation: if kind == BoundaryKind::Symbol {
                            body_documentation(node, source)
                        } else {
                            None
                        },
                    };
                    parent = Some(result.boundaries.len() as u32);
                    result.boundaries.push(record);
                }
            }
        }
        if cursor.goto_first_child() {
            parents.push(parent);
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return result;
            }
            parents.pop();
        }
    }
    result
}
