//! Shared rusqlite row → struct mappers for the lite row types that are
//! selected from more than one query site.
//!
//! Each mapper documents the SELECT column order it expects; every call site
//! must keep its column list in exactly that order. Queries that project a
//! different column set (e.g. JOINs with extra columns, or `RouteNodeLite`
//! variants that differ in whether `normalized_path` is selected) keep their
//! local closures and are intentionally not unified here.

use crate::index_db::{CallEdgeLite, HttpCallEdgeLite, RouteEdgeLite, SymbolLiteRow, SymbolRow};

/// Map a row selected as:
/// `symbol_id, symbol_uid, name, kind, file_path, container, start_line,
///  end_line, qname, signature` (from `symbols`).
pub(crate) fn symbol_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRow> {
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
}

/// Map a row selected as:
/// `symbol_uid, name, file_path, kind, community_id` (from `symbols`).
pub(crate) fn symbol_lite(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolLiteRow> {
    Ok(SymbolLiteRow {
        symbol_uid: row.get::<_, String>(0)?,
        name: row.get::<_, String>(1)?,
        file_path: row.get::<_, String>(2)?,
        kind: row.get::<_, String>(3)?,
        community_id: row.get::<_, Option<u32>>(4)?,
    })
}

/// Map a row selected as:
/// `file_path, line, caller_symbol, callee_symbol, caller_symbol_uid,
///  callee_symbol_uid, resolution_kind, resolution_confidence, dispatch_kind,
///  synthesized_by, synthesis_key, registered_file, registered_line`
/// (from `call_edges`).
pub(crate) fn call_edge_lite(row: &rusqlite::Row<'_>) -> rusqlite::Result<CallEdgeLite> {
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
}

/// Map a row selected as:
/// `edge_id, file_path, route_path, handler_name, method, line, end_line,
///  handler_symbol_uid, framework, confidence` (from `routes`).
pub(crate) fn route_edge_lite(row: &rusqlite::Row<'_>) -> rusqlite::Result<RouteEdgeLite> {
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
}

/// Map a row selected as:
/// `edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path,
///  method, call_kind, line, confidence` (from `http_call_edges`).
pub(crate) fn http_call_edge_lite(row: &rusqlite::Row<'_>) -> rusqlite::Result<HttpCallEdgeLite> {
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
}
