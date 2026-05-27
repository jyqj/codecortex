//! IndexDb methods: graph traversal, community detection, framework post-processing.

use std::collections::HashMap;

use rusqlite::Connection;

use cc_model::{CcError, CcResult};

use crate::index_db::{
    is_actionable_reference_name, CallEdgeLite, DataFlowEdgeLite, EdgeLiteBfs,
    FileEdgesForReresolve, FileFrameworkRecord, IndexDb, RepoFrameworkRecord, ResolutionAttemptRow,
    SymbolCoverRow, SymbolDegreeInfo, SymbolRefLite, SymbolRow,
};

impl IndexDb {
    pub fn symbols_covering(
        &self,
        file_path: &str,
        line: u32,
        limit: usize,
    ) -> CcResult<Vec<SymbolCoverRow>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, symbol_uid, name, file_path, start_line, end_line
                 FROM symbols
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2
                 ORDER BY (end_line - start_line) ASC
                 LIMIT ?3",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn caller_rows_by_uid(
        &self,
        callee_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE callee_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![callee_uid, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn callee_rows_by_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<CallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, caller_symbol, callee_symbol, caller_symbol_uid, callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind, synthesized_by, synthesis_key, registered_file, registered_line
                 FROM call_edges
                 WHERE caller_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid, limit as i64], |row| {
                let registered_line: Option<i32> = row.get(12)?;
                Ok(CallEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    caller_symbol: row.get(2)?,
                    callee_symbol: row.get(3)?,
                    caller_symbol_uid: row.get(4)?,
                    callee_symbol_uid: row.get(5)?,
                    resolution_kind: row.get(6)?,
                    confidence: row.get(7)?,
                    dispatch_kind: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                    synthesized_by: row.get(9)?,
                    synthesis_key: row.get(10)?,
                    registered_file: row.get(11)?,
                    registered_line: registered_line.map(|v| v as u32),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn symbol_ref_rows_by_uid(
        &self,
        target_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<SymbolRefLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, symbol_name, target_symbol_uid, resolution_kind, resolution_confidence
                 FROM symbol_refs
                 WHERE target_symbol_uid = ?1
                 ORDER BY line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn data_flow_edges_by_uid(
        &self,
        uid: &str,
        limit: usize,
    ) -> CcResult<Vec<DataFlowEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT file_path, line, flow_kind, source_symbol_uid, target_symbol_uid, confidence, env_key
                 FROM data_flow_edges
                 WHERE source_symbol_uid = ?1 OR target_symbol_uid = ?1
                 ORDER BY confidence DESC, line ASC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![uid, limit as i64], |row| {
                Ok(DataFlowEdgeLite {
                    file_path: row.get(0)?,
                    line: row.get(1)?,
                    flow_kind: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol_uid: row.get(4)?,
                    confidence: row.get(5)?,
                    env_key: row.get(6)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Return a summary of environment variable accesses, ordered by frequency.
    ///
    /// Each tuple: `(env_key, count, comma_separated_file_paths)`.
    pub fn env_var_summary(&self, limit: usize) -> CcResult<Vec<(String, i64, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn.prepare(
            "SELECT env_key, COUNT(*) as cnt, GROUP_CONCAT(DISTINCT file_path) \
             FROM data_flow_edges \
             WHERE flow_kind = 'env_access' AND env_key IS NOT NULL \
             GROUP BY env_key \
             ORDER BY cnt DESC \
             LIMIT ?1"
        ).map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        }).map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    /// Rebuild the unresolved-reference feedback table from current refs/calls.
    pub fn rebuild_resolution_attempts(&self) -> CcResult<usize> {
        #[derive(Debug)]
        struct Seed {
            source_table: String,
            source_id: String,
            file_path: String,
            reference_name: String,
            reference_kind: String,
            line: u32,
            column_no: u32,
            container: Option<String>,
            resolution_strategy: String,
            parser_tier: String,
            parser_confidence: f64,
            language: Option<String>,
        }

        let read = self.read_conn()?;
        let mut seeds = Vec::new();

        {
            let mut stmt = read
                .prepare(
                    "SELECT sr.ref_id, sr.file_path, sr.symbol_name, sr.ref_kind, sr.line, sr.column_no, sr.container, sr.resolution_strategy, sr.parser_tier, sr.parser_confidence, f.language
                     FROM symbol_refs sr
                     LEFT JOIN files f ON f.file_path = sr.file_path
                     WHERE sr.resolution_kind = 'unresolved' OR sr.target_symbol_uid IS NULL
                     LIMIT 20000",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Seed {
                        source_table: "symbol_refs".to_string(),
                        source_id: row.get(0)?,
                        file_path: row.get(1)?,
                        reference_name: row.get(2)?,
                        reference_kind: row.get(3)?,
                        line: row.get(4)?,
                        column_no: row.get(5)?,
                        container: row.get(6)?,
                        resolution_strategy: row.get(7)?,
                        parser_tier: row.get(8)?,
                        parser_confidence: row.get(9)?,
                        language: row.get(10)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            seeds.extend(rows.filter_map(|r| r.ok()));
        }

        {
            let mut stmt = read
                .prepare(
                    "SELECT ce.edge_id, ce.file_path, ce.callee_symbol, 'call', ce.line, ce.start_col, ce.caller_symbol, ce.resolution_strategy, ce.parser_tier, ce.parser_confidence, f.language
                     FROM call_edges ce
                     LEFT JOIN files f ON f.file_path = ce.file_path
                     WHERE ce.resolution_kind = 'unresolved' OR ce.callee_symbol_uid IS NULL
                     LIMIT 20000",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Seed {
                        source_table: "call_edges".to_string(),
                        source_id: row.get(0)?,
                        file_path: row.get(1)?,
                        reference_name: row.get(2)?,
                        reference_kind: row.get(3)?,
                        line: row.get(4)?,
                        column_no: row.get(5)?,
                        container: row.get(6)?,
                        resolution_strategy: row.get(7)?,
                        parser_tier: row.get(8)?,
                        parser_confidence: row.get(9)?,
                        language: row.get(10)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            seeds.extend(rows.filter_map(|r| r.ok()));
        }

        let mut prepared = Vec::with_capacity(seeds.len());
        for seed in seeds {
            if !is_actionable_reference_name(&seed.reference_name) {
                continue;
            }
            let candidates = Self::candidate_json_for_reference(&read, &seed.reference_name)?;
            let candidate_count = candidates.as_array().map(|a| a.len()).unwrap_or(0);
            if candidate_count == 0 {
                continue;
            }
            let failure_reason = if candidate_count == 1 {
                "single_candidate_not_selected"
            } else {
                "ambiguous_candidates"
            };
            prepared.push((seed, candidates.to_string(), failure_reason.to_string()));
        }
        drop(read);

        let mut write = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = write
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM resolution_attempts", [])
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut count = 0usize;
        for (seed, candidates_json, failure_reason) in prepared {
            let attempt_id = format!("{}:{}", seed.source_table, seed.source_id);
            tx.execute(
                "INSERT OR REPLACE INTO resolution_attempts(attempt_id,source_table,source_id,file_path,reference_name,reference_kind,line,column_no,container,candidates_json,failure_reason,resolution_strategy,parser_tier,parser_confidence,language,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                rusqlite::params![
                    attempt_id,
                    seed.source_table,
                    seed.source_id,
                    seed.file_path,
                    seed.reference_name,
                    seed.reference_kind,
                    seed.line,
                    seed.column_no,
                    seed.container,
                    candidates_json,
                    failure_reason,
                    seed.resolution_strategy,
                    seed.parser_tier,
                    seed.parser_confidence,
                    seed.language,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            count += 1;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    fn candidate_json_for_reference(
        conn: &Connection,
        reference_name: &str,
    ) -> CcResult<serde_json::Value> {
        let suffix_like = format!("%.{}", reference_name);
        let mut stmt = conn
            .prepare(
                "SELECT name, qname, file_path, kind, symbol_uid, start_line, end_line,
                        CASE
                          WHEN qname = ?1 THEN 0
                          WHEN name = ?1 THEN 1
                          WHEN qname LIKE ?2 THEN 2
                          ELSE 4
                        END AS rank
                 FROM symbols
                 WHERE qname = ?1 OR name = ?1 OR qname LIKE ?2
                 ORDER BY rank ASC, file_path ASC, start_line ASC
                 LIMIT 8",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![reference_name, suffix_like], |row| {
                Ok(serde_json::json!({
                    "name": row.get::<_, String>(0)?,
                    "qname": row.get::<_, Option<String>>(1)?,
                    "file_path": row.get::<_, String>(2)?,
                    "kind": row.get::<_, String>(3)?,
                    "symbol_uid": row.get::<_, Option<String>>(4)?,
                    "start_line": row.get::<_, u32>(5)?,
                    "end_line": row.get::<_, u32>(6)?,
                    "rank": row.get::<_, i64>(7)?,
                }))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut candidates = Vec::new();
        for row in rows {
            if let Ok(v) = row {
                candidates.push(v);
            }
        }
        Ok(serde_json::Value::Array(candidates))
    }

    pub fn list_resolution_attempts(
        &self,
        limit: usize,
        file_path: Option<&str>,
        kind: Option<&str>,
    ) -> CcResult<Vec<ResolutionAttemptRow>> {
        let conn = self.read_conn()?;
        let mut sql = "SELECT attempt_id, source_table, source_id, file_path, reference_name, reference_kind, line, column_no, container, candidates_json, failure_reason, resolution_strategy, parser_tier, parser_confidence, language FROM resolution_attempts".to_string();
        let mut where_parts = Vec::new();
        let mut params = Vec::new();
        if let Some(fp) = file_path {
            where_parts.push("file_path = ?".to_string());
            params.push(fp.to_string());
        }
        if let Some(k) = kind {
            where_parts.push("reference_kind = ?".to_string());
            params.push(k.to_string());
        }
        if !where_parts.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_parts.join(" AND "));
        }
        sql.push_str(" ORDER BY file_path ASC, line ASC LIMIT ?");
        params.push(limit.to_string());

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let candidates_json: String = row.get(9)?;
                Ok(ResolutionAttemptRow {
                    attempt_id: row.get(0)?,
                    source_table: row.get(1)?,
                    source_id: row.get(2)?,
                    file_path: row.get(3)?,
                    reference_name: row.get(4)?,
                    reference_kind: row.get(5)?,
                    line: row.get(6)?,
                    column_no: row.get(7)?,
                    container: row.get(8)?,
                    candidates: serde_json::from_str(&candidates_json)
                        .unwrap_or_else(|_| serde_json::json!([])),
                    failure_reason: row.get(10)?,
                    resolution_strategy: row.get(11)?,
                    parser_tier: row.get(12)?,
                    parser_confidence: row.get(13)?,
                    language: row.get(14)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    // ── Graph / framework post-processing ───────────────────────

    pub fn call_uid_edges(&self) -> CcResult<Vec<(String, String)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT caller_symbol_uid, callee_symbol_uid
                 FROM call_edges
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn call_uid_edges_lite(&self) -> CcResult<Vec<EdgeLiteBfs>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT caller_symbol_uid, callee_symbol_uid, dispatch_kind, \
                        synthesized_by, synthesis_key, resolution_confidence, \
                        file_path, line, registered_file, registered_line, \
                        resolution_kind, parser_tier, resolution_strategy, parser_confidence \
                 FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn symbol_names_by_uid(&self) -> CcResult<HashMap<String, String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT symbol_uid, name FROM symbols WHERE symbol_uid IS NOT NULL")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut map = HashMap::new();
        for row in rows {
            let (uid, name) = row.map_err(|e| CcError::Database(e.to_string()))?;
            map.insert(uid, name);
        }
        Ok(map)
    }

    /// Bulk lookup symbol metadata by UIDs. Batches in chunks of 500.
    pub fn symbol_rows_by_uids(&self, uids: &[String]) -> CcResult<HashMap<String, SymbolRow>> {
        let conn = self.read_conn()?;
        let mut result = HashMap::new();
        for chunk in uids.chunks(500) {
            let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
            let sql = format!(
                "SELECT symbol_id, symbol_uid, name, kind, file_path, container, \
                        start_line, end_line, qname, signature \
                 FROM symbols WHERE symbol_uid IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let params: Vec<&dyn rusqlite::types::ToSql> =
                chunk.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok(SymbolRow {
                        symbol_id: row.get(0)?,
                        symbol_uid: row.get(1)?,
                        name: row.get(2)?,
                        kind: row.get(3)?,
                        file_path: row.get(4)?,
                        container: row.get(5)?,
                        start_line: row.get(6)?,
                        end_line: row.get(7)?,
                        qname: row.get(8)?,
                        signature: row.get(9)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            for r in rows.flatten() {
                if let Some(uid) = r.symbol_uid.clone() {
                    result.insert(uid, r);
                }
            }
        }
        Ok(result)
    }

    /// Get degree info for a single symbol UID.
    pub fn symbol_degree_details(&self, uid: &str) -> CcResult<SymbolDegreeInfo> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT \
                    (SELECT COUNT(*) FROM call_edges WHERE callee_symbol_uid = ?1) AS in_degree, \
                    (SELECT COUNT(*) FROM call_edges WHERE caller_symbol_uid = ?1) AS out_degree, \
                    (SELECT COUNT(DISTINCT caller_symbol_uid) FROM call_edges WHERE callee_symbol_uid = ?1) AS caller_count, \
                    (SELECT COUNT(DISTINCT callee_symbol_uid) FROM call_edges WHERE caller_symbol_uid = ?1) AS callee_count, \
                    (SELECT COUNT(*) FROM symbol_refs WHERE target_symbol_uid = ?1) AS ref_count",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(info)
    }

    pub fn update_communities(
        &self,
        assignments: &HashMap<String, u32>,
        labels: &HashMap<u32, String>,
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;

        tx.execute("UPDATE symbols SET community_id = NULL", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM communities", [])
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut member_counts: HashMap<u32, usize> = HashMap::new();
        for (uid, community_id) in assignments {
            tx.execute(
                "UPDATE symbols SET community_id = ?1 WHERE symbol_uid = ?2",
                rusqlite::params![community_id, uid],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            *member_counts.entry(*community_id).or_insert(0) += 1;
        }

        for (community_id, label) in labels {
            let member_count = member_counts.get(community_id).copied().unwrap_or(0);
            tx.execute(
                "INSERT INTO communities(community_id,label,member_count,representative_file,top_symbols_json)
                 VALUES(?1,?2,?3,NULL,'[]')",
                rusqlite::params![community_id, label, member_count as i64],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }

        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn replace_repo_frameworks(&self, signals: &[RepoFrameworkRecord]) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM repo_frameworks", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        for (framework_key, confidence, evidences) in signals {
            let signals_json = serde_json::to_string(evidences).unwrap_or_else(|_| "[]".into());
            tx.execute(
                "INSERT INTO repo_frameworks(framework_key,confidence,signals_json,updated_at)
                 VALUES(?1,?2,?3,?4)",
                rusqlite::params![framework_key, confidence, signals_json, now],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn replace_file_frameworks(&self, by_file: &[FileFrameworkRecord]) -> CcResult<()> {
        if by_file.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for (file_path, signals) in by_file {
            tx.execute(
                "DELETE FROM file_frameworks WHERE file_path = ?1",
                rusqlite::params![file_path],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
            for (framework_key, confidence, evidence) in signals {
                let signals_json =
                    serde_json::to_string(&vec![evidence.clone()]).unwrap_or_else(|_| "[]".into());
                tx.execute(
                    "INSERT INTO file_frameworks(file_path,framework_key,confidence,signals_json)
                     VALUES(?1,?2,?3,?4)",
                    rusqlite::params![file_path, framework_key, confidence, signals_json],
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            }
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Find symbols by exact name, filtering to function/class/component kinds.
    pub fn find_symbols_by_name_and_kinds(
        &self,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        let conn = self.read_conn()?;
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
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(name.to_string()));
        for kind in kinds {
            params.push(Box::new(kind.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(SymbolRow {
                    symbol_id: row.get(0)?,
                    symbol_uid: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    file_path: row.get(4)?,
                    container: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    qname: row.get(8)?,
                    signature: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Delete synthetic semantic edges whose edge_id starts with a given prefix.
    pub fn delete_synthetic_semantic_edges(&self, edge_id_prefix: &str) -> CcResult<usize> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let pattern = format!("{}%", edge_id_prefix);
        let count = conn
            .execute(
                "DELETE FROM semantic_edges WHERE edge_id LIKE ?1",
                rusqlite::params![pattern],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    /// Find the symbol_uid of a method named `method_name` contained in the same class
    /// as the given symbol_uid.
    pub fn find_method_in_same_class(
        &self,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        let container: Option<String> = conn
            .query_row(
                "SELECT container FROM symbols WHERE symbol_uid = ?1",
                rusqlite::params![member_symbol_uid],
                |row| row.get(0),
            )
            .ok()
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
            .ok();
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
            .ok()
            .flatten();
        Ok(result)
    }

    /// Find all methods belonging to a given container (class/struct name).
    pub fn find_methods_by_container(
        &self,
        container: &str,
    ) -> CcResult<Vec<(String, String, String, u32)>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_uid, name, file_path, start_line \
                 FROM symbols WHERE container = ?1 AND kind = 'method' AND symbol_uid IS NOT NULL \
                 ORDER BY file_path, start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![container], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Find all classes that have methods matching any of the given name patterns.
    pub fn find_classes_with_method_names(
        &self,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        if method_names.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.read_conn()?;
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
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Get the export fingerprint for a file.
    pub fn get_export_fingerprint(&self, file_path: &str) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_uid, name, COALESCE(signature, '') as sig, COALESCE(export_name, '') as exp
                 FROM symbols
                 WHERE file_path = ?1
                   AND (export_name IS NOT NULL OR is_default_export = 1)
                 ORDER BY symbol_uid",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut parts: Vec<String> = Vec::new();
        for row in rows {
            let (uid, name, sig, exp) = row.map_err(|e| CcError::Database(e.to_string()))?;
            parts.push(format!("{}|{}|{}|{}", uid, name, sig, exp));
        }

        if parts.is_empty() {
            return Ok(None);
        }

        let combined = parts.join("\n");
        let hash = blake3::hash(combined.as_bytes());
        Ok(Some(hash.to_hex().to_string()))
    }

    /// Find all files that import the given resolved paths.
    pub fn find_importers_of(&self, resolved_paths: &[String]) -> CcResult<Vec<String>> {
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
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let params: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|p| p as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?;
            for row in rows {
                result.insert(row.map_err(|e| CcError::Database(e.to_string()))?);
            }
        }

        Ok(result.into_iter().collect())
    }

    /// Load file edge data for re-resolve scenarios.
    pub fn load_file_edges_for_reresolve(
        &self,
        file_path: &str,
    ) -> CcResult<FileEdgesForReresolve> {
        let conn = self.read_conn()?;

        // symbols
        let mut sym_stmt = conn
            .prepare(
                "SELECT symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,\
                 signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,scope_id,\
                 export_name,is_default_export,symbol_uid,framework_role,receiver_type,\
                 param_types,return_type,param_count,base_types,implements \
                 FROM symbols WHERE file_path = ?1 ORDER BY start_line",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sym_rows = sym_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let kind: String = row.get(3)?;
                let parser_tier_str: String = row.get(11)?;
                let param_count: Option<i64> = row.get(23)?;
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
                    scope_id: row.get(15)?,
                    export_name: row.get(16)?,
                    is_default_export: row.get::<_, i64>(17)? != 0,
                    symbol_uid: row.get(18)?,
                    framework_role: row.get(19)?,
                    receiver_type: row.get(20)?,
                    param_types: row.get(21)?,
                    return_type: row.get(22)?,
                    param_count: param_count.map(|v| v as u32),
                    base_types: row.get(24)?,
                    implements: row.get(25)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let symbols: Vec<cc_model::SymbolRecord> = sym_rows.filter_map(|r| r.ok()).collect();

        // imports
        let mut imp_stmt = conn
            .prepare(
                "SELECT file_path,import_string,resolved_path,imported_name,alias,\
                 is_namespace,is_default,is_reexport \
                 FROM imports WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let imports: Vec<cc_model::ImportRecord> = imp_rows.filter_map(|r| r.ok()).collect();

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
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let call_edges: Vec<cc_model::CallEdgeRecord> = ce_rows.filter_map(|r| r.ok()).collect();

        // symbol_refs
        let mut sr_stmt = conn
            .prepare(
                "SELECT ref_id,file_path,symbol_name,container,ref_kind,line,column_no,\
                 target_symbol_id,target_file_path,target_symbol_uid,ref_name,scope_id,\
                 resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence \
                 FROM symbol_refs WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sr_rows = sr_stmt
            .query_map(rusqlite::params![file_path], |row| {
                let resolution_str: String = row.get(12)?;
                let tier_str: String = row.get(17)?;
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
                    scope_id: row.get(11)?,
                    resolution_kind: match resolution_str.as_str() {
                        "exact" => cc_model::ResolutionKind::Exact,
                        "qualified" => cc_model::ResolutionKind::Qualified,
                        "scope_resolved" => cc_model::ResolutionKind::ScopeResolved,
                        "heuristic" => cc_model::ResolutionKind::Heuristic,
                        _ => cc_model::ResolutionKind::Unresolved,
                    },
                    resolution_confidence: row.get(13)?,
                    resolution_strategy: row.get(14)?,
                    ref_end_line: row.get(15)?,
                    ref_end_col: row.get(16)?,
                    parser_tier: crate::index_db::parse_parser_tier(&tier_str),
                    parser_confidence: row.get(18)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let symbol_refs: Vec<cc_model::SymbolRefRecord> = sr_rows.filter_map(|r| r.ok()).collect();

        // semantic_edges
        let mut se_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,\
                 target_symbol_uid,relation_kind,line,confidence,parser_tier \
                 FROM semantic_edges WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let semantic_edges: Vec<cc_model::SemanticEdgeRecord> =
            se_rows.filter_map(|r| r.ok()).collect();

        // dispatch_sites
        let mut ds_stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
                    key: row.get(7)?,
                    handler_expr: row.get(8)?,
                    handler_symbol_uid: row.get(9)?,
                    confidence: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let dispatch_sites: Vec<cc_model::DispatchSiteRecord> =
            ds_rows.filter_map(|r| r.ok()).collect();

        // route_edges
        let mut re_stmt = conn
            .prepare(
                "SELECT edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,\
                 handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,\
                 route_kind,confidence,parser_tier \
                 FROM route_edges WHERE file_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let route_edges: Vec<cc_model::edge::RouteEdgeRecord> =
            re_rows.filter_map(|r| r.ok()).collect();

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
}
