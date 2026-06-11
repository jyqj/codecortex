//! HTTP/async bridge edge synthesis: caller → route-handler edges built from
//! `http_call_edges` and `routes` evidence, plus their generation cache.

use cc_db::index_db::IndexDb;
use cc_db::GraphReads;
use cc_model::CcResult;
use std::collections::HashMap;
use std::sync::Arc;

use crate::graph_types::{BfsAdj, EdgeLite};

use super::cache::{
    generation_cached, BridgeEdgesByCaller, GraphReadGeneration, SharedBridgeEdges, BRIDGE_CACHE,
};
use super::GraphReadModel;

impl GraphReadModel {
    /// Build edge-labeled call adjacency including bounded HTTP bridge edges.
    pub(crate) fn call_adjacency_with_bridges(db: &IndexDb) -> CcResult<BfsAdj> {
        let mut bfs = Self::call_adjacency(db)?;
        for (caller_uid, bridge_edges) in Self::bridge_edges_by_caller(db)? {
            bfs.adj.entry(caller_uid).or_default().extend(bridge_edges);
        }
        Ok(bfs)
    }

    /// Build synthesized caller → route-handler edges from HTTP/async evidence.
    pub(crate) fn bridge_edges_by_caller(db: &IndexDb) -> CcResult<BridgeEdgesByCaller> {
        let limit: usize = std::env::var("CODECORTEX_BRIDGE_EDGE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);
        let reads = GraphReads::new(db);
        let http_edges = reads.all_http_call_edges_lite(limit)?;
        let route_nodes = reads.all_route_nodes_lite(limit)?;
        if http_edges.len() == limit {
            tracing::warn!(
                count = http_edges.len(),
                limit,
                "HTTP bridge edges may be truncated"
            );
        }
        if route_nodes.len() == limit {
            tracing::warn!(
                count = route_nodes.len(),
                limit,
                "HTTP bridge route nodes may be truncated"
            );
        }

        let mut route_lookup: HashMap<(String, String), Vec<(String, f64)>> = HashMap::new();
        let mut route_any_method_lookup: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        let mut route_path_lookup: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for rn in &route_nodes {
            if let (Some(norm_path), Some(handler_uid)) =
                (&rn.normalized_path, &rn.handler_symbol_uid)
            {
                let handler = (handler_uid.clone(), rn.confidence);
                route_path_lookup
                    .entry(norm_path.clone())
                    .or_default()
                    .push(handler.clone());
                if let Some(method) = normalize_bridge_method(rn.method.as_deref()) {
                    route_lookup
                        .entry((norm_path.clone(), method))
                        .or_default()
                        .push(handler);
                } else {
                    route_any_method_lookup
                        .entry(norm_path.clone())
                        .or_default()
                        .push(handler);
                }
            }
        }

        let mut http_bridges: HashMap<String, Vec<EdgeLite>> = HashMap::new();
        for hce in &http_edges {
            let caller_uid = match &hce.caller_symbol_uid {
                Some(uid) => uid,
                None => continue,
            };
            let norm_path = match &hce.normalized_path {
                Some(path) => path,
                None => continue,
            };
            let mut matched_handlers: Vec<&(String, f64)> = Vec::new();
            if let Some(method) = normalize_bridge_method(hce.method.as_deref()) {
                if let Some(handlers) = route_lookup.get(&(norm_path.clone(), method)) {
                    matched_handlers.extend(handlers);
                }
                if let Some(handlers) = route_any_method_lookup.get(norm_path) {
                    matched_handlers.extend(handlers);
                }
            } else {
                matched_handlers.extend(route_path_lookup.get(norm_path).into_iter().flatten());
            }
            for (handler_uid, handler_confidence) in matched_handlers {
                let dispatch_kind = if hce.call_kind.eq_ignore_ascii_case("http") {
                    "http_bridge"
                } else {
                    "async_bridge"
                };
                let bridge_edge = EdgeLite {
                    caller_uid: caller_uid.clone(),
                    callee_uid: handler_uid.clone(),
                    dispatch_kind: dispatch_kind.to_string(),
                    synthesized_by: Some(dispatch_kind.to_string()),
                    synthesis_key: Some(norm_path.clone()),
                    confidence: f64::min(hce.confidence, *handler_confidence),
                    file_path: hce.file_path.clone(),
                    line: hce.line,
                    registered_file: None,
                    registered_line: None,
                    resolution_kind: Some("synthesized".to_string()),
                    parser_tier: None,
                    resolution_strategy: None,
                    parser_confidence: None,
                };
                http_bridges
                    .entry(caller_uid.clone())
                    .or_default()
                    .push(bridge_edge);
            }
        }

        Ok(http_bridges)
    }

    pub(super) fn cached_bridge_edges_by_caller(
        db: &Arc<IndexDb>,
        generation: &GraphReadGeneration,
    ) -> CcResult<SharedBridgeEdges> {
        generation_cached(&BRIDGE_CACHE, generation, || {
            Ok(Arc::new(Self::bridge_edges_by_caller(db)?))
        })
    }
}

pub(super) fn normalize_bridge_method(method: Option<&str>) -> Option<String> {
    method
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(|method| method.to_ascii_uppercase())
}
