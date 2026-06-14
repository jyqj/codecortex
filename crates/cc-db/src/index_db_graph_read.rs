//! IndexDb methods: task-shaped graph read queries for the server's
//! graph/impact/exploration tools.
//!
//! Every method here was lifted verbatim from cc-server SQL call sites
//! (graph_read_model, engine_query, type hierarchy, env-var search) so the
//! server no longer owns SQL strings. Query semantics (WHERE clauses, LIMIT
//! placement, confidence filters, ordering) are preserved exactly.

use std::collections::HashMap;

use cc_model::{CcError, CcResult};
use serde_json::Value;

use crate::index_db::{
    CallEdgeProvenanceCounts, DeadCodeSymbolRow, EdgeLiteBfs, HttpCallEdgeLite, ImportWitnessRow,
    IndexDb, IndexGeneration, ReadOps, RouteNodeLite, ServiceBindingRows, SymbolLiteRow,
};
use crate::sql_util::{sql_in_placeholders, IN_BATCH_SIZE};

fn db_err(err: impl std::fmt::Display) -> CcError {
    CcError::Database(err.to_string())
}

/// Render a community id column value the same way the previous JSON
/// projection did: integers and floats via their JSON representation,
/// strings verbatim, anything else skipped.
fn community_value_to_string(value: rusqlite::types::Value) -> Option<String> {
    match value {
        rusqlite::types::Value::Integer(n) => Some(n.to_string()),
        rusqlite::types::Value::Real(f) => Some(serde_json::json!(f).to_string()),
        rusqlite::types::Value::Text(s) => Some(s),
        _ => None,
    }
}

impl IndexDb {
    /// Graph read model: task-shaped graph read queries (adjacency, impact,
    /// dead-code, imports/communities, HTTP/async bridges) consumed by the
    /// server's graph/impact/exploration tools. See [`GraphReads`].
    pub fn graph_reads(&self) -> GraphReads<'_> {
        GraphReads::new(self)
    }

    /// `(name, kind, signature)` of direct children of `parent_uid`, in
    /// source order (symbol outline view).
    pub(crate) fn child_symbol_outline_rows(
        &self,
        parent_uid: &str,
    ) -> CcResult<Vec<(String, String, Option<String>)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT name, kind, signature FROM symbols WHERE parent_symbol_id = ?1 ORDER BY start_line",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![parent_uid], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Candidate symbol rows for source retrieval, ranked exact-first.
    /// `exact` restricts to qname/name equality; otherwise LIKE fallback.
    pub(crate) fn symbol_source_candidates(
        &self,
        symbol: &str,
        exact: bool,
    ) -> CcResult<Vec<Value>> {
        if exact {
            self.query_json(
                "SELECT name, kind, file_path, container, start_line, end_line, qname, signature, symbol_uid
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1
                 ORDER BY CASE WHEN qname = ?1 THEN 0 WHEN name = ?1 THEN 1 ELSE 2 END, file_path, start_line
                 LIMIT 8",
                &[symbol.to_string()],
            )
        } else {
            let pat = format!("%{}%", symbol);
            self.query_json(
                "SELECT name, kind, file_path, container, start_line, end_line, qname, signature, symbol_uid
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1 OR qname LIKE ?2 OR name LIKE ?2
                 ORDER BY CASE WHEN qname = ?1 THEN 0 WHEN name = ?1 THEN 1 WHEN qname LIKE ?2 THEN 2 ELSE 3 END, file_path, start_line
                 LIMIT 8",
                &[symbol.to_string(), pat],
            )
        }
    }

    /// Symbol kind counts, most frequent first (graph schema overview).
    pub(crate) fn symbol_kind_counts(&self) -> CcResult<Vec<(String, i64)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) AS cnt FROM symbols GROUP BY kind ORDER BY cnt DESC")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Provenance counters over `call_edges`. Each sub-query degrades to an
    /// empty breakdown on failure (best-effort schema overview).
    pub(crate) fn call_edge_provenance(&self) -> CcResult<CallEdgeProvenanceCounts> {
        let conn = self.read_conn()?;

        fn grouped_counts(conn: &rusqlite::Connection, sql: &str) -> Vec<(Option<String>, i64)> {
            let Ok(mut stmt) = conn.prepare(sql) else {
                return Vec::new();
            };
            let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?))
            }) else {
                return Vec::new();
            };
            rows.filter_map(|r| r.ok()).collect()
        }

        let by_dispatch_kind = grouped_counts(
            &conn,
            "SELECT dispatch_kind, COUNT(*) AS cnt FROM call_edges GROUP BY dispatch_kind",
        );
        let synthesized_total: i64 = conn
            .query_row(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let by_synthesized_by = grouped_counts(
            &conn,
            "SELECT synthesized_by, COUNT(*) AS cnt FROM call_edges WHERE synthesized_by IS NOT NULL GROUP BY synthesized_by ORDER BY cnt DESC",
        );
        let by_resolution_kind = grouped_counts(
            &conn,
            "SELECT resolution_kind, COUNT(*) AS cnt FROM call_edges GROUP BY resolution_kind",
        );

        Ok(CallEdgeProvenanceCounts {
            by_dispatch_kind,
            synthesized_total,
            by_synthesized_by,
            by_resolution_kind,
        })
    }

    /// `(caller_file, callee_file)` pairs for resolved cross-file call edges
    /// (package boundary analysis input).
    pub(crate) fn cross_file_call_file_pairs(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT s1.file_path AS caller_file, s2.file_path AS callee_file \
                 FROM call_edges ce \
                 JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
                 JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
                 WHERE ce.caller_symbol_uid IS NOT NULL \
                   AND ce.callee_symbol_uid IS NOT NULL \
                   AND s1.file_path != s2.file_path",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// `uid -> param_count` for a set of symbol UIDs (override-compatibility
    /// checks in the type hierarchy).
    pub(crate) fn param_counts_for_uids(
        &self,
        uids: &[String],
    ) -> CcResult<HashMap<String, Option<u32>>> {
        let mut result = HashMap::new();
        if uids.is_empty() {
            return Ok(result);
        }
        let conn = self.read_conn()?;
        for chunk in uids.chunks(100) {
            let sql = format!(
                "SELECT symbol_uid, param_count FROM symbols WHERE symbol_uid IN ({})",
                sql_in_placeholders(chunk.len())
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|uid| uid as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?.map(|n| n as u32),
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (uid, count) = row.map_err(db_err)?;
                result.insert(uid, count);
            }
        }
        Ok(result)
    }

    /// Environment variable access rows from `data_flow_edges`, LIKE-filtered
    /// by env key pattern and optionally by file path pattern. Patterns are
    /// passed through verbatim (callers control wildcard wrapping).
    pub(crate) fn env_access_rows(
        &self,
        env_key_pattern: &str,
        file_path_pattern: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<Value>> {
        if let Some(file_pattern) = file_path_pattern {
            self.query_json(
                &format!(
                    "SELECT env_key, file_path, line, source_symbol_uid \
                     FROM data_flow_edges \
                     WHERE flow_kind = 'env_access' AND env_key LIKE ?1 AND file_path LIKE ?2 \
                     ORDER BY env_key, file_path \
                     LIMIT {}",
                    limit
                ),
                &[env_key_pattern.to_string(), file_pattern.to_string()],
            )
        } else {
            self.query_json(
                &format!(
                    "SELECT env_key, file_path, line, source_symbol_uid \
                     FROM data_flow_edges \
                     WHERE flow_kind = 'env_access' AND env_key LIKE ?1 \
                     ORDER BY env_key, file_path \
                     LIMIT {}",
                    limit
                ),
                &[env_key_pattern.to_string()],
            )
        }
    }
}

/// Narrow graph-read facet over [`IndexDb`] consumed by the server's
/// `GraphReadModel` (adjacency loading, projections, bridge synthesis, and
/// generation-keyed caching).
///
/// Same seam pattern as `index_db_retrieval.rs` for cc-search: the SQL stays
/// on [`IndexDb`] typed methods (other callers keep using them directly);
/// this facet only enumerates the exact read surface the graph read model
/// depends on, so "what GraphReadModel needs" is one narrow interface
/// instead of a scattering of calls into the 130+-method `IndexDb` facade.
///
/// Borrowed and `Copy` so it can be materialized for free from either an
/// owned `Arc<IndexDb>` or a plain `&IndexDb` (both entry shapes exist on
/// the server side).
#[derive(Clone, Copy)]
pub struct GraphReads<'a> {
    db: &'a IndexDb,
}

impl<'a> GraphReads<'a> {
    pub fn new(db: &'a IndexDb) -> Self {
        Self { db }
    }

    // ── Cache identity (generation-keyed caching) ───────────────────────

    /// Process-unique, never-reused id of the underlying [`IndexDb`] handle.
    pub fn instance_id(&self) -> u64 {
        self.db.instance_id()
    }

    /// Persisted epoch vector (`index_epoch` / `evidence_epoch`).
    pub fn generation(&self) -> CcResult<IndexGeneration> {
        self.db.generation()
    }

    // ── Call graph adjacency ────────────────────────────────────────────

    /// All UID-resolved call edges (full-graph adjacency load).
    pub fn call_uid_edges_lite(&self) -> CcResult<Vec<EdgeLiteBfs>> {
        self.db.call_uid_edges_lite()
    }

    /// Outgoing call edges of one caller UID (lazy per-node adjacency).
    pub fn call_edges_from_uid_lite(&self, caller_uid: &str) -> CcResult<Vec<EdgeLiteBfs>> {
        self.db.call_edges_from_uid_lite(caller_uid)
    }

    /// Distinct callers of any of `callee_uids` (impact reverse BFS).
    pub fn reverse_callers(
        &self,
        callee_uids: &[String],
        confidence_threshold: Option<f64>,
        limit: Option<usize>,
    ) -> CcResult<Vec<SymbolLiteRow>> {
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

            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
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
                .query_map(param_refs.as_slice(), crate::rows::symbol_lite)
                .map_err(db_err)?;
            for row in rows {
                callers.push(row.map_err(db_err)?);
            }
        }

        Ok(callers)
    }

    /// Callee UIDs with at least one non-self caller (dead-code input).
    pub fn callees_with_nonself_callers(&self, limit: usize) -> CcResult<Vec<String>> {
        let conn = self.db.read_conn()?;
        let sql = format!(
            "SELECT DISTINCT callee_symbol_uid FROM call_edges \
             WHERE callee_symbol_uid IS NOT NULL \
               AND (caller_symbol_uid IS NULL OR caller_symbol_uid != callee_symbol_uid) \
             LIMIT {}",
            limit
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        // Row errors must propagate: a silently truncated Ok here would be
        // cached for the whole generation by the server's dead-code path.
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    // ── Semantic edges ──────────────────────────────────────────────────

    /// Semantic edges, optionally filtered (semantic adjacency load).
    pub fn query_semantic_edges(
        &self,
        source_uid: Option<&str>,
        target_uid: Option<&str>,
        relation_kind: Option<&str>,
    ) -> CcResult<Vec<cc_model::edge::SemanticEdgeRecord>> {
        self.db
            .query_semantic_edges(source_uid, target_uid, relation_kind)
    }

    // ── Imports / communities ───────────────────────────────────────────

    /// Distinct resolved `(file_path, resolved_path)` import pairs.
    pub fn file_import_pairs(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT file_path, resolved_path FROM imports WHERE resolved_path IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Resolved import rows from `file_path` (cycle witness edges).
    pub fn import_witness_rows(&self, file_path: &str) -> CcResult<Vec<ImportWitnessRow>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT resolved_path, import_string FROM imports WHERE file_path = ?1 AND resolved_path IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(ImportWitnessRow {
                    resolved_path: row.get::<_, String>(0)?,
                    import_string: row.get::<_, Option<String>>(1)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Direct importers of `file_path`, bounded.
    pub fn direct_importers_of_file(&self, file_path: &str, limit: usize) -> CcResult<Vec<String>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT DISTINCT file_path FROM imports \
                 WHERE resolved_path = ?1 AND file_path != ?1 \
                 ORDER BY file_path LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![file_path, limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Files importing any of `resolved_paths` (2-hop dependents), bounded.
    pub fn importers_of_paths(
        &self,
        resolved_paths: &[String],
        limit: usize,
    ) -> CcResult<Vec<String>> {
        if resolved_paths.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        let sql = format!(
            "SELECT DISTINCT file_path FROM imports WHERE resolved_path IN ({}) LIMIT {}",
            sql_in_placeholders(resolved_paths.len()),
            limit
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = resolved_paths
            .iter()
            .map(|path| path as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Distinct cross-community call pairs.
    pub fn community_adjacency_pairs(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT s1.community_id AS from_community, s2.community_id AS to_community \
                 FROM call_edges ce \
                 JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
                 JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
                 WHERE s1.community_id IS NOT NULL \
                   AND s2.community_id IS NOT NULL \
                   AND s1.community_id != s2.community_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, rusqlite::types::Value>(0)?,
                    row.get::<_, rusqlite::types::Value>(1)?,
                ))
            })
            .map_err(db_err)?;
        let mut pairs = Vec::new();
        for row in rows {
            let (from_value, to_value) = row.map_err(db_err)?;
            if let (Some(from), Some(to)) = (
                community_value_to_string(from_value),
                community_value_to_string(to_value),
            ) {
                pairs.push((from, to));
            }
        }
        Ok(pairs)
    }

    // ── Symbols / tests ─────────────────────────────────────────────────

    /// Symbols (with stable UID) living in any of `files`.
    pub fn symbols_lite_in_files(&self, files: &[String]) -> CcResult<Vec<SymbolLiteRow>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT symbol_uid, name, file_path, kind, community_id \
                 FROM symbols WHERE file_path=?1 AND symbol_uid IS NOT NULL",
            )
            .map_err(db_err)?;
        let mut symbols = Vec::new();
        for file in files {
            let rows = stmt
                .query_map(rusqlite::params![file], crate::rows::symbol_lite)
                .map_err(db_err)?;
            for row in rows {
                symbols.push(row.map_err(db_err)?);
            }
        }
        Ok(symbols)
    }

    /// Batch `uid -> name` resolution.
    pub fn symbol_names_for_uids(&self, uids: &[String]) -> CcResult<HashMap<String, String>> {
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
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|uid| uid as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (uid, name) = row.map_err(db_err)?;
                map.insert(uid, name);
            }
        }
        Ok(map)
    }

    /// Raw symbol rows for the dead-code scan.
    pub fn dead_code_symbol_scan(
        &self,
        scope: Option<&str>,
        scan_limit: usize,
    ) -> CcResult<Vec<DeadCodeSymbolRow>> {
        let conn = self.db.read_conn()?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<DeadCodeSymbolRow> {
            Ok(DeadCodeSymbolRow {
                name: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                symbol_uid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                file_path: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                kind: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            })
        };
        if let Some(prefix) = scope {
            let pattern = format!("%{}%", prefix);
            let sql = format!(
                "SELECT name, symbol_uid, file_path, kind FROM symbols \
                 WHERE file_path LIKE ?1 LIMIT {}",
                scan_limit
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params![pattern], map_row)
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
        } else {
            let sql = format!(
                "SELECT name, symbol_uid, file_path, kind FROM symbols LIMIT {}",
                scan_limit
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt.query_map([], map_row).map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
        }
    }

    /// `(target_symbol_uid, container)` reference rows (dead-code phase 2).
    pub fn symbol_ref_containers_for_targets(
        &self,
        target_uids: &[String],
    ) -> CcResult<Vec<(String, Option<String>)>> {
        let conn = self.db.read_conn()?;
        let mut out = Vec::new();
        for batch in target_uids.chunks(IN_BATCH_SIZE) {
            if batch.is_empty() {
                continue;
            }
            let sql = format!(
                "SELECT target_symbol_uid, container FROM symbol_refs \
                 WHERE target_symbol_uid IN ({})",
                sql_in_placeholders(batch.len())
            );
            let Ok(mut stmt) = conn.prepare(&sql) else {
                continue;
            };
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|uid| uid as &dyn rusqlite::types::ToSql)
                .collect();
            let Ok(rows) = stmt.query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(1)?,
                ))
            }) else {
                continue;
            };
            out.extend(rows.flatten());
        }
        Ok(out)
    }

    /// Distinct test files covering any of `code_files`.
    pub fn suggested_test_files(&self, code_files: &[String]) -> CcResult<Vec<String>> {
        if code_files.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        let sql = format!(
            "SELECT DISTINCT test_file_path FROM test_edges \
             WHERE code_file_path IN ({}) ORDER BY test_file_path",
            sql_in_placeholders(code_files.len())
        );
        let params: Vec<&dyn rusqlite::types::ToSql> = code_files
            .iter()
            .map(|file| file as &dyn rusqlite::types::ToSql)
            .collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    // ── HTTP/async bridges and infra bindings ───────────────────────────

    /// UID-resolved HTTP call edges (bridge synthesis input), bounded.
    pub fn all_http_call_edges_lite(&self, limit: usize) -> CcResult<Vec<HttpCallEdgeLite>> {
        self.db.all_http_call_edges_lite(limit)
    }

    /// UID-resolved route nodes (bridge synthesis input), bounded.
    pub fn all_route_nodes_lite(&self, limit: usize) -> CcResult<Vec<RouteNodeLite>> {
        self.db.all_route_nodes_lite(limit)
    }

    /// HTTP route handler rows, optionally LIKE-filtered, bounded.
    pub fn route_handler_rows(
        &self,
        route_path: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<Value>> {
        if let Some(pattern) = route_path {
            let like_pattern = format!("%{}%", pattern);
            self.db.query_json(
                &format!(
                    "SELECT route_path, method, handler_name, file_path, framework, line \
                     FROM routes WHERE route_path LIKE ?1 LIMIT {}",
                    limit
                ),
                &[like_pattern],
            )
        } else {
            self.db.query_json(
                &format!(
                    "SELECT route_path, method, handler_name, file_path, framework, line \
                     FROM routes LIMIT {}",
                    limit
                ),
                &[],
            )
        }
    }

    /// Consumers of a topic/queue (infra edges).
    pub fn async_consumer_rows(&self, topic_or_queue: &str) -> CcResult<Vec<Value>> {
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

    /// Infra bindings for a service or route, plus connecting edges.
    pub fn service_binding_rows(&self, service_or_route: &str) -> CcResult<ServiceBindingRows> {
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

        Ok(ServiceBindingRows {
            matched_infra_nodes,
            matched_routes,
            related_edges,
        })
    }

    // ── Runtime evidence ────────────────────────────────────────────────

    /// `normalized_path -> (observed_count, last_seen)` runtime evidence for
    /// synthesized HTTP bridge edges.
    pub fn evidence_for_normalized_paths(
        &self,
        paths: &[String],
    ) -> CcResult<HashMap<String, (u32, String)>> {
        self.db.evidence_for_normalized_paths(paths)
    }
}

// Read-only facet delegates (see `IndexDb::reads()`).
impl ReadOps<'_> {
    /// Distinct `(file_path, resolved_path)` import pairs with a resolved
    pub fn file_import_pairs(&self) -> CcResult<Vec<(String, String)>> {
        self.0.graph_reads().file_import_pairs()
    }

    /// Resolved import rows originating from `file_path`, with the original
    pub fn import_witness_rows(&self, file_path: &str) -> CcResult<Vec<ImportWitnessRow>> {
        self.0.graph_reads().import_witness_rows(file_path)
    }

    /// Distinct cross-community call pairs `(from_community, to_community)`
    pub fn community_adjacency_pairs(&self) -> CcResult<Vec<(String, String)>> {
        self.0.graph_reads().community_adjacency_pairs()
    }

    /// Symbols (with stable UID) living in any of `files`, in file order.
    pub fn symbols_lite_in_files(&self, files: &[String]) -> CcResult<Vec<SymbolLiteRow>> {
        self.0.graph_reads().symbols_lite_in_files(files)
    }

    /// Distinct callers of any of `callee_uids`, optionally filtered by
    pub fn reverse_callers(
        &self,
        callee_uids: &[String],
        confidence_threshold: Option<f64>,
        limit: Option<usize>,
    ) -> CcResult<Vec<SymbolLiteRow>> {
        self.0
            .graph_reads()
            .reverse_callers(callee_uids, confidence_threshold, limit)
    }

    /// Distinct test files covering any of `code_files`, ordered by path.
    pub fn suggested_test_files(&self, code_files: &[String]) -> CcResult<Vec<String>> {
        self.0.graph_reads().suggested_test_files(code_files)
    }

    /// Resolve a batch of symbol UIDs to names (`uid -> name`); UIDs without
    pub fn symbol_names_for_uids(&self, uids: &[String]) -> CcResult<HashMap<String, String>> {
        self.0.graph_reads().symbol_names_for_uids(uids)
    }

    /// Direct importers of `file_path` (self-imports excluded), bounded and
    pub fn direct_importers_of_file(&self, file_path: &str, limit: usize) -> CcResult<Vec<String>> {
        self.0
            .graph_reads()
            .direct_importers_of_file(file_path, limit)
    }

    /// Files importing any of `resolved_paths`, in a single bounded query
    pub fn importers_of_paths(
        &self,
        resolved_paths: &[String],
        limit: usize,
    ) -> CcResult<Vec<String>> {
        self.0
            .graph_reads()
            .importers_of_paths(resolved_paths, limit)
    }

    /// Distinct callee UIDs that have at least one non-self caller
    pub fn callees_with_nonself_callers(&self, limit: usize) -> CcResult<Vec<String>> {
        self.0.graph_reads().callees_with_nonself_callers(limit)
    }

    /// Raw symbol rows for the dead-code scan. `scope` becomes a
    pub fn dead_code_symbol_scan(
        &self,
        scope: Option<&str>,
        scan_limit: usize,
    ) -> CcResult<Vec<DeadCodeSymbolRow>> {
        self.0
            .graph_reads()
            .dead_code_symbol_scan(scope, scan_limit)
    }

    /// `(target_symbol_uid, container)` reference rows for any of
    pub fn symbol_ref_containers_for_targets(
        &self,
        target_uids: &[String],
    ) -> CcResult<Vec<(String, Option<String>)>> {
        self.0
            .graph_reads()
            .symbol_ref_containers_for_targets(target_uids)
    }

    /// HTTP route handler rows, optionally LIKE-filtered by route path
    pub fn route_handler_rows(
        &self,
        route_path: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<Value>> {
        self.0.graph_reads().route_handler_rows(route_path, limit)
    }

    /// Consumers of a topic/queue: infra edges with kind in
    pub fn async_consumer_rows(&self, topic_or_queue: &str) -> CcResult<Vec<Value>> {
        self.0.graph_reads().async_consumer_rows(topic_or_queue)
    }

    /// Infra bindings for a service or route, matched on two dimensions
    pub fn service_binding_rows(&self, service_or_route: &str) -> CcResult<ServiceBindingRows> {
        self.0.graph_reads().service_binding_rows(service_or_route)
    }

    /// `(name, kind, signature)` of direct children of `parent_uid`, in
    pub fn child_symbol_outline_rows(
        &self,
        parent_uid: &str,
    ) -> CcResult<Vec<(String, String, Option<String>)>> {
        self.0.child_symbol_outline_rows(parent_uid)
    }

    /// Candidate symbol rows for source retrieval, ranked exact-first.
    pub fn symbol_source_candidates(&self, symbol: &str, exact: bool) -> CcResult<Vec<Value>> {
        self.0.symbol_source_candidates(symbol, exact)
    }

    /// Symbol kind counts, most frequent first (graph schema overview).
    pub fn symbol_kind_counts(&self) -> CcResult<Vec<(String, i64)>> {
        self.0.symbol_kind_counts()
    }

    /// Provenance counters over `call_edges`. Each sub-query degrades to an
    pub fn call_edge_provenance(&self) -> CcResult<CallEdgeProvenanceCounts> {
        self.0.call_edge_provenance()
    }

    /// `(caller_file, callee_file)` pairs for resolved cross-file call edges
    pub fn cross_file_call_file_pairs(&self) -> CcResult<Vec<(String, String)>> {
        self.0.cross_file_call_file_pairs()
    }

    /// `uid -> param_count` for a set of symbol UIDs (override-compatibility
    pub fn param_counts_for_uids(&self, uids: &[String]) -> CcResult<HashMap<String, Option<u32>>> {
        self.0.param_counts_for_uids(uids)
    }

    /// Environment variable access rows from `data_flow_edges`, LIKE-filtered
    pub fn env_access_rows(
        &self,
        env_key_pattern: &str,
        file_path_pattern: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<Value>> {
        self.0
            .env_access_rows(env_key_pattern, file_path_pattern, limit)
    }
}

#[cfg(test)]
mod tests {
    use crate::index_db::IndexDb;
    use tempfile::TempDir;

    fn open_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        (tmp, db)
    }

    // Fixtures seed through the write connection (the pooled read
    // connections are query_only). This intentionally bypasses WriteOps
    // and therefore does not bump the epoch vector; none of these tests
    // depend on epoch-keyed caches.
    fn insert_file(db: &IndexDb, file_path: &str) {
        db.write_conn.lock().unwrap()
            .execute(
                "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, 'rust', 'hash', 0.0, 100, '2025-01-01')",
                rusqlite::params![file_path],
            )
            .unwrap();
    }

    fn insert_symbol(db: &IndexDb, uid: &str, name: &str, file_path: &str, kind: &str) {
        insert_file(db, file_path);
        db.write_conn.lock().unwrap()
            .execute(
                "INSERT OR REPLACE INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, symbol_uid)
                 VALUES(?1, ?2, ?3, ?4, 1, 10, ?5)",
                rusqlite::params![format!("sid_{uid}"), file_path, name, kind, uid],
            )
            .unwrap();
    }

    fn insert_call_edge(db: &IndexDb, edge_id: &str, caller_uid: Option<&str>, callee_uid: &str) {
        insert_file(db, "src/app.rs");
        db.write_conn.lock().unwrap()
            .execute(
                "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid)
                 VALUES(?1, 'src/app.rs', 'callee', 5, ?2, ?3)",
                rusqlite::params![edge_id, caller_uid, callee_uid],
            )
            .unwrap();
    }

    fn insert_import(db: &IndexDb, file_path: &str, import_string: &str, resolved: Option<&str>) {
        insert_file(db, file_path);
        db.write_conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO imports(file_path, import_string, resolved_path) VALUES(?1, ?2, ?3)",
                rusqlite::params![file_path, import_string, resolved],
            )
            .unwrap();
    }

    #[test]
    fn file_import_pairs_returns_only_resolved_distinct_pairs() {
        let (_tmp, db) = open_db();
        insert_import(&db, "src/a.rs", "use b", Some("src/b.rs"));
        insert_import(&db, "src/a.rs", "use b again", Some("src/b.rs"));
        insert_import(&db, "src/a.rs", "use ext", None);

        let pairs = db.graph_reads().file_import_pairs().unwrap();
        assert_eq!(
            pairs,
            vec![("src/a.rs".to_string(), "src/b.rs".to_string())]
        );
    }

    #[test]
    fn import_witness_rows_keeps_import_string() {
        let (_tmp, db) = open_db();
        insert_import(&db, "src/a.rs", "use crate::b", Some("src/b.rs"));
        insert_import(&db, "src/c.rs", "use crate::a", Some("src/a.rs"));

        let rows = db.graph_reads().import_witness_rows("src/a.rs").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].resolved_path, "src/b.rs");
        assert_eq!(rows[0].import_string.as_deref(), Some("use crate::b"));
    }

    #[test]
    fn community_adjacency_pairs_cross_community_only() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_a", "a", "src/a.rs", "function");
        insert_symbol(&db, "uid_b", "b", "src/b.rs", "function");
        insert_symbol(&db, "uid_c", "c", "src/c.rs", "function");
        let conn = db.write_conn.lock().unwrap();
        for (uid, community) in [("uid_a", 1), ("uid_b", 2), ("uid_c", 1)] {
            conn.execute(
                "UPDATE symbols SET community_id = ?2 WHERE symbol_uid = ?1",
                rusqlite::params![uid, community],
            )
            .unwrap();
        }
        drop(conn);
        insert_call_edge(&db, "e1", Some("uid_a"), "uid_b"); // cross 1 -> 2
        insert_call_edge(&db, "e2", Some("uid_a"), "uid_c"); // same community

        let pairs = db.graph_reads().community_adjacency_pairs().unwrap();
        assert_eq!(pairs, vec![("1".to_string(), "2".to_string())]);
    }

    #[test]
    fn symbols_lite_in_files_skips_files_not_requested_and_uidless() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_a", "a", "src/a.rs", "function");
        insert_symbol(&db, "uid_b", "b", "src/b.rs", "function");
        // Symbol without UID in a requested file must be skipped.
        db.write_conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line)
                 VALUES('sid_nouid', 'src/a.rs', 'nouid', 'function', 20, 22)",
                [],
            )
            .unwrap();

        let rows = db
            .graph_reads()
            .symbols_lite_in_files(&["src/a.rs".to_string()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol_uid, "uid_a");
        assert_eq!(rows[0].kind, "function");
    }

    #[test]
    fn reverse_callers_applies_confidence_and_limit() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_callee", "callee", "src/a.rs", "function");
        insert_symbol(&db, "uid_hi", "hi_conf", "src/b.rs", "function");
        insert_symbol(&db, "uid_lo", "lo_conf", "src/c.rs", "function");
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid, parser_confidence)
             VALUES('e_hi', 'src/b.rs', 'callee', 1, 'uid_hi', 'uid_callee', 0.9)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid, parser_confidence)
             VALUES('e_lo', 'src/c.rs', 'callee', 2, 'uid_lo', 'uid_callee', 0.3)",
            [],
        )
        .unwrap();
        drop(conn);

        let all = db
            .graph_reads()
            .reverse_callers(&["uid_callee".to_string()], None, None)
            .unwrap();
        assert_eq!(all.len(), 2);

        let confident = db
            .graph_reads()
            .reverse_callers(&["uid_callee".to_string()], Some(0.8), None)
            .unwrap();
        assert_eq!(confident.len(), 1);
        assert_eq!(confident[0].symbol_uid, "uid_hi");

        let limited = db
            .graph_reads()
            .reverse_callers(&["uid_callee".to_string()], None, Some(1))
            .unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn suggested_test_files_distinct_and_ordered() {
        let (_tmp, db) = open_db();
        let conn = db.write_conn.lock().unwrap();
        for (edge_id, test_file, code_file) in [
            ("t1", "tests/z_test.rs", "src/a.rs"),
            ("t2", "tests/a_test.rs", "src/a.rs"),
            ("t3", "tests/a_test.rs", "src/b.rs"),
            ("t4", "tests/other.rs", "src/unrelated.rs"),
        ] {
            conn.execute(
                "INSERT INTO test_edges(edge_id, test_file_path, code_file_path, reason)
                 VALUES(?1, ?2, ?3, 'imports')",
                rusqlite::params![edge_id, test_file, code_file],
            )
            .unwrap();
        }
        drop(conn);

        let tests = db
            .graph_reads()
            .suggested_test_files(&["src/a.rs".to_string(), "src/b.rs".to_string()])
            .unwrap();
        assert_eq!(tests, vec!["tests/a_test.rs", "tests/z_test.rs"]);
        assert!(db
            .graph_reads()
            .suggested_test_files(&[])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn symbol_names_for_uids_skips_missing() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_a", "alpha", "src/a.rs", "function");

        let map = db
            .graph_reads()
            .symbol_names_for_uids(&["uid_a".to_string(), "uid_missing".to_string()])
            .unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("uid_a").map(String::as_str), Some("alpha"));
    }

    #[test]
    fn direct_importers_exclude_self_and_respect_limit() {
        let (_tmp, db) = open_db();
        insert_import(&db, "src/b.rs", "use a", Some("src/a.rs"));
        insert_import(&db, "src/c.rs", "use a", Some("src/a.rs"));
        insert_import(&db, "src/a.rs", "use self", Some("src/a.rs"));

        let importers = db
            .graph_reads()
            .direct_importers_of_file("src/a.rs", 200)
            .unwrap();
        assert_eq!(importers, vec!["src/b.rs", "src/c.rs"]);
        let capped = db
            .graph_reads()
            .direct_importers_of_file("src/a.rs", 1)
            .unwrap();
        assert_eq!(capped, vec!["src/b.rs"]);
    }

    #[test]
    fn importers_of_paths_batches_into_single_query() {
        let (_tmp, db) = open_db();
        insert_import(&db, "src/x.rs", "use b", Some("src/b.rs"));
        insert_import(&db, "src/y.rs", "use c", Some("src/c.rs"));

        let mut importers = db
            .graph_reads()
            .importers_of_paths(&["src/b.rs".to_string(), "src/c.rs".to_string()], 10000)
            .unwrap();
        importers.sort();
        assert_eq!(importers, vec!["src/x.rs", "src/y.rs"]);
        assert!(db
            .graph_reads()
            .importers_of_paths(&[], 10000)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn callees_with_nonself_callers_excludes_self_loops() {
        let (_tmp, db) = open_db();
        insert_call_edge(&db, "e1", Some("uid_caller"), "uid_called");
        insert_call_edge(&db, "e2", Some("uid_self"), "uid_self");
        insert_call_edge(&db, "e3", None, "uid_orphan_caller");

        let mut callees = db
            .graph_reads()
            .callees_with_nonself_callers(10000)
            .unwrap();
        callees.sort();
        assert_eq!(callees, vec!["uid_called", "uid_orphan_caller"]);
    }

    #[test]
    fn dead_code_symbol_scan_applies_scope_like_filter() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_a", "a", "src/core/a.rs", "function");
        insert_symbol(&db, "uid_b", "b", "src/util/b.rs", "function");

        let scoped = db
            .graph_reads()
            .dead_code_symbol_scan(Some("src/core"), 100)
            .unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].symbol_uid, "uid_a");

        let all = db.graph_reads().dead_code_symbol_scan(None, 100).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn symbol_ref_containers_for_targets_returns_uid_container_pairs() {
        let (_tmp, db) = open_db();
        insert_file(&db, "src/a.rs");
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO symbol_refs(ref_id, file_path, symbol_name, container, ref_kind, line, target_symbol_uid)
             VALUES('r1', 'src/a.rs', 'helper', 'other_fn', 'call', 3, 'uid_helper')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO symbol_refs(ref_id, file_path, symbol_name, ref_kind, line, target_symbol_uid)
             VALUES('r2', 'src/a.rs', 'helper', 'call', 7, 'uid_helper')",
            [],
        )
        .unwrap();
        drop(conn);

        let mut rows = db
            .graph_reads()
            .symbol_ref_containers_for_targets(&["uid_helper".to_string()])
            .unwrap();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("uid_helper".to_string(), None),
                ("uid_helper".to_string(), Some("other_fn".to_string())),
            ]
        );
    }

    fn insert_route(db: &IndexDb, edge_id: &str, route_path: &str, method: Option<&str>) {
        insert_file(db, "src/routes.ts");
        db.write_conn.lock().unwrap()
            .execute(
                "INSERT INTO routes(edge_id, file_path, route_path, method, handler_name, framework, line, normalized_path, route_id, handler_symbol_uid)
                 VALUES(?1, 'src/routes.ts', ?2, ?3, 'handler', 'express', 10, ?2, ?1, 'uid_handler')",
                rusqlite::params![edge_id, route_path, method],
            )
            .unwrap();
    }

    #[test]
    fn route_handler_rows_filters_by_path_substring() {
        let (_tmp, db) = open_db();
        insert_route(&db, "r1", "/api/users", Some("GET"));
        insert_route(&db, "r2", "/health", Some("GET"));

        let rows = db
            .graph_reads()
            .route_handler_rows(Some("users"), 50)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["route_path"], "/api/users");
        let all = db.graph_reads().route_handler_rows(None, 50).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn async_consumer_rows_match_topic_in_name_or_properties() {
        let (_tmp, db) = open_db();
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO infra_nodes(node_id, file_path, kind, name) VALUES('n1', 'infra.yml', 'queue', 'orders-queue')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO infra_edges(edge_id, source_node_id, target_node_id, kind) VALUES('ie1', 'n1', 'n2', 'consumes_queue')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO infra_edges(edge_id, source_node_id, target_node_id, kind) VALUES('ie2', 'n1', 'n2', 'depends_on')",
            [],
        )
        .unwrap();
        drop(conn);

        let rows = db.graph_reads().async_consumer_rows("orders").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["kind"], "consumes_queue");
        assert_eq!(rows[0]["source_name"], "orders-queue");
    }

    #[test]
    fn service_binding_rows_match_nodes_routes_and_edges() {
        let (_tmp, db) = open_db();
        insert_route(&db, "r_pay", "/payment/charge", Some("POST"));
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO infra_nodes(node_id, file_path, kind, name) VALUES('n_pay', 'infra.yml', 'service', 'payment-svc')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO infra_edges(edge_id, source_node_id, target_node_id, kind) VALUES('ie1', 'n_pay', 'r_pay', 'routes_to')",
            [],
        )
        .unwrap();
        drop(conn);

        let bindings = db.graph_reads().service_binding_rows("payment").unwrap();
        assert_eq!(bindings.matched_infra_nodes.len(), 1);
        assert_eq!(bindings.matched_routes.len(), 1);
        assert_eq!(bindings.related_edges.len(), 1);
        assert_eq!(bindings.related_edges[0]["target_type"], "route");
    }

    #[test]
    fn child_symbol_outline_rows_ordered_by_start_line() {
        let (_tmp, db) = open_db();
        insert_file(&db, "src/a.rs");
        let conn = db.write_conn.lock().unwrap();
        for (sid, name, line, sig) in [
            ("sid_m2", "method_b", 8, Some("fn method_b()")),
            ("sid_m1", "method_a", 3, None),
        ] {
            conn.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, parent_symbol_id, signature)
                 VALUES(?1, 'src/a.rs', ?2, 'method', ?3, ?3, 'uid_parent', ?4)",
                rusqlite::params![sid, name, line, sig],
            )
            .unwrap();
        }
        drop(conn);

        let rows = db.child_symbol_outline_rows("uid_parent").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "method_a");
        assert_eq!(rows[1].2.as_deref(), Some("fn method_b()"));
    }

    #[test]
    fn symbol_source_candidates_rank_exact_before_fuzzy() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_exact", "lookup", "src/a.rs", "function");
        insert_symbol(&db, "uid_fuzzy", "lookup_table", "src/b.rs", "function");

        let exact = db.symbol_source_candidates("lookup", true).unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0]["name"], "lookup");

        let fuzzy = db.symbol_source_candidates("lookup", false).unwrap();
        assert_eq!(fuzzy.len(), 2);
        assert_eq!(fuzzy[0]["name"], "lookup");
    }

    #[test]
    fn symbol_kind_counts_orders_by_frequency() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_f1", "f1", "src/a.rs", "function");
        insert_symbol(&db, "uid_f2", "f2", "src/a.rs", "function");
        insert_symbol(&db, "uid_c1", "C1", "src/a.rs", "class");

        let counts = db.symbol_kind_counts().unwrap();
        assert_eq!(counts[0], ("function".to_string(), 2));
        assert_eq!(counts[1], ("class".to_string(), 1));
    }

    #[test]
    fn call_edge_provenance_counts_breakdowns() {
        let (_tmp, db) = open_db();
        insert_file(&db, "src/a.rs");
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, dispatch_kind, resolution_kind)
             VALUES('e1', 'src/a.rs', 'x', 1, 'direct', 'exact')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id, file_path, callee_symbol, line, dispatch_kind, resolution_kind, synthesized_by)
             VALUES('e2', 'src/a.rs', 'y', 2, 'http_bridge', 'heuristic', 'http_bridge')",
            [],
        )
        .unwrap();
        drop(conn);

        let provenance = db.call_edge_provenance().unwrap();
        assert_eq!(provenance.synthesized_total, 1);
        assert_eq!(provenance.by_dispatch_kind.len(), 2);
        assert!(provenance
            .by_synthesized_by
            .iter()
            .any(|(by, cnt)| by.as_deref() == Some("http_bridge") && *cnt == 1));
        assert!(provenance
            .by_resolution_kind
            .iter()
            .any(|(kind, cnt)| kind.as_deref() == Some("exact") && *cnt == 1));
    }

    #[test]
    fn cross_file_call_file_pairs_skip_same_file_edges() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_a", "a", "src/a.rs", "function");
        insert_symbol(&db, "uid_b", "b", "src/b.rs", "function");
        insert_symbol(&db, "uid_a2", "a2", "src/a.rs", "function");
        insert_call_edge(&db, "e1", Some("uid_a"), "uid_b");
        insert_call_edge(&db, "e2", Some("uid_a"), "uid_a2");

        let pairs = db.cross_file_call_file_pairs().unwrap();
        assert_eq!(
            pairs,
            vec![("src/a.rs".to_string(), "src/b.rs".to_string())]
        );
    }

    #[test]
    fn param_counts_for_uids_maps_null_to_none() {
        let (_tmp, db) = open_db();
        insert_symbol(&db, "uid_two", "two_params", "src/a.rs", "method");
        insert_symbol(&db, "uid_null", "no_count", "src/a.rs", "method");
        db.write_conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE symbols SET param_count = 2 WHERE symbol_uid = 'uid_two'",
                [],
            )
            .unwrap();

        let counts = db
            .param_counts_for_uids(&["uid_two".to_string(), "uid_null".to_string()])
            .unwrap();
        assert_eq!(counts.get("uid_two"), Some(&Some(2)));
        assert_eq!(counts.get("uid_null"), Some(&None));
        assert!(db.param_counts_for_uids(&[]).unwrap().is_empty());
    }

    #[test]
    fn env_access_rows_filter_by_key_and_file_patterns() {
        let (_tmp, db) = open_db();
        let conn = db.write_conn.lock().unwrap();
        for (edge_id, file, key) in [
            ("d1", "src/config.rs", "DATABASE_URL"),
            ("d2", "src/auth.rs", "AUTH_SECRET"),
        ] {
            conn.execute(
                "INSERT INTO data_flow_edges(edge_id, file_path, flow_kind, line, env_key)
                 VALUES(?1, ?2, 'env_access', 4, ?3)",
                rusqlite::params![edge_id, file, key],
            )
            .unwrap();
        }
        // Non-env flow must never match.
        conn.execute(
            "INSERT INTO data_flow_edges(edge_id, file_path, flow_kind, line, env_key)
             VALUES('d3', 'src/config.rs', 'read', 5, 'DATABASE_URL')",
            [],
        )
        .unwrap();
        drop(conn);

        let by_key = db.env_access_rows("%DATABASE%", None, 50).unwrap();
        assert_eq!(by_key.len(), 1);
        assert_eq!(by_key[0]["env_key"], "DATABASE_URL");

        let by_key_and_file = db.env_access_rows("%A%", Some("%auth%"), 50).unwrap();
        assert_eq!(by_key_and_file.len(), 1);
        assert_eq!(by_key_and_file[0]["file_path"], "src/auth.rs");
    }
}
