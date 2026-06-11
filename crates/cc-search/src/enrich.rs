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

    // Degree metrics and both adjacency directions come from one batched
    // query each instead of one point query per resolved symbol; failures
    // degrade to empty maps, matching the old per-UID `if let Ok(..)`
    // swallowing.
    let resolved_uids: Vec<&str> = resolved.iter().map(|(_, uid)| uid.as_str()).collect();
    let degree_by_uid = db
        .reads()
        .symbol_degree_details_batch(&resolved_uids)
        .unwrap_or_default();

    // 3. Degree metrics → graph_score per chunk.
    for (chunk_id, uid) in &resolved {
        if let Some(info) = degree_by_uid.get(uid) {
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
    let callers_by_uid = db
        .reads()
        .caller_rows_by_uids(&resolved_uids, limits.callers_per_sym)
        .unwrap_or_default();
    let callees_by_uid = db
        .reads()
        .callee_rows_by_uids(&resolved_uids, limits.callees_per_sym)
        .unwrap_or_default();

    for (_chunk_id, uid) in &resolved {
        if let Some(callers) = callers_by_uid.get(uid) {
            for edge in callers {
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
        if let Some(callees) = callees_by_uid.get(uid) {
            for edge in callees {
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

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::FileWriteUnit;
    use cc_model::{CallEdgeRecord, ChunkRecord, Language, ParseOutcome, ParserTier, SymbolRecord};

    /// Insert one file containing a single symbol, its chunk, and the given
    /// call edges (mirrors the fixture helper in `engine.rs` tests).
    fn insert_graph_file(
        db: &IndexDb,
        file_path: &str,
        symbol_name: &str,
        symbol_uid: &str,
        call_edges: Vec<CallEdgeRecord>,
    ) {
        let chunk = ChunkRecord {
            chunk_id: format!("chunk:{}", file_path),
            file_path: file_path.to_string(),
            language: Language::Rust,
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            breadcrumb: "root".to_string(),
            text: format!("fn {symbol_name}() {{}}"),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        };
        let symbol = SymbolRecord {
            symbol_id: format!("sym:{file_path}:{symbol_name}"),
            file_path: file_path.to_string(),
            name: symbol_name.to_string(),
            kind: cc_model::SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: None,
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        };
        let outcome = ParseOutcome {
            summary: String::new(),
            chunks: vec![chunk],
            symbols: vec![symbol],
            call_edges,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        };
        let conn = db.reads().read_conn().unwrap();
        IndexDb::insert_file_data(
            &conn,
            &FileWriteUnit {
                rel_path: file_path.to_string(),
                language: Language::Rust,
                content_hash: format!("hash:{file_path}"),
                mtime: 0.0,
                size: 10,
                outcome,
            },
        )
        .unwrap();
    }

    fn make_hit(chunk_id: &str, file_path: &str, symbol_name: &str) -> SearchHit {
        SearchHit {
            chunk_id: chunk_id.to_string(),
            file_path: file_path.to_string(),
            language: Language::Rust,
            start_line: 1,
            end_line: 1,
            breadcrumb: "root".to_string(),
            symbol_name: Some(symbol_name.to_string()),
            symbol_kind: None,
            text: String::new(),
            fused_score: 1.0,
            lexical_score: 1.0,
            grep_score: 0.0,
            graph_score: 0.0,
            rerank_score: 1.0,
            reasons: vec![],
            score_trace: vec![],
            source: "lexical".to_string(),
            lane: None,
            metadata: serde_json::Value::Null,
        }
    }

    /// Output lock for `graph_enrich`: the per-chunk score values, node
    /// identity/order, and counters asserted here were produced by the
    /// per-UID point-query implementation and must survive the batched
    /// adjacency path unchanged.
    #[test]
    fn test_graph_enrich_locks_scores_nodes_and_counters() {
        let tmp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;

        // alpha (src/a.rs) calls beta; external uid:x calls alpha (call site
        // recorded in src/b.rs); beta calls an unindexed uid:gamma.
        let alpha_to_beta = CallEdgeRecord {
            edge_id: "edge:alpha->beta".to_string(),
            file_path: "src/a.rs".to_string(),
            caller_symbol: Some("alpha".to_string()),
            callee_symbol: "beta".to_string(),
            line: 5,
            caller_symbol_uid: Some("uid:alpha".to_string()),
            callee_symbol_uid: Some("uid:beta".to_string()),
            ..Default::default()
        };
        let x_to_alpha = CallEdgeRecord {
            edge_id: "edge:x->alpha".to_string(),
            file_path: "src/b.rs".to_string(),
            caller_symbol: Some("x_caller".to_string()),
            callee_symbol: "alpha".to_string(),
            line: 3,
            caller_symbol_uid: Some("uid:x".to_string()),
            callee_symbol_uid: Some("uid:alpha".to_string()),
            ..Default::default()
        };
        let beta_to_gamma = CallEdgeRecord {
            edge_id: "edge:beta->gamma".to_string(),
            file_path: "src/b.rs".to_string(),
            caller_symbol: Some("beta".to_string()),
            callee_symbol: "gamma".to_string(),
            line: 7,
            caller_symbol_uid: Some("uid:beta".to_string()),
            callee_symbol_uid: Some("uid:gamma".to_string()),
            ..Default::default()
        };
        insert_graph_file(&db, "src/a.rs", "alpha", "uid:alpha", vec![alpha_to_beta]);
        insert_graph_file(
            &db,
            "src/b.rs",
            "beta",
            "uid:beta",
            vec![x_to_alpha, beta_to_gamma],
        );
        {
            // Two refs against uid:alpha feed the ref_count score bonus.
            let conn = db.reads().read_conn().unwrap();
            for ref_id in ["sr1", "sr2"] {
                conn.execute(
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,target_symbol_uid,resolution_kind,resolution_confidence,resolution_strategy,parser_tier,parser_confidence)
                     VALUES(?1,'src/a.rs','alpha','','usage',2,'uid:alpha','exact',0.9,'import_map','tree_sitter',0.8)",
                    rusqlite::params![ref_id],
                )
                .unwrap();
            }
        }

        let hits = vec![
            make_hit("chunk:src/a.rs", "src/a.rs", "alpha"),
            make_hit("chunk:src/b.rs", "src/b.rs", "beta"),
        ];
        let limits = GraphEnrichLimits {
            max_resolve: 10,
            callers_per_sym: 5,
            callees_per_sym: 5,
            max_tests: 5,
            max_routes: 1,
            graph_budget_pct: 50,
        };
        let (scores, enrichment) = graph_enrich(&db, &hits, &limits, 10_000);

        assert_eq!(enrichment.symbols_resolved, 2);
        assert_eq!(enrichment.callers_added, 2);
        assert_eq!(enrichment.callees_added, 2);
        assert_eq!(enrichment.tests_found, 0);

        // Node identity and order follow hit order (alpha then beta), each
        // seed contributing callers before callees, deduped across seeds.
        let node_ids: Vec<&str> = enrichment
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect();
        assert_eq!(
            node_ids,
            [
                "graph:caller:uid:x",
                "graph:callee:uid:beta",
                "graph:caller:uid:alpha",
                "graph:callee:uid:gamma",
            ]
        );
        assert_eq!(
            enrichment.nodes[0].text,
            "caller: x_caller → alpha (src/b.rs:3)"
        );
        assert_eq!(
            enrichment.nodes[1].text,
            "callee: alpha → beta (src/a.rs:5)"
        );

        // Score formula: ln(in+out+1)/10 + min(ref_count,10)/100, capped 0.4.
        // alpha: in 1 (x->alpha), out 1 (alpha->beta), refs 2.
        // beta:  in 1 (alpha->beta), out 1 (beta->gamma), refs 0.
        assert_eq!(scores.len(), 2);
        let expected_alpha = (2.0f64 + 1.0).ln() / 10.0 + 0.02;
        let expected_beta = (2.0f64 + 1.0).ln() / 10.0;
        assert!(
            (scores["chunk:src/a.rs"] - expected_alpha).abs() < 1e-9,
            "alpha score {} != expected {expected_alpha}",
            scores["chunk:src/a.rs"]
        );
        assert!(
            (scores["chunk:src/b.rs"] - expected_beta).abs() < 1e-9,
            "beta score {} != expected {expected_beta}",
            scores["chunk:src/b.rs"]
        );
    }
}
