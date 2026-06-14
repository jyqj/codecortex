//! IndexDb methods: graph traversal, community detection, framework post-processing.
//!
//! Convention: hot-path point reads with constant SQL string literals use
//! `prepare_cached`; dynamically built SQL (e.g. variable-arity `IN (...)`
//! placeholders) must keep using `prepare` so it does not pollute the
//! per-connection statement cache.

use std::collections::HashMap;

use cc_model::CcResult;
use rusqlite::OptionalExtension;

use crate::index_db::{
    CallEdgeLite, EdgeLiteBfs, FileEdgesForReresolve, FileFrameworkRecord, IndexDb, ReadOps,
    RepoFrameworkRecord, ResolutionAttemptRow, SymbolCoverRow, SymbolDegreeInfo, SymbolRefLite,
    SymbolRow, WriteOps,
};
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

/// Methods grouped by container name: container -> [(symbol_uid, name, file_path, start_line)].
pub(crate) type MethodsByContainer = HashMap<String, Vec<(String, String, String, u32)>>;

impl IndexDb {
    pub(crate) fn symbols_covering(
        &self,
        file_path: &str,
        line: u32,
        limit: usize,
    ) -> CcResult<Vec<SymbolCoverRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT symbol_id, symbol_uid, name, file_path, start_line, end_line
                 FROM symbols
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2
                 ORDER BY (end_line - start_line) ASC
                 LIMIT ?3",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![file_path, line, limit as i64], |row| {
                Ok(SymbolCoverRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    file_path: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn caller_rows_by_uid(
        &self,
        callee_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE callee_symbol_uid = ?1
                 ORDER BY line ASC, rowid ASC
                 LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![callee_uid, limit as i64],
                crate::rows::call_edge_lite,
            )
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn callee_rows_by_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE caller_symbol_uid = ?1
                 ORDER BY line ASC, rowid ASC
                 LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![caller_uid, limit as i64],
                crate::rows::call_edge_lite,
            )
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Batched variant of [`Self::caller_rows_by_uid`]: fetch the top
    /// `per_seed_limit` caller edges for many callee UIDs in
    /// [`IN_BATCH_SIZE`]-sized queries instead of one round-trip per UID.
    /// Returns rows grouped by callee UID; UIDs with no callers are
    /// absent from the map. Both the single-UID point queries and this
    /// batched variant order rows by `(line ASC, rowid ASC)` as a
    /// contract, so per-UID row selection and order are identical.
    /// Multi-symbol neighbor resolution should use the batched variants
    /// (together with [`Self::symbol_degree_details_batch`]) instead of
    /// looping the point queries — `cc-search`'s enrich, preselect, and
    /// lanes adapters are the reference call sites.
    pub(crate) fn caller_rows_by_uids(
        &self,
        callee_uids: &[&str],
        per_seed_limit: usize,
    ) -> CcResult<HashMap<String, Vec<CallEdgeLite>>> {
        self.call_edge_rows_by_uids(callee_uids, per_seed_limit, "callee_symbol_uid")
    }

    /// Batched variant of [`Self::callee_rows_by_uid`]; see
    /// [`Self::caller_rows_by_uids`] for grouping and ordering semantics.
    pub(crate) fn callee_rows_by_uids(
        &self,
        caller_uids: &[&str],
        per_seed_limit: usize,
    ) -> CcResult<HashMap<String, Vec<CallEdgeLite>>> {
        self.call_edge_rows_by_uids(caller_uids, per_seed_limit, "caller_symbol_uid")
    }

    /// Shared impl for the batched adjacency accessors: per-seed top-k via
    /// `ROW_NUMBER() OVER (PARTITION BY seed ORDER BY line, rowid)`.
    /// `seed_column` is one of the two UID columns of `call_edges`.
    fn call_edge_rows_by_uids(
        &self,
        seed_uids: &[&str],
        per_seed_limit: usize,
        seed_column: &str,
    ) -> CcResult<HashMap<String, Vec<CallEdgeLite>>> {
        let mut grouped: HashMap<String, Vec<CallEdgeLite>> = HashMap::new();
        if seed_uids.is_empty() || per_seed_limit == 0 {
            return Ok(grouped);
        }
        // Dedupe so a seed repeated across chunks cannot collect its edges twice.
        let mut unique: Vec<&str> = seed_uids.to_vec();
        unique.sort_unstable();
        unique.dedup();

        let conn = self.read_conn()?;
        for chunk in unique.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(chunk.len());
            let limit_param = chunk.len() + 1;
            let sql = format!(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line, seed_uid
                 FROM (
                     SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line,
                            {seed_column} AS seed_uid,
                            ROW_NUMBER() OVER (PARTITION BY {seed_column} ORDER BY line ASC, rowid ASC) AS rn
                     FROM call_edges
                     WHERE {seed_column} IN ({placeholders})
                 )
                 WHERE rn <= ?{limit_param}
                 ORDER BY seed_uid, rn"
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let limit_value = per_seed_limit as i64;
            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(chunk.len() + 1);
            for uid in chunk {
                params.push(uid);
            }
            params.push(&limit_value);
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    let edge = crate::rows::call_edge_lite(row)?;
                    let seed: String = row.get(13)?;
                    Ok((seed, edge))
                })
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                let (seed, edge) = row;
                grouped.entry(seed).or_default().push(edge);
            }
        }
        Ok(grouped)
    }

    pub(crate) fn symbol_ref_rows_by_uid(
        &self,
        target_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<SymbolRefLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT file_path, line, symbol_name, target_symbol_uid, resolution_kind, resolution_confidence
                 FROM symbol_refs
                 WHERE target_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![target_uid, limit as i64], |row| {
                Ok(SymbolRefLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    symbol_name: row.get(2)?,
                    target_symbol_uid: row.get(3)?,
                    resolution_kind: row.get(4)?,
                    confidence: row.get(5)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Return a summary of environment variable accesses, ordered by frequency.
    ///
    /// Each tuple: `(env_key, count, comma_separated_file_paths)`.
    pub(crate) fn env_var_summary(&self, limit: usize) -> CcResult<Vec<(String, i64, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT env_key, COUNT(*) as cnt, GROUP_CONCAT(DISTINCT file_path) \
             FROM data_flow_edges \
             WHERE flow_kind = 'env_access' AND env_key IS NOT NULL \
             GROUP BY env_key \
             ORDER BY cnt DESC \
             LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(db_err)?);
        }
        Ok(result)
    }

    /// No-op: resolution_attempts table removed in schema consolidation.
    pub(crate) fn list_resolution_attempts(
        &self,
        _limit: usize,
        _file_path: Option<&str>,
        _kind: Option<&str>,
    ) -> CcResult<Vec<ResolutionAttemptRow>> {
        Ok(Vec::new())
    }

    // ── Graph / framework post-processing ───────────────────────

    pub(crate) fn call_uid_edges(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT caller_symbol_uid, callee_symbol_uid
                 FROM call_edges
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Return BFS-friendly outgoing edges for a single caller UID.
    ///
    /// This is the per-node variant of `call_uid_edges_lite`, used by lazy BFS
    /// to avoid loading the full edge set into memory.
    pub(crate) fn call_edges_from_uid_lite(&self, caller_uid: &str) -> CcResult<Vec<EdgeLiteBfs>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT caller_symbol_uid, callee_symbol_uid, dispatch_kind, \
                        synthesized_by, synthesis_key, resolution_confidence, \
                        file_path, line, registered_file, registered_line, \
                        resolution_kind, parser_tier, resolution_strategy, parser_confidence \
                 FROM call_edges \
                 WHERE caller_symbol_uid = ?1 AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid], |row| {
                Ok(EdgeLiteBfs {
                    caller_uid: row.get(0)?,
                    callee_uid: row.get(1)?,
                    dispatch_kind: row.get::<_, String>(2).unwrap_or_default(),
                    synthesized_by: row.get(3).ok(),
                    synthesis_key: row.get(4).ok(),
                    confidence: row.get::<_, f64>(5).unwrap_or(0.0),
                    file_path: row.get::<_, String>(6).unwrap_or_default(),
                    line: row.get::<_, u32>(7).unwrap_or(0),
                    registered_file: row.get(8).ok(),
                    registered_line: row.get(9).ok(),
                    resolution_kind: row.get(10).ok(),
                    parser_tier: row.get(11).ok(),
                    resolution_strategy: row.get(12).ok(),
                    parser_confidence: row.get(13).ok(),
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn call_uid_edges_lite(&self) -> CcResult<Vec<EdgeLiteBfs>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT caller_symbol_uid, callee_symbol_uid, dispatch_kind, \
                        synthesized_by, synthesis_key, resolution_confidence, \
                        file_path, line, registered_file, registered_line, \
                        resolution_kind, parser_tier, resolution_strategy, parser_confidence \
                 FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(EdgeLiteBfs {
                    caller_uid: row.get(0)?,
                    callee_uid: row.get(1)?,
                    dispatch_kind: row.get::<_, String>(2).unwrap_or_default(),
                    synthesized_by: row.get(3).ok(),
                    synthesis_key: row.get(4).ok(),
                    confidence: row.get::<_, f64>(5).unwrap_or(0.0),
                    file_path: row.get::<_, String>(6).unwrap_or_default(),
                    line: row.get::<_, u32>(7).unwrap_or(0),
                    registered_file: row.get(8).ok(),
                    registered_line: row.get(9).ok(),
                    resolution_kind: row.get(10).ok(),
                    parser_tier: row.get(11).ok(),
                    resolution_strategy: row.get(12).ok(),
                    parser_confidence: row.get(13).ok(),
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn symbol_names_by_uid(&self) -> CcResult<HashMap<String, String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT symbol_uid, name FROM symbols WHERE symbol_uid IS NOT NULL")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        let mut map = HashMap::new();
        for row in rows {
            let (uid, name) = row.map_err(db_err)?;
            map.insert(uid, name);
        }
        Ok(map)
    }

    /// Bulk lookup symbol metadata by UIDs. Batches in [`IN_BATCH_SIZE`] chunks.
    pub(crate) fn symbol_rows_by_uids(
        &self,
        uids: &[String],
    ) -> CcResult<HashMap<String, SymbolRow>> {
        let conn = self.read_conn()?;
        let mut result = HashMap::new();
        for chunk in uids.chunks(IN_BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            let sql = format!(
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, \
                        start_line, end_line, qname, signature \
                 FROM symbols WHERE symbol_uid IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|s| s as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), crate::rows::symbol_row)
                .map_err(db_err)?;
            for r in rows {
                let r = r.map_err(db_err)?;
                if let Some(uid) = r.symbol_uid.clone() {
                    result.insert(uid, r);
                }
            }
        }
        Ok(result)
    }

    /// Get degree info for a single symbol UID.
    pub(crate) fn symbol_degree_details(&self, uid: &str) -> CcResult<SymbolDegreeInfo> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT \
                    (SELECT COUNT(*) FROM call_edges WHERE callee_symbol_uid = ?1) AS in_degree, \
                    (SELECT COUNT(*) FROM call_edges WHERE caller_symbol_uid = ?1) AS out_degree, \
                    (SELECT COUNT(DISTINCT caller_symbol_uid) FROM call_edges WHERE callee_symbol_uid = ?1) AS caller_count, \
                    (SELECT COUNT(DISTINCT callee_symbol_uid) FROM call_edges WHERE caller_symbol_uid = ?1) AS callee_count, \
                    (SELECT COUNT(*) FROM symbol_refs WHERE target_symbol_uid = ?1) AS ref_count",
            )
            .map_err(db_err)?;
        let info = stmt
            .query_row([uid], |row| {
                Ok(SymbolDegreeInfo {
                    in_degree: row.get(0)?,
                    out_degree: row.get(1)?,
                    caller_count: row.get(2)?,
                    callee_count: row.get(3)?,
                    ref_count: row.get(4)?,
                })
            })
            .map_err(db_err)?;
        Ok(info)
    }

    /// Batched variant of [`Self::symbol_degree_details`]: compute the same
    /// five subcounts for many UIDs in [`IN_BATCH_SIZE`]-sized `IN (...)`
    /// queries (three `GROUP BY` aggregates per chunk) instead of five
    /// correlated subqueries per UID. Every requested UID is present in the
    /// returned map; UIDs with no edges/refs — including UIDs unknown to the
    /// index — carry all-zero counts, exactly what the single-UID query
    /// returns for them. Multi-symbol degree resolution should use this
    /// batched variant (`cc-search`'s enrich is the reference adapter).
    pub(crate) fn symbol_degree_details_batch(
        &self,
        uids: &[&str],
    ) -> CcResult<HashMap<String, SymbolDegreeInfo>> {
        let mut result: HashMap<String, SymbolDegreeInfo> = uids
            .iter()
            .map(|uid| {
                (
                    uid.to_string(),
                    SymbolDegreeInfo {
                        in_degree: 0,
                        out_degree: 0,
                        caller_count: 0,
                        callee_count: 0,
                        ref_count: 0,
                    },
                )
            })
            .collect();
        if result.is_empty() {
            return Ok(result);
        }
        let mut unique: Vec<&str> = uids.to_vec();
        unique.sort_unstable();
        unique.dedup();

        let conn = self.read_conn()?;
        for chunk in unique.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(chunk.len());
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|uid| uid as &dyn rusqlite::types::ToSql)
                .collect();

            // Callee side: in_degree + distinct caller count.
            let sql = format!(
                "SELECT callee_symbol_uid, COUNT(*), COUNT(DISTINCT caller_symbol_uid)
                 FROM call_edges WHERE callee_symbol_uid IN ({placeholders})
                 GROUP BY callee_symbol_uid"
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                let (uid, in_degree, caller_count) = row;
                if let Some(info) = result.get_mut(&uid) {
                    info.in_degree = in_degree;
                    info.caller_count = caller_count;
                }
            }

            // Caller side: out_degree + distinct callee count.
            let sql = format!(
                "SELECT caller_symbol_uid, COUNT(*), COUNT(DISTINCT callee_symbol_uid)
                 FROM call_edges WHERE caller_symbol_uid IN ({placeholders})
                 GROUP BY caller_symbol_uid"
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                let (uid, out_degree, callee_count) = row;
                if let Some(info) = result.get_mut(&uid) {
                    info.out_degree = out_degree;
                    info.callee_count = callee_count;
                }
            }

            // Reference count.
            let sql = format!(
                "SELECT target_symbol_uid, COUNT(*)
                 FROM symbol_refs WHERE target_symbol_uid IN ({placeholders})
                 GROUP BY target_symbol_uid"
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                let (uid, ref_count) = row;
                if let Some(info) = result.get_mut(&uid) {
                    info.ref_count = ref_count;
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn update_communities(
        &self,
        assignments: &HashMap<String, u32>,
        labels: &HashMap<u32, String>,
    ) -> CcResult<()> {
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.transaction().map_err(db_err)?;

        tx.execute("UPDATE symbols SET community_id = NULL", [])
            .map_err(db_err)?;
        tx.execute("DELETE FROM communities", []).map_err(db_err)?;

        let mut member_counts: HashMap<u32, usize> = HashMap::new();
        {
            let mut stmt = tx
                .prepare_cached("UPDATE symbols SET community_id = ?1 WHERE symbol_uid = ?2")
                .map_err(db_err)?;
            for (uid, community_id) in assignments {
                stmt.execute(rusqlite::params![community_id, uid])
                    .map_err(db_err)?;
                *member_counts.entry(*community_id).or_insert(0) += 1;
            }
        }

        for (community_id, label) in labels {
            let member_count = member_counts.get(community_id).copied().unwrap_or(0);
            tx.execute(
                "INSERT INTO communities(community_id,label,member_count,representative_file,top_symbols_json)
                 VALUES(?1,?2,?3,NULL,'[]')",
                rusqlite::params![community_id, label, member_count as i64],
            )
            .map_err(db_err)?;
        }

        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Degraded community assignment: assign all symbols that have no community
    /// to the given `community_id`. Used when edge count exceeds the threshold
    /// and full Louvain detection would risk OOM.
    pub(crate) fn assign_all_symbols_to_community(&self, community_id: u32) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        tx.execute(
            "UPDATE symbols SET community_id = ?1 WHERE community_id IS NULL",
            rusqlite::params![community_id],
        )
        .map_err(db_err)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    pub(crate) fn replace_repo_frameworks(&self, signals: &[RepoFrameworkRecord]) -> CcResult<()> {
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM frameworks WHERE scope='repo'", [])
            .map_err(db_err)?;
        let now = chrono::Utc::now().to_rfc3339();
        for (framework_key, confidence, evidences) in signals {
            let signals_json = serde_json::to_string(evidences).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT INTO frameworks(framework_key,scope,scope_id,confidence,signals_json,updated_at)
                 VALUES(?1,'repo','',?2,?3,?4)",
                rusqlite::params![framework_key, confidence, signals_json, now],
            )
            .map_err(db_err)?;
        }
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    pub(crate) fn replace_file_frameworks(&self, by_file: &[FileFrameworkRecord]) -> CcResult<()> {
        if by_file.is_empty() {
            return Ok(());
        }
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.transaction().map_err(db_err)?;
        for (file_path, signals) in by_file {
            tx.execute(
                "DELETE FROM frameworks WHERE scope='file' AND scope_id = ?1",
                rusqlite::params![file_path],
            )
            .map_err(db_err)?;
            for (framework_key, confidence, evidence) in signals {
                let signals_json =
                    serde_json::to_string(&vec![evidence.clone()]).unwrap_or_else(|_| "[]".into());
                tx.execute(
                    "INSERT INTO frameworks(framework_key,scope,scope_id,confidence,signals_json)
                     VALUES(?1,'file',?2,?3,?4)",
                    rusqlite::params![framework_key, file_path, confidence, signals_json],
                )
                .map_err(db_err)?;
            }
        }
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Find symbols by exact name, filtering to function/class/component kinds.
    pub(crate) fn find_symbols_by_name_and_kinds(
        &self,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
        Self::find_symbols_by_name_and_kinds_on(&conn, name, kinds)
    }

    pub(crate) fn find_symbols_by_name_and_kinds_on(
        conn: &rusqlite::Connection,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: String = kinds
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 2))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature \
             FROM symbols WHERE name = ?1 AND kind IN ({}) ORDER BY file_path",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(name.to_string()));
        for kind in kinds {
            params.push(Box::new(kind.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), crate::rows::symbol_row)
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Batched variant of [`Self::find_symbols_by_name_and_kinds`]: resolves
    /// many names with [`IN_BATCH_SIZE`]-sized `IN (...)` queries instead of
    /// one round-trip per name. Returns rows grouped by symbol name; names
    /// with no match are absent from the map. Within each name, row order
    /// matches the single-name variant (`ORDER BY file_path`).
    pub(crate) fn find_symbols_by_names_and_kinds(
        &self,
        names: &[&str],
        kinds: &[&str],
    ) -> CcResult<HashMap<String, Vec<SymbolRow>>> {
        let mut grouped: HashMap<String, Vec<SymbolRow>> = HashMap::new();
        if names.is_empty() || kinds.is_empty() {
            return Ok(grouped);
        }
        let conn = self.read_conn()?;
        for batch in names.chunks(IN_BATCH_SIZE) {
            let name_placeholders = sql_in_placeholders(batch.len());
            let kind_placeholders: String = (1..=kinds.len())
                .map(|idx| format!("?{}", batch.len() + idx))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line, qname, signature \
                 FROM symbols WHERE name IN ({}) AND kind IN ({}) ORDER BY name, file_path",
                name_placeholders, kind_placeholders
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let mut params: Vec<&dyn rusqlite::types::ToSql> =
                Vec::with_capacity(batch.len() + kinds.len());
            for name in batch {
                params.push(name);
            }
            for kind in kinds {
                params.push(kind);
            }
            let rows = stmt
                .query_map(params.as_slice(), crate::rows::symbol_row)
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                grouped.entry(row.name.clone()).or_default().push(row);
            }
        }
        Ok(grouped)
    }

    /// Delete synthetic semantic edges whose edge_id starts with a given prefix.
    pub(crate) fn delete_synthetic_semantic_edges(&self, edge_id_prefix: &str) -> CcResult<usize> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        let count = Self::delete_synthetic_semantic_edges_on(&tx, edge_id_prefix)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(count)
    }

    pub(crate) fn delete_synthetic_semantic_edges_on(
        conn: &rusqlite::Connection,
        edge_id_prefix: &str,
    ) -> CcResult<usize> {
        // `synth:%` rows are outside every signature aggregate, so this
        // delete needs no aggregate maintenance — which is only sound while
        // the prefix stays within the synthetic id namespace.
        debug_assert!(
            edge_id_prefix.starts_with("synth:"),
            "delete_synthetic_semantic_edges requires a 'synth:' prefix \
             (real semantic edges are signature-aggregate tracked)"
        );
        let pattern = format!("{}%", edge_id_prefix);
        conn.execute(
            "DELETE FROM semantic_edges WHERE edge_id LIKE ?1",
            rusqlite::params![pattern],
        )
        .map_err(db_err)
    }

    /// Find the symbol_uid of a method named `method_name` contained in the same class
    /// as the given symbol_uid.
    pub(crate) fn find_method_in_same_class(
        &self,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        Self::find_method_in_same_class_on(&conn, member_symbol_uid, method_name)
    }

    pub(crate) fn find_method_in_same_class_on(
        conn: &rusqlite::Connection,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        let container: Option<String> = conn
            .query_row(
                "SELECT container FROM symbols WHERE symbol_uid = ?1",
                rusqlite::params![member_symbol_uid],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?
            .flatten();
        let container = match container {
            Some(c) => c,
            None => return Ok(None),
        };
        let file_path: Option<String> = conn
            .query_row(
                "SELECT file_path FROM symbols WHERE symbol_uid = ?1",
                rusqlite::params![member_symbol_uid],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let file_path = match file_path {
            Some(fp) => fp,
            None => return Ok(None),
        };
        let result: Option<String> = conn
            .query_row(
                "SELECT symbol_uid FROM symbols WHERE file_path = ?1 AND container = ?2 AND name = ?3 AND kind = 'method' LIMIT 1",
                rusqlite::params![file_path, container, method_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?
            .flatten();
        Ok(result)
    }

    /// Fetch methods for many containers (class/struct names) in a single
    /// `IN (...)` query, grouped by container name. Avoids the per-container
    /// N+1 round-trips in dispatch synthesis.
    pub(crate) fn find_methods_by_containers(
        &self,
        containers: &[&str],
    ) -> CcResult<MethodsByContainer> {
        if containers.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.read_conn()?;
        Self::find_methods_by_containers_on(&conn, containers)
    }

    pub(crate) fn find_methods_by_containers_on(
        conn: &rusqlite::Connection,
        containers: &[&str],
    ) -> CcResult<MethodsByContainer> {
        let mut grouped: MethodsByContainer = HashMap::new();
        if containers.is_empty() {
            return Ok(grouped);
        }
        let placeholders: String = containers
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT container, symbol_uid, name, file_path, start_line \
             FROM symbols WHERE container IN ({}) AND kind = 'method' AND symbol_uid IS NOT NULL \
             ORDER BY file_path, start_line",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let params: Vec<Box<dyn rusqlite::types::ToSql>> = containers
            .iter()
            .map(|c| Box::new(c.to_string()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u32>(4)?,
                ))
            })
            .map_err(db_err)?;
        for row in rows {
            let (container, uid, name, file_path, line) = row.map_err(db_err)?;
            grouped
                .entry(container)
                .or_default()
                .push((uid, name, file_path, line));
        }
        Ok(grouped)
    }

    /// Find all classes that have methods matching any of the given name patterns.
    pub(crate) fn find_classes_with_method_names(
        &self,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        if method_names.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
        Self::find_classes_with_method_names_on(&conn, method_names)
    }

    pub(crate) fn find_classes_with_method_names_on(
        conn: &rusqlite::Connection,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        if method_names.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: String = method_names
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT DISTINCT container, file_path \
             FROM symbols WHERE kind = 'method' AND container IS NOT NULL AND name IN ({}) \
             ORDER BY file_path",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for name in method_names {
            params.push(Box::new(name.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Get the export fingerprint for a file.
    pub(crate) fn get_export_fingerprint(&self, file_path: &str) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_uid, name, COALESCE(signature, '') as sig, COALESCE(export_name, '') as exp
                 FROM symbols
                 WHERE file_path = ?1
                   AND (export_name IS NOT NULL OR is_default_export = 1)
                 ORDER BY symbol_uid",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_err)?;

        let mut parts: Vec<String> = Vec::new();
        for row in rows {
            let (uid, name, sig, exp) = row.map_err(db_err)?;
            parts.push(format!("{}|{}|{}|{}", uid, name, sig, exp));
        }

        if parts.is_empty() {
            return Ok(None);
        }

        let combined = parts.join("\n");
        let hash = blake3::hash(combined.as_bytes());
        Ok(Some(hash.to_hex().to_string()))
    }

    /// Batch variant of [`Self::get_export_fingerprint`]: compute the export
    /// fingerprint for many files in a single query, avoiding N+1 round trips
    /// during dirty propagation.
    ///
    /// Returns a map of `file_path -> fingerprint` containing only files that
    /// have at least one exported symbol (matching the single-file method,
    /// which returns `None` for files with no exports). The per-file hash is
    /// byte-for-byte identical to `get_export_fingerprint(path)`.
    pub(crate) fn get_export_fingerprints(
        &self,
        file_paths: &[String],
    ) -> CcResult<HashMap<String, String>> {
        let mut result: HashMap<String, String> = HashMap::new();
        if file_paths.is_empty() {
            return Ok(result);
        }

        const BATCH_SIZE: usize = 500;
        let conn = self.read_conn()?;

        for chunk in file_paths.chunks(BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            // Order by file_path first so each file's rows are contiguous, then
            // by symbol_uid to match the single-file query's ORDER BY.
            let sql = format!(
                "SELECT file_path, symbol_uid, name, COALESCE(signature, '') as sig, \
                        COALESCE(export_name, '') as exp
                 FROM symbols
                 WHERE file_path IN ({})
                   AND (export_name IS NOT NULL OR is_default_export = 1)
                 ORDER BY file_path, symbol_uid",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                })
                .map_err(db_err)?;

            // Group rows per file (rows are contiguous thanks to ORDER BY).
            let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
            for row in rows {
                let (file_path, uid, name, sig, exp) = row.map_err(db_err)?;
                grouped
                    .entry(file_path)
                    .or_default()
                    .push(format!("{}|{}|{}|{}", uid, name, sig, exp));
            }

            for (file_path, parts) in grouped {
                if parts.is_empty() {
                    continue;
                }
                let combined = parts.join("\n");
                let hash = blake3::hash(combined.as_bytes());
                result.insert(file_path, hash.to_hex().to_string());
            }
        }

        Ok(result)
    }

    /// Find all files that import the given resolved paths.
    pub(crate) fn find_importers_of(&self, resolved_paths: &[String]) -> CcResult<Vec<String>> {
        if resolved_paths.is_empty() {
            return Ok(Vec::new());
        }
        const BATCH_SIZE: usize = 500;
        let conn = self.read_conn()?;
        let mut result: std::collections::HashSet<String> = std::collections::HashSet::new();

        for chunk in resolved_paths.chunks(BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            let sql = format!(
                "SELECT DISTINCT file_path FROM imports WHERE resolved_path IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| row.get::<_, String>(0))
                .map_err(db_err)?;
            for row in rows {
                result.insert(row.map_err(db_err)?);
            }
        }

        Ok(result.into_iter().collect())
    }

    /// Resolved re-export targets for many files in one batched query:
    /// `file_path → [resolved_path...]` over `imports` rows with
    /// `is_reexport = 1 AND resolved_path IS NOT NULL`. Files with no such
    /// rows are absent from the map. Used by dirty propagation to decide
    /// whether a promoted file's effective export surface changed.
    ///
    /// Known limitation: the jsts extractor sets `is_reexport = 1` for
    /// single-statement re-exports (`export * from './b'`,
    /// `export { x } from './b'`) and two-step forwarding of ES imports
    /// (`import { x } from './b'; export { x };`). CommonJS forwarding
    /// (`const { x } = require('./b')` then exported) and other languages'
    /// equivalents (Python star re-exports, Rust `pub use`) are stored as
    /// plain imports, so those surface changes are still missed here.
    pub(crate) fn reexport_targets_for_files(
        &self,
        file_paths: &[&str],
    ) -> CcResult<HashMap<String, Vec<String>>> {
        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        if file_paths.is_empty() {
            return Ok(result);
        }
        const BATCH_SIZE: usize = 500;
        let conn = self.read_conn()?;

        for chunk in file_paths.chunks(BATCH_SIZE) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            let sql = format!(
                "SELECT file_path, resolved_path FROM imports \
                 WHERE file_path IN ({}) AND is_reexport = 1 AND resolved_path IS NOT NULL",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (file_path, resolved_path) = row.map_err(db_err)?;
                result.entry(file_path).or_default().push(resolved_path);
            }
        }

        Ok(result)
    }

    /// Load file edge data for re-resolve scenarios.
    pub(crate) fn load_file_edges_for_reresolve(
        &self,
        file_path: &str,
    ) -> CcResult<FileEdgesForReresolve> {
        let conn = self.read_conn()?;

        // symbols
        let mut sym_stmt = conn
            .prepare(
                "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,\
                 signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,\
                 export_name,is_default_export,symbol_uid,framework_role,receiver_type,\
                 param_types,return_type,param_count,base_types,implements \
                 FROM symbols WHERE file_path = ?1 ORDER BY start_line",
            )
            .map_err(db_err)?;
        let sym_rows = sym_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let kind: String = row.get(3)?;
                let parser_tier_str: String = row.get(11)?;
                let param_count: Option<i64> = row.get(22)?;
                Ok(cc_model::SymbolRecord {
                    symbol_id: row.get(0)?,
                    file_path: row.get(1)?,
                    name: row.get(2)?,
                    kind: cc_model::SymbolKind::from_str_lenient(&kind)
                        .unwrap_or(cc_model::SymbolKind::Variable),
                    container: row.get(4)?,
                    start_line: row.get(5)?,
                    end_line: row.get(6)?,
                    start_col: row.get(7)?,
                    end_col: row.get(8)?,
                    signature: row.get(9)?,
                    doc: row.get(10)?,
                    parser_tier: crate::index_db::parse_parser_tier(&parser_tier_str),
                    parser_confidence: row.get(12)?,
                    qname: row.get(13)?,
                    parent_symbol_id: row.get(14)?,
                    scope_id: None,
                    export_name: row.get(15)?,
                    is_default_export: row.get::<_, i64>(16)? != 0,
                    symbol_uid: row.get(17)?,
                    framework_role: row.get(18)?,
                    receiver_type: row.get(19)?,
                    param_types: row.get(20)?,
                    return_type: row.get(21)?,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(23)?,
                    implements: row.get(24)?,
                })
            })
            .map_err(db_err)?;
        let symbols: Vec<cc_model::SymbolRecord> =
            sym_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // imports
        let mut imp_stmt = conn
            .prepare(
                "SELECT file_path,import_string,resolved_path,imported_name,alias,\
                 is_namespace,is_default,is_reexport \
                 FROM imports WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let imp_rows = imp_stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok(cc_model::ImportRecord {
                    file_path: row.get(0)?,
                    import_string: row.get(1)?,
                    resolved_path: row.get(2)?,
                    imported_name: row.get(3)?,
                    alias: row.get(4)?,
                    is_namespace: row.get::<_, i64>(5)? != 0,
                    is_default: row.get::<_, i64>(6)? != 0,
                    is_reexport: row.get::<_, i64>(7)? != 0,
                })
            })
            .map_err(db_err)?;
        let imports: Vec<cc_model::ImportRecord> =
            imp_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // call_edges
        let mut ce_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,\
                 target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,\
                 caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,\
                 resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,\
                 parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line \
                 FROM call_edges WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let ce_rows = ce_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let dispatch_str: String = row.get(14)?;
                let resolution_str: String = row.get(16)?;
                let tier_str: String = row.get(24)?;
                let arg_count: Option<i32> = row.get(20)?;
                let registered_line: Option<i32> = row.get(29)?;
                Ok(cc_model::CallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    line: row.get(4)?,
                    start_col: row.get(5)?,
                    end_line: row.get(6)?,
                    end_col: row.get(7)?,
                    target_symbol_id: row.get(8)?,
                    target_file_path: row.get(9)?,
                    caller_symbol_id: row.get(10)?,
                    callee_ref_id: row.get(11)?,
                    caller_symbol_uid: row.get(12)?,
                    callee_symbol_uid: row.get(13)?,
                    dispatch_kind: match dispatch_str.as_str() {
                        "dynamic" => cc_model::DispatchKind::Dynamic,
                        "virtual_dispatch" => cc_model::DispatchKind::VirtualDispatch,
                        "optional_chain" => cc_model::DispatchKind::OptionalChain,
                        "constructor" => cc_model::DispatchKind::Constructor,
                        "event_emitter" => cc_model::DispatchKind::EventEmitter,
                        "callback_relay" => cc_model::DispatchKind::CallbackRelay,
                        "reactive_binding" => cc_model::DispatchKind::ReactiveBinding,
                        "field_observer" => cc_model::DispatchKind::FieldObserver,
                        _ => cc_model::DispatchKind::Direct,
                    },
                    call_kind: row.get(15)?,
                    resolution_kind: match resolution_str.as_str() {
                        "exact" => cc_model::ResolutionKind::Exact,
                        "qualified" => cc_model::ResolutionKind::Qualified,
                        "scope_resolved" => cc_model::ResolutionKind::ScopeResolved,
                        "heuristic" => cc_model::ResolutionKind::Heuristic,
                        _ => cc_model::ResolutionKind::Unresolved,
                    },
                    resolution_confidence: row.get(17)?,
                    resolution_strategy: row.get(18)?,
                    receiver_expr: row.get(19)?,
                    arg_count: arg_count.map(|v| v as u32),
                    is_optional_chain: row.get::<_, i64>(21)? != 0,
                    is_awaited: row.get::<_, i64>(22)? != 0,
                    is_constructor: row.get::<_, i64>(23)? != 0,
                    parser_tier: crate::index_db::parse_parser_tier(&tier_str),
                    parser_confidence: row.get(25)?,
                    synthesized_by: row.get(26)?,
                    synthesis_key: row.get(27)?,
                    registered_file: row.get(28)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(db_err)?;
        let call_edges: Vec<cc_model::CallEdgeRecord> =
            ce_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // symbol_refs
        let mut sr_stmt = conn
            .prepare(
                "SELECT ref_id,file_path,symbol_name,container,ref_kind,line,column_no,\
                 target_symbol_id,target_file_path,target_symbol_uid,ref_name,\
                 resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence \
                 FROM symbol_refs WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let sr_rows = sr_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let resolution_str: String = row.get(11)?;
                let tier_str: String = row.get(16)?;
                Ok(cc_model::SymbolRefRecord {
                    ref_id: row.get(0)?,
                    file_path: row.get(1)?,
                    symbol_name: row.get(2)?,
                    container: row.get(3)?,
                    ref_kind: row.get(4)?,
                    line: row.get(5)?,
                    column: row.get(6)?,
                    target_symbol_id: row.get(7)?,
                    target_file_path: row.get(8)?,
                    target_symbol_uid: row.get(9)?,
                    ref_name: row.get(10)?,
                    scope_id: None,
                    resolution_kind: match resolution_str.as_str() {
                        "exact" => cc_model::ResolutionKind::Exact,
                        "qualified" => cc_model::ResolutionKind::Qualified,
                        "scope_resolved" => cc_model::ResolutionKind::ScopeResolved,
                        "heuristic" => cc_model::ResolutionKind::Heuristic,
                        _ => cc_model::ResolutionKind::Unresolved,
                    },
                    resolution_confidence: row.get(12)?,
                    resolution_strategy: row.get(13)?,
                    ref_end_line: row.get(14)?,
                    ref_end_col: row.get(15)?,
                    parser_tier: crate::index_db::parse_parser_tier(&tier_str),
                    parser_confidence: row.get(17)?,
                })
            })
            .map_err(db_err)?;
        let symbol_refs: Vec<cc_model::SymbolRefRecord> =
            sr_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // semantic_edges
        let mut se_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,\
                 target_symbol_uid,relation_kind,line,confidence,parser_tier \
                 FROM semantic_edges WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let se_rows = se_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let relation_str: String = row.get(6)?;
                let tier_str: String = row.get(9)?;
                Ok(cc_model::SemanticEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    source_symbol: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol: row.get(4)?,
                    target_symbol_uid: row.get(5)?,
                    relation_kind: match relation_str.as_str() {
                        "inherits" => cc_model::SemanticRelation::Inherits,
                        "implements" => cc_model::SemanticRelation::Implements,
                        "decorates" => cc_model::SemanticRelation::Decorates,
                        "throws" => cc_model::SemanticRelation::Throws,
                        "uses_type" => cc_model::SemanticRelation::UsesType,
                        "defines" => cc_model::SemanticRelation::Defines,
                        "defines_method" => cc_model::SemanticRelation::DefinesMethod,
                        "contains_file" => cc_model::SemanticRelation::ContainsFile,
                        "contains_module" => cc_model::SemanticRelation::ContainsModule,
                        "renders_component" => cc_model::SemanticRelation::RendersComponent,
                        "injects" => cc_model::SemanticRelation::Injects,
                        other => {
                            tracing::warn!(kind = %other, "unknown semantic relation_kind in DB, mapping to Unknown");
                            cc_model::SemanticRelation::Unknown
                        }
                    },
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: crate::index_db::parse_parser_tier(&tier_str),
                })
            })
            .map_err(db_err)?;
        let semantic_edges: Vec<cc_model::SemanticEdgeRecord> =
            se_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // dispatch_sites
        let mut ds_stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let ds_rows = ds_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let kind_str: String = row.get(6)?;
                Ok(cc_model::DispatchSiteRecord {
                    site_id: row.get(0)?,
                    file_path: row.get(1)?,
                    line: row.get(2)?,
                    col: row.get(3)?,
                    enclosing_symbol_uid: row.get(4)?,
                    receiver_expr: row.get(5)?,
                    site_kind: cc_model::DispatchSiteKind::parse_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(db_err)?;
        let dispatch_sites: Vec<cc_model::DispatchSiteRecord> =
            ds_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        // route_edges
        let mut re_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,\
                 handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,\
                 route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence \
                 FROM routes WHERE file_path = ?1",
            )
            .map_err(db_err)?;
        let re_rows = re_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let tier_str: String = row.get(16)?;
                Ok(cc_model::edge::RouteEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    start_col: row.get(6)?,
                    end_line: row.get(7)?,
                    end_col: row.get(8)?,
                    handler_symbol_id: row.get(9)?,
                    handler_symbol_uid: row.get(10)?,
                    handler_expr: row.get(11)?,
                    router_symbol_uid: row.get(12)?,
                    framework: row.get(13)?,
                    route_kind: row.get(14)?,
                    confidence: row.get(15)?,
                    parser_tier: crate::index_db::parse_parser_tier(&tier_str),
                    resolution_strategy: row.get(17)?,
                    resolution_confidence: row.get(18)?,
                })
            })
            .map_err(db_err)?;
        let route_edges: Vec<cc_model::edge::RouteEdgeRecord> =
            re_rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;

        Ok(FileEdgesForReresolve {
            symbols,
            imports,
            call_edges,
            symbol_refs,
            semantic_edges,
            dispatch_sites,
            route_edges,
        })
    }

    pub(crate) fn symbols_by_file_paths(&self, file_paths: &[&str]) -> CcResult<Vec<SymbolRow>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
        let placeholders: Vec<&str> = file_paths.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT symbol_id, symbol_uid, name, kind, file_path, container, \
                    start_line, end_line, qname, signature \
             FROM symbols \
             WHERE file_path IN ({}) AND symbol_uid IS NOT NULL \
             ORDER BY file_path, start_line",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), crate::rows::symbol_row)
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }
}

// Read-only facet delegates (see `IndexDb::reads()`).
impl ReadOps<'_> {
    pub fn symbols_covering(
        &self,
        file_path: &str,
        line: u32,
        limit: usize,
    ) -> CcResult<Vec<SymbolCoverRow>> {
        self.0.symbols_covering(file_path, line, limit)
    }

    pub fn caller_rows_by_uid(
        &self,
        callee_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        self.0.caller_rows_by_uid(callee_uid, limit)
    }

    pub fn callee_rows_by_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        self.0.callee_rows_by_uid(caller_uid, limit)
    }

    /// Batched variant of [`Self::caller_rows_by_uid`]: fetch the top
    pub fn caller_rows_by_uids(
        &self,
        callee_uids: &[&str],
        per_seed_limit: usize,
    ) -> CcResult<HashMap<String, Vec<CallEdgeLite>>> {
        self.0.caller_rows_by_uids(callee_uids, per_seed_limit)
    }

    /// Batched variant of [`Self::callee_rows_by_uid`]; see
    pub fn callee_rows_by_uids(
        &self,
        caller_uids: &[&str],
        per_seed_limit: usize,
    ) -> CcResult<HashMap<String, Vec<CallEdgeLite>>> {
        self.0.callee_rows_by_uids(caller_uids, per_seed_limit)
    }

    pub fn symbol_ref_rows_by_uid(
        &self,
        target_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<SymbolRefLite>> {
        self.0.symbol_ref_rows_by_uid(target_uid, limit)
    }

    /// Return a summary of environment variable accesses, ordered by frequency.
    pub fn env_var_summary(&self, limit: usize) -> CcResult<Vec<(String, i64, String)>> {
        self.0.env_var_summary(limit)
    }

    /// No-op: resolution_attempts table removed in schema consolidation.
    pub fn list_resolution_attempts(
        &self,
        _limit: usize,
        _file_path: Option<&str>,
        _kind: Option<&str>,
    ) -> CcResult<Vec<ResolutionAttemptRow>> {
        self.0.list_resolution_attempts(_limit, _file_path, _kind)
    }

    pub fn call_uid_edges(&self) -> CcResult<Vec<(String, String)>> {
        self.0.call_uid_edges()
    }

    /// Return BFS-friendly outgoing edges for a single caller UID.
    pub fn call_edges_from_uid_lite(&self, caller_uid: &str) -> CcResult<Vec<EdgeLiteBfs>> {
        self.0.call_edges_from_uid_lite(caller_uid)
    }

    pub fn call_uid_edges_lite(&self) -> CcResult<Vec<EdgeLiteBfs>> {
        self.0.call_uid_edges_lite()
    }

    pub fn symbol_names_by_uid(&self) -> CcResult<HashMap<String, String>> {
        self.0.symbol_names_by_uid()
    }

    /// Bulk lookup symbol metadata by UIDs. Batches in [`IN_BATCH_SIZE`] chunks.
    pub fn symbol_rows_by_uids(&self, uids: &[String]) -> CcResult<HashMap<String, SymbolRow>> {
        self.0.symbol_rows_by_uids(uids)
    }

    /// Get degree info for a single symbol UID.
    pub fn symbol_degree_details(&self, uid: &str) -> CcResult<SymbolDegreeInfo> {
        self.0.symbol_degree_details(uid)
    }

    /// Batched variant of [`Self::symbol_degree_details`]: every requested
    /// UID is present in the map (all-zero counts when it has no edges or
    /// refs, matching the single-UID query). Multi-symbol degree resolution
    /// should use this instead of looping the point query (`cc-search`'s
    /// enrich is the reference adapter).
    pub fn symbol_degree_details_batch(
        &self,
        uids: &[&str],
    ) -> CcResult<HashMap<String, SymbolDegreeInfo>> {
        self.0.symbol_degree_details_batch(uids)
    }

    /// Find symbols by exact name, filtering to function/class/component kinds.
    pub fn find_symbols_by_name_and_kinds(
        &self,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        self.0.find_symbols_by_name_and_kinds(name, kinds)
    }

    /// Batched variant of [`Self::find_symbols_by_name_and_kinds`]: resolves
    pub fn find_symbols_by_names_and_kinds(
        &self,
        names: &[&str],
        kinds: &[&str],
    ) -> CcResult<HashMap<String, Vec<SymbolRow>>> {
        self.0.find_symbols_by_names_and_kinds(names, kinds)
    }

    /// Find the symbol_uid of a method named `method_name` contained in the same class
    pub fn find_method_in_same_class(
        &self,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        self.0
            .find_method_in_same_class(member_symbol_uid, method_name)
    }

    /// Fetch methods for many containers (class/struct names) in a single
    pub fn find_methods_by_containers(&self, containers: &[&str]) -> CcResult<MethodsByContainer> {
        self.0.find_methods_by_containers(containers)
    }

    /// Find all classes that have methods matching any of the given name patterns.
    pub fn find_classes_with_method_names(
        &self,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        self.0.find_classes_with_method_names(method_names)
    }

    /// Get the export fingerprint for a file.
    pub fn get_export_fingerprint(&self, file_path: &str) -> CcResult<Option<String>> {
        self.0.get_export_fingerprint(file_path)
    }

    /// Batch variant of [`Self::get_export_fingerprint`]: compute the export
    pub fn get_export_fingerprints(
        &self,
        file_paths: &[String],
    ) -> CcResult<HashMap<String, String>> {
        self.0.get_export_fingerprints(file_paths)
    }

    /// Find all files that import the given resolved paths.
    pub fn find_importers_of(&self, resolved_paths: &[String]) -> CcResult<Vec<String>> {
        self.0.find_importers_of(resolved_paths)
    }

    /// Resolved re-export targets for many files in one batched query:
    pub fn reexport_targets_for_files(
        &self,
        file_paths: &[&str],
    ) -> CcResult<HashMap<String, Vec<String>>> {
        self.0.reexport_targets_for_files(file_paths)
    }

    /// Load file edge data for re-resolve scenarios.
    pub fn load_file_edges_for_reresolve(
        &self,
        file_path: &str,
    ) -> CcResult<FileEdgesForReresolve> {
        self.0.load_file_edges_for_reresolve(file_path)
    }

    pub fn symbols_by_file_paths(&self, file_paths: &[&str]) -> CcResult<Vec<SymbolRow>> {
        self.0.symbols_by_file_paths(file_paths)
    }
}

// Write facet delegates (see `IndexDb::writes()`).
impl WriteOps<'_> {
    pub fn update_communities(
        &self,
        assignments: &HashMap<String, u32>,
        labels: &HashMap<u32, String>,
    ) -> CcResult<()> {
        self.0.update_communities(assignments, labels)
    }

    /// Degraded community assignment: assign all symbols that have no community
    pub fn assign_all_symbols_to_community(&self, community_id: u32) -> CcResult<()> {
        self.0.assign_all_symbols_to_community(community_id)
    }

    pub fn replace_repo_frameworks(&self, signals: &[RepoFrameworkRecord]) -> CcResult<()> {
        self.0.replace_repo_frameworks(signals)
    }

    pub fn replace_file_frameworks(&self, by_file: &[FileFrameworkRecord]) -> CcResult<()> {
        self.0.replace_file_frameworks(by_file)
    }

    /// Delete synthetic semantic edges whose edge_id starts with a given prefix.
    pub fn delete_synthetic_semantic_edges(&self, edge_id_prefix: &str) -> CcResult<usize> {
        self.0.delete_synthetic_semantic_edges(edge_id_prefix)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::index_db::IndexDb;
    use tempfile::TempDir;

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        (db, tmp)
    }

    /// Helper: insert a file row so foreign-key constraints are satisfied.
    fn insert_file(db: &IndexDb, file_path: &str) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
             VALUES(?1,'Rust','hash1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            rusqlite::params![file_path],
        )
        .unwrap();
    }

    #[test]
    fn test_symbols_covering() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // Outer function: lines 1-50
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/main.rs","outer","Function","",1,50,0,0,"fn outer()","","tree_sitter",0.8,"outer","uid_outer"],
            ).unwrap();
            // Inner block: lines 10-30 (smaller span)
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id2","src/main.rs","inner","Function","outer",10,30,0,0,"fn inner()","","tree_sitter",0.8,"inner","uid_inner"],
            ).unwrap();
            // Unrelated symbol: lines 60-70 (does not cover line 15)
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id3","src/main.rs","unrelated","Function","",60,70,0,0,"fn unrelated()","","tree_sitter",0.8,"unrelated","uid_unrelated"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let rows = db.symbols_covering("src/main.rs", 15, 10).unwrap();
        assert_eq!(rows.len(), 2);
        // Smallest span first: inner (span 20) before outer (span 49)
        assert_eq!(rows[0].name, "inner");
        assert_eq!(rows[1].name, "outer");
    }

    #[test]
    fn test_find_symbols_by_names_and_kinds_matches_single() {
        let (db, _tmp) = setup();
        for f in ["src/a.tsx", "src/b.tsx"] {
            insert_file(&db, f);
        }

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // "Button" exists in both files; "Modal" only in b.tsx; "helper" has
            // a kind outside the queried set.
            for (id, file, name, kind, uid) in [
                ("s1", "src/a.tsx", "Button", "component", "uid_a_button"),
                ("s2", "src/b.tsx", "Button", "component", "uid_b_button"),
                ("s3", "src/b.tsx", "Modal", "function", "uid_b_modal"),
                ("s4", "src/a.tsx", "helper", "variable", "uid_a_helper"),
            ] {
                tx.execute(
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                    rusqlite::params![id, file, name, kind, "", 1, 5, 0, 0, "", "", "tree_sitter", 0.9, name, uid],
                ).unwrap();
            }
            tx.commit().unwrap();
        }

        let kinds: &[&str] = &["function", "class", "component", "hook"];
        let names: &[&str] = &["Button", "Modal", "helper", "Missing"];
        let batch = db.find_symbols_by_names_and_kinds(names, kinds).unwrap();

        // Batch result must equal the per-name query for every name.
        for name in names {
            let single = db.find_symbols_by_name_and_kinds(name, kinds).unwrap();
            let batched = batch.get(*name).cloned().unwrap_or_default();
            assert_eq!(single.len(), batched.len(), "row count mismatch for {name}");
            for (s, b) in single.iter().zip(batched.iter()) {
                assert_eq!(s.symbol_uid, b.symbol_uid, "uid mismatch for {name}");
                assert_eq!(s.file_path, b.file_path, "file mismatch for {name}");
            }
        }
        // Names with no rows are absent, not present-but-empty.
        assert!(!batch.contains_key("helper"));
        assert!(!batch.contains_key("Missing"));
    }

    #[test]
    fn test_get_export_fingerprints_batch_matches_single() {
        let (db, _tmp) = setup();
        for f in ["src/a.rs", "src/b.rs", "src/c_no_exports.rs"] {
            insert_file(&db, f);
        }

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // src/a.rs: two exported symbols (intentionally out of uid order to
            // exercise the ORDER BY contract).
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid,export_name,is_default_export)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params!["a2","src/a.rs","beta","Function","",1,5,0,0,"fn beta()","","tree_sitter",0.9,"beta","uid_a_beta","beta",0],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid,export_name,is_default_export)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params!["a1","src/a.rs","alpha","Function","",6,9,0,0,"fn alpha()","","tree_sitter",0.9,"alpha","uid_a_alpha","alpha",0],
            ).unwrap();
            // A non-exported symbol in a.rs must be ignored by both paths.
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid,export_name,is_default_export)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params!["a3","src/a.rs","private_fn","Function","",10,12,0,0,"fn private_fn()","","tree_sitter",0.9,"priv","uid_a_priv",Option::<String>::None,0],
            ).unwrap();
            // src/b.rs: one default-export symbol.
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid,export_name,is_default_export)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params!["b1","src/b.rs","Widget","Class","",1,20,0,0,"class Widget","","tree_sitter",0.9,"Widget","uid_b_widget",Option::<String>::None,1],
            ).unwrap();
            // src/c_no_exports.rs: only private symbols (no fingerprint).
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid,export_name,is_default_export)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params!["c1","src/c_no_exports.rs","helper","Function","",1,3,0,0,"fn helper()","","tree_sitter",0.9,"helper","uid_c_helper",Option::<String>::None,0],
            ).unwrap();
            tx.commit().unwrap();
        }

        let paths = vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c_no_exports.rs".to_string(),
            "src/missing.rs".to_string(),
        ];

        let batch = db.get_export_fingerprints(&paths).unwrap();

        // Batch result must equal the per-file query for every path.
        for path in &paths {
            let single = db.get_export_fingerprint(path).unwrap();
            assert_eq!(
                batch.get(path).cloned(),
                single,
                "batch fingerprint for {path} must match single-file result"
            );
        }

        // Files with exports are present; files without exports / missing are absent.
        assert!(batch.contains_key("src/a.rs"));
        assert!(batch.contains_key("src/b.rs"));
        assert!(!batch.contains_key("src/c_no_exports.rs"));
        assert!(!batch.contains_key("src/missing.rs"));
    }

    #[test]
    fn test_get_export_fingerprints_empty_input() {
        let (db, _tmp) = setup();
        let batch = db.get_export_fingerprints(&[]).unwrap();
        assert!(batch.is_empty());
    }

    /// Helper: insert an imports row with the given re-export flag.
    fn insert_import(
        db: &IndexDb,
        file_path: &str,
        resolved_path: Option<&str>,
        is_reexport: bool,
    ) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport)
             VALUES(?1,?2,?3,NULL,NULL,0,0,?4)",
            rusqlite::params![
                file_path,
                resolved_path.unwrap_or("./unresolved"),
                resolved_path,
                is_reexport as i32
            ],
        )
        .unwrap();
    }

    #[test]
    fn test_reexport_targets_for_files() {
        let (db, _tmp) = setup();
        for f in ["src/a.ts", "src/b.ts", "src/c.ts", "src/plain.ts"] {
            insert_file(&db, f);
        }

        // a.ts: two resolved re-exports plus a plain import (excluded) and an
        // unresolved re-export (excluded).
        insert_import(&db, "src/a.ts", Some("src/b.ts"), true);
        insert_import(&db, "src/a.ts", Some("src/c.ts"), true);
        insert_import(&db, "src/a.ts", Some("src/plain.ts"), false);
        insert_import(&db, "src/a.ts", None, true);
        // b.ts: one resolved re-export.
        insert_import(&db, "src/b.ts", Some("src/c.ts"), true);
        // plain.ts: only a plain import → must be absent from the map.
        insert_import(&db, "src/plain.ts", Some("src/c.ts"), false);

        let targets = db
            .reexport_targets_for_files(&["src/a.ts", "src/b.ts", "src/plain.ts", "src/missing.ts"])
            .unwrap();

        let mut a_targets = targets.get("src/a.ts").cloned().unwrap_or_default();
        a_targets.sort();
        assert_eq!(
            a_targets,
            vec!["src/b.ts".to_string(), "src/c.ts".to_string()],
            "only resolved is_reexport=1 rows count"
        );
        assert_eq!(
            targets.get("src/b.ts").cloned().unwrap_or_default(),
            vec!["src/c.ts".to_string()]
        );
        assert!(
            !targets.contains_key("src/plain.ts"),
            "files without resolved re-exports are absent"
        );
        assert!(!targets.contains_key("src/missing.ts"));
        assert!(
            !targets.contains_key("src/c.ts"),
            "files not queried must not appear"
        );
    }

    #[test]
    fn test_reexport_targets_for_files_empty_input() {
        let (db, _tmp) = setup();
        let targets = db.reexport_targets_for_files(&[]).unwrap();
        assert!(targets.is_empty());
    }

    #[test]
    fn test_caller_callee_rows() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce1","src/main.rs","caller_a","callee_b",5,"uid_a","uid_b","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce2","src/main.rs","caller_c","callee_b",10,"uid_c","uid_b","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce3","src/main.rs","caller_a","callee_d",15,"uid_a","uid_d","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();
            tx.commit().unwrap();
        }

        // caller_rows_by_uid: who calls uid_b?
        let callers = db.caller_rows_by_uid("uid_b", 10).unwrap();
        assert_eq!(callers.len(), 2);
        assert_eq!(callers[0].caller_symbol_uid.as_deref(), Some("uid_a"));
        assert_eq!(callers[1].caller_symbol_uid.as_deref(), Some("uid_c"));

        // callee_rows_by_uid: what does uid_a call?
        let callees = db.callee_rows_by_uid("uid_a", 10).unwrap();
        assert_eq!(callees.len(), 2);
        assert_eq!(callees[0].callee_symbol_uid.as_deref(), Some("uid_b"));
        assert_eq!(callees[1].callee_symbol_uid.as_deref(), Some("uid_d"));
    }

    /// Helper: insert a call_edges row with the given UIDs and line.
    fn insert_call_edge(
        db: &IndexDb,
        edge_id: &str,
        caller_uid: &str,
        callee_uid: &str,
        line: u32,
    ) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
             VALUES(?1,'src/main.rs',?2,?3,?4,?5,?6,'direct','sync','exact','tree_sitter',0.8)",
            rusqlite::params![edge_id, caller_uid, callee_uid, line, caller_uid, callee_uid],
        )
        .unwrap();
    }

    /// Equivalence lock: the batched `caller_rows_by_uids` /
    /// `callee_rows_by_uids` must return, per seed, exactly what the
    /// per-seed point queries return — same rows, same order — across
    /// limits below / equal to / above the available edge count, with
    /// no-edge seeds, duplicate seeds in the input, and equal-line ties
    /// (broken by `rowid`, i.e. insertion order, on both paths).
    #[test]
    fn test_caller_callee_rows_by_uids_match_single_uid_queries() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        // uid_a: 4 callers, 4 callees (exceeds limit 2; lines out of insert
        // order; one equal-line tie on each side resolved by rowid).
        insert_call_edge(&db, "e1", "uid_p2", "uid_a", 10);
        insert_call_edge(&db, "e2", "uid_p3", "uid_a", 20);
        insert_call_edge(&db, "e3", "uid_p1", "uid_a", 30);
        insert_call_edge(&db, "e4", "uid_a", "uid_y", 5);
        insert_call_edge(&db, "e5", "uid_a", "uid_z", 15);
        insert_call_edge(&db, "e6", "uid_a", "uid_x", 25);
        insert_call_edge(&db, "e12", "uid_p4", "uid_a", 20); // ties e2 at line 20
        insert_call_edge(&db, "e13", "uid_a", "uid_w", 15); // ties e5 at line 15
                                                            // uid_b: 3 callers, 2 callees; the caller tie at line 8 straddles
                                                            // the limit-2 cut, so the tiebreak decides which row survives.
        insert_call_edge(&db, "e7", "uid_p2", "uid_b", 3);
        insert_call_edge(&db, "e8", "uid_p1", "uid_b", 8);
        insert_call_edge(&db, "e14", "uid_p3", "uid_b", 8); // ties e8 at line 8
        insert_call_edge(&db, "e9", "uid_b", "uid_y", 2);
        insert_call_edge(&db, "e10", "uid_b", "uid_x", 7);
        // Noise edge: neither endpoint is a requested seed.
        insert_call_edge(&db, "e11", "uid_noise", "uid_noise2", 50);

        // uid_c has no edges; uid_a appears twice (duplicate seed).
        let seeds = ["uid_a", "uid_b", "uid_c", "uid_a"];

        for limit in [1usize, 2, 3, 10] {
            let batch_callers = db.caller_rows_by_uids(&seeds, limit).unwrap();
            let batch_callees = db.callee_rows_by_uids(&seeds, limit).unwrap();
            for uid in ["uid_a", "uid_b", "uid_c"] {
                let single_callers = db.caller_rows_by_uid(uid, limit).unwrap();
                let single_callees = db.callee_rows_by_uid(uid, limit).unwrap();
                assert_eq!(
                    format!("{:?}", batch_callers.get(uid).cloned().unwrap_or_default()),
                    format!("{:?}", single_callers),
                    "callers for {uid} at limit {limit} must match point query"
                );
                assert_eq!(
                    format!("{:?}", batch_callees.get(uid).cloned().unwrap_or_default()),
                    format!("{:?}", single_callees),
                    "callees for {uid} at limit {limit} must match point query"
                );
            }
            // Seeds with no edges and non-requested UIDs are absent.
            assert!(!batch_callers.contains_key("uid_c"));
            assert!(!batch_callers.contains_key("uid_noise2"));
            assert!(!batch_callees.contains_key("uid_c"));
            assert!(!batch_callees.contains_key("uid_noise"));
        }

        // Tie contract: equal-line rows come back in rowid (insertion)
        // order, on the point query and therefore on the batch as well.
        let tied_callers: Vec<_> = db
            .caller_rows_by_uid("uid_a", 10)
            .unwrap()
            .into_iter()
            .map(|edge| edge.caller_symbol_uid.unwrap())
            .collect();
        assert_eq!(tied_callers, ["uid_p2", "uid_p3", "uid_p4", "uid_p1"]);
        let tied_callees: Vec<_> = db
            .callee_rows_by_uid("uid_a", 10)
            .unwrap()
            .into_iter()
            .map(|edge| edge.callee_symbol_uid.unwrap())
            .collect();
        assert_eq!(tied_callees, ["uid_y", "uid_z", "uid_w", "uid_x"]);
        // Limit cut inside the line-8 tie group keeps the lower rowid (e8).
        let cut_callers: Vec<_> = db
            .caller_rows_by_uid("uid_b", 2)
            .unwrap()
            .into_iter()
            .map(|edge| edge.caller_symbol_uid.unwrap())
            .collect();
        assert_eq!(cut_callers, ["uid_p2", "uid_p1"]);

        // Empty seed list -> empty map, no SQL issued.
        assert!(db.caller_rows_by_uids(&[], 5).unwrap().is_empty());
        assert!(db.callee_rows_by_uids(&[], 5).unwrap().is_empty());
    }

    #[test]
    fn test_update_communities() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id1","src/main.rs","alpha","Function","",1,10,0,0,"fn alpha()","","tree_sitter",0.8,"alpha","uid_alpha"],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id2","src/main.rs","beta","Function","",20,30,0,0,"fn beta()","","tree_sitter",0.8,"beta","uid_beta"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let mut assignments = HashMap::new();
        assignments.insert("uid_alpha".to_string(), 1u32);
        assignments.insert("uid_beta".to_string(), 1u32);
        let mut labels = HashMap::new();
        labels.insert(1u32, "core-module".to_string());

        db.update_communities(&assignments, &labels).unwrap();

        // Verify symbols have correct community_id
        let conn = db.read_conn().unwrap();
        let cid: u32 = conn
            .query_row(
                "SELECT community_id FROM symbols WHERE symbol_uid = 'uid_alpha'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cid, 1);

        let cid2: u32 = conn
            .query_row(
                "SELECT community_id FROM symbols WHERE symbol_uid = 'uid_beta'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cid2, 1);

        // Verify communities table
        let communities = db.list_communities().unwrap();
        assert_eq!(communities.len(), 1);
        assert_eq!(communities[0].community_id, 1);
        assert_eq!(communities[0].label, "core-module");
        assert_eq!(communities[0].member_count, 2);
    }

    #[test]
    fn test_symbol_degree_details() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // The target symbol
            tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                rusqlite::params!["id_target","src/main.rs","target_fn","Function","",1,10,0,0,"fn target_fn()","","tree_sitter",0.8,"target_fn","uid_target"],
            ).unwrap();

            // 2 incoming call edges (others call uid_target)
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce_in1","src/main.rs","caller1","target_fn",5,"uid_c1","uid_target","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce_in2","src/main.rs","caller2","target_fn",10,"uid_c2","uid_target","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();

            // 1 outgoing call edge (uid_target calls something)
            tx.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params!["ce_out1","src/main.rs","target_fn","helper",15,"uid_target","uid_helper","direct","sync","exact","tree_sitter",0.8],
            ).unwrap();

            // 3 symbol_refs pointing at uid_target
            tx.execute(
                "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,target_symbol_uid,resolution_kind,resolution_confidence,resolution_strategy,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,'usage',1,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["sr1","src/main.rs","target_fn","","uid_target","exact",0.9,"import_map","tree_sitter",0.8],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,target_symbol_uid,resolution_kind,resolution_confidence,resolution_strategy,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,'usage',1,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["sr2","src/main.rs","target_fn","other","uid_target","exact",0.9,"import_map","tree_sitter",0.8],
            ).unwrap();
            tx.execute(
                "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,target_symbol_uid,resolution_kind,resolution_confidence,resolution_strategy,parser_tier,parser_confidence)
                 VALUES(?1,?2,?3,?4,'usage',1,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["sr3","src/main.rs","target_fn","another","uid_target","exact",0.9,"import_map","tree_sitter",0.8],
            ).unwrap();

            tx.commit().unwrap();
        }

        let info = db.symbol_degree_details("uid_target").unwrap();
        assert_eq!(info.in_degree, 2); // 2 incoming call edges
        assert_eq!(info.out_degree, 1); // 1 outgoing call edge
        assert_eq!(info.caller_count, 2); // 2 distinct callers
        assert_eq!(info.callee_count, 1); // 1 distinct callee
        assert_eq!(info.ref_count, 3); // 3 symbol refs
    }

    /// Equivalence lock: `symbol_degree_details_batch` must return, per
    /// requested UID, exactly what the single-UID query returns. The single
    /// query yields all-zero counts for UIDs with no edges/refs — including
    /// UIDs unknown to the index — so the batch keeps every requested UID
    /// present in the map with zeroed counts instead of omitting it.
    /// Also covers: duplicate seeds, a NULL caller UID (counts toward
    /// `in_degree` but not the DISTINCT caller count on either path), and
    /// noise edges whose endpoints were not requested.
    #[test]
    fn test_symbol_degree_details_batch_matches_single() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");

        // uid_a: two in-edges from the same caller (in_degree 2 vs
        // caller_count 1), one out-edge, no refs.
        insert_call_edge(&db, "e1", "uid_p1", "uid_a", 10);
        insert_call_edge(&db, "e2", "uid_p1", "uid_a", 20);
        insert_call_edge(&db, "e3", "uid_a", "uid_b", 5);
        // uid_b: one in-edge (e3), no out-edges, two refs.
        // Noise edge: neither endpoint is a requested seed.
        insert_call_edge(&db, "e5", "uid_noise", "uid_noise2", 50);
        {
            let conn = db.write_conn.lock().unwrap();
            // NULL caller UID into uid_a: in_degree counts it, DISTINCT does not.
            conn.execute(
                "INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,parser_tier,parser_confidence)
                 VALUES('e4','src/main.rs','anon','callee_a',30,NULL,'uid_a','direct','sync','exact','tree_sitter',0.8)",
                [],
            )
            .unwrap();
            for ref_id in ["sr1", "sr2"] {
                conn.execute(
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,target_symbol_uid,resolution_kind,resolution_confidence,resolution_strategy,parser_tier,parser_confidence)
                     VALUES(?1,'src/main.rs','b_fn','','usage',1,'uid_b','exact',0.9,'import_map','tree_sitter',0.8)",
                    rusqlite::params![ref_id],
                )
                .unwrap();
            }
            // uid_zero: present in symbols but with no edges/refs anywhere.
            conn.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,symbol_uid)
                 VALUES('id_zero','src/main.rs','zero_fn','Function','',1,3,0,0,'fn zero_fn()','','tree_sitter',0.8,'zero_fn','uid_zero')",
                [],
            )
            .unwrap();
        }

        // uid_unknown appears nowhere; uid_a appears twice (duplicate seed).
        let seeds = ["uid_a", "uid_b", "uid_zero", "uid_unknown", "uid_a"];
        let batch = db.symbol_degree_details_batch(&seeds).unwrap();

        assert_eq!(batch.len(), 4, "duplicate seeds collapse to one entry");
        for uid in ["uid_a", "uid_b", "uid_zero", "uid_unknown"] {
            let single = db.symbol_degree_details(uid).unwrap();
            let batched = batch.get(uid).expect("every requested UID is present");
            assert_eq!(
                format!("{:?}", batched),
                format!("{:?}", single),
                "degree info for {uid} must match point query"
            );
        }

        // Hard-coded spot checks so both paths cannot drift in lockstep.
        let info_a = batch.get("uid_a").unwrap();
        assert_eq!(info_a.in_degree, 3); // e1, e2, e4 (NULL caller counted)
        assert_eq!(info_a.caller_count, 1); // DISTINCT skips NULL, dedupes uid_p1
        assert_eq!(info_a.out_degree, 1);
        assert_eq!(info_a.callee_count, 1);
        assert_eq!(info_a.ref_count, 0);
        let info_b = batch.get("uid_b").unwrap();
        assert_eq!(info_b.in_degree, 1);
        assert_eq!(info_b.out_degree, 0);
        assert_eq!(info_b.ref_count, 2);
        assert_eq!(batch.get("uid_unknown").unwrap().in_degree, 0);
        assert!(!batch.contains_key("uid_noise"));
        assert!(!batch.contains_key("uid_noise2"));

        // Empty seed list -> empty map, no SQL issued.
        assert!(db.symbol_degree_details_batch(&[]).unwrap().is_empty());
    }

    /// Regression: a step-phase execution error must surface as `Err`, never
    /// be swallowed into `Ok(empty)` by the row iterator (invariant 8: read
    /// failures are visible). Reproduced by dropping `call_edges` through a
    /// side connection after the pooled read connection has loaded the
    /// schema: the stale in-memory schema lets `prepare` succeed while the
    /// re-prepare at step time fails with "no such table".
    #[test]
    fn step_error_after_side_connection_drop_is_err_not_empty_ok() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/main.rs");
        insert_call_edge(&db, "e1", "uid_a", "uid_b", 5);

        // Warm-up through the read pool so the pooled connection prepares
        // statements involving `call_edges` against the current schema
        // (stale in-memory schema precondition).
        assert_eq!(db.call_edges_from_uid_lite("uid_a").unwrap().len(), 1);
        assert_eq!(db.caller_rows_by_uid("uid_b", 10).unwrap().len(), 1);
        assert_eq!(db.caller_rows_by_uids(&["uid_b"], 10).unwrap().len(), 1);

        // Drop the table behind the pool's back.
        let side = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        side.execute("DROP TABLE call_edges", []).unwrap();

        // Collect form (point query, cached statement) ...
        assert!(db.call_edges_from_uid_lite("uid_a").is_err());
        // ... collect form via the shared row mapper ...
        assert!(db.caller_rows_by_uid("uid_b", 10).is_err());
        // ... and for-loop form (batched dynamic-SQL query).
        assert!(db.caller_rows_by_uids(&["uid_b"], 10).is_err());
    }
}
