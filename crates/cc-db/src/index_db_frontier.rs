//! IndexDb methods: route, HTTP call, diagnostic, and route-node queries (frontier expansion).

use cc_model::{CcError, CcResult};

use crate::index_db::{HttpCallEdgeLite, IndexDb, RouteEdgeLite, RouteNodeLite};

impl IndexDb {
    pub fn route_rows_by_path(
        &self,
        route_path: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, route_path, handler_name, method, line,
                        end_line, handler_symbol_uid, framework, confidence
                 FROM routes
                 WHERE route_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![route_path, limit as i64], |row| {
                Ok(RouteEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    end_line: row.get(6)?,
                    handler_symbol_uid: row.get(7)?,
                    framework: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn route_rows_by_handler_uid(
        &self,
        handler_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, route_path, handler_name, method, line,
                        end_line, handler_symbol_uid, framework, confidence
                 FROM routes
                 WHERE handler_symbol_uid = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![handler_uid, limit as i64], |row| {
                Ok(RouteEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    handler_name: row.get(3)?,
                    method: row.get(4)?,
                    line: row.get(5)?,
                    end_line: row.get(6)?,
                    handler_symbol_uid: row.get(7)?,
                    framework: row.get(8)?,
                    confidence: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn http_calls_by_caller_uid(
        &self,
        caller_uid: &str,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                        method, call_kind, line, confidence
                 FROM http_call_edges
                 WHERE caller_symbol_uid = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid, limit as i64], |row| {
                Ok(HttpCallEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn http_callers_by_normalized_path(
        &self,
        normalized_path: &str,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                        method, call_kind, line, confidence
                 FROM http_call_edges
                 WHERE normalized_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path, limit as i64], |row| {
                Ok(HttpCallEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn http_callers_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<HttpCallEdgeLite>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                            method, call_kind, line, confidence
                     FROM http_call_edges
                     WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)
                     ORDER BY confidence DESC
                     LIMIT ?3",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m, limit as i64], |row| {
                    Ok(HttpCallEdgeLite {
                        edge_id: row.get(0)?,
                        file_path: row.get(1)?,
                        caller_symbol_uid: row.get(2)?,
                        url_or_path: row.get(3)?,
                        normalized_path: row.get(4)?,
                        method: row.get(5)?,
                        call_kind: row.get(6)?,
                        line: row.get(7)?,
                        confidence: row.get(8)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<HttpCallEdgeLite> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        self.http_callers_by_normalized_path(normalized_path, limit)
    }

    pub fn route_nodes_by_normalized_path(
        &self,
        normalized_path: &str,
        limit: usize,
    ) -> CcResult<Vec<RouteNodeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid,
                        handler_name, framework, line, end_line, confidence
                 FROM routes
                 WHERE normalized_path = ?1
                 ORDER BY confidence DESC
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path, limit as i64], |row| {
                Ok(RouteNodeLite {
                    route_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    method: row.get(3)?,
                    handler_symbol_uid: row.get(4)?,
                    handler_name: row.get(5)?,
                    framework: row.get(6)?,
                    line: row.get(7)?,
                    end_line: row.get(8)?,
                    confidence: row.get(9)?,
                    normalized_path: None,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn route_nodes_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<RouteNodeLite>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT route_id, file_path, route_path, method, handler_symbol_uid,
                            handler_name, framework, line, end_line, confidence
                     FROM routes
                     WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)
                     ORDER BY confidence DESC
                     LIMIT ?3",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m, limit as i64], |row| {
                    Ok(RouteNodeLite {
                        route_id: row.get(0)?,
                        file_path: row.get(1)?,
                        route_path: row.get(2)?,
                        method: row.get(3)?,
                        handler_symbol_uid: row.get(4)?,
                        handler_name: row.get(5)?,
                        framework: row.get(6)?,
                        line: row.get(7)?,
                        end_line: row.get(8)?,
                        confidence: row.get(9)?,
                        normalized_path: None,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<RouteNodeLite> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        self.route_nodes_by_normalized_path(normalized_path, limit)
    }

    pub fn all_http_call_edges_lite(&self, limit: usize) -> CcResult<Vec<HttpCallEdgeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
                        method, call_kind, line, confidence
                 FROM http_call_edges
                 WHERE caller_symbol_uid IS NOT NULL AND normalized_path IS NOT NULL
                 ORDER BY confidence DESC
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(HttpCallEdgeLite {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn all_route_nodes_lite(&self, limit: usize) -> CcResult<Vec<RouteNodeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid,
                        handler_name, framework, line, end_line, confidence, normalized_path
                 FROM routes
                 WHERE handler_symbol_uid IS NOT NULL AND normalized_path IS NOT NULL
                 ORDER BY confidence DESC
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(RouteNodeLite {
                    route_id: row.get(0)?,
                    file_path: row.get(1)?,
                    route_path: row.get(2)?,
                    method: row.get(3)?,
                    handler_symbol_uid: row.get(4)?,
                    handler_name: row.get(5)?,
                    framework: row.get(6)?,
                    line: row.get(7)?,
                    end_line: row.get(8)?,
                    confidence: row.get(9)?,
                    normalized_path: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }
}

#[cfg(test)]
mod tests {
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
    fn test_route_rows_by_path() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/routes.js");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params!["re1","src/routes.js","/api/users","listUsers","GET",10,0.9,"tree_sitter"],
            ).unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params!["re2","src/routes.js","/api/users","createUser","POST",20,0.7,"tree_sitter"],
            ).unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params!["re3","src/routes.js","/api/posts","listPosts","GET",30,0.8,"tree_sitter"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let rows = db.route_rows_by_path("/api/users", 10).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by confidence DESC: 0.9 first, then 0.7
        assert_eq!(rows[0].edge_id, "re1");
        assert!((rows[0].confidence - 0.9).abs() < 1e-9);
        assert_eq!(rows[1].edge_id, "re2");
        assert!((rows[1].confidence - 0.7).abs() < 1e-9);

        // Different path returns only its own route
        let posts = db.route_rows_by_path("/api/posts", 10).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].edge_id, "re3");
    }

    #[test]
    fn test_http_calls_by_caller_uid() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/client.ts");

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,method,normalized_path,call_kind,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["hce1","src/client.ts","uid_fetch","/api/users","GET","/api/users","http",15,0.7,"tree_sitter"],
            ).unwrap();
            tx.execute(
                "INSERT INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,method,normalized_path,call_kind,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["hce2","src/client.ts","uid_fetch","/api/posts","GET","/api/posts","http",20,0.8,"tree_sitter"],
            ).unwrap();
            tx.execute(
                "INSERT INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,method,normalized_path,call_kind,line,confidence,parser_tier)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params!["hce3","src/client.ts","uid_other","/api/admin","POST","/api/admin","http",30,0.9,"tree_sitter"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let rows = db.http_calls_by_caller_uid("uid_fetch", 10).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by confidence DESC: 0.8 first, then 0.7
        assert_eq!(rows[0].edge_id, "hce2");
        assert_eq!(rows[1].edge_id, "hce1");

        // Different caller_uid
        let other = db.http_calls_by_caller_uid("uid_other", 10).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].edge_id, "hce3");
    }

    #[test]
    fn test_route_nodes_by_normalized_path() {
        let (db, _tmp) = setup();

        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES('routes.js', 'javascript', 'hash1', 1000, 100, '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                rusqlite::params!["e_rn1","routes.js","/api/users","GET","uid_handler","listUsers","express",5,10,"/api/users",0.9,"tree_sitter","rn1"],
            ).unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                rusqlite::params!["e_rn2","routes.js","/api/users","POST","uid_create","createUser","express",15,20,"/api/users",0.8,"tree_sitter","rn2"],
            ).unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier,route_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                rusqlite::params!["e_rn3","routes.js","/api/posts","GET","uid_posts","listPosts","express",25,30,"/api/posts",0.7,"tree_sitter","rn3"],
            ).unwrap();
            tx.commit().unwrap();
        }

        let rows = db.route_nodes_by_normalized_path("/api/users", 10).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by confidence DESC
        assert_eq!(rows[0].route_id, "rn1");
        assert_eq!(rows[0].handler_symbol_uid.as_deref(), Some("uid_handler"));
        assert_eq!(rows[1].route_id, "rn2");

        // Different path
        let posts = db.route_nodes_by_normalized_path("/api/posts", 10).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].route_id, "rn3");
    }
}
