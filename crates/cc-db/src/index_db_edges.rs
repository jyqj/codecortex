//! IndexDb methods: edge batch operations, dispatch sites, infra, synthetic edges, runtime evidence.

use std::path::Path;

use tracing::warn;

use cc_model::{CcError, CcResult, ParserTier};

use crate::index_db::{CoChangeLite, IndexDb};

/// Extract the "base clean" stem from a test file stem for matching against code files.
///
/// Mirrors the original logic: strip `test_` prefix, then `_test` suffix (with the same
/// fallback-to-original-stem behaviour), then strip `.test` / `.spec` suffix.
fn test_stem_to_base_clean(test_stem: &str) -> String {
    let base_clean = test_stem
        .strip_prefix("test_")
        .unwrap_or(test_stem)
        .strip_suffix("_test")
        .unwrap_or(test_stem);
    let base_clean = base_clean
        .strip_suffix(".test")
        .or_else(|| base_clean.strip_suffix(".spec"))
        .unwrap_or(base_clean);
    base_clean.to_string()
}

impl IndexDb {
    pub fn rebuild_test_edges_for_files(&self, changed: &[String]) -> CcResult<()> {
        if changed.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;

        for fp in changed {
            tx.execute(
                "DELETE FROM test_edges WHERE test_file_path = ?1 OR code_file_path = ?1",
                rusqlite::params![fp],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }

        // For each changed file, query only candidate matches via SQL LIKE
        // instead of loading ALL test/code files and cross-joining.
        let changed_set: std::collections::HashSet<&str> =
            changed.iter().map(|s| s.as_str()).collect();

        for fp in changed {
            // Determine if this changed file is a test file or a code file.
            let is_test: bool = {
                let mut stmt = tx
                    .prepare_cached("SELECT is_test_file FROM files WHERE file_path = ?1")
                    .map_err(|e| CcError::Database(e.to_string()))?;
                stmt.query_row(rusqlite::params![fp], |row| row.get::<_, bool>(0))
                    .unwrap_or(false)
            };

            let stem = Path::new(fp)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if is_test {
                // Changed file is a test file; find matching code files.
                //
                // A pair (test_file, code_file) matches when:
                //   a) code_stem == base_clean          -> same-basename (0.9)
                //   b) code_file.contains(base_clean)   -> path-overlap  (0.7)
                //   c) test_file.contains(code_stem)    -> path-overlap  (0.7)
                //
                // Conditions (a)+(b) are covered by: LIKE '%base_clean%' on code files.
                // Condition (c) requires finding code files whose stem is a substring
                // of the test file path. We add LIKE patterns from the test file stem's
                // sub-components to cover this.
                let base_clean = test_stem_to_base_clean(stem);

                let mut patterns: Vec<String> = Vec::new();
                // Primary pattern: catches conditions (a)+(b)
                if !base_clean.is_empty() {
                    patterns.push(format!("%{}%", base_clean));
                }
                // Additional patterns from test stem sub-components: catches condition
                // (c) where a code file stem is a sub-segment of the test file name.
                for part in stem.split('_') {
                    if part.len() >= 3 && part != "test" {
                        let pat = format!("%{}%", part);
                        if !patterns.contains(&pat) {
                            patterns.push(pat);
                        }
                    }
                }
                for part in stem.split('-') {
                    if part.len() >= 3 && part != "test" {
                        let pat = format!("%{}%", part);
                        if !patterns.contains(&pat) {
                            patterns.push(pat);
                        }
                    }
                }

                let mut seen = std::collections::HashSet::new();
                let mut candidates: Vec<String> = Vec::new();
                for pat in &patterns {
                    let mut stmt = tx
                        .prepare_cached(
                            "SELECT file_path FROM files WHERE is_test_file = 0 AND file_path LIKE ?1",
                        )
                        .map_err(|e| CcError::Database(e.to_string()))?;
                    let rows = stmt
                        .query_map(rusqlite::params![pat], |row| row.get::<_, String>(0))
                        .map_err(|e| CcError::Database(e.to_string()))?;
                    for r in rows.flatten() {
                        if seen.insert(r.clone()) {
                            candidates.push(r);
                        }
                    }
                }

                for code_file in &candidates {
                    let code_stem = Path::new(code_file)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");

                    let (confidence, reason) = if code_stem == base_clean {
                        (0.9, "same-basename")
                    } else if code_file.contains(&*base_clean) || fp.contains(code_stem) {
                        (0.7, "path-overlap")
                    } else {
                        continue;
                    };

                    let edge_id = format!("test:{}:{}", fp, code_file);
                    tx.execute(
                        "INSERT OR REPLACE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                        rusqlite::params![edge_id, fp, code_file, reason, confidence],
                    ).map_err(|e| CcError::Database(e.to_string()))?;
                }
            } else {
                // Changed file is a code file; find matching test files.
                //
                // A pair (test_file, code_file) matches when:
                //   a) code_stem == base_clean(test_stem)     -> same-basename (0.9)
                //   b) code_file.contains(base_clean)         -> path-overlap  (0.7)
                //   c) test_file.contains(code_stem)          -> path-overlap  (0.7)
                //
                // Condition (c) is covered by: LIKE '%code_stem%' on test files.
                // Conditions (a)+(b) require knowing base_clean, which depends on the
                // test file stem. We collect unique stem fragments from the code file
                // path to build additional LIKE patterns that cover (a)+(b).
                let code_stem_owned = stem.to_string();

                // Build a set of LIKE patterns that cover all three conditions.
                let mut patterns: Vec<String> = Vec::new();
                // Primary pattern: catches condition (c)
                if !code_stem_owned.is_empty() {
                    patterns.push(format!("%{}%", code_stem_owned));
                }
                // Additional patterns from stem sub-components: catches conditions
                // (a)+(b) where base_clean is a sub-segment of the code stem.
                // E.g. code_stem = "user_service" => also search "%user%" and "%service%".
                for part in code_stem_owned.split('_') {
                    if part.len() >= 3 {
                        let pat = format!("%{}%", part);
                        if !patterns.contains(&pat) {
                            patterns.push(pat);
                        }
                    }
                }
                // Also add patterns from hyphen-split if present.
                for part in code_stem_owned.split('-') {
                    if part.len() >= 3 {
                        let pat = format!("%{}%", part);
                        if !patterns.contains(&pat) {
                            patterns.push(pat);
                        }
                    }
                }

                // Collect candidate test files from all patterns, deduplicated.
                let mut seen = std::collections::HashSet::new();
                let mut candidates: Vec<String> = Vec::new();
                for pat in &patterns {
                    let mut stmt = tx
                        .prepare_cached(
                            "SELECT file_path FROM files WHERE is_test_file = 1 AND file_path LIKE ?1",
                        )
                        .map_err(|e| CcError::Database(e.to_string()))?;
                    let rows = stmt
                        .query_map(rusqlite::params![pat], |row| row.get::<_, String>(0))
                        .map_err(|e| CcError::Database(e.to_string()))?;
                    for r in rows.flatten() {
                        if seen.insert(r.clone()) {
                            candidates.push(r);
                        }
                    }
                }

                for test_file in &candidates {
                    // Skip test files that are also in the changed set
                    // (they will be handled by their own iteration).
                    if changed_set.contains(test_file.as_str()) {
                        continue;
                    }

                    let test_file_stem = Path::new(test_file)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let base_clean = test_stem_to_base_clean(test_file_stem);

                    let (confidence, reason) = if code_stem_owned == base_clean {
                        (0.9, "same-basename")
                    } else if fp.contains(&*base_clean) || test_file.contains(&*code_stem_owned) {
                        (0.7, "path-overlap")
                    } else {
                        continue;
                    };

                    let edge_id = format!("test:{}:{}", test_file, fp);
                    tx.execute(
                        "INSERT OR REPLACE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                        rusqlite::params![edge_id, test_file, fp, reason, confidence],
                    ).map_err(|e| CcError::Database(e.to_string()))?;
                }
            }
        }

        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn rebuild_test_edges(&self) -> CcResult<()> {
        let all_files: Vec<String> = {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare("SELECT file_path FROM files")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected
        };
        {
            let conn = self
                .write_conn
                .lock()
                .map_err(|e| CcError::Database(e.to_string()))?;
            conn.execute("DELETE FROM test_edges", [])
                .map_err(|e| CcError::Database(e.to_string()))?;
        }
        self.rebuild_test_edges_for_files(&all_files)
    }

    pub fn insert_route_nodes_batch(
        &self,
        routes: &[cc_model::edge::RouteNodeRecord],
    ) -> CcResult<()> {
        if routes.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        Self::insert_route_nodes_on(&tx, routes)?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    /// Insert route nodes on a caller-owned connection/transaction.
    pub(crate) fn insert_route_nodes_on(
        conn: &rusqlite::Connection,
        routes: &[cc_model::edge::RouteNodeRecord],
    ) -> CcResult<()> {
        for r in routes {
            conn.execute(
                "INSERT OR REPLACE INTO routes(edge_id,route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                rusqlite::params![r.route_id, r.route_id, r.file_path, r.route_path, r.method, r.handler_symbol_uid, r.handler_name, r.framework, r.line, r.end_line, r.normalized_path, r.confidence, r.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        Ok(())
    }

    pub fn insert_semantic_edges_batch(
        &self,
        edges: &[cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            tx.execute(
                "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![e.edge_id, e.file_path, e.source_symbol, e.source_symbol_uid, e.target_symbol, e.target_symbol_uid, e.relation_kind.as_str(), e.line, e.confidence, e.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn remove_semantic_edges_by_file(&self, file_path: &str) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM semantic_edges WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn query_semantic_edges(
        &self,
        source_uid: Option<&str>,
        target_uid: Option<&str>,
        relation_kind: Option<&str>,
    ) -> CcResult<Vec<cc_model::edge::SemanticEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut sql = String::from(
            "SELECT edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier FROM semantic_edges WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(uid) = source_uid {
            params.push(Box::new(uid.to_string()));
            sql.push_str(&format!(" AND source_symbol_uid = ?{}", params.len()));
        }
        if let Some(uid) = target_uid {
            params.push(Box::new(uid.to_string()));
            sql.push_str(&format!(" AND target_symbol_uid = ?{}", params.len()));
        }
        if let Some(kind) = relation_kind {
            params.push(Box::new(kind.to_string()));
            sql.push_str(&format!(" AND relation_kind = ?{}", params.len()));
        }
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                let relation_str: String = row.get(6)?;
                let tier_str: String = row.get(9)?;
                Ok(cc_model::edge::SemanticEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    source_symbol: row.get(2)?,
                    source_symbol_uid: row.get(3)?,
                    target_symbol: row.get(4)?,
                    target_symbol_uid: row.get(5)?,
                    relation_kind: match relation_str.as_str() {
                        "inherits" => cc_model::edge::SemanticRelation::Inherits,
                        "implements" => cc_model::edge::SemanticRelation::Implements,
                        "decorates" => cc_model::edge::SemanticRelation::Decorates,
                        "throws" => cc_model::edge::SemanticRelation::Throws,
                        "uses_type" => cc_model::edge::SemanticRelation::UsesType,
                        "defines" => cc_model::edge::SemanticRelation::Defines,
                        "defines_method" => cc_model::edge::SemanticRelation::DefinesMethod,
                        "contains_file" => cc_model::edge::SemanticRelation::ContainsFile,
                        "contains_module" => cc_model::edge::SemanticRelation::ContainsModule,
                        "renders_component" => cc_model::edge::SemanticRelation::RendersComponent,
                        "injects" => cc_model::edge::SemanticRelation::Injects,
                        other => {
                            warn!(kind = %other, "unknown semantic relation_kind in DB, mapping to Unknown");
                            cc_model::edge::SemanticRelation::Unknown
                        }
                    },
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: match tier_str.as_str() {
                        "generic" => ParserTier::Generic,
                        "heuristic" => ParserTier::Heuristic,
                        "tree_sitter" => ParserTier::TreeSitter,
                        "semantic" => ParserTier::Semantic,
                        "verified" => ParserTier::Verified,
                        _ => ParserTier::Generic,
                    },
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn insert_co_change_edges_batch(
        &self,
        edges: &[cc_model::edge::CoChangeEdgeRecord],
    ) -> CcResult<()> {
        if edges.is_empty() {
            return Ok(());
        }
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute("DELETE FROM co_change_edges", [])
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            tx.execute(
                "INSERT INTO co_change_edges(edge_id,file_a,file_b,co_change_count,total_commits_a,total_commits_b,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![e.edge_id, e.file_a, e.file_b, e.co_change_count, e.total_commits_a, e.total_commits_b, e.confidence],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn get_co_changes_for_file(
        &self,
        file_path: &str,
        min_confidence: f64,
    ) -> CcResult<Vec<CoChangeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_a, file_b, co_change_count, total_commits_a, total_commits_b, confidence
                 FROM co_change_edges
                 WHERE (file_a = ?1 OR file_b = ?1) AND confidence >= ?2
                 ORDER BY confidence DESC",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![file_path, min_confidence], |row| {
                Ok(CoChangeLite {
                    edge_id: row.get(0)?,
                    file_a: row.get(1)?,
                    file_b: row.get(2)?,
                    co_change_count: row.get(3)?,
                    total_commits_a: row.get(4)?,
                    total_commits_b: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn replace_infra_data(
        &self,
        nodes: &[cc_model::infra::InfraNode],
        edges: &[cc_model::infra::InfraEdge],
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for node in nodes {
            tx.execute(
                "INSERT OR REPLACE INTO infra_nodes (node_id, file_path, kind, name, namespace, line, end_line, properties, bound_symbol_uid, binding_confidence) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    node.node_id,
                    node.file_path,
                    node.kind.as_str(),
                    node.name,
                    node.namespace,
                    node.line,
                    node.end_line,
                    node.properties.to_string(),
                    node.bound_symbol_uid,
                    node.binding_confidence,
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        for edge in edges {
            tx.execute(
                "INSERT OR REPLACE INTO infra_edges (edge_id, source_node_id, target_node_id, kind, confidence, properties) VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    edge.edge_id,
                    edge.source_node_id,
                    edge.target_node_id,
                    edge.kind.as_str(),
                    edge.confidence,
                    edge.properties.to_string(),
                ],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn replace_dispatch_sites(
        &self,
        file_path: &str,
        sites: &[cc_model::DispatchSiteRecord],
    ) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM dispatch_sites WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        for ds in sites {
            Self::execute_cached(
                &tx,
                "INSERT INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
            )?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn load_all_dispatch_sites(&self) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn load_dispatch_sites_by_kind(
        &self,
        kind: &str,
    ) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE site_kind = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![kind], |row| {
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn delete_synthetic_call_edges(&self, synthesized_by: &str) -> CcResult<usize> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let count = conn
            .execute(
                "DELETE FROM call_edges WHERE synthesized_by = ?1",
                rusqlite::params![synthesized_by],
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(count)
    }

    pub fn insert_synthetic_call_edges(
        &self,
        edges: &[cc_model::CallEdgeRecord],
    ) -> CcResult<usize> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        for e in edges {
            Self::execute_cached(
                &tx,
                "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                rusqlite::params![
                    e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol,
                    e.line, e.start_col, e.end_line, e.end_col,
                    e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id,
                    e.caller_symbol_uid, e.callee_symbol_uid,
                    e.dispatch_kind.as_str(), e.call_kind,
                    e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy,
                    e.receiver_expr, e.arg_count.map(|v| v as i32),
                    e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32,
                    e.parser_tier.as_str(), e.parser_confidence,
                    e.synthesized_by, e.synthesis_key, e.registered_file,
                    e.registered_line.map(|v| v as i32)
                ],
            )?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(edges.len())
    }

    pub fn upsert_runtime_evidence(
        &self,
        evidence_id: &str,
        service_name: &str,
        method: Option<&str>,
        path: &str,
        status_code: Option<&str>,
        now: &str,
    ) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO runtime_evidence(evidence_id, service_name, method, path, status_code, observed_count, first_seen, last_seen)
             VALUES(?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
             ON CONFLICT(evidence_id) DO UPDATE SET observed_count = observed_count + 1, last_seen = ?6",
            rusqlite::params![evidence_id, service_name, method, path, status_code, now],
        ).map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn link_evidence_to_edge(&self, evidence_id: &str, http_edge_id: &str) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE runtime_evidence SET http_edge_id = ?2 WHERE evidence_id = ?1",
            rusqlite::params![evidence_id, http_edge_id],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_evidence_p95(&self, evidence_id: &str, duration_ms: f64) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE runtime_evidence SET p95_ms = CASE \
               WHEN p95_ms IS NULL THEN ?1 \
               ELSE p95_ms * 0.95 + ?1 * 0.05 \
             END WHERE evidence_id = ?2",
            rusqlite::params![duration_ms, evidence_id],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn update_evidence_route_id(&self, evidence_id: &str, route_id: &str) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE runtime_evidence SET route_id = ?1 WHERE evidence_id = ?2",
            rusqlite::params![route_id, evidence_id],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn boost_http_edge_confidence(&self, http_edge_id: &str, boost: f64) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE http_call_edges SET confidence = MIN(1.0, confidence + ?2) WHERE edge_id = ?1",
            rusqlite::params![http_edge_id, boost],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn runtime_evidence_stats(&self) -> CcResult<serde_json::Value> {
        let conn = self.read_conn()?;
        let evidence_rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM runtime_evidence", [], |r| r.get(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        let total_observations: u64 = conn
            .query_row(
                "SELECT COALESCE(SUM(observed_count), 0) FROM runtime_evidence",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let linked_rows: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM runtime_evidence WHERE http_edge_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let distinct_linked_edges: u32 = conn
            .query_row("SELECT COUNT(DISTINCT http_edge_id) FROM runtime_evidence WHERE http_edge_id IS NOT NULL", [], |r| r.get(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(serde_json::json!({
            "evidence_rows": evidence_rows,
            "total_observations": total_observations,
            "linked_evidence_rows": linked_rows,
            "distinct_linked_edges": distinct_linked_edges,
        }))
    }

    /// Query aggregated runtime evidence keyed by normalized path.
    ///
    /// For each normalized path, returns (total_observed_count, latest_last_seen).
    /// Matches evidence whose linked http_edge_id has the given normalized_path.
    pub fn evidence_for_normalized_paths(
        &self,
        paths: &[String],
    ) -> CcResult<std::collections::HashMap<String, (u32, String)>> {
        if paths.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let conn = self.read_conn()?;
        let placeholders: Vec<String> = (1..=paths.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT hce.normalized_path, SUM(re.observed_count) AS total_count, MAX(re.last_seen) AS latest_seen \
             FROM runtime_evidence re \
             JOIN http_call_edges hce ON re.http_edge_id = hce.edge_id \
             WHERE hce.normalized_path IN ({}) \
             GROUP BY hce.normalized_path",
            placeholders.join(",")
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let params: Vec<&dyn rusqlite::types::ToSql> = paths
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                let norm_path: String = row.get(0)?;
                let count: u32 = row.get(1)?;
                let last_seen: String = row.get(2)?;
                Ok((norm_path, count, last_seen))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = std::collections::HashMap::new();
        for (norm_path, count, last_seen) in rows.flatten() {
            result.insert(norm_path, (count, last_seen));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::index_db::IndexDb;

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        (db, tmp)
    }

    #[test]
    fn test_stem_to_base_clean() {
        // strip_prefix("test_") succeeds -> "user", but strip_suffix("_test")
        // fails on "user" so unwrap_or falls back to original test_stem.
        assert_eq!(super::test_stem_to_base_clean("test_user"), "test_user");
        assert_eq!(super::test_stem_to_base_clean("user_test"), "user");
        assert_eq!(super::test_stem_to_base_clean("user.test"), "user");
        assert_eq!(super::test_stem_to_base_clean("user.spec"), "user");
        // Both prefix and suffix match: "test_user_test" -> "user_test" -> "user"
        assert_eq!(super::test_stem_to_base_clean("test_user_test"), "user");
        assert_eq!(super::test_stem_to_base_clean("plain"), "plain");
    }

    #[test]
    fn test_rebuild_test_edges_for_files() {
        let (db, _tmp) = setup();

        // Insert files table entries
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) \
                 VALUES('src/user.py','Python','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) \
                 VALUES('tests/test_user.py','Python','h2',1.0,200,'','','tree_sitter',1.0,1,'2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            tx.commit().unwrap();
        }

        let changed = vec!["src/user.py".to_string(), "tests/test_user.py".to_string()];
        db.rebuild_test_edges_for_files(&changed).unwrap();

        // Verify test_edges are created
        let conn = db.read_conn().unwrap();
        let mut stmt = conn
            .prepare("SELECT edge_id, test_file_path, code_file_path, reason, confidence FROM test_edges")
            .unwrap();
        let edges: Vec<(String, String, String, String, f64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(!edges.is_empty(), "should create at least one test edge");
        let edge = edges
            .iter()
            .find(|e| e.1 == "tests/test_user.py" && e.2 == "src/user.py")
            .expect("should have edge between test_user.py and user.py");
        // base_clean("test_user") = "test_user" (see function logic), code_stem = "user",
        // so same-basename does not match; but "tests/test_user.py".contains("user") = true
        // -> path-overlap with confidence 0.7
        assert_eq!(edge.3, "path-overlap");
        assert!((edge.4 - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_co_change_edges_roundtrip() {
        let (db, _tmp) = setup();

        let edges = vec![
            cc_model::edge::CoChangeEdgeRecord {
                edge_id: "cc:a:b".to_string(),
                file_a: "src/a.rs".to_string(),
                file_b: "src/b.rs".to_string(),
                co_change_count: 5,
                total_commits_a: 10,
                total_commits_b: 8,
                confidence: 0.8,
            },
            cc_model::edge::CoChangeEdgeRecord {
                edge_id: "cc:a:c".to_string(),
                file_a: "src/a.rs".to_string(),
                file_b: "src/c.rs".to_string(),
                co_change_count: 2,
                total_commits_a: 10,
                total_commits_b: 12,
                confidence: 0.3,
            },
        ];
        db.insert_co_change_edges_batch(&edges).unwrap();

        // Query all co-changes for file_a with min_confidence 0.0
        let results = db.get_co_changes_for_file("src/a.rs", 0.0).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].edge_id, "cc:a:b"); // highest confidence first
        assert_eq!(results[1].edge_id, "cc:a:c");

        // Query with min_confidence filtering
        let filtered = db.get_co_changes_for_file("src/a.rs", 0.5).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].edge_id, "cc:a:b");
        assert_eq!(filtered[0].co_change_count, 5);
    }

    #[test]
    fn test_semantic_edges_roundtrip() {
        let (db, _tmp) = setup();

        let edges = vec![
            cc_model::edge::SemanticEdgeRecord {
                edge_id: "sem:1".to_string(),
                file_path: "src/animal.py".to_string(),
                source_symbol: "Dog".to_string(),
                source_symbol_uid: Some("uid:dog".to_string()),
                target_symbol: "Animal".to_string(),
                target_symbol_uid: Some("uid:animal".to_string()),
                relation_kind: cc_model::edge::SemanticRelation::Inherits,
                line: 10,
                confidence: 0.9,
                parser_tier: cc_model::ParserTier::TreeSitter,
            },
            cc_model::edge::SemanticEdgeRecord {
                edge_id: "sem:2".to_string(),
                file_path: "src/animal.py".to_string(),
                source_symbol: "Cat".to_string(),
                source_symbol_uid: Some("uid:cat".to_string()),
                target_symbol: "Animal".to_string(),
                target_symbol_uid: Some("uid:animal".to_string()),
                relation_kind: cc_model::edge::SemanticRelation::Inherits,
                line: 20,
                confidence: 0.85,
                parser_tier: cc_model::ParserTier::TreeSitter,
            },
        ];
        db.insert_semantic_edges_batch(&edges).unwrap();

        // Query by source_uid only
        let by_source = db
            .query_semantic_edges(Some("uid:dog"), None, None)
            .unwrap();
        assert_eq!(by_source.len(), 1);
        assert_eq!(by_source[0].edge_id, "sem:1");

        // Query by relation_kind only
        let by_kind = db
            .query_semantic_edges(None, None, Some("inherits"))
            .unwrap();
        assert_eq!(by_kind.len(), 2);

        // Query all (no filters)
        let all = db.query_semantic_edges(None, None, None).unwrap();
        assert_eq!(all.len(), 2);

        // Test removal
        db.remove_semantic_edges_by_file("src/animal.py").unwrap();
        let after_remove = db.query_semantic_edges(None, None, None).unwrap();
        assert!(after_remove.is_empty());
    }

    #[test]
    fn test_runtime_evidence_lifecycle() {
        let (db, _tmp) = setup();

        // Insert first evidence
        db.upsert_runtime_evidence(
            "ev1",
            "svc-a",
            Some("GET"),
            "/api/users",
            Some("200"),
            "2024-01-01T00:00:00Z",
        )
        .unwrap();

        let stats = db.runtime_evidence_stats().unwrap();
        assert_eq!(stats["evidence_rows"], 1);
        assert_eq!(stats["total_observations"], 1);

        // Upsert again to increment observed_count
        db.upsert_runtime_evidence(
            "ev1",
            "svc-a",
            Some("GET"),
            "/api/users",
            Some("200"),
            "2024-01-02T00:00:00Z",
        )
        .unwrap();

        let stats = db.runtime_evidence_stats().unwrap();
        assert_eq!(stats["evidence_rows"], 1);
        assert_eq!(stats["total_observations"], 2);

        // Update p95 — first call sets p95_ms directly
        db.update_evidence_p95("ev1", 100.0).unwrap();
        {
            let conn = db.read_conn().unwrap();
            let p95: f64 = conn
                .query_row(
                    "SELECT p95_ms FROM runtime_evidence WHERE evidence_id = 'ev1'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!((p95 - 100.0).abs() < 1e-9, "first p95 should be 100.0");
        }

        // Update p95 again — EMA: 100.0 * 0.95 + 200.0 * 0.05 = 105.0
        db.update_evidence_p95("ev1", 200.0).unwrap();
        {
            let conn = db.read_conn().unwrap();
            let p95: f64 = conn
                .query_row(
                    "SELECT p95_ms FROM runtime_evidence WHERE evidence_id = 'ev1'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                (p95 - 105.0).abs() < 1e-9,
                "EMA p95 should be 105.0, got {}",
                p95
            );
        }
    }

    #[test]
    fn test_dispatch_sites_roundtrip() {
        let (db, _tmp) = setup();

        let sites = vec![
            cc_model::DispatchSiteRecord {
                site_id: "ds:1".to_string(),
                file_path: "src/events.ts".to_string(),
                line: 42,
                col: 5,
                enclosing_symbol_uid: Some("uid:handler".to_string()),
                receiver_expr: Some("emitter".to_string()),
                site_kind: cc_model::DispatchSiteKind::EventEmit,
                key: "user:created".to_string(),
                handler_expr: Some("onUserCreated".to_string()),
                handler_symbol_uid: Some("uid:onUserCreated".to_string()),
                confidence: 0.95,
            },
            cc_model::DispatchSiteRecord {
                site_id: "ds:2".to_string(),
                file_path: "src/events.ts".to_string(),
                line: 50,
                col: 5,
                enclosing_symbol_uid: Some("uid:listener".to_string()),
                receiver_expr: Some("emitter".to_string()),
                site_kind: cc_model::DispatchSiteKind::EventOn,
                key: "user:created".to_string(),
                handler_expr: Some("handleUserCreated".to_string()),
                handler_symbol_uid: Some("uid:handleUserCreated".to_string()),
                confidence: 0.9,
            },
        ];
        db.replace_dispatch_sites("src/events.ts", &sites).unwrap();

        // Load all
        let all = db.load_all_dispatch_sites().unwrap();
        assert_eq!(all.len(), 2);

        // Load by kind
        let emits = db.load_dispatch_sites_by_kind("event_emit").unwrap();
        assert_eq!(emits.len(), 1);
        assert_eq!(emits[0].site_id, "ds:1");
        assert_eq!(emits[0].site_kind, cc_model::DispatchSiteKind::EventEmit);
        assert_eq!(emits[0].key, "user:created");

        let ons = db.load_dispatch_sites_by_kind("event_on").unwrap();
        assert_eq!(ons.len(), 1);
        assert_eq!(ons[0].site_id, "ds:2");
        assert_eq!(ons[0].site_kind, cc_model::DispatchSiteKind::EventOn);

        // Replace should clear old data for that file
        db.replace_dispatch_sites("src/events.ts", &[]).unwrap();
        let after_clear = db.load_all_dispatch_sites().unwrap();
        assert!(after_clear.is_empty());
    }
}
