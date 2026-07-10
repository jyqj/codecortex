//! HTTP/async bridge edge synthesis: caller → route-handler edges built from
//! `http_call_edges` and `routes` evidence, plus their generation cache.

use cc_db::index_db::IndexDb;
use cc_db::GraphReads;
use cc_model::CcResult;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::graph_types::EdgeLite;

use super::bridge_spec::{bridge_edge_limit, dispatch_kind_for, resolution_kind_for};
#[cfg(test)]
use super::cache::BridgeEdgesByCaller;
use super::cache::{
    generation_cached, BridgeIndex, GraphReadGeneration, SharedBridgeEdges, BRIDGE_CACHE,
};
use super::GraphReadModel;

impl GraphReadModel {
    /// Build synthesized caller → route-handler edges from HTTP/async
    /// evidence. Production reads go through the cached `bridge_index`; this
    /// direct form only backs test assertions.
    #[cfg(test)]
    pub(crate) fn bridge_edges_by_caller(db: &IndexDb) -> CcResult<BridgeEdgesByCaller> {
        Ok(Self::bridge_index(db)?.by_caller)
    }

    /// Like [`Self::bridge_edges_by_caller`] but keeps the load-time
    /// truncation fact alongside the edges, under the configured cap.
    pub(super) fn bridge_index(db: &IndexDb) -> CcResult<BridgeIndex> {
        Self::bridge_index_with_limit(db, bridge_edge_limit())
    }

    pub(super) fn bridge_index_with_limit(db: &IndexDb, limit: usize) -> CcResult<BridgeIndex> {
        let reads = GraphReads::new(db);
        let http_edges = reads.all_http_call_edges_lite(limit)?;
        let route_nodes = reads.all_route_nodes_lite(limit)?;
        // A full bucket means rows beyond the cap were likely dropped; the
        // flag rides on the index so consumers can report `bridge_cap`.
        let truncated = http_edges.len() == limit || route_nodes.len() == limit;
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
        let mut unmatched: BTreeMap<String, usize> = BTreeMap::new();
        for hce in &http_edges {
            let caller_uid = match &hce.caller_symbol_uid {
                Some(uid) => uid,
                None => {
                    *unmatched.entry("no_caller_uid".to_string()).or_default() += 1;
                    continue;
                }
            };
            let norm_path = match &hce.normalized_path {
                Some(path) => path,
                None => {
                    *unmatched
                        .entry("no_normalized_path".to_string())
                        .or_default() += 1;
                    continue;
                }
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
            // HTTP calls that matched no route handler produce no bridge edge
            // (the `no_route_handler` category — see bridge_spec). The edge
            // kinds produced below come from the closed bridge registry: the
            // single source of the call_kind → virtual-kind mapping, so a new
            // bridge kind can't appear here as a literal.
            if matched_handlers.is_empty() {
                *unmatched.entry("no_route_handler".to_string()).or_default() += 1;
            }
            let dispatch_kind = dispatch_kind_for(&hce.call_kind);
            let resolution_kind = resolution_kind_for(dispatch_kind);
            for (handler_uid, handler_confidence) in matched_handlers {
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
                    resolution_kind: resolution_kind.map(str::to_string),
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

        Ok(BridgeIndex {
            by_caller: http_bridges,
            truncated,
            unmatched,
        })
    }

    pub(super) fn cached_bridge_edges_by_caller(
        db: &Arc<IndexDb>,
        generation: &GraphReadGeneration,
    ) -> CcResult<SharedBridgeEdges> {
        generation_cached(&BRIDGE_CACHE, generation, || {
            Ok(Arc::new(Self::bridge_index(db)?))
        })
    }
}

pub(super) fn normalize_bridge_method(method: Option<&str>) -> Option<String> {
    method
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(|method| method.to_ascii_uppercase())
}
