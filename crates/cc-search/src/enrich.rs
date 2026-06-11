//! Graph enrichment for context search — resolves search hits to symbol
//! UIDs, derives a connectivity-based `graph_score` per chunk, and collects
//! neighbor (caller/callee) and test-coverage context nodes.
//!
//! Moved verbatim from cc-server so the graph contribution to
//! `rerank_score` is applied — and the final ordering decided — inside
//! cc-search (see `SearchEngine::search_with_graph_context`).

use std::collections::{HashMap, HashSet};

use cc_db::index_db::IndexDb;
use cc_model::config::GraphEnrichLimits;
use cc_model::context::{ContextNode, NodeType, Role};
use cc_model::search::SearchHit;

/// Non-score outputs of graph enrichment: context nodes for the envelope
/// plus counters for evidence summaries.
#[derive(Default)]
pub struct GraphEnrichment {
    pub nodes: Vec<ContextNode>,
    pub symbols_resolved: usize,
    pub callers_added: usize,
    pub callees_added: usize,
    pub tests_found: usize,
}

/// Compute per-chunk graph scores and collect graph context nodes.
///
/// Returns `(scores, enrichment)` where `scores` maps `chunk_id` to the raw
/// graph connectivity score (before the `graph_rerank_weight` multiplier).
pub(crate) fn graph_enrich(
    db: &IndexDb,
    hits: &[SearchHit],
    limits: &GraphEnrichLimits,
    token_budget: u32,
) -> (HashMap<String, f64>, GraphEnrichment) {
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut enrichment = GraphEnrichment::default();
    if hits.is_empty() {
        return (scores, enrichment);
    }

    // 1. Batch load symbols for hit file paths.
    let file_paths: Vec<&str> = hits
        .iter()
        .take(limits.max_resolve)
        .map(|h| h.file_path.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let all_symbols = db
        .reads()
        .symbols_by_file_paths(&file_paths)
        .unwrap_or_default();

    // 2. Resolve each hit → symbol_uid via (file_path, symbol_name) or span overlap.
    let mut resolved: Vec<(String, String)> = Vec::new(); // (chunk_id, uid)
    let mut seen_uids = HashSet::new();
    for hit in hits.iter().take(limits.max_resolve) {
        let uid = all_symbols.iter().find(|s| {
            s.file_path == hit.file_path
                && (hit.symbol_name.as_deref() == Some(&s.name)
                    || (s.start_line <= hit.start_line && s.end_line >= hit.end_line))
        });
        if let Some(sym) = uid {
            if let Some(ref uid) = sym.symbol_uid {
                if seen_uids.insert(uid.clone()) {
                    resolved.push((hit.chunk_id.clone(), uid.clone()));
                }
            }
        }
    }
    enrichment.symbols_resolved = resolved.len();

    // 3. Degree metrics → graph_score per chunk.
    for (chunk_id, uid) in &resolved {
        if let Ok(info) = db.reads().symbol_degree_details(uid) {
            let total = (info.in_degree + info.out_degree) as f64;
            let connectivity = (total + 1.0).ln() / 10.0;
            let ref_bonus = (info.ref_count as f64).min(10.0) / 100.0;
            let score = (connectivity + ref_bonus).min(0.4);
            scores.insert(chunk_id.clone(), score);
        }
    }

    // 4. Callers + callees → ContextNodes.
    let graph_budget = (token_budget * limits.graph_budget_pct) / 100;
    let mut graph_tokens = 0u32;
    let mut neighbor_uids = HashSet::new();

    for (_chunk_id, uid) in &resolved {
        if let Ok(callers) = db.reads().caller_rows_by_uid(uid, limits.callers_per_sym) {
            for edge in &callers {
                let caller_uid = edge.caller_symbol_uid.as_deref().unwrap_or("");
                if caller_uid.is_empty() || !neighbor_uids.insert(caller_uid.to_string()) {
                    continue;
                }
                let text = format!(
                    "caller: {} → {} ({}:{})",
                    edge.caller_symbol.as_deref().unwrap_or("?"),
                    edge.callee_symbol,
                    edge.file_path,
                    edge.line
                );
                let est = (text.len() / 4).max(10) as u32;
                if graph_tokens + est > graph_budget {
                    break;
                }
                graph_tokens += est;
                let node_id = format!("graph:caller:{}", caller_uid);
                let mut node = ContextNode::new(
                    node_id.clone(),
                    NodeType::CallEdge,
                    Role::Neighbor,
                    format!("Caller: {}", edge.caller_symbol.as_deref().unwrap_or("?")),
                    text,
                );
                node.file_path = Some(edge.file_path.clone());
                node.start_line = Some(edge.line);
                node.confidence = edge.confidence;
                node.token_estimate = est;
                node.source = "graph".to_string();
                enrichment.nodes.push(node);
                enrichment.callers_added += 1;
            }
        }
        if let Ok(callees) = db.reads().callee_rows_by_uid(uid, limits.callees_per_sym) {
            for edge in &callees {
                let callee_uid = edge.callee_symbol_uid.as_deref().unwrap_or("");
                if callee_uid.is_empty() || !neighbor_uids.insert(callee_uid.to_string()) {
                    continue;
                }
                let text = format!(
                    "callee: {} → {} ({}:{})",
                    edge.caller_symbol.as_deref().unwrap_or("?"),
                    edge.callee_symbol,
                    edge.file_path,
                    edge.line
                );
                let est = (text.len() / 4).max(10) as u32;
                if graph_tokens + est > graph_budget {
                    break;
                }
                graph_tokens += est;
                let node_id = format!("graph:callee:{}", callee_uid);
                let mut node = ContextNode::new(
                    node_id.clone(),
                    NodeType::CallEdge,
                    Role::Neighbor,
                    format!("Callee: {}", edge.callee_symbol),
                    text,
                );
                node.file_path = Some(edge.file_path.clone());
                node.start_line = Some(edge.line);
                node.confidence = edge.confidence;
                node.token_estimate = est;
                node.source = "graph".to_string();
                enrichment.nodes.push(node);
                enrichment.callees_added += 1;
            }
        }
    }

    // 5. Test coverage.
    let hit_files: Vec<String> = file_paths.iter().map(|s| s.to_string()).collect();
    if let Ok(tests) = db.reads().find_impacted_tests(&hit_files) {
        for test_path in tests.iter().take(limits.max_tests) {
            let text = format!("test file: {}", test_path);
            let est = (text.len() / 4).max(10) as u32;
            if graph_tokens + est > graph_budget {
                break;
            }
            graph_tokens += est;
            let node_id = format!("graph:test:{}", test_path);
            let mut node = ContextNode::new(
                node_id,
                NodeType::TestEdge,
                Role::Test,
                format!("Test: {}", test_path),
                text,
            );
            node.file_path = Some(test_path.clone());
            node.token_estimate = est;
            node.source = "graph".to_string();
            enrichment.nodes.push(node);
            enrichment.tests_found += 1;
        }
    }

    (scores, enrichment)
}
