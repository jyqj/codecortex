//! Shared data-flow extraction helpers used across language parsers.

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CallEdgeRecord, DataFlowEdgeRecord, ResolutionKind, StableId};

/// Find the innermost enclosing function/method symbol for a given line.
///
/// Among all `Function`/`Method` symbols whose span covers `line`, returns the
/// one with the smallest span (most tightly nested). Shared by the per-language
/// `extract_env_accesses` helpers so the "smallest containing symbol" logic
/// lives in one place.
pub(crate) fn find_enclosing_symbol(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| s.start_line <= line && s.end_line >= line)
        .min_by_key(|s| s.end_line - s.start_line)
}

/// Extract param_pass and return_flow data flow edges from call edges.
pub(crate) fn extract_param_return_flow(
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
        if ce.resolution_kind == ResolutionKind::Unresolved {
            continue;
        }
        if ce.arg_count.unwrap_or(0) > 0 {
            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfp", file_path, ce.line, ce.start_col),
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
            edge_id: StableId::edge_id("dfr", file_path, ce.line, ce.start_col),
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

#[cfg(test)]
mod tests {
    use super::*;
    use cc_model::ParserTier;

    fn sym(name: &str, kind: SymbolKind, start_line: u32, end_line: u32) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym:{name}"),
            file_path: "t.rs".to_string(),
            name: name.to_string(),
            kind,
            container: None,
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::Semantic,
            parser_confidence: 0.8,
            qname: Some(name.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(format!("uid:{name}")),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    #[test]
    fn find_enclosing_symbol_picks_innermost() {
        // `outer` spans 1..20, `inner` spans 5..10 — line 7 is inside both,
        // so the tighter `inner` span must win.
        let symbols = vec![
            sym("outer", SymbolKind::Function, 1, 20),
            sym("inner", SymbolKind::Method, 5, 10),
            sym("MyClass", SymbolKind::Class, 1, 30),
        ];
        let found = find_enclosing_symbol(&symbols, 7).unwrap();
        assert_eq!(found.name, "inner");
    }

    #[test]
    fn find_enclosing_symbol_skips_non_callable_and_misses() {
        let symbols = vec![sym("MyClass", SymbolKind::Class, 1, 30)];
        // Class is not Function/Method, so no enclosing callable symbol.
        assert!(find_enclosing_symbol(&symbols, 7).is_none());
        // Line outside any span yields None.
        let symbols = vec![sym("f", SymbolKind::Function, 1, 5)];
        assert!(find_enclosing_symbol(&symbols, 99).is_none());
    }
}
