//! B1 only: same-file prior declarations and global ordinary qualified names.
//! No relative lookup, type nesting, aliases, templates or cross-file inference.
use cc_model::{
    cpp_owner::{CppQualifiedOwnerProof, CppQualifiedOwnerState as State},
    id::StableId,
    source::{ByteSpan, SourceSnapshot},
    symbol::{SymbolKind, SymbolRecord},
};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

const MAX_NODES: usize = 250_000;
const MAX_OWNERS: usize = 50_000;
const MAX_SEGMENTS: usize = 64;
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerKind {
    Namespace,
    Type,
    Unsupported,
}
struct Owner {
    kind: OwnerKind,
    witness: Option<ByteSpan>,
    ambiguous: bool,
}
struct Catalog {
    entries: HashMap<Vec<String>, Owner>,
    complete: bool,
    visited: usize,
    owner_limit: usize,
}
impl Catalog {
    fn new(tree: &Tree, source: &[u8]) -> Self {
        let mut catalog = Self {
            entries: HashMap::new(),
            complete: !tree.root_node().has_error(),
            visited: 0,
            owner_limit: MAX_OWNERS,
        };
        let mut pending = vec![(tree.root_node(), Vec::<String>::new())];
        while let Some((scope, prefix)) = pending.pop() {
            let mut cursor = scope.walk();
            for node in scope.named_children(&mut cursor) {
                catalog.visited += 1;
                if catalog.visited > MAX_NODES || catalog.entries.len() >= catalog.owner_limit {
                    catalog.complete = false;
                    return catalog;
                }
                match node.kind() {
                    "namespace_definition" => {
                        let Some(name) = node.child_by_field_name("name") else {
                            // Anonymous namespaces introduce implicit visibility
                            // that this bounded owner census does not resolve.
                            catalog.complete = false;
                            continue;
                        };
                        let Some(parts) = namespace_parts(name, source) else {
                            catalog.complete = false;
                            continue;
                        };
                        if prefix.len() + parts.len() > MAX_SEGMENTS {
                            catalog.complete = false;
                            continue;
                        }
                        let mut path = prefix.clone();
                        for part in parts {
                            path.push(part);
                            catalog.insert(path.clone(), OwnerKind::Namespace, Some(span(node)));
                        }
                        if let Some(body) = node.child_by_field_name("body") {
                            pending.push((body, path));
                        }
                    }
                    "class_specifier" | "struct_specifier" => {
                        let Some(name) = node.child_by_field_name("name") else {
                            continue;
                        };
                        if !matches!(name.kind(), "type_identifier" | "identifier") {
                            catalog.complete = false;
                            continue;
                        }
                        let Some(name) = name.utf8_text(source).ok() else {
                            catalog.complete = false;
                            continue;
                        };
                        let mut path = prefix.clone();
                        path.push(name.into());
                        if path.len() > MAX_SEGMENTS {
                            catalog.complete = false;
                            continue;
                        }
                        catalog.insert(
                            path,
                            OwnerKind::Type,
                            node.child_by_field_name("body").map(|_| span(node)),
                        );
                    }
                    "declaration"
                    | "function_definition"
                    | "type_definition"
                    | "enum_specifier"
                    | "union_specifier"
                    | "namespace_alias_definition"
                    | "alias_declaration"
                    | "template_declaration" => {
                        catalog.negative_claims(node, &prefix, source, 0);
                    }
                    // A conditional can hide a conflicting claim. No partial
                    // catalog may prove B1 owners until preprocessing is supported.
                    "preproc_if"
                    | "preproc_ifdef"
                    | "using_declaration"
                    | "linkage_specification" => catalog.complete = false,
                    _ => {}
                }
            }
        }
        catalog
    }
    /// Unsupported wrappers may still contain a competing owner declaration.
    /// This census never creates a positive witness or enters a type/function body.
    fn negative_claims(&mut self, node: Node<'_>, prefix: &[String], source: &[u8], depth: usize) {
        self.visited += 1;
        if depth > 16 || self.visited > MAX_NODES || self.entries.len() >= self.owner_limit {
            self.complete = false;
            return;
        }
        match node.kind() {
            "declaration" | "function_definition" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.negative_claims(ty, prefix, source, depth + 1);
                }
            }
            "type_definition" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.negative_claims(ty, prefix, source, depth + 1);
                }
                let mut cursor = node.walk();
                let mut count = 0;
                for declarator in node.children_by_field_name("declarator", &mut cursor) {
                    count += 1;
                    self.negative_name(declarator, prefix, source, OwnerKind::Unsupported);
                }
                if count == 0 {
                    self.complete = false;
                }
            }
            "template_declaration" => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    let ty = if child.kind() == "declaration" {
                        child.child_by_field_name("type")
                    } else {
                        Some(child)
                    };
                    if let Some(ty) =
                        ty.filter(|n| matches!(n.kind(), "class_specifier" | "struct_specifier"))
                    {
                        if let Some(name) = ty.child_by_field_name("name") {
                            // Even a template forward claim conflicts with an
                            // ordinary type; it is not a compatible redeclaration.
                            self.negative_name(name, prefix, source, OwnerKind::Unsupported);
                        }
                    } else {
                        self.negative_claims(child, prefix, source, depth + 1);
                    }
                }
            }
            "class_specifier"
            | "struct_specifier"
            | "enum_specifier"
            | "union_specifier"
            | "namespace_alias_definition"
            | "alias_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let kind = if matches!(node.kind(), "class_specifier" | "struct_specifier")
                        && node.child_by_field_name("body").is_none()
                    {
                        OwnerKind::Type
                    } else {
                        OwnerKind::Unsupported
                    };
                    self.negative_name(name, prefix, source, kind);
                }
            }
            _ => {}
        }
    }
    fn negative_name(&mut self, node: Node<'_>, prefix: &[String], source: &[u8], kind: OwnerKind) {
        if !matches!(
            node.kind(),
            "identifier" | "type_identifier" | "namespace_identifier"
        ) || prefix.len() >= MAX_SEGMENTS
        {
            // A specialization/complex declarator is not an unrelated literal
            // key. Unknown conflict knowledge invalidates positive B1 proof.
            self.complete = false;
            return;
        }
        let Ok(name) = node.utf8_text(source) else {
            self.complete = false;
            return;
        };
        let mut path = prefix.to_vec();
        path.push(name.into());
        self.insert(path, kind, None);
    }
    fn insert(&mut self, path: Vec<String>, kind: OwnerKind, witness: Option<ByteSpan>) {
        if self.entries.len() >= self.owner_limit && !self.entries.contains_key(&path) {
            self.complete = false;
            return;
        }
        self.entries
            .entry(path)
            .and_modify(|entry| {
                if entry.kind != kind
                    || (kind == OwnerKind::Type && entry.witness.is_some() && witness.is_some())
                {
                    entry.ambiguous = true;
                }
                // For reopened namespaces retain the first completed declaration.
                if let Some(span) = witness {
                    if entry.witness.is_none_or(|old| span.end < old.end) {
                        entry.witness = Some(span);
                    }
                }
            })
            .or_insert(Owner {
                kind,
                witness,
                ambiguous: false,
            });
    }
}
fn span(node: Node<'_>) -> ByteSpan {
    ByteSpan {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}
fn namespace_parts(node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    let mut pending = vec![node];
    let mut parts = Vec::new();
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited > 128 || node.has_error() || node.is_missing() {
            return None;
        }
        match node.kind() {
            "namespace_identifier" => parts.push(node.utf8_text(source).ok()?.into()),
            "nested_namespace_specifier" => {
                let mut cursor = node.walk();
                pending.extend(
                    node.named_children(&mut cursor)
                        .filter(|n| n.kind() != "comment")
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev(),
                );
            }
            _ => return None,
        }
    }
    (!parts.is_empty() && parts.len() <= MAX_SEGMENTS).then_some(parts)
}
/// None is outside B1 (notably templates); Some(None) is recognized but unsafe.
fn qualified_parts(node: Node<'_>, source: &[u8]) -> Option<Option<Vec<String>>> {
    let mut pending = vec![node];
    let mut parts = Vec::new();
    let mut visited = 0;
    while let Some(node) = pending.pop() {
        visited += 1;
        if visited > 128 || node.has_error() || node.is_missing() {
            return Some(None);
        }
        match node.kind() {
            "identifier" | "namespace_identifier" => {
                parts.push(node.utf8_text(source).ok()?.into())
            }
            "qualified_identifier" => {
                pending.push(node.child_by_field_name("name")?);
                if let Some(scope) = node.child_by_field_name("scope") {
                    pending.push(scope);
                } else if node.child(0).is_none_or(|n| n.kind() != "::") {
                    return Some(None);
                }
            }
            _ => return None,
        }
    }
    if parts.len() == 1 {
        // ::f names the global namespace, with no named owner to resolve.
        // Preserve its existing global-function identity outside B1.
        return None;
    }
    Some((parts.len() >= 2 && parts.len() <= MAX_SEGMENTS + 1).then_some(parts))
}

/// Definition-local method qualifiers from the already-selected B1 declarator.
/// Only direct post-parameter children are inspected. Known non-identity tails
/// stay opaque, so their return types, expressions and attributes cannot add cv/ref.
/// None means the scan is incomplete or unsupported, never an empty suffix.
fn method_qualifier_suffix(decl: Node<'_>, source: &[u8]) -> Option<String> {
    if decl.kind() != "function_declarator" || decl.has_error() || decl.is_missing() {
        return None;
    }
    let parameters = decl.child_by_field_name("parameters")?;
    if parameters.kind() != "parameter_list" || parameters.has_error() || parameters.is_missing() {
        return None;
    }
    let mut seen_parameters = false;
    let mut is_const = false;
    let mut is_volatile = false;
    let mut reference = None;
    let mut cursor = decl.walk();
    for (index, child) in decl.children(&mut cursor).enumerate() {
        if index >= 128 || child.has_error() || child.is_missing() {
            return None;
        }
        if child.id() == parameters.id() {
            if seen_parameters {
                return None;
            }
            seen_parameters = true;
            continue;
        }
        if !seen_parameters || child.kind() == "comment" {
            continue;
        }
        match child.kind() {
            "type_qualifier" => match child.utf8_text(source).ok()? {
                "const" if !is_const => is_const = true,
                "volatile" if !is_volatile => is_volatile = true,
                _ => return None,
            },
            "ref_qualifier" => {
                let text = child.utf8_text(source).ok()?;
                if reference.is_some() || !matches!(text, "&" | "&&") {
                    return None;
                }
                reference = Some(text);
            }
            "noexcept"
            | "throw_specifier"
            | "trailing_return_type"
            | "attribute_specifier"
            | "attribute_declaration"
            | "gnu_asm_expression"
            | "requires_clause" => {}
            _ => return None,
        }
    }
    if !seen_parameters {
        return None;
    }
    let mut parts = Vec::with_capacity(3);
    if is_const {
        parts.push("const");
    }
    if is_volatile {
        parts.push("volatile");
    }
    if let Some(reference) = reference {
        parts.push(reference);
    }
    Some(parts.join(" "))
}

/// Applies only to already-extracted symbols matched at their exact definition
/// coordinates. No new extraction/traversal is introduced for unsupported forms.
pub(crate) fn apply(
    tree: &Tree,
    source: &[u8],
    symbols: &mut [SymbolRecord],
) -> Vec<CppQualifiedOwnerProof> {
    let catalog = Catalog::new(tree, source);
    let snapshot = SourceSnapshot::new(source);
    let mut proofs = Vec::new();
    // Exact-coordinate lookup is linear to prepare and constant-time per
    // definition, rather than scanning all symbols for every function.
    let mut coordinates = HashMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        coordinates
            .entry((
                symbol.start_line,
                symbol.start_col,
                symbol.end_line,
                symbol.end_col,
            ))
            .and_modify(|previous| *previous = usize::MAX)
            .or_insert(index);
    }
    let root = tree.root_node();
    let mut cursor = root.walk();
    for node in root.named_children(&mut cursor) {
        if node.kind() != "function_definition" || node.child_by_field_name("type").is_none() {
            // Constructor/destructor/conversion syntax is not an ordinary
            // typed function, even when its final name is an identifier.
            continue;
        }
        let Some(decl) = node
            .child_by_field_name("declarator")
            .filter(|n| n.kind() == "function_declarator")
        else {
            continue;
        };
        let Some(name) = decl
            .child_by_field_name("declarator")
            .filter(|n| n.kind() == "qualified_identifier")
        else {
            continue;
        };
        let Some(parts) = qualified_parts(name, source) else {
            continue;
        };
        let start = node.start_position();
        let end = node.end_position();
        let key = (
            start.row as u32 + 1,
            start.column as u32,
            end.row as u32 + 1,
            end.column as u32,
        );
        let Some(index) = coordinates.get(&key).copied() else {
            continue;
        };
        if index == usize::MAX {
            for symbol in symbols
                .iter_mut()
                .filter(|s| (s.start_line, s.start_col, s.end_line, s.end_col) == key)
            {
                symbol.cpp_qualified_owner = State::Ambiguous;
                symbol.qname = None;
                symbol.symbol_uid = None;
            }
            continue;
        }
        let symbol = &mut symbols[index];
        symbol.cpp_qualified_owner = State::Unproven;
        symbol.qname = None;
        symbol.symbol_uid = None;
        let Some(mut parts) = parts.filter(|_| catalog.complete) else {
            continue;
        };
        let Some(leaf) = parts.pop() else { continue };
        if leaf != symbol.name {
            continue;
        }
        let Some(owner) = catalog.entries.get(&parts) else {
            continue;
        };
        if owner.ambiguous {
            symbol.cpp_qualified_owner = State::Ambiguous;
            continue;
        }
        let Some(witness) = owner.witness.filter(|w| w.end <= node.start_byte()) else {
            continue;
        };
        let (state, kind) = match owner.kind {
            OwnerKind::Namespace => (State::ProvenNamespace, SymbolKind::Function),
            OwnerKind::Type => (State::ProvenType, SymbolKind::Method),
            OwnerKind::Unsupported => continue,
        };
        // Every prefix must itself be a unique proven namespace, never a type,
        // alias or a partial spelling borrowed from a different declaration.
        if !(1..parts.len()).all(|len| {
            catalog.entries.get(&parts[..len]).is_some_and(|entry| {
                entry.kind == OwnerKind::Namespace
                    && !entry.ambiguous
                    && entry.witness.is_some_and(|w| w.end <= node.start_byte())
            })
        }) {
            continue;
        }
        // Owner proof remains a prerequisite. A failed qualifier scan retains
        // Unproven and cannot publish a shortened signature as positive identity.
        if state == State::ProvenType {
            let Some(suffix) = method_qualifier_suffix(decl, source) else {
                continue;
            };
            if !suffix.is_empty() {
                let Some(signature) = symbol.signature.as_mut() else {
                    continue;
                };
                signature.push(' ');
                signature.push_str(&suffix);
            }
        }
        let qname = format!("{}::{}", parts.join("::"), leaf);
        let uid = StableId::symbol_uid(
            &symbol.file_path,
            &qname,
            kind.as_str(),
            symbol.signature.as_deref(),
        );
        symbol.cpp_qualified_owner = state;
        symbol.kind = kind;
        symbol.qname = Some(qname.clone());
        symbol.symbol_uid = Some(uid.clone());
        proofs.push(CppQualifiedOwnerProof {
            source: snapshot.identity().clone(),
            definition: span(node),
            owner_declaration: witness,
            owner_path: parts,
            state,
            symbol_id: symbol.symbol_id.clone(),
            qname,
            symbol_uid: uid,
        });
    }
    proofs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shorthand_prefix_insertion_never_silently_exceeds_owner_budget() {
        let mut catalog = Catalog {
            entries: HashMap::new(),
            complete: true,
            visited: 0,
            owner_limit: 2,
        };
        catalog.insert(vec!["a".into()], OwnerKind::Namespace, None);
        catalog.insert(vec!["a".into(), "b".into()], OwnerKind::Namespace, None);
        assert!(catalog.complete);
        catalog.insert(
            vec!["a".into(), "b".into(), "c".into()],
            OwnerKind::Namespace,
            None,
        );
        assert!(!catalog.complete);
        assert_eq!(catalog.entries.len(), 2);
    }
}
