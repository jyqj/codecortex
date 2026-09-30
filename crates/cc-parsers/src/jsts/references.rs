//! References come from syntax nodes and actual call edges, never regex text.
//! Target selection belongs to the resolver; a same-named declaration is not
//! sufficient evidence for parser_exact (shadowing and overloads exist).
use cc_model::{
    edge::CallEdgeRecord,
    symbol::{SymbolKind, SymbolRecord, SymbolRefRecord},
    ElementKind, ParserTier, ResolutionKind, StableId,
};
use std::collections::{BTreeMap, HashSet};
use tree_sitter::{Node, Tree};

pub(super) fn extract(
    tree: &Tree,
    source: &[u8],
    file: &str,
    symbols: &[SymbolRecord],
    calls: &mut [CallEdgeRecord],
) -> Vec<SymbolRefRecord> {
    let mut refs = BTreeMap::new();
    let mut call_positions = HashSet::new();
    for c in calls {
        let id = StableId::ref_id(file, &c.callee_symbol, c.line, c.start_col);
        call_positions.insert((c.line, c.start_col));
        c.callee_ref_id = Some(id.clone());
        refs.insert(
            id.clone(),
            SymbolRefRecord {
                ref_id: id,
                file_path: file.into(),
                symbol_name: c.callee_symbol.clone(),
                container: c.caller_symbol.clone(),
                ref_kind: "call".into(),
                line: c.line,
                column: c.start_col,
                target_symbol_id: None,
                target_file_path: None,
                target_symbol_uid: None,
                ref_name: Some(c.callee_symbol.clone()),
                scope_id: None,
                resolution_kind: ResolutionKind::Unresolved,
                resolution_confidence: 0.0,
                resolution_strategy: c.resolution_strategy.clone(),
                ref_end_line: c.end_line,
                ref_end_col: Some(c.end_col),
                parser_tier: ParserTier::Semantic,
                parser_confidence: ParserTier::Semantic.element_confidence(ElementKind::CallRef),
            },
        );
    }
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "comment" | "string" | "regex" | "string_fragment"
        ) {
            continue;
        }
        if matches!(
            node.kind(),
            "identifier" | "shorthand_property_identifier" | "type_identifier"
        ) && !binding(node)
        {
            let point = node.start_position();
            let line = point.row as u32 + 1;
            let col = point.column as u32;
            if !call_positions.contains(&(line, col)) {
                if let Ok(name) = node.utf8_text(source) {
                    let owner = symbols
                        .iter()
                        .filter(|s| {
                            matches!(s.kind, SymbolKind::Function | SymbolKind::Method)
                                && s.start_line <= line
                                && s.end_line >= line
                        })
                        .min_by_key(|s| s.end_line - s.start_line);
                    let id = StableId::ref_id(file, name, line, col);
                    refs.entry(id.clone()).or_insert_with(|| SymbolRefRecord {
                        ref_id: id,
                        file_path: file.into(),
                        symbol_name: name.into(),
                        container: owner.and_then(|s| s.qname.clone()),
                        ref_kind: "identifier".into(),
                        line,
                        column: col,
                        target_symbol_id: None,
                        target_file_path: None,
                        target_symbol_uid: None,
                        ref_name: Some(name.into()),
                        scope_id: owner.and_then(|s| s.scope_id.clone()),
                        resolution_kind: ResolutionKind::Unresolved,
                        resolution_confidence: 0.0,
                        resolution_strategy: "unresolved".into(),
                        ref_end_line: Some(node.end_position().row as u32 + 1),
                        ref_end_col: Some(node.end_position().column as u32),
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: ParserTier::Semantic
                            .element_confidence(ElementKind::IdentifierRef),
                    });
                }
            }
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    refs.into_values().collect()
}
fn binding(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent
        .child_by_field_name("name")
        .is_some_and(|n| n.id() == node.id())
        && matches!(
            parent.kind(),
            "function_declaration"
                | "function_expression"
                | "generator_function_declaration"
                | "class_declaration"
                | "method_definition"
                | "variable_declarator"
                | "interface_declaration"
                | "type_alias_declaration"
        )
    {
        return true;
    }
    if matches!(
        parent.kind(),
        "formal_parameters"
            | "import_clause"
            | "import_specifier"
            | "namespace_import"
            | "export_specifier"
    ) {
        return true;
    }
    if matches!(
        parent.kind(),
        "required_parameter" | "optional_parameter" | "rest_pattern" | "assignment_pattern"
    ) {
        return parent
            .child_by_field_name("pattern")
            .or_else(|| parent.child_by_field_name("left"))
            .is_none_or(|n| n.id() == node.id());
    }
    false
}
