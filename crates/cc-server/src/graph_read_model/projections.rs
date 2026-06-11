//! Projection builders over cc-db typed queries: call adjacency, semantic
//! edges, file-import and community adjacency, and the dead-code caller set.

use cc_db::index_db::IndexDb;
use cc_db::GraphReads;
use cc_model::edge::SemanticRelation;
use cc_model::CcResult;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::graph_types::{BfsAdj, EdgeLite, InternalEdge};

use super::cache::{
    generation_cached, SharedCalleeSet, CALLEES_WITH_CALLERS_CACHE, COMMUNITY_ADJ_CACHE,
    IMPORT_ADJ_CACHE,
};
use super::{DeadCodeCandidate, GraphReadModel};

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

#[derive(Default)]
pub(super) struct SemanticAdjPair {
    pub(super) by_source: HashMap<String, Vec<SemanticEdgeLite>>,
    pub(super) by_target: HashMap<String, Vec<SemanticEdgeLite>>,
}

impl GraphReadModel {
    /// Lazily load all semantic edges into the shared cache on first access.
    ///
    /// Returns the mutex guard so callers can query `by_source` / `by_target`
    /// without a second lock acquisition.
    fn ensure_semantic_loaded(&self) -> std::sync::MutexGuard<'_, SemanticAdjPair> {
        let mut guard = self.shared_semantic.lock().unwrap();
        if guard.by_source.is_empty() && guard.by_target.is_empty() {
            // A failed load degrades to "no semantic edges" for this request
            // (and is retried on the next access since the pair stays empty);
            // log instead of swallowing the error silently.
            let edges = match self.reads().query_semantic_edges(None, None, None) {
                Ok(edges) => edges,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        "semantic edge load failed; degrading to empty semantic adjacency"
                    );
                    Vec::new()
                }
            };
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
        let edges = GraphReads::new(db).call_uid_edges_lite()?;
        let mut adj: HashMap<String, Vec<EdgeLite>> = HashMap::new();
        for edge in edges {
            adj.entry(edge.caller_uid.clone()).or_default().push(edge);
        }
        Ok(BfsAdj { adj })
    }

    /// Return outgoing edges for a caller UID, backed by a process-global cache.
    ///
    /// The adjacency map is shared across all `GraphReadModel` instances of the
    /// same generation and bridge dimension, so edges queried in one tool call
    /// are reused by later calls without hitting SQLite again.
    pub(crate) fn neighbors(&self, uid: &str) -> Vec<EdgeLite> {
        // Degradation is still logged by the discarded collector
        // (`record_read_error` always emits a tracing::warn).
        let mut explain = cc_model::GraphExplainCollector::new();
        self.neighbors_with_explain(uid, &mut explain)
    }

    /// Like [`Self::neighbors`] but records an adjacency-query failure into
    /// `explain` instead of silently degrading to an isolated node.
    pub(crate) fn neighbors_with_explain(
        &self,
        uid: &str,
        explain: &mut cc_model::GraphExplainCollector,
    ) -> Vec<EdgeLite> {
        // Fast path: check if the UID is already cached.
        {
            let adj = self.shared_adjacency.lock().unwrap();
            if let Some(edges) = adj.get(uid) {
                return edges.clone();
            }
        }
        // Slow path: query DB *without* holding the adjacency lock. A failure
        // degrades to "no outgoing edges" for this UID (same graceful
        // behavior as before), but is recorded for the explain envelope.
        let mut edges = match self.reads().call_edges_from_uid_lite(uid) {
            Ok(edges) => edges,
            Err(err) => {
                explain.record_read_error("call_edges_from_uid_lite", &err);
                Vec::new()
            }
        };
        if let Some(bridges) = self.http_bridges.get(uid) {
            edges.extend(bridges.iter().cloned());
        }
        // Insert and return.
        let mut adj = self.shared_adjacency.lock().unwrap();
        // Another thread may have inserted between our read and write; use the
        // existing entry if present so callers see a consistent snapshot.
        adj.entry(uid.to_string()).or_insert(edges).clone()
    }

    pub(crate) fn projected_import_adjacency<F>(
        &self,
        mut project_node: F,
    ) -> CcResult<HashMap<String, Vec<String>>>
    where
        F: FnMut(&str) -> String,
    {
        let pairs = self.reads().file_import_pairs()?;

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (file_path, resolved_path) in &pairs {
            let from = project_node(file_path);
            let to = project_node(resolved_path);
            if from != to {
                adj.entry(from).or_default().push(to);
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
            for row in self.reads().import_witness_rows(node)? {
                if scc_set.contains(row.resolved_path.as_str()) {
                    edges.push(InternalEdge {
                        from: node.clone(),
                        to: row.resolved_path,
                        import: row.import_string,
                        line: None,
                    });
                }
            }
        }

        Ok(edges)
    }

    pub(crate) fn community_call_adjacency(&self) -> CcResult<HashMap<String, Vec<String>>> {
        let shared = generation_cached(&COMMUNITY_ADJ_CACHE, &self.generation, || {
            let pairs = self.reads().community_adjacency_pairs()?;

            let mut adj: HashMap<String, Vec<String>> = HashMap::new();
            for (from, to) in pairs {
                adj.entry(from).or_default().push(to);
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

    /// Files that depend on (import) `file_path`, direct plus 2-hop transitive,
    /// deduplicated and sorted. Two bounded SQL queries: 200 direct importers,
    /// 10k transitive rows — no full-table materialization.
    pub(crate) fn dependents_of_file(&self, file_path: &str) -> CcResult<Vec<String>> {
        // Direct importers, bounded in SQL; self-imports are excluded in the
        // query so the cap counts only real dependents. A failure here
        // propagates to the caller.
        let direct = self.reads().direct_importers_of_file(file_path, 200)?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut dependents: Vec<String> = Vec::new();
        for fp in direct {
            if seen.insert(fp.clone()) {
                dependents.push(fp);
            }
        }

        // 2-hop: importers of the direct dependents in a single batched query.
        // A failure here degrades to direct-only results (partial answer beats
        // none), matching the original handler behavior — but is logged now.
        if !dependents.is_empty() {
            let transitive_rows = self
                .reads()
                .importers_of_paths(&dependents, 10_000)
                .unwrap_or_else(|err| {
                    tracing::warn!(
                        error = %err,
                        "transitive importer query failed; returning direct dependents only"
                    );
                    Vec::new()
                });
            let mut transitive: Vec<String> = Vec::new();
            for fp in transitive_rows {
                if fp != file_path && seen.insert(fp.clone()) {
                    transitive.push(fp);
                }
            }
            dependents.extend(transitive);
        }

        dependents.sort();
        Ok(dependents)
    }

    /// Callee UIDs that have at least one non-self caller. Cached per
    /// generation on success; derived purely from `call_edges`.
    pub(super) fn callees_with_external_callers(&self) -> SharedCalleeSet {
        let computed = generation_cached(&CALLEES_WITH_CALLERS_CACHE, &self.generation, || {
            let uids = self.reads().callees_with_nonself_callers(10_000)?;
            Ok(Arc::new(uids.into_iter().collect::<HashSet<String>>()))
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

    /// Multiplier on the desired dead-code item cap for the symbol scan.
    pub(crate) const DEAD_CODE_SCAN_FACTOR: usize = 40;
    /// Ceiling on the dead-code symbol scan to bound query cost.
    pub(crate) const DEAD_CODE_SCAN_MAX: usize = 5000;

    /// Default adaptive scan budget for [`Self::dead_code_candidates`]:
    /// `item_cap × 40`, capped at 5000 to bound query cost. This is the same
    /// default the MCP `find_dead_code` handler applies, so direct engine
    /// callers passing `dead_code_scan_limit(cap)` observe identical behaviour.
    pub(crate) fn dead_code_scan_limit(item_cap: usize) -> usize {
        (item_cap * Self::DEAD_CODE_SCAN_FACTOR).min(Self::DEAD_CODE_SCAN_MAX)
    }

    /// Symbols that appear to be dead code: no non-self callers and no
    /// external references. `scope` filters by file-path prefix; `scan_limit`
    /// bounds the symbols scan. Result is untruncated (callers clip it).
    pub(crate) fn dead_code_candidates(
        &self,
        scope: Option<&str>,
        scan_limit: usize,
    ) -> CcResult<Vec<DeadCodeCandidate>> {
        let all_symbols = self.reads().dead_code_symbol_scan(scope, scan_limit)?;

        let has_callers = self.callees_with_external_callers();

        let excluded_names = ["main", "__init__", "__main__", "setup", "configure"];
        let excluded_prefixes = ["test_", "Test"];

        // Phase 1: scope / exclusion / no-caller filters. Reference status is
        // resolved in a batched second pass to avoid per-candidate queries.
        let mut candidates: Vec<DeadCodeCandidate> = Vec::new();
        for row in &all_symbols {
            if row.symbol_uid.is_empty() || row.name.is_empty() {
                continue;
            }
            if let Some(prefix) = scope {
                if !row.file_path.starts_with(prefix) {
                    continue;
                }
            }
            if excluded_names.contains(&row.name.as_str()) {
                continue;
            }
            if excluded_prefixes.iter().any(|p| row.name.starts_with(p)) {
                continue;
            }
            if !has_callers.contains(&row.symbol_uid) {
                candidates.push(DeadCodeCandidate {
                    name: row.name.clone(),
                    uid: row.symbol_uid.clone(),
                    file_path: row.file_path.clone(),
                    kind: row.kind.clone(),
                });
            }
        }

        // Phase 2: batch-load references and drop candidates with an
        // *external* reference (container differs from the symbol's own name).
        // The reference query is best-effort (a failure keeps the candidate).
        let mut uids_with_external_refs: HashSet<String> = HashSet::new();
        let candidate_uids: Vec<String> = candidates.iter().map(|c| c.uid.clone()).collect();
        let name_by_uid: HashMap<&str, &str> = candidates
            .iter()
            .map(|c| (c.uid.as_str(), c.name.as_str()))
            .collect();
        let ref_rows = self
            .reads()
            .symbol_ref_containers_for_targets(&candidate_uids)
            .unwrap_or_else(|err| {
                tracing::warn!(
                    error = %err,
                    "dead-code reference query failed; keeping unfiltered candidates"
                );
                Vec::new()
            });
        for (target_uid, container) in &ref_rows {
            if target_uid.is_empty() {
                continue;
            }
            let own_name = name_by_uid.get(target_uid.as_str()).copied();
            let is_external = match (container.as_deref(), own_name) {
                (None, _) => true,
                (Some(c), Some(n)) => c != n,
                (Some(_), None) => true,
            };
            if is_external {
                uids_with_external_refs.insert(target_uid.clone());
            }
        }

        candidates.retain(|cand| !uids_with_external_refs.contains(&cand.uid));
        Ok(candidates)
    }
}
