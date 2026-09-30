//! File-local declared interface extraction from the parser's existing AST.
//! No second parse, source execution, project I/O, or compiler claims.
pub(crate) mod conservative;
pub(crate) mod go;
pub(crate) mod jsts;
pub(crate) mod python;
pub(crate) mod rust;

use cc_model::{
    public_surface::{PublicSurface, SurfaceEntry, SurfaceToken, VisibilityDomain},
    ImportRecord,
};
use tree_sitter::Node;

pub(super) fn named(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}
pub(super) fn text<'a>(node: Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}
pub(super) fn field<'a>(node: Node<'_>, name: &str, source: &'a [u8]) -> &'a str {
    node.child_by_field_name(name)
        .map(|n| text(n, source))
        .unwrap_or("")
}
pub(super) fn is_comment(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "comment" | "line_comment" | "block_comment" | "doc_comment"
    )
}
pub(super) fn is_function(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "function"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "method_signature"
            | "abstract_method_signature"
    )
}

/// Omit only function executable bodies (plus the explicitly requested root
/// body). Preserve literal bytes, punctuation and parameter order, not trivia.
pub(super) fn signature(
    node: Node<'_>,
    source: &[u8],
    omit_root_body: bool,
    surface: &mut PublicSurface,
) -> Vec<SurfaceToken> {
    let mut tokens = Vec::new();
    let mut stack = vec![(node, 0usize)];
    let root_body = if omit_root_body {
        node.child_by_field_name("body").map(|n| n.id())
    } else {
        None
    };
    let mut visited = 0usize;
    while let Some((n, depth)) = stack.pop() {
        visited += 1;
        if visited > 100_000 || depth > 256 {
            surface.mark_unknown("surface_syntax_budget_exceeded");
            break;
        }
        if is_comment(n) || Some(n.id()) == root_body {
            continue;
        }
        if n.is_error() || n.is_missing() {
            surface.mark_unknown("syntax_error");
        }
        if matches!(n.kind(), "attribute_item" | "inner_attribute_item")
            && rust::attribute_needs_expansion(n, source)
        {
            surface.mark_unknown("attribute_expansion_not_evaluated");
        }
        // Includes associated const functions inside impls, not just top-level
        // declarations. An unevaluated const body may determine a public type.
        if n.kind() == "function_item"
            && named(n).iter().any(|m| {
                m.kind() == "function_modifiers"
                    && text(*m, source).split_whitespace().any(|w| w == "const")
            })
        {
            surface.mark_unknown("const_function_evaluation_not_modeled");
        }
        if matches!(n.kind(), "macro_invocation" | "macro_definition") {
            surface.mark_unknown("surface_macro_expansion_not_evaluated");
        }
        // Literal contents include meaningful spaces and comment-like text.
        let atomic = matches!(
            n.kind(),
            "string"
                | "string_literal"
                | "raw_string_literal"
                | "interpreted_string_literal"
                | "rune_literal"
                | "char_literal"
                | "template_string"
                | "regex"
        );
        if n.child_count() == 0 || atomic {
            tokens.push(SurfaceToken {
                kind: n.kind().into(),
                text: text(n, source).into(),
            });
        } else {
            let body = if is_function(n) {
                n.child_by_field_name("body").map(|c| c.id())
            } else {
                None
            };
            let mut cursor = n.walk();
            let children: Vec<_> = n.children(&mut cursor).collect();
            for child in children.into_iter().rev() {
                if Some(child.id()) != body {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
    tokens
}

pub(super) fn add_entry(
    surface: &mut PublicSurface,
    node: Node<'_>,
    source: &[u8],
    name: &str,
    exported: &str,
    visibility: VisibilityDomain,
    omit_body: bool,
) {
    let signature = signature(node, source, omit_body, surface);
    surface.entries.push(SurfaceEntry {
        qualified_name: name.into(),
        exported_name: exported.into(),
        kind: node.kind().into(),
        visibility,
        signature,
        conditions: vec![],
    });
}

/// Accept literal module/name strings without escapes/interpolation. Anything
/// requiring language-specific evaluation is explicitly outside this extractor.
pub(super) fn literal(node: Node<'_>, source: &[u8]) -> Option<String> {
    if !matches!(node.kind(), "string" | "string_literal") {
        return None;
    }
    let raw = text(node, source);
    let quote = raw.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || raw.len() < 2 || raw.as_bytes().last() != Some(&quote) {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    if inner.contains(['\\', '\n', '\r']) || inner.starts_with(['\'', '"']) {
        return None;
    }
    Some(inner.into())
}

/// Reuse resolved import records for forwarding closure. A source-level match
/// may conservatively mark multiple bindings from that source; never widen to
/// unrelated modules and never invent a resolved target.
pub(super) fn mark_forwards(surface: &PublicSurface, imports: &mut [ImportRecord]) {
    for import in imports {
        if surface
            .forwards
            .iter()
            .any(|f| f.source == import.import_string)
        {
            import.is_reexport = true;
        }
    }
}
