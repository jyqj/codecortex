//! IndexDb methods: edge batch operations, dispatch sites, infra, synthetic edges, runtime evidence.

use std::path::Path;

use tracing::warn;

use cc_model::{CcError, CcResult, ParserTier};

use crate::index_db::{CoChangeLite, IndexDb};

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

        let all_tests: std::collections::HashSet<String> = {
            let mut stmt = tx
                .prepare("SELECT file_path FROM files WHERE is_test_file = 1")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected.into_iter().collect()
        };
        let all_code: std::collections::HashSet<String> = {
            let mut stmt = tx
                .prepare("SELECT file_path FROM files WHERE is_test_file = 0")
                .map_err(|e| CcError::Database(e.to_string()))?;
            let collected: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| CcError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
            collected.into_iter().collect()
        };

        let changed_set: std::collections::HashSet<&str> =
            changed.iter().map(|s| s.as_str()).collect();

        let mut pairs: Vec<(String, String)> = Vec::new();
        for tf in &all_tests {
            if changed_set.contains(tf.as_str()) {
                for cf in &all_code {
                    pairs.push((tf.clone(), cf.clone()));
                }
            }
        }
        for cf in &all_code {
            if changed_set.contains(cf.as_str()) {
                for tf in &all_tests {
                    if !changed_set.contains(tf.as_str()) {
                        pairs.push((tf.clone(), cf.clone()));
                    }
                }
            }
        }

        for (test_file, code_file) in &pairs {
            let test_stem = Path::new(test_file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let code_stem = Path::new(code_file)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            let base_clean = test_stem
                .strip_prefix("test_")
                .unwrap_or(test_stem)
                .strip_suffix("_test")
                .unwrap_or(test_stem);
            let base_clean = base_clean
                .strip_suffix(".test")
                .or_else(|| base_clean.strip_suffix(".spec"))
                .unwrap_or(base_clean);

            let (confidence, reason) = if code_stem == base_clean {
                (0.9, "same-basename")
            } else if code_file.contains(base_clean) || test_file.contains(code_stem) {
                (0.7, "path-overlap")
            } else {
                continue;
            };

            let edge_id = format!("test:{}:{}", test_file, code_file);
            tx.execute(
                "INSERT OR REPLACE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![edge_id, test_file, code_file, reason, confidence],
            ).map_err(|e| CcError::Database(e.to_string()))?;
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
        for r in routes {
            tx.execute(
                "INSERT OR REPLACE INTO route_nodes(route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                rusqlite::params![r.route_id, r.file_path, r.route_path, r.method, r.handler_symbol_uid, r.handler_name, r.framework, r.line, r.end_line, r.normalized_path, r.confidence, r.parser_tier.as_str()],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn insert_data_flow_edges_batch(
        &self,
        edges: &[cc_model::edge::DataFlowEdgeRecord],
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
                "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier,env_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![e.edge_id, e.file_path, e.source_symbol_uid, e.target_symbol_uid, e.flow_kind, e.line, e.confidence, e.parser_tier.as_str(), e.env_key],
            ).map_err(|e| CcError::Database(e.to_string()))?;
        }
        tx.commit().map_err(|e| CcError::Database(e.to_string()))?;
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

    pub fn insert_route_edges_batch(
        &self,
        edges: &[cc_model::edge::RouteEdgeRecord],
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
        for r in edges {
            tx.execute(
                "INSERT OR REPLACE INTO route_edges(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str()],
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

    pub fn co_change_neighbors(
        &self,
        file_path: &str,
        min_confidence: f64,
        limit: usize,
    ) -> CcResult<Vec<CoChangeLite>> {
        let mut rows = self.get_co_changes_for_file(file_path, min_confidence)?;
        rows.truncate(limit);
        Ok(rows)
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

    pub fn delete_infra_by_file(&self, file_path: &str) -> CcResult<()> {
        let mut conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM infra_edges WHERE source_node_id IN (SELECT node_id FROM infra_nodes WHERE file_path = ?1) OR target_node_id IN (SELECT node_id FROM infra_nodes WHERE file_path = ?1)",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        tx.execute(
            "DELETE FROM infra_nodes WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
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
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
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

    pub fn load_dispatch_sites_by_kind_key(
        &self,
        kind: &str,
        key: &str,
    ) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,\
                 site_kind,key,handler_expr,handler_symbol_uid,confidence \
                 FROM dispatch_sites WHERE site_kind = ?1 AND key = ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![kind, key], |row| {
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
        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(|e| CcError::Database(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn delete_dispatch_sites_for_file(&self, file_path: &str) -> CcResult<()> {
        let conn = self
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "DELETE FROM dispatch_sites WHERE file_path = ?1",
            rusqlite::params![file_path],
        )
        .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
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
                    site_kind: cc_model::DispatchSiteKind::from_str(&kind_str),
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
        let conn = self.write_conn.lock().map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO runtime_evidence(evidence_id, service_name, method, path, status_code, observed_count, first_seen, last_seen)
             VALUES(?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
             ON CONFLICT(evidence_id) DO UPDATE SET observed_count = observed_count + 1, last_seen = ?6",
            rusqlite::params![evidence_id, service_name, method, path, status_code, now],
        ).map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn link_evidence_to_edge(
        &self,
        evidence_id: &str,
        http_edge_id: &str,
    ) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE runtime_evidence SET http_edge_id = ?2 WHERE evidence_id = ?1",
            rusqlite::params![evidence_id, http_edge_id],
        ).map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn boost_http_edge_confidence(&self, http_edge_id: &str, boost: f64) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "UPDATE http_call_edges SET confidence = MIN(1.0, confidence + ?2) WHERE edge_id = ?1",
            rusqlite::params![http_edge_id, boost],
        ).map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn runtime_evidence_stats(&self) -> CcResult<serde_json::Value> {
        let conn = self.read_conn()?;
        let evidence_rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM runtime_evidence", [], |r| r.get(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        let total_observations: u64 = conn
            .query_row("SELECT COALESCE(SUM(observed_count), 0) FROM runtime_evidence", [], |r| r.get(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        let linked_rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM runtime_evidence WHERE http_edge_id IS NOT NULL", [], |r| r.get(0))
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
        let mut stmt = conn.prepare(&sql).map_err(|e| CcError::Database(e.to_string()))?;
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
        for row in rows {
            if let Ok((norm_path, count, last_seen)) = row {
                result.insert(norm_path, (count, last_seen));
            }
        }
        Ok(result)
    }

    /// Query aggregated runtime evidence for a set of http_edge_ids.
    ///
    /// Returns a map of http_edge_id -> (total_observed_count, latest_last_seen).
    pub fn evidence_for_http_edges(
        &self,
        edge_ids: &[String],
    ) -> CcResult<std::collections::HashMap<String, (u32, String)>> {
        if edge_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let conn = self.read_conn()?;
        let placeholders: Vec<String> = (1..=edge_ids.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT http_edge_id, SUM(observed_count) AS total_count, MAX(last_seen) AS latest_seen \
             FROM runtime_evidence \
             WHERE http_edge_id IN ({}) \
             GROUP BY http_edge_id",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| CcError::Database(e.to_string()))?;
        let params: Vec<&dyn rusqlite::types::ToSql> = edge_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params.as_slice(), |row| {
                let eid: String = row.get(0)?;
                let count: u32 = row.get(1)?;
                let last_seen: String = row.get(2)?;
                Ok((eid, count, last_seen))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        let mut result = std::collections::HashMap::new();
        for row in rows {
            if let Ok((eid, count, last_seen)) = row {
                result.insert(eid, (count, last_seen));
            }
        }
        Ok(result)
    }
}
