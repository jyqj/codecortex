//! Shared graph read model for graph/impact tools.
//!
//! This Module keeps adjacency loading, neighborhood BFS, and edge projection in
//! one place so trace/flow/cycles/impact can share the same read path without
//! changing their external output shape.

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_model::{CcError, CcResult};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use crate::graph_types::{
    BfsAdj, EdgeLite, InternalEdge, LabeledPath, TraceEdge, TraceEdgeEvidence,
};

type BridgeEdgesByCaller = HashMap<String, Vec<EdgeLite>>;
type SharedBridgeEdges = Arc<BridgeEdgesByCaller>;
type BridgeCache = Mutex<HashMap<GraphReadGeneration, SharedBridgeEdges>>;

/// Cache/reuse discriminator for graph read data.
///
/// The current schema does not expose a monotonic graph generation, so this key
/// is derived from index metadata that changes across rebuilds when available.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct GraphReadGeneration {
    db_identity: usize,
    last_indexed_at: Option<String>,
    index_version: Option<String>,
}

impl GraphReadGeneration {
    fn from_db(db: &Arc<IndexDb>) -> Self {
        Self {
            db_identity: Arc::as_ptr(db) as usize,
            last_indexed_at: db.get_metadata("last_indexed_at").ok().flatten(),
            index_version: db.get_metadata("index_version").ok().flatten(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GraphSymbolLite {
    pub symbol_uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub community_id: Option<u32>,
}

/// Read-only graph view with per-instance adjacency caches.
pub(crate) struct GraphReadModel {
    db: Arc<IndexDb>,
    generation: GraphReadGeneration,
    /// Cached outgoing call edges keyed by caller UID.
    outgoing_cache: HashMap<String, Vec<EdgeLite>>,
    /// Pre-loaded HTTP/async bridge edges keyed by caller UID.
    http_bridges: SharedBridgeEdges,
}

impl GraphReadModel {
    /// Build a read model for trace/flow traversal, including HTTP/async bridge
    /// edges so callers see the same synthesized paths as before.
    pub(crate) fn new(db: Arc<IndexDb>) -> CcResult<Self> {
        let generation = GraphReadGeneration::from_db(&db);
        let http_bridges = Self::cached_bridge_edges_by_caller(&db, &generation)?;
        Ok(Self {
            db,
            generation,
            outgoing_cache: HashMap::new(),
            http_bridges,
        })
    }

    /// Build a read model for operations that do not need HTTP bridge edges.
    pub(crate) fn without_http_bridges(db: Arc<IndexDb>) -> Self {
        let generation = GraphReadGeneration::from_db(&db);
        Self {
            db,
            generation,
            outgoing_cache: HashMap::new(),
            http_bridges: Arc::new(HashMap::new()),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn generation(&self) -> &GraphReadGeneration {
        &self.generation
    }

    /// Build edge-labeled call adjacency from persisted call edges.
    pub(crate) fn call_adjacency(db: &IndexDb) -> CcResult<BfsAdj> {
        let edges = db.call_uid_edges_lite()?;
        let mut adj: HashMap<String, Vec<EdgeLite>> = HashMap::new();
        for edge in edges {
            adj.entry(edge.caller_uid.clone()).or_default().push(edge);
        }
        Ok(BfsAdj { adj })
    }

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
        let http_edges = db.all_http_call_edges_lite(5000)?;
        let route_nodes = db.all_route_nodes_lite(5000)?;

        let mut route_lookup: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for rn in &route_nodes {
            if let (Some(norm_path), Some(handler_uid)) =
                (&rn.normalized_path, &rn.handler_symbol_uid)
            {
                route_lookup
                    .entry(norm_path.clone())
                    .or_default()
                    .push((handler_uid.clone(), rn.confidence));
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
            if let Some(handlers) = route_lookup.get(norm_path) {
                for (handler_uid, handler_confidence) in handlers {
                    let dispatch_kind = if hce.call_kind == "http" {
                        "http_bridge"
                    } else {
                        "async_bridge"
                    };
                    let bridge_edge = EdgeLite {
                        caller_uid: caller_uid.clone(),
                        callee_uid: handler_uid.clone(),
                        dispatch_kind: dispatch_kind.to_string(),
                        synthesized_by: Some("http_bridge".to_string()),
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
        }

        Ok(http_bridges)
    }

    fn cached_bridge_edges_by_caller(
        db: &Arc<IndexDb>,
        generation: &GraphReadGeneration,
    ) -> CcResult<SharedBridgeEdges> {
        static BRIDGE_CACHE: OnceLock<BridgeCache> = OnceLock::new();

        let cache = BRIDGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(cache) = cache.lock() {
            if let Some(cached) = cache.get(generation) {
                return Ok(Arc::clone(cached));
            }
        }

        let bridges = Arc::new(Self::bridge_edges_by_caller(db)?);
        if let Ok(mut cache) = cache.lock() {
            cache.insert(generation.clone(), Arc::clone(&bridges));
        }
        Ok(bridges)
    }

    /// Return outgoing edges for a caller UID, backed by an on-demand cache.
    pub(crate) fn neighbors(&mut self, uid: &str) -> &[EdgeLite] {
        if !self.outgoing_cache.contains_key(uid) {
            let mut edges = self.db.call_edges_from_uid_lite(uid).unwrap_or_default();
            if let Some(bridges) = self.http_bridges.get(uid) {
                edges.extend(bridges.iter().cloned());
            }
            self.outgoing_cache.insert(uid.to_string(), edges);
        }
        self.outgoing_cache
            .get(uid)
            .map(|edges| edges.as_slice())
            .unwrap_or(&[])
    }

    pub(crate) fn paths_between(
        &mut self,
        from_uid: &str,
        to_uid: &str,
        max_depth: usize,
        max_paths: usize,
    ) -> Vec<LabeledPath> {
        bfs_paths_labeled_with(from_uid, to_uid, max_depth, max_paths, |uid| {
            self.neighbors(uid).to_vec()
        })
    }

    #[cfg(test)]
    pub(crate) fn paths_between_adj(
        adj: &BfsAdj,
        from_uid: &str,
        to_uid: &str,
        max_depth: usize,
        max_paths: usize,
    ) -> Vec<LabeledPath> {
        bfs_paths_labeled_with(from_uid, to_uid, max_depth, max_paths, |uid| {
            adj.adj.get(uid).cloned().unwrap_or_default()
        })
    }

    /// Build backward-compatible name paths plus deduplicated TraceEdge values.
    pub(crate) fn named_paths_and_trace_edges(
        &self,
        labeled_paths: &[LabeledPath],
        sym_map: &HashMap<String, SymbolRow>,
        include_runtime_evidence: bool,
    ) -> (Vec<Vec<String>>, Vec<TraceEdge>) {
        let uid_names: HashMap<String, String> = sym_map
            .iter()
            .map(|(uid, row)| (uid.clone(), row.name.clone()))
            .collect();

        let paths = labeled_paths
            .iter()
            .map(|path| {
                path.node_uids
                    .iter()
                    .map(|uid| uid_names.get(uid).cloned().unwrap_or_else(|| uid.clone()))
                    .collect()
            })
            .collect();
        let edges = self.project_trace_edges(
            labeled_paths.iter().flat_map(|path| path.edge_lites.iter()),
            include_runtime_evidence,
        );
        (paths, edges)
    }

    /// Project EdgeLite values into public TraceEdge output, optionally adding
    /// runtime evidence for synthesized HTTP bridge edges.
    pub(crate) fn project_trace_edges<'a, I>(
        &self,
        edge_lites: I,
        include_runtime_evidence: bool,
    ) -> Vec<TraceEdge>
    where
        I: IntoIterator<Item = &'a EdgeLite>,
    {
        let edge_lites: Vec<&EdgeLite> = edge_lites.into_iter().collect();
        let mut bridge_norm_paths: Vec<String> = Vec::new();
        if include_runtime_evidence {
            for edge in &edge_lites {
                if edge.synthesized_by.as_deref() == Some("http_bridge") {
                    if let Some(synthesis_key) = &edge.synthesis_key {
                        bridge_norm_paths.push(synthesis_key.clone());
                    }
                }
            }
            bridge_norm_paths.sort();
            bridge_norm_paths.dedup();
        }

        let evidence_map = if include_runtime_evidence && !bridge_norm_paths.is_empty() {
            self.db
                .evidence_for_normalized_paths(&bridge_norm_paths)
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        let mut edges = Vec::new();
        let mut edge_seen: HashSet<(String, String, u32)> = HashSet::new();
        for edge in edge_lites {
            let key = (edge.caller_uid.clone(), edge.callee_uid.clone(), edge.line);
            if !edge_seen.insert(key) {
                continue;
            }

            let evidence = if include_runtime_evidence
                && edge.synthesized_by.as_deref() == Some("http_bridge")
            {
                edge.synthesis_key
                    .as_ref()
                    .and_then(|key| evidence_map.get(key))
                    .map(|(count, last_seen)| TraceEdgeEvidence {
                        observed_count: *count,
                        last_seen: last_seen.clone(),
                    })
            } else {
                None
            };

            edges.push(TraceEdge {
                from_uid: edge.caller_uid.clone(),
                to_uid: edge.callee_uid.clone(),
                dispatch_kind: edge.dispatch_kind.clone(),
                synthesized_by: edge.synthesized_by.clone(),
                synthesis_key: edge.synthesis_key.clone(),
                confidence: edge.confidence,
                file_path: edge.file_path.clone(),
                line: edge.line,
                registered_file: edge.registered_file.clone(),
                registered_line: edge.registered_line,
                resolution_kind: edge.resolution_kind.clone(),
                parser_tier: edge.parser_tier.clone(),
                resolution_strategy: edge.resolution_strategy.clone(),
                parser_confidence: edge.parser_confidence,
                evidence,
            });
        }
        edges
    }

    pub(crate) fn projected_import_adjacency<F>(
        &self,
        mut project_node: F,
    ) -> CcResult<HashMap<String, Vec<String>>>
    where
        F: FnMut(&str) -> String,
    {
        let rows = self.db.query_json(
            "SELECT DISTINCT file_path, resolved_path FROM imports WHERE resolved_path IS NOT NULL",
            &[],
        )?;

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for row in &rows {
            let file_path = row.get("file_path").and_then(|value| value.as_str());
            let resolved_path = row.get("resolved_path").and_then(|value| value.as_str());
            if let (Some(file_path), Some(resolved_path)) = (file_path, resolved_path) {
                let from = project_node(file_path);
                let to = project_node(resolved_path);
                if from != to {
                    adj.entry(from).or_default().push(to);
                }
            }
        }

        for targets in adj.values_mut() {
            targets.sort();
            targets.dedup();
        }
        Ok(adj)
    }

    pub(crate) fn file_import_adjacency(&self) -> CcResult<HashMap<String, Vec<String>>> {
        self.projected_import_adjacency(|path| path.to_string())
    }

    pub(crate) fn file_import_witness_edges(&self, scc: &[String]) -> CcResult<Vec<InternalEdge>> {
        let scc_set: HashSet<&str> = scc.iter().map(|node| node.as_str()).collect();
        let mut edges = Vec::new();

        for node in scc {
            let rows = self.db.query_json(
                "SELECT resolved_path, import_string FROM imports WHERE file_path = ?1 AND resolved_path IS NOT NULL",
                std::slice::from_ref(node),
            )?;
            for row in &rows {
                let resolved_path = row.get("resolved_path").and_then(|value| value.as_str());
                let import = row.get("import_string").and_then(|value| value.as_str());
                if let Some(resolved_path) = resolved_path {
                    if scc_set.contains(resolved_path) {
                        edges.push(InternalEdge {
                            from: node.clone(),
                            to: resolved_path.to_string(),
                            import: import.map(|value| value.to_string()),
                            line: None,
                        });
                    }
                }
            }
        }

        Ok(edges)
    }

    pub(crate) fn community_call_adjacency(&self) -> CcResult<HashMap<String, Vec<String>>> {
        let sym_rows = self.db.query_json(
            "SELECT DISTINCT symbol_uid, community_id FROM symbols WHERE community_id IS NOT NULL AND symbol_uid IS NOT NULL",
            &[],
        )?;

        let mut uid_to_community: HashMap<String, String> = HashMap::new();
        for row in &sym_rows {
            let uid = row.get("symbol_uid").and_then(|value| value.as_str());
            let community_id = row.get("community_id");
            if let (Some(uid), Some(community_id)) = (uid, community_id) {
                let community = match community_id {
                    serde_json::Value::Number(number) => number.to_string(),
                    serde_json::Value::String(value) => value.clone(),
                    _ => continue,
                };
                uid_to_community.insert(uid.to_string(), community);
            }
        }

        let call_edges = self.db.call_uid_edges()?;
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (caller_uid, callee_uid) in &call_edges {
            let from = uid_to_community.get(caller_uid);
            let to = uid_to_community.get(callee_uid);
            if let (Some(from), Some(to)) = (from, to) {
                if from != to {
                    adj.entry(from.clone()).or_default().push(to.clone());
                }
            }
        }

        for targets in adj.values_mut() {
            targets.sort();
            targets.dedup();
        }
        Ok(adj)
    }

    pub(crate) fn internal_edges_from_adjacency(
        adj: &HashMap<String, Vec<String>>,
        scc: &[String],
    ) -> Vec<InternalEdge> {
        let scc_set: HashSet<&str> = scc.iter().map(|node| node.as_str()).collect();
        let mut edges = Vec::new();

        for node in scc {
            if let Some(targets) = adj.get(node) {
                for target in targets {
                    if scc_set.contains(target.as_str()) {
                        edges.push(InternalEdge {
                            from: node.clone(),
                            to: target.clone(),
                            import: None,
                            line: None,
                        });
                    }
                }
            }
        }

        edges
    }

    pub(crate) fn symbols_in_files(&self, files: &[String]) -> CcResult<Vec<GraphSymbolLite>> {
        let conn = self.db.read_conn()?;
        let mut symbols = Vec::new();
        for file in files {
            let mut stmt = conn
                .prepare(
                    "SELECT symbol_uid, name, file_path, kind, community_id \
                     FROM symbols WHERE file_path=?1 AND symbol_uid IS NOT NULL",
                )
                .map_err(|err| CcError::Database(err.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![file], |row| {
                    Ok(GraphSymbolLite {
                        symbol_uid: row.get::<_, String>(0)?,
                        name: row.get::<_, String>(1)?,
                        file_path: row.get::<_, String>(2)?,
                        kind: row.get::<_, String>(3)?,
                        community_id: row.get::<_, Option<u32>>(4)?,
                    })
                })
                .map_err(|err| CcError::Database(err.to_string()))?;
            for row in rows.flatten() {
                symbols.push(row);
            }
        }
        Ok(symbols)
    }

    pub(crate) fn reverse_callers(
        &self,
        callee_uids: &[String],
        confidence_threshold: Option<f64>,
    ) -> CcResult<Vec<GraphSymbolLite>> {
        let conn = self.db.read_conn()?;
        let mut callers = Vec::new();
        let batch_size = 200;

        for batch in callee_uids.chunks(batch_size) {
            if batch.is_empty() {
                continue;
            }

            let placeholders = (0..batch.len())
                .map(|idx| format!("?{}", idx + 1))
                .collect::<Vec<_>>()
                .join(",");
            let conf_clause = if confidence_threshold.is_some() {
                format!("AND ce.parser_confidence >= ?{}", batch.len() + 1)
            } else {
                String::new()
            };
            let sql = format!(
                "SELECT DISTINCT ce.caller_symbol_uid, s.name, s.file_path, s.kind, s.community_id \
                 FROM call_edges ce \
                 JOIN symbols s ON s.symbol_uid = ce.caller_symbol_uid \
                 WHERE ce.callee_symbol_uid IN ({}) \
                 AND ce.caller_symbol_uid IS NOT NULL \
                 {}",
                placeholders, conf_clause
            );

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|err| CcError::Database(err.to_string()))?;
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            for uid in batch {
                params.push(Box::new(uid.clone()));
            }
            if let Some(threshold) = confidence_threshold {
                params.push(Box::new(threshold));
            }
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|param| param.as_ref()).collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(GraphSymbolLite {
                        symbol_uid: row.get::<_, String>(0)?,
                        name: row.get::<_, String>(1)?,
                        file_path: row.get::<_, String>(2)?,
                        kind: row.get::<_, String>(3)?,
                        community_id: row.get::<_, Option<u32>>(4)?,
                    })
                })
                .map_err(|err| CcError::Database(err.to_string()))?;
            callers.extend(rows.flatten());
        }

        Ok(callers)
    }

    pub(crate) fn suggested_tests_for_files(&self, files: &[String]) -> CcResult<Vec<String>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.db.read_conn()?;
        let placeholders = (1..=files.len())
            .map(|idx| format!("?{}", idx))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT test_file_path FROM test_edges \
             WHERE code_file_path IN ({}) ORDER BY test_file_path",
            placeholders
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = files
            .iter()
            .map(|file| file as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|err| CcError::Database(err.to_string()))?;
        let rows = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))
            .map_err(|err| CcError::Database(err.to_string()))?;

        let mut suggested_tests = Vec::new();
        let mut seen = HashSet::new();
        for test_file in rows.flatten() {
            if seen.insert(test_file.clone()) {
                suggested_tests.push(test_file);
            }
        }
        Ok(suggested_tests)
    }

    pub(crate) fn symbol_names_by_uid(&self, uids: &[String]) -> CcResult<HashMap<String, String>> {
        if uids.is_empty() {
            return Ok(HashMap::new());
        }

        let conn = self.db.read_conn()?;
        let mut map = HashMap::new();
        let batch_size = 200;
        for batch in uids.chunks(batch_size) {
            let placeholders = (0..batch.len())
                .map(|idx| format!("?{}", idx + 1))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT symbol_uid, name FROM symbols WHERE symbol_uid IN ({})",
                placeholders
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|err| CcError::Database(err.to_string()))?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|uid| uid as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|err| CcError::Database(err.to_string()))?;
            for (uid, name) in rows.flatten() {
                map.insert(uid, name);
            }
        }
        Ok(map)
    }
}

fn bfs_paths_labeled_with<F>(
    from_uid: &str,
    to_uid: &str,
    max_depth: usize,
    max_paths: usize,
    mut neighbors: F,
) -> Vec<LabeledPath>
where
    F: FnMut(&str) -> Vec<EdgeLite>,
{
    let mut results = Vec::new();
    let mut queue: VecDeque<(Vec<String>, Vec<EdgeLite>)> = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back((vec![from_uid.to_string()], Vec::new()));
    visited.insert(from_uid.to_string());

    while let Some((nodes, edges)) = queue.pop_front() {
        if results.len() >= max_paths {
            break;
        }
        if nodes.len() > max_depth + 1 {
            break;
        }
        let current = nodes.last().expect("path has at least one uid").clone();
        if current == to_uid {
            results.push(LabeledPath {
                node_uids: nodes,
                edge_lites: edges,
            });
            continue;
        }

        for edge in neighbors(&current) {
            if !visited.contains(&edge.callee_uid) {
                visited.insert(edge.callee_uid.clone());
                let mut new_nodes = nodes.clone();
                new_nodes.push(edge.callee_uid.clone());
                let mut new_edges = edges.clone();
                new_edges.push(edge);
                queue.push_back((new_nodes, new_edges));
            }
        }
    }

    results
}
