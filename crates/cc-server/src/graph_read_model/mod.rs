//! Shared graph read model for graph/impact tools.
//!
//! This module keeps adjacency loading, neighborhood BFS, and edge projection
//! in one place so trace/flow/cycles/impact can share the same read path
//! without changing their external output shape. It is split by
//! responsibility:
//!
//! - [`cache`]: process-global generation-keyed cache slots and
//!   `GraphReadGeneration` (cache identity). Caching stays in cc-server per
//!   ADR-0001.
//! - [`projections`]: call/semantic/import/community adjacency builders and
//!   the dead-code caller-set projection.
//! - [`bridges`]: HTTP/async bridge edge synthesis from routes + http edges.
//!
//! All SQL lives in cc-db typed query methods, reached through the narrow
//! [`cc_db::GraphReads`] facet (see `reads()`); this module only orchestrates
//! caching and in-memory projection.

mod bridges;
mod cache;
mod projections;

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_db::GraphReads;
use cc_model::CcResult;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::graph_types::{EdgeLite, LabeledPath, TraceEdge, TraceEdgeEvidence};

use bridges::normalize_bridge_method;
use cache::{GraphReadGeneration, SharedAdjacency, SharedBridgeEdges, SharedSemanticAdj};

#[allow(unused_imports)] // re-exported for graph_type_hierarchy
pub(crate) use projections::SemanticEdgeLite;

/// Lightweight symbol row used across impact/graph projections.
pub(crate) use cc_db::index_db::SymbolLiteRow as GraphSymbolLite;

/// Infra nodes, routes, and connecting edges matched for a service/route query.
pub(crate) use cc_db::index_db::ServiceBindingRows as ServiceBindings;

/// Symbol with no incoming callers and no external references.
#[derive(Debug, Clone)]
pub(crate) struct DeadCodeCandidate {
    pub name: String,
    pub uid: String,
    pub file_path: String,
    pub kind: String,
}

/// Read-only graph view with process-global adjacency caches.
pub(crate) struct GraphReadModel {
    db: Arc<IndexDb>,
    generation: GraphReadGeneration,
    /// Process-global adjacency cache shared across all instances of the same
    /// generation and bridge dimension. Populated lazily by `neighbors()`.
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
        let shared_adjacency = cache::cached_adjacency(&generation, true);
        let shared_semantic = cache::cached_semantic_adjacency(&generation);
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
        let shared_adjacency = cache::cached_adjacency(&generation, false);
        let shared_semantic = cache::cached_semantic_adjacency(&generation);
        Self {
            db,
            generation,
            shared_adjacency,
            http_bridges: Arc::new(HashMap::new()),
            shared_semantic,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn generation(&self) -> &GraphReadGeneration {
        &self.generation
    }

    /// The narrow cc-db read facet this model is allowed to query through.
    /// Every db call below (including `projections` and `bridges`) goes via
    /// this seam, so the model's full read surface is the [`GraphReads`]
    /// method list.
    fn reads(&self) -> GraphReads<'_> {
        GraphReads::new(&self.db)
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
        adj: &crate::graph_types::BfsAdj,
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
            self.reads()
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

    // ── Plain typed-query facades (SQL lives in cc-db) ─────────────────────

    pub(crate) fn symbols_in_files(&self, files: &[String]) -> CcResult<Vec<GraphSymbolLite>> {
        self.reads().symbols_lite_in_files(files)
    }

    pub(crate) fn reverse_callers(
        &self,
        callee_uids: &[String],
        confidence_threshold: Option<f64>,
        limit: Option<usize>,
    ) -> CcResult<Vec<GraphSymbolLite>> {
        self.reads()
            .reverse_callers(callee_uids, confidence_threshold, limit)
    }

    pub(crate) fn suggested_tests_for_files(&self, files: &[String]) -> CcResult<Vec<String>> {
        self.reads().suggested_test_files(files)
    }

    pub(crate) fn symbol_names_by_uid(&self, uids: &[String]) -> CcResult<HashMap<String, String>> {
        self.reads().symbol_names_for_uids(uids)
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
        let rows = self.reads().route_handler_rows(route_path, limit)?;

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
        self.reads().async_consumer_rows(topic_or_queue)
    }

    /// Infra bindings for a service or route, matched on two dimensions
    /// (infra node name/bound UID, route path/handler) plus connecting edges.
    pub(crate) fn service_bindings(&self, service_or_route: &str) -> CcResult<ServiceBindings> {
        self.reads().service_binding_rows(service_or_route)
    }
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
    use crate::graph_types::BfsAdj;
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

    /// DB with one file. The process-unique `IndexDb::instance_id` already
    /// guarantees the process-global caches cannot collide across tests.
    fn setup_callee_db() -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("test.db")).unwrap().0);
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
        let (_tmp, db) = setup_callee_db();
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
        let (_tmp, db) = setup_callee_db();
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
    fn index_write_invalidates_callee_cache_for_new_read_model() {
        let (_tmp, db) = setup_callee_db();
        insert_callee_edge(&db, "ce1", "uid_caller", "uid_callee");

        let first_model = GraphReadModel::without_http_bridges(Arc::clone(&db));
        assert!(first_model
            .callees_with_external_callers()
            .contains("uid_callee"));

        // A committed cc-db write bumps index_epoch; a read model built after
        // it must observe a new generation and recompute, no manual clearing.
        db.insert_synthetic_call_edges(&[cc_model::CallEdgeRecord {
            edge_id: "ce2".to_string(),
            file_path: "src/app.ts".to_string(),
            callee_symbol: "fresh".to_string(),
            line: 9,
            caller_symbol_uid: Some("uid_caller".to_string()),
            callee_symbol_uid: Some("uid_fresh_callee".to_string()),
            ..Default::default()
        }])
        .unwrap();

        let second_model = GraphReadModel::without_http_bridges(Arc::clone(&db));
        assert_ne!(first_model.generation(), second_model.generation());
        assert!(
            second_model
                .callees_with_external_callers()
                .contains("uid_fresh_callee"),
            "index_epoch bump must invalidate the per-generation callee cache"
        );
    }

    #[test]
    fn evidence_write_invalidates_bridge_and_adjacency_but_not_index_only_caches() {
        let (_tmp, db) = setup_bridge_db();
        let db = Arc::new(db);
        insert_http_call(
            &db,
            "http_get_users",
            "caller_get_users",
            Some("GET"),
            "http",
        );

        let first_model = GraphReadModel::new(Arc::clone(&db)).unwrap();
        let first_edges = first_model.neighbors("caller_get_users");
        let first_conf = first_edges
            .iter()
            .find(|edge| edge.callee_uid == "handler_get_users")
            .expect("bridge edge present")
            .confidence;
        assert!(
            (first_conf - 0.88).abs() < 1e-9,
            "min(0.88 call, 0.91 route)"
        );

        // Evidence ingestion boosts the http edge confidence and bumps
        // evidence_epoch inside cc-db — no manual clear_bridge_cache().
        db.boost_http_edge_confidence("http_get_users", 0.15)
            .unwrap();

        let second_model = GraphReadModel::new(Arc::clone(&db)).unwrap();
        // Full generation changed (bridge/adjacency caches miss) ...
        assert_ne!(first_model.generation(), second_model.generation());
        // ... but the evidence-independent key is unchanged, so semantic /
        // import / community caches survive evidence ingestion.
        assert_eq!(
            first_model.generation().index_only(),
            second_model.generation().index_only()
        );

        let second_edges = second_model.neighbors("caller_get_users");
        let second_conf = second_edges
            .iter()
            .find(|edge| edge.callee_uid == "handler_get_users")
            .expect("bridge edge present after evidence write")
            .confidence;
        assert!(
            (second_conf - 0.91).abs() < 1e-9,
            "boosted http confidence (1.0) capped by route confidence 0.91, got {second_conf}"
        );
    }

    // ── M3 regression: plain vs bridged adjacency cache isolation ─────────

    /// Bridge fixture plus a real call edge from the same caller, so both
    /// projections have content to disagree about.
    fn setup_mixed_edge_db() -> (TempDir, Arc<IndexDb>) {
        let (tmp, db) = setup_bridge_db();
        let db = Arc::new(db);
        insert_http_call(
            &db,
            "http_get_users",
            "caller_get_users",
            Some("GET"),
            "http",
        );
        db.read_conn()
            .unwrap()
            .execute(
                "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid)
                 VALUES('ce_plain', 'src/client.ts', 'helper', 7, 'caller_get_users', 'uid_helper')",
                [],
            )
            .unwrap();
        (tmp, db)
    }

    fn has_bridge_edge(edges: &[EdgeLite]) -> bool {
        edges
            .iter()
            .any(|edge| edge.synthesized_by.as_deref() == Some("http_bridge"))
    }

    #[test]
    fn plain_model_first_does_not_starve_bridged_model_of_bridge_edges() {
        let (_tmp, db) = setup_mixed_edge_db();

        // Plain model populates its adjacency first (M3: this used to write a
        // bridge-less entry into a slot shared with the bridged projection).
        let plain = GraphReadModel::without_http_bridges(Arc::clone(&db));
        let plain_edges = plain.neighbors("caller_get_users");
        assert!(!has_bridge_edge(&plain_edges));
        assert_eq!(sorted_callees(&plain_edges), vec!["uid_helper"]);

        // The bridged model of the SAME generation must still see bridge edges.
        let bridged = GraphReadModel::new(Arc::clone(&db)).unwrap();
        let bridged_edges = bridged.neighbors("caller_get_users");
        assert!(
            has_bridge_edge(&bridged_edges),
            "bridged projection lost its bridge edges to the plain model's cache fill"
        );
        assert_eq!(
            sorted_callees(&bridged_edges),
            vec!["handler_get_users", "uid_helper"]
        );

        // And the plain projection stays clean afterwards.
        assert!(!has_bridge_edge(&plain.neighbors("caller_get_users")));
    }

    #[test]
    fn bridged_model_first_does_not_leak_bridge_edges_into_plain_model() {
        let (_tmp, db) = setup_mixed_edge_db();

        // Bridged model populates its adjacency first.
        let bridged = GraphReadModel::new(Arc::clone(&db)).unwrap();
        assert!(has_bridge_edge(&bridged.neighbors("caller_get_users")));

        // The plain model of the SAME generation must not see synthesized edges
        // (impact/dead-code would otherwise follow http_bridge edges).
        let plain = GraphReadModel::without_http_bridges(Arc::clone(&db));
        let plain_edges = plain.neighbors("caller_get_users");
        assert!(
            !has_bridge_edge(&plain_edges),
            "plain projection leaked synthesized bridge edges from the bridged cache"
        );
        assert_eq!(sorted_callees(&plain_edges), vec!["uid_helper"]);
    }

    #[test]
    fn evidence_write_keeps_plain_adjacency_but_evicts_bridged_adjacency() {
        let (_tmp, db) = setup_mixed_edge_db();

        let plain_before = GraphReadModel::without_http_bridges(Arc::clone(&db));
        let bridged_before = GraphReadModel::new(Arc::clone(&db)).unwrap();

        // Evidence-only write: bumps evidence_epoch, leaves index_epoch alone.
        db.boost_http_edge_confidence("http_get_users", 0.15)
            .unwrap();

        let plain_after = GraphReadModel::without_http_bridges(Arc::clone(&db));
        let bridged_after = GraphReadModel::new(Arc::clone(&db)).unwrap();

        assert!(
            Arc::ptr_eq(
                &plain_before.shared_adjacency,
                &plain_after.shared_adjacency
            ),
            "plain adjacency holds no evidence-derived content; an evidence-only \
             epoch bump must not evict its slot"
        );
        assert!(
            !Arc::ptr_eq(
                &bridged_before.shared_adjacency,
                &bridged_after.shared_adjacency
            ),
            "bridged adjacency absorbs evidence-boosted bridge edges; an \
             evidence-only epoch bump must evict its slot"
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
