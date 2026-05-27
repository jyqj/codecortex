//! IndexDb methods: route, HTTP call, diagnostic, and route-node queries (frontier expansion).

use cc_model::edge::{HttpCallEdgeRecord, RouteNodeRecord};
use cc_model::{CcError, CcResult};

use crate::index_db::{
    parse_parser_tier, DiagnosticLite, HttpCallEdgeLite, IndexDb, RouteEdgeLite, RouteNodeLite,
};

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
                 FROM route_edges
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
                 FROM route_edges
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
                 FROM route_nodes
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
                     FROM route_nodes
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

    pub fn diagnostic_rows_by_message(
        &self,
        query: &str,
        limit: usize,
    ) -> CcResult<Vec<DiagnosticLite>> {
        let conn = self.read_conn()?;
        let fts_query = query
            .replace('"', "\"\"")
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(
                "SELECT d.diagnostic_id, d.file_path, d.severity, d.message, d.line,
                        d.end_line, d.source, d.code, d.confidence, d.symbol_uid
                 FROM diagnostics d
                 JOIN diagnostics_fts f ON d.diagnostic_id = f.diagnostic_id
                 WHERE diagnostics_fts MATCH ?1
                 LIMIT ?2",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![fts_query, limit as i64], |row| {
                Ok(DiagnosticLite {
                    diagnostic_id: row.get(0)?,
                    file_path: row.get(1)?,
                    severity: row.get(2)?,
                    message: row.get(3)?,
                    line: row.get(4)?,
                    end_line: row.get(5)?,
                    source: row.get(6)?,
                    code: row.get(7)?,
                    confidence: row.get(8)?,
                    symbol_uid: row.get(9)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_route_nodes(&self) -> CcResult<Vec<RouteNodeLite>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence
                 FROM route_nodes
                 ORDER BY route_path",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
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
                 FROM route_nodes
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

    pub fn http_call_records_by_caller_uid(
        &self,
        caller_uid: &str,
    ) -> CcResult<Vec<HttpCallEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier, broker_type
                 FROM http_call_edges WHERE caller_symbol_uid = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![caller_uid], |row| {
                Ok(HttpCallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: parse_parser_tier(row.get::<_, String>(9)?.as_str()),
                    broker_type: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn http_callers_for_route_path(
        &self,
        normalized_path: &str,
    ) -> CcResult<Vec<HttpCallEdgeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier, broker_type
                 FROM http_call_edges WHERE normalized_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path], |row| {
                Ok(HttpCallEdgeRecord {
                    edge_id: row.get(0)?,
                    file_path: row.get(1)?,
                    caller_symbol_uid: row.get(2)?,
                    url_or_path: row.get(3)?,
                    normalized_path: row.get(4)?,
                    method: row.get(5)?,
                    call_kind: row.get(6)?,
                    line: row.get(7)?,
                    confidence: row.get(8)?,
                    parser_tier: parse_parser_tier(row.get::<_, String>(9)?.as_str()),
                    broker_type: row.get(10)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn routes_by_normalized_path(
        &self,
        normalized_path: &str,
    ) -> CcResult<Vec<RouteNodeRecord>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence, parser_tier, normalized_path
                 FROM route_nodes WHERE normalized_path = ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![normalized_path], |row| {
                Ok(RouteNodeRecord {
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
                    parser_tier: parse_parser_tier(row.get::<_, String>(10)?.as_str()),
                    normalized_path: row.get(11)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn routes_by_normalized_path_and_method(
        &self,
        normalized_path: &str,
        method: Option<&str>,
    ) -> CcResult<Vec<RouteNodeRecord>> {
        if let Some(m) = method {
            let conn = self.read_conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT route_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, confidence, parser_tier, normalized_path
                     FROM route_nodes WHERE normalized_path = ?1 AND UPPER(method) = UPPER(?2)",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![normalized_path, m], |row| {
                    Ok(RouteNodeRecord {
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
                        parser_tier: parse_parser_tier(row.get::<_, String>(10)?.as_str()),
                        normalized_path: row.get(11)?,
                    })
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let exact: Vec<RouteNodeRecord> = rows.filter_map(|r| r.ok()).collect();
            if !exact.is_empty() {
                return Ok(exact);
            }
        }
        self.routes_by_normalized_path(normalized_path)
    }
}
