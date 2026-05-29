//! Shared data-flow extraction helpers used across language parsers.

use cc_model::{CallEdgeRecord, DataFlowEdgeRecord, ResolutionKind, StableId};

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
