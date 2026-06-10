//! Shared graph read model for graph/impact tools.
//!
//! This Module keeps adjacency loading, neighborhood BFS, and edge projection in
//! one place so trace/flow/cycles/impact can share the same read path without
//! changing their external output shape.

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_db::sql_util::{sql_in_placeholders, IN_BATCH_SIZE};
use cc_model::edge::SemanticRelation;
use cc_model::{CcError, CcResult};
use lru::LruCache;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, OnceLock};

use crate::graph_types::{
    BfsAdj, EdgeLite, InternalEdge, LabeledPath, TraceEdge, TraceEdgeEvidence,
};

type BridgeEdgesByCaller = HashMap<String, Vec<EdgeLite>>;
type SharedBridgeEdges = Arc<BridgeEdgesByCaller>;

/// One process-global generation-cache slot: project identity -> the value
/// computed for that project's latest `GraphReadGeneration`.
type GenerationSlot<T> = OnceLock<Mutex<LruCache<usize, (GraphReadGeneration, Arc<T>)>>>;

/// Process-global adjacency cache shared across all `GraphReadModel` instances.
///
/// The inner map is keyed by caller UID → outgoing edges.
type SharedAdjacency = Arc<Mutex<HashMap<String, Vec<EdgeLite>>>>;

/// Per-project capacity for the process-global graph caches. Aligned with the
/// project-session LRU so a multi-project workload keeps each project's graph
/// hot instead of thrashing a single shared slot. Override with
/// CODECORTEX_GRAPH_CACHE_SIZE.
fn graph_cache_capacity() -> NonZeroUsize {
    std::env::var("CODECORTEX_GRAPH_CACHE_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(NonZeroUsize::new)
        .unwrap_or(NonZeroUsize::new(16).unwrap())
}

/// Process-global adjacency cache keyed by project identity (`db_identity`).
///
/// Each project keeps a single slot holding its latest `GraphReadGeneration`; an
/// incremental rebuild replaces the slot in place, while distinct projects coexist
/// up to `graph_cache_capacity()` so multi-project workloads do not thrash.
static ADJ_CACHE: GenerationSlot<Mutex<HashMap<String, Vec<EdgeLite>>>> = OnceLock::new();

/// Process-global bridge edge cache, keyed by `db_identity`.
static BRIDGE_CACHE: GenerationSlot<BridgeEdgesByCaller> = OnceLock::new();

// ---------------------------------------------------------------------------
// Semantic edge cache
// ---------------------------------------------------------------------------

/// Lightweight projection of `SemanticEdgeRecord` for in-memory caching.
#[derive(Debug, Clone)]
pub(crate) struct SemanticEdgeLite {
    pub source_symbol_uid: String,
    pub target_symbol_uid: String,
    pub source_symbol: String,
    pub target_symbol: String,
    pub relation_kind: SemanticRelation,
    #[allow(dead_code)]
    pub confidence: f64,
}

struct SemanticAdjPair {
    by_source: HashMap<String, Vec<SemanticEdgeLite>>,
    by_target: HashMap<String, Vec<SemanticEdgeLite>>,
}

type SharedSemanticAdj = Arc<Mutex<SemanticAdjPair>>;

/// Process-global semantic edge cache, keyed by `db_identity`.
static SEMANTIC_CACHE: GenerationSlot<Mutex<SemanticAdjPair>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Import adjacency cache
// ---------------------------------------------------------------------------

static IMPORT_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Community adjacency cache
// ---------------------------------------------------------------------------

static COMMUNITY_ADJ_CACHE: GenerationSlot<HashMap<String, Vec<String>>> = OnceLock::new();

// ---------------------------------------------------------------------------
// Dead-code caller set cache (callee UIDs with at least one non-self caller)
// ---------------------------------------------------------------------------

type SharedCalleeSet = Arc<HashSet<String>>;

static CALLEES_WITH_CALLERS_CACHE: GenerationSlot<HashSet<String>> = OnceLock::new();

/// Clear the process-global bridge edge cache.
///
/// Call this when runtime evidence (e.g. ingested traces) may have invalidated
/// the synthesized HTTP bridge edges.
pub(crate) fn clear_bridge_cache() {
    if let Some(cache) = BRIDGE_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.clear();
        }
    }
}

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

/// Shared generation-cache ritual: look up `slot` by project identity, hit
/// only when the stored generation matches, otherwise compute outside the
/// lock and install the fresh value under the latest generation.
///
/// A failed `compute` is NOT cached — the error is returned and the next call
/// retries, so per-request degradation at the call site never becomes sticky
/// for the whole generation.
fn generation_cached<T>(
    slot: &'static GenerationSlot<T>,
    generation: &GraphReadGeneration,
    compute: impl FnOnce() -> CcResult<Arc<T>>,
) -> CcResult<Arc<T>> {
    let cache = slot.get_or_init(|| Mutex::new(LruCache::new(graph_cache_capacity())));
    if let Ok(mut guard) = cache.lock() {
        if let Some((stored_gen, value)) = guard.get(&generation.db_identity) {
            if stored_gen == generation {
                return Ok(Arc::clone(value));
            }
        }
    }

    // Miss, or same project with a newer generation: compute outside the
    // lock, then replace this project's slot with the latest generation.
    let value = compute()?;
    if let Ok(mut guard) = cache.lock() {
        guard.put(
            generation.db_identity,
            (generation.clone(), Arc::clone(&value)),
        );
    }
    Ok(value)
}

#[derive(Debug, Clone)]
pub(crate) struct GraphSymbolLite {
    pub symbol_uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub community_id: Option<u32>,
}

/// Symbol with no incoming callers and no external references.
#[derive(Debug, Clone)]
pub(crate) struct DeadCodeCandidate {
    pub name: String,
    pub uid: String,
    pub file_path: String,
    pub kind: String,
}

/// Infra nodes, routes, and connecting edges matched for a service/route query.
pub(crate) struct ServiceBindings {
    pub matched_infra_nodes: Vec<serde_json::Value>,
    pub matched_routes: Vec<serde_json::Value>,
    pub related_edges: Vec<serde_json::Value>,
}

/// Read-only graph view with process-global adjacency caches.
pub(crate) struct GraphReadModel {
    db: Arc<IndexDb>,
    generation: GraphReadGeneration,
    /// Process-global adjacency cache shared across all instances of the same
    /// generation.  Populated lazily by `neighbors()`.
    shared_adjacency: SharedAdjacency,
    /// Pre-loaded HTTP/async bridge edges keyed by caller UID.
    http_bridges: SharedBridgeEdges,
    /// Process-global semantic edge cache, lazily loaded on first access.
    shared_semantic: SharedSemanticAdj,
}

impl GraphReadModel {
    /// Build a read model for trace/flow traversal, including HTTP/async bridge
    /// edges so callers see the same synthesized paths as before.
    pub(crate) fn new(db: Arc<IndexDb>) -> CcResult<Self> {
        let generation = GraphReadGeneration::from_db(&db);
        let http_bridges = Self::cached_bridge_edges_by_caller(&db, &generation)?;
        let shared_adjacency = Self::cached_adjacency(&generation);
        let shared_semantic = Self::cached_semantic_adjacency(&generation);
        Ok(Self {
            db,
            generation,
            shared_adjacency,
            http_bridges,
            shared_semantic,
        })
    }

    /// Build a read model for operations that do not need HTTP bridge edges.
    pub(crate) fn without_http_bridges(db: Arc<IndexDb>) -> Self {
        let generation = GraphReadGeneration::from_db(&db);
        let shared_adjacency = Self::cached_adjacency(&generation);
        let shared_semantic = Self::cached_semantic_adjacency(&generation);
        Self {
            db,
            generation,
            shared_adjacency,
            http_bridges: Arc::new(HashMap::new()),
            shared_semantic,
        }
    }

    /// Get or create the process-global shared adjacency map for `gen`.
    ///
    /// Keyed by project identity: a cache hit requires both the same project and
    /// the same generation. A new generation for the same project (e.g. after an
    /// incremental rebuild) replaces the project's slot with a fresh empty map so
    /// callers never read stale edges.
    fn cached_adjacency(gen: &GraphReadGeneration) -> SharedAdjacency {
        // A miss installs a fresh empty adjacency under the latest generation
        // for this project's slot; `neighbors()` populates it lazily.
        generation_cached(&ADJ_CACHE, gen, || Ok(Arc::new(Mutex::new(HashMap::new()))))
            .expect("empty adjacency construction is infallible")
    }

    /// Get or create the process-global shared semantic adjacency for `gen`.
    ///
    /// Mirrors `cached_adjacency()`: keyed by project identity, a cache hit
    /// requires both the same project and the same generation.
    fn cached_semantic_adjacency(gen: &GraphReadGeneration) -> SharedSemanticAdj {
        generation_cached(&SEMANTIC_CACHE, gen, || {
            Ok(Arc::new(Mutex::new(SemanticAdjPair {
                by_source: HashMap::new(),
                by_target: HashMap::new(),
            })))
        })
        .expect("empty semantic adjacency construction is infallible")
    }

    #[allow(dead_code)]
    pub(crate) fn generation(&self) -> &GraphReadGeneration {
        &self.generation
    }

    /// Lazily load all semantic edges into the shared cache on first access.
    ///
    /// Returns the mutex guard so callers can query `by_source` / `by_target`
    /// without a second lock acquisition.
    fn ensure_semantic_loaded(&self) -> std::sync::MutexGuard<'_, SemanticAdjPair> {
        let mut guard = self.shared_semantic.lock().unwrap();
        if guard.by_source.is_empty() && guard.by_target.is_empty() {
            if let Ok(edges) = self.db.query_semantic_edges(None, None, None) {
                for e in edges {
                    let lite = SemanticEdgeLite {
                        source_symbol_uid: e.source_symbol_uid.clone().unwrap_or_default(),
                        target_symbol_uid: e.target_symbol_uid.clone().unwrap_or_default(),
                        source_symbol: e.source_symbol.clone(),
                        target_symbol: e.target_symbol.clone(),
                        relation_kind: e.relation_kind,
                        confidence: e.confidence,
                    };
                    if !lite.source_symbol_uid.is_empty() {
                        guard
                            .by_source
                            .entry(lite.source_symbol_uid.clone())
                            .or_default()
                            .push(lite.clone());
                    }
                    if !lite.target_symbol_uid.is_empty() {
                        guard
                            .by_target
                            .entry(lite.target_symbol_uid.clone())
                            .or_default()
                            .push(lite);
                    }
                }
            }
        }
        guard
    }

    /// Get semantic edges originating from `source_uid` (e.g. for ancestor BFS).
    pub(crate) fn semantic_edges_from(&self, source_uid: &str) -> Vec<SemanticEdgeLite> {
        let guard = self.ensure_semantic_loaded();
        guard.by_source.get(source_uid).cloned().unwrap_or_default()
    }

    /// Get semantic edges pointing to `target_uid` (e.g. for descendant BFS).
    pub(crate) fn semantic_edges_to(&self, target_uid: &str) -> Vec<SemanticEdgeLite> {
        let guard = self.ensure_semantic_loaded();
        guard.by_target.get(target_uid).cloned().unwrap_or_default()
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
        let limit: usize = std::env::var("CODECORTEX_BRIDGE_EDGE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);
        let http_edges = db.all_http_call_edges_lite(limit)?;
        let route_nodes = db.all_route_nodes_lite(limit)?;
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

    fn cached_bridge_edges_by_caller(
        db: &Arc<IndexDb>,
        generation: &GraphReadGeneration,
    ) -> CcResult<SharedBridgeEdges> {
        generation_cached(&BRIDGE_CACHE, generation, || {
            Ok(Arc::new(Self::bridge_edges_by_caller(db)?))
        })
    }

    /// Return outgoing edges for a caller UID, backed by a process-global cache.
    ///
    /// The adjacency map is shared across all `GraphReadModel` instances of the
    /// same generation, so edges queried in one tool call are reused by later
    /// calls without hitting SQLite again.
    pub(crate) fn neighbors(&self, uid: &str) -> Vec<EdgeLite> {
        // Fast path: check if the UID is already cached.
        {
            let adj = self.shared_adjacency.lock().unwrap();
            if let Some(edges) = adj.get(uid) {
                return edges.clone();
            }
        }
        // Slow path: query DB *without* holding the adjacency lock.
        let mut edges = self.db.call_edges_from_uid_lite(uid).unwrap_or_default();
        if let Some(bridges) = self.http_bridges.get(uid) {
            edges.extend(bridges.iter().cloned());
        }
        // Insert and return.
        let mut adj = self.shared_adjacency.lock().unwrap();
        // Another thread may have inserted between our read and write; use the
        // existing entry if present so callers see a consistent snapshot.
        adj.entry(uid.to_string()).or_insert(edges).clone()
    }

    pub(crate) fn paths_between(
        &self,
        from_uid: &str,
        to_uid: &str,
        max_depth: usize,
        max_paths: usize,
    ) -> Vec<LabeledPath> {
        bfs_paths_labeled_with(from_uid, to_uid, max_depth, max_paths, |uid| {
            self.neighbors(uid)
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
        let shared = generation_cached(&IMPORT_ADJ_CACHE, &self.generation, || {
            // Compute via identity projection.
            Ok(Arc::new(
                self.projected_import_adjacency(|path| path.to_string())?,
            ))
        })?;
        Ok(HashMap::clone(&shared))
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
        let shared = generation_cached(&COMMUNITY_ADJ_CACHE, &self.generation, || {
            let rows = self.db.query_json(
                "SELECT DISTINCT s1.community_id AS from_community, s2.community_id AS to_community \
                 FROM call_edges ce \
                 JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
                 JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
                 WHERE s1.community_id IS NOT NULL \
                   AND s2.community_id IS NOT NULL \
                   AND s1.community_id != s2.community_id",
                &[],
            )?;

            let mut adj: HashMap<String, Vec<String>> = HashMap::new();
            for row in &rows {
                let from = row.get("from_community");
                let to = row.get("to_community");
                if let (Some(from_val), Some(to_val)) = (from, to) {
                    let from_str = match from_val {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => continue,
                    };
                    let to_str = match to_val {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => continue,
                    };
                    adj.entry(from_str).or_default().push(to_str);
                }
            }

            // Rows are DISTINCT, but multiple rows can share the same from_community.
            for targets in adj.values_mut() {
                targets.sort();
                targets.dedup();
            }
            Ok(Arc::new(adj))
        })?;
        Ok(HashMap::clone(&shared))
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
        limit: Option<usize>,
    ) -> CcResult<Vec<GraphSymbolLite>> {
        let conn = self.db.read_conn()?;
        let mut callers = Vec::new();

        for batch in callee_uids.chunks(IN_BATCH_SIZE) {
            if batch.is_empty() {
                continue;
            }

            let placeholders = sql_in_placeholders(batch.len());
            // Parameter slots after the IN(...) uids: optional confidence
            // threshold, then optional LIMIT, in that bind order.
            let mut next_param = batch.len() + 1;
            let conf_clause = if confidence_threshold.is_some() {
                let clause = format!("AND ce.parser_confidence >= ?{}", next_param);
                next_param += 1;
                clause
            } else {
                String::new()
            };
            let limit_clause = if limit.is_some() {
                format!("LIMIT ?{}", next_param)
            } else {
                String::new()
            };
            let sql = format!(
                "SELECT DISTINCT ce.caller_symbol_uid, s.name, s.file_path, s.kind, s.community_id \
                 FROM call_edges ce \
                 JOIN symbols s ON s.symbol_uid = ce.caller_symbol_uid \
                 WHERE ce.callee_symbol_uid IN ({}) \
                 AND ce.caller_symbol_uid IS NOT NULL \
                 {} \
                 {}",
                placeholders, conf_clause, limit_clause
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
            if let Some(cap) = limit {
                params.push(Box::new(cap as i64));
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
        let placeholders = sql_in_placeholders(files.len());
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
        for batch in uids.chunks(IN_BATCH_SIZE) {
            let sql = format!(
                "SELECT symbol_uid, name FROM symbols WHERE symbol_uid IN ({})",
                sql_in_placeholders(batch.len())
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

    /// Files that depend on (import) `file_path`, direct plus 2-hop transitive,
    /// deduplicated and sorted. Two bounded SQL queries: 200 direct importers,
    /// 10k transitive rows — no full-table materialization.
    pub(crate) fn dependents_of_file(&self, file_path: &str) -> CcResult<Vec<String>> {
        // Direct importers, bounded in SQL; self-imports are excluded in the
        // query so the cap counts only real dependents. A failure here
        // propagates to the caller.
        let rows = self.db.query_json(
            "SELECT DISTINCT file_path FROM imports \
             WHERE resolved_path = ?1 AND file_path != ?1 \
             ORDER BY file_path LIMIT 200",
            &[file_path.to_string()],
        )?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut dependents: Vec<String> = Vec::new();
        for row in &rows {
            if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
                if seen.insert(fp.to_string()) {
                    dependents.push(fp.to_string());
                }
            }
        }

        // 2-hop: importers of the direct dependents in a single batched query.
        // A failure here degrades to direct-only results (partial answer beats
        // none), matching the original handler behavior.
        if !dependents.is_empty() {
            let sql = format!(
                "SELECT DISTINCT file_path FROM imports WHERE resolved_path IN ({}) LIMIT 10000",
                sql_in_placeholders(dependents.len())
            );
            let rows2 = self.db.query_json(&sql, &dependents).unwrap_or_default();
            let mut transitive: Vec<String> = Vec::new();
            for row in &rows2 {
                if let Some(fp) = row.get("file_path").and_then(|v| v.as_str()) {
                    if fp != file_path && seen.insert(fp.to_string()) {
                        transitive.push(fp.to_string());
                    }
                }
            }
            dependents.extend(transitive);
        }

        dependents.sort();
        Ok(dependents)
    }

    /// Callee UIDs that have at least one non-self caller. Cached per
    /// generation on success; derived purely from `call_edges`.
    fn callees_with_external_callers(&self) -> SharedCalleeSet {
        let computed = generation_cached(&CALLEES_WITH_CALLERS_CACHE, &self.generation, || {
            let rows = self.db.query_json(
                "SELECT DISTINCT callee_symbol_uid FROM call_edges \
                 WHERE callee_symbol_uid IS NOT NULL \
                   AND (caller_symbol_uid IS NULL OR caller_symbol_uid != callee_symbol_uid) \
                 LIMIT 10000",
                &[],
            )?;
            let mut has_callers = HashSet::new();
            for row in &rows {
                if let Some(uid) = row.get("callee_symbol_uid").and_then(|v| v.as_str()) {
                    has_callers.insert(uid.to_string());
                }
            }
            Ok(Arc::new(has_callers))
        });
        match computed {
            Ok(set) => set,
            // A query failure degrades to "no callers known" for THIS request
            // only (matching the old per-call unwrap_or_default). The empty
            // set is never cached, so the next request retries the query
            // instead of mass-reporting dead code for the whole generation.
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "dead-code caller query failed; degrading to uncached empty caller set"
                );
                Arc::new(HashSet::new())
            }
        }
    }

    /// Symbols that appear to be dead code: no non-self callers and no
    /// external references. `scope` filters by file-path prefix; `scan_limit`
    /// bounds the symbols scan. Result is untruncated (callers clip it).
    pub(crate) fn dead_code_candidates(
        &self,
        scope: Option<&str>,
        scan_limit: usize,
    ) -> CcResult<Vec<DeadCodeCandidate>> {
        let all_symbols = if let Some(prefix) = scope {
            let pattern = format!("%{}%", prefix);
            self.db.query_json(
                &format!(
                    "SELECT name, symbol_uid, file_path, kind FROM symbols \
                     WHERE file_path LIKE ?1 LIMIT {}",
                    scan_limit
                ),
                &[pattern],
            )?
        } else {
            self.db.query_json(
                &format!(
                    "SELECT name, symbol_uid, file_path, kind FROM symbols LIMIT {}",
                    scan_limit
                ),
                &[],
            )?
        };

        let has_callers = self.callees_with_external_callers();

        let excluded_names = ["main", "__init__", "__main__", "setup", "configure"];
        let excluded_prefixes = ["test_", "Test"];

        // Phase 1: scope / exclusion / no-caller filters. Reference status is
        // resolved in a batched second pass to avoid per-candidate queries.
        let mut candidates: Vec<DeadCodeCandidate> = Vec::new();
        for row in &all_symbols {
            let name = row.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
            let file_path = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("");

            if uid.is_empty() || name.is_empty() {
                continue;
            }
            if let Some(prefix) = scope {
                if !file_path.starts_with(prefix) {
                    continue;
                }
            }
            if excluded_names.contains(&name) {
                continue;
            }
            if excluded_prefixes.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            if !has_callers.contains(uid) {
                candidates.push(DeadCodeCandidate {
                    name: name.to_string(),
                    uid: uid.to_string(),
                    file_path: file_path.to_string(),
                    kind: kind.to_string(),
                });
            }
        }

        // Phase 2: batch-load references and drop candidates with an
        // *external* reference (container differs from the symbol's own name).
        let mut uids_with_external_refs: HashSet<String> = HashSet::new();
        let candidate_uids: Vec<String> = candidates.iter().map(|c| c.uid.clone()).collect();
        let name_by_uid: HashMap<&str, &str> = candidates
            .iter()
            .map(|c| (c.uid.as_str(), c.name.as_str()))
            .collect();
        for batch in candidate_uids.chunks(IN_BATCH_SIZE) {
            let sql = format!(
                "SELECT target_symbol_uid, container FROM symbol_refs \
                 WHERE target_symbol_uid IN ({})",
                sql_in_placeholders(batch.len())
            );
            let ref_rows = self.db.query_json(&sql, batch).unwrap_or_default();
            for row in &ref_rows {
                let target_uid = row
                    .get("target_symbol_uid")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if target_uid.is_empty() {
                    continue;
                }
                let container = row.get("container").and_then(|v| v.as_str());
                let own_name = name_by_uid.get(target_uid).copied();
                let is_external = match (container, own_name) {
                    (None, _) => true,
                    (Some(c), Some(n)) => c != n,
                    (Some(_), None) => true,
                };
                if is_external {
                    uids_with_external_refs.insert(target_uid.to_string());
                }
            }
        }

        candidates.retain(|cand| !uids_with_external_refs.contains(&cand.uid));
        Ok(candidates)
    }

    /// HTTP route handler rows matching the optional `route_path` LIKE pattern,
    /// post-filtered case-insensitively by method/framework.
    pub(crate) fn route_handlers(
        &self,
        route_path: Option<&str>,
        method: Option<&str>,
        framework: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<serde_json::Value>> {
        let rows = if let Some(pattern) = route_path {
            let like_pattern = format!("%{}%", pattern);
            self.db.query_json(
                &format!(
                    "SELECT route_path, method, handler_name, file_path, framework, line \
                     FROM routes WHERE route_path LIKE ?1 LIMIT {}",
                    limit
                ),
                &[like_pattern],
            )?
        } else {
            self.db.query_json(
                &format!(
                    "SELECT route_path, method, handler_name, file_path, framework, line \
                     FROM routes LIMIT {}",
                    limit
                ),
                &[],
            )?
        };

        let method_filter = normalize_bridge_method(method);
        let filtered = rows
            .into_iter()
            .filter(|row| {
                if let Some(wanted) = &method_filter {
                    let row_method =
                        normalize_bridge_method(row.get("method").and_then(|v| v.as_str()));
                    if row_method.as_ref() != Some(wanted) {
                        return false;
                    }
                }
                if let Some(fw) = framework {
                    let row_fw = row.get("framework").and_then(|v| v.as_str()).unwrap_or("");
                    if !row_fw.eq_ignore_ascii_case(fw) {
                        return false;
                    }
                }
                true
            })
            .collect();
        Ok(filtered)
    }

    /// Consumers of a topic/queue: infra edges with kind in
    /// (binds_topic, consumes_queue) whose source name or properties match.
    pub(crate) fn async_consumers(&self, topic_or_queue: &str) -> CcResult<Vec<serde_json::Value>> {
        let pattern = format!("%{}%", topic_or_queue);
        self.db.query_json(
            "SELECT ie.edge_id, ie.source_node_id, ie.target_node_id, ie.kind, \
                    ie.confidence, ie.properties, \
                    src.name AS source_name, src.kind AS source_kind, \
                    src.file_path AS source_file, \
                    src.bound_symbol_uid AS source_bound_uid, \
                    CASE \
                        WHEN tgt_route.route_id IS NOT NULL THEN 'route' \
                        WHEN tgt_infra.node_id IS NOT NULL THEN 'infra_node' \
                        ELSE 'unknown' \
                    END AS target_type, \
                    COALESCE(tgt_infra.name, tgt_route.handler_name) AS target_name, \
                    tgt_infra.kind AS target_kind, \
                    COALESCE(tgt_infra.file_path, tgt_route.file_path) AS target_file, \
                    COALESCE(tgt_infra.bound_symbol_uid, tgt_route.handler_symbol_uid) AS target_bound_uid, \
                    tgt_route.route_path AS target_route_path, \
                    tgt_route.method AS target_method, \
                    tgt_route.handler_symbol_uid AS target_handler_symbol_uid \
             FROM infra_edges ie \
             LEFT JOIN infra_nodes src ON ie.source_node_id = src.node_id \
             LEFT JOIN infra_nodes tgt_infra ON ie.target_node_id = tgt_infra.node_id \
             LEFT JOIN routes tgt_route ON ie.target_node_id = tgt_route.route_id \
             WHERE ie.kind IN ('binds_topic', 'consumes_queue') \
               AND (src.name LIKE ?1 OR ie.properties LIKE ?1)",
            &[pattern],
        )
    }

    /// Infra bindings for a service or route, matched on two dimensions
    /// (infra node name/bound UID, route path/handler) plus connecting edges.
    pub(crate) fn service_bindings(&self, service_or_route: &str) -> CcResult<ServiceBindings> {
        let pattern = format!("%{}%", service_or_route);

        let matched_infra_nodes = self.db.query_json(
            "SELECT node_id, file_path, kind, name, namespace, line, end_line, \
                    properties, bound_symbol_uid, binding_confidence \
             FROM infra_nodes \
             WHERE name LIKE ?1 OR bound_symbol_uid LIKE ?1",
            std::slice::from_ref(&pattern),
        )?;

        let infra_node_ids = matched_infra_nodes.iter().filter_map(|node| {
            node.get("node_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

        let matched_routes = self.db.query_json(
            "SELECT route_id, file_path, route_path, method, handler_symbol_uid, \
                    handler_name, framework, line, end_line, normalized_path, confidence \
             FROM routes \
             WHERE route_path LIKE ?1 \
                OR normalized_path LIKE ?1 \
                OR handler_name LIKE ?1 \
                OR handler_symbol_uid LIKE ?1",
            &[pattern],
        )?;

        let route_ids = matched_routes.iter().filter_map(|route| {
            route
                .get("route_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

        // All matched IDs (infra node_ids + route_ids) feed one edge query.
        let all_ids: Vec<String> = infra_node_ids.chain(route_ids).collect();
        let related_edges = if all_ids.is_empty() {
            Vec::new()
        } else {
            let ph_str = sql_in_placeholders(all_ids.len());
            let sql = format!(
                "SELECT ie.edge_id, ie.source_node_id, ie.target_node_id, ie.kind, \
                        ie.confidence, ie.properties, \
                        src.name AS source_name, src.kind AS source_kind, \
                        CASE \
                            WHEN tgt_route.route_id IS NOT NULL THEN 'route' \
                            WHEN tgt_infra.node_id IS NOT NULL THEN 'infra_node' \
                            ELSE 'unknown' \
                        END AS target_type, \
                        COALESCE(tgt_infra.name, tgt_route.handler_name) AS target_name, \
                        COALESCE(tgt_infra.kind, 'route') AS target_kind, \
                        tgt_route.route_path AS target_route_path, \
                        tgt_route.method AS target_method, \
                        tgt_route.handler_symbol_uid AS target_handler_symbol_uid \
                 FROM infra_edges ie \
                 LEFT JOIN infra_nodes src ON ie.source_node_id = src.node_id \
                 LEFT JOIN infra_nodes tgt_infra ON ie.target_node_id = tgt_infra.node_id \
                 LEFT JOIN routes tgt_route ON ie.target_node_id = tgt_route.route_id \
                 WHERE ie.source_node_id IN ({ph}) OR ie.target_node_id IN ({ph})",
                ph = ph_str,
            );
            self.db.query_json(&sql, &all_ids)?
        };

        Ok(ServiceBindings {
            matched_infra_nodes,
            matched_routes,
            related_edges,
        })
    }
}

fn normalize_bridge_method(method: Option<&str>) -> Option<String> {
    method
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(|method| method.to_ascii_uppercase())
}

fn bfs_paths_labeled_with<F>(
    from_uid: &str,
    to_uid: &str,
    max_depth: usize,
    max_paths: usize,
    neighbors: F,
) -> Vec<LabeledPath>
where
    F: FnMut(&str) -> Vec<EdgeLite>,
{
    let budget = crate::graph_walk::WalkBudget::for_path_enumeration(max_depth, max_paths);
    crate::graph_walk::bfs_simple_paths(from_uid, to_uid, &budget, neighbors, |edge: &EdgeLite| {
        Some(edge.callee_uid.as_str())
    })
    .into_iter()
    .map(|(node_uids, edge_lites)| LabeledPath {
        node_uids,
        edge_lites,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_bridge_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        let conn = db.read_conn().unwrap();

        for file_path in ["src/client.ts", "src/routes.ts"] {
            conn.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
                 VALUES(?1,'TypeScript',?2,1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                rusqlite::params![file_path, format!("hash:{file_path}")],
            )
            .unwrap();
        }

        for (edge_id, method, handler_uid, handler_name, confidence) in [
            (
                "route_get_users",
                "GET",
                "handler_get_users",
                "get_users",
                0.91,
            ),
            (
                "route_post_users",
                "POST",
                "handler_post_users",
                "post_users",
                0.97,
            ),
        ] {
            conn.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES(?1,'src/routes.ts','/api/users',?2,?3,?4,'express',10,12,'/api/users',?5,'tree_sitter',?1)",
                rusqlite::params![edge_id, method, handler_uid, handler_name, confidence],
            )
            .unwrap();
        }

        (tmp, db)
    }

    fn insert_http_call(
        db: &IndexDb,
        edge_id: &str,
        caller_uid: &str,
        method: Option<&str>,
        call_kind: &str,
    ) {
        db.read_conn()
            .unwrap()
            .execute(
                "INSERT INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,normalized_path,method,call_kind,line,confidence,parser_tier)
                 VALUES(?1,'src/client.ts',?2,'/api/users','/api/users',?3,?4,20,0.88,'tree_sitter')",
                rusqlite::params![edge_id, caller_uid, method, call_kind],
            )
            .unwrap();
    }

    fn insert_methodless_route(db: &IndexDb, edge_id: &str, handler_uid: &str) {
        db.read_conn()
            .unwrap()
            .execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES(?1,'src/routes.ts','/api/users',NULL,?2,?2,'express',30,32,'/api/users',0.83,'tree_sitter',?1)",
                rusqlite::params![edge_id, handler_uid],
            )
            .unwrap();
    }

    fn sorted_callees(edges: &[EdgeLite]) -> Vec<String> {
        let mut callees: Vec<String> = edges.iter().map(|edge| edge.callee_uid.clone()).collect();
        callees.sort();
        callees
    }

    #[test]
    fn bridge_edges_match_http_method_and_normalized_path() {
        let (_tmp, db) = setup_bridge_db();
        insert_http_call(
            &db,
            "http_get_users",
            "caller_get_users",
            Some("GET"),
            "http",
        );

        let bridges = GraphReadModel::bridge_edges_by_caller(&db).unwrap();
        let caller_edges = bridges
            .get("caller_get_users")
            .expect("GET caller should get bridge edges");

        assert_eq!(sorted_callees(caller_edges), vec!["handler_get_users"]);
        assert!(caller_edges
            .iter()
            .all(|edge| edge.synthesis_key.as_deref() == Some("/api/users")));
    }

    #[test]
    fn bridge_edges_keep_methodless_routes_as_method_specific_fallback() {
        let (_tmp, db) = setup_bridge_db();
        insert_methodless_route(&db, "route_any_users", "handler_any_users");
        insert_http_call(
            &db,
            "http_get_users",
            "caller_get_users",
            Some("GET"),
            "http",
        );

        let bridges = GraphReadModel::bridge_edges_by_caller(&db).unwrap();
        let caller_edges = bridges
            .get("caller_get_users")
            .expect("GET caller should get exact and methodless bridge edges");

        assert_eq!(
            sorted_callees(caller_edges),
            vec!["handler_any_users", "handler_get_users"]
        );
    }

    #[test]
    fn bridge_edges_without_http_method_fall_back_to_normalized_path() {
        let (_tmp, db) = setup_bridge_db();
        insert_http_call(
            &db,
            "http_unknown_users",
            "caller_unknown_users",
            None,
            "http",
        );

        let bridges = GraphReadModel::bridge_edges_by_caller(&db).unwrap();
        let caller_edges = bridges
            .get("caller_unknown_users")
            .expect("method-less caller should get path fallback bridge edges");

        assert_eq!(
            sorted_callees(caller_edges),
            vec!["handler_get_users", "handler_post_users"]
        );
        assert!(caller_edges.iter().all(|edge| {
            edge.dispatch_kind == "http_bridge"
                && edge.synthesized_by.as_deref() == Some("http_bridge")
        }));
    }

    #[test]
    fn bridge_edges_keep_non_http_synthesis_metadata_consistent() {
        let (_tmp, db) = setup_bridge_db();
        insert_http_call(
            &db,
            "async_get_users",
            "caller_async_users",
            Some("GET"),
            "message",
        );

        let bridges = GraphReadModel::bridge_edges_by_caller(&db).unwrap();
        let edge = bridges
            .get("caller_async_users")
            .and_then(|edges| edges.first())
            .expect("async caller should get a bridge edge");

        assert_eq!(edge.dispatch_kind, "async_bridge");
        assert_eq!(edge.synthesized_by.as_deref(), Some("async_bridge"));
        assert_eq!(edge.callee_uid, "handler_get_users");
    }

    /// DB with one file and a unique generation marker so the process-global
    /// caches cannot collide with other tests reusing a freed Arc address.
    fn setup_callee_db(tag: &str) -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap().0);
        db.set_metadata("last_indexed_at", &format!("{tag}-{:p}", Arc::as_ptr(&db)))
            .unwrap();
        db.read_conn()
            .unwrap()
            .execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
                 VALUES('src/app.ts','TypeScript','hash',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        (tmp, db)
    }

    fn insert_callee_edge(db: &IndexDb, edge_id: &str, caller_uid: &str, callee_uid: &str) {
        db.read_conn()
            .unwrap()
            .execute(
                "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid)
                 VALUES(?1, 'src/app.ts', 'callee', 5, ?2, ?3)",
                rusqlite::params![edge_id, caller_uid, callee_uid],
            )
            .unwrap();
    }

    #[test]
    fn callees_with_callers_caches_successful_query_per_generation() {
        let (_tmp, db) = setup_callee_db("callees-ok");
        insert_callee_edge(&db, "ce1", "uid_caller", "uid_callee");

        let grm = GraphReadModel::without_http_bridges(Arc::clone(&db));
        let first = grm.callees_with_external_callers();
        assert!(first.contains("uid_callee"));

        // Delete the underlying rows: a second call within the same generation
        // must be served from the cache and still see the callee.
        db.read_conn()
            .unwrap()
            .execute("DELETE FROM call_edges", [])
            .unwrap();
        let second = grm.callees_with_external_callers();
        assert!(
            second.contains("uid_callee"),
            "successful query result should be cached for the generation"
        );
    }

    #[test]
    fn callees_with_callers_failure_is_not_cached() {
        let (_tmp, db) = setup_callee_db("callees-err");
        let grm = GraphReadModel::without_http_bridges(Arc::clone(&db));

        // Capture the table schema, then drop it to force a query failure.
        let schema: String = db
            .read_conn()
            .unwrap()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='call_edges'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        db.read_conn()
            .unwrap()
            .execute("DROP TABLE call_edges", [])
            .unwrap();

        // Failure degrades to an empty set for this request only.
        let degraded = grm.callees_with_external_callers();
        assert!(degraded.is_empty());

        // Restore the table and add an edge: the next call must see it, i.e.
        // the failed (empty) result must NOT have been cached.
        db.read_conn().unwrap().execute(&schema, []).unwrap();
        insert_callee_edge(&db, "ce1", "uid_caller", "uid_callee");
        let recovered = grm.callees_with_external_callers();
        assert!(
            recovered.contains("uid_callee"),
            "a failed query must not stick an empty set in the generation cache"
        );
    }

    #[test]
    fn bfs_finds_multiple_paths_through_shared_node() {
        // Graph: A→B, A→C, B→D, C→D
        // Expected: two distinct paths A→B→D and A→C→D.
        fn make_edge(caller: &str, callee: &str) -> EdgeLite {
            EdgeLite {
                caller_uid: caller.to_string(),
                callee_uid: callee.to_string(),
                dispatch_kind: "call".to_string(),
                synthesized_by: None,
                synthesis_key: None,
                confidence: 1.0,
                file_path: String::new(),
                line: 0,
                registered_file: None,
                registered_line: None,
                resolution_kind: None,
                parser_tier: None,
                resolution_strategy: None,
                parser_confidence: None,
            }
        }

        let mut adj_map: HashMap<String, Vec<EdgeLite>> = HashMap::new();
        adj_map.insert(
            "A".to_string(),
            vec![make_edge("A", "B"), make_edge("A", "C")],
        );
        adj_map.insert("B".to_string(), vec![make_edge("B", "D")]);
        adj_map.insert("C".to_string(), vec![make_edge("C", "D")]);

        let adj = BfsAdj { adj: adj_map };
        let paths = GraphReadModel::paths_between_adj(&adj, "A", "D", 5, 10);

        assert_eq!(paths.len(), 2, "should find 2 paths through shared node D");

        let mut path_strs: Vec<String> = paths.iter().map(|p| p.node_uids.join("→")).collect();
        path_strs.sort();
        assert_eq!(path_strs, vec!["A→B→D", "A→C→D"]);
    }
}
