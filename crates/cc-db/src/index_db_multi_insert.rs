//! Multi-row INSERT batching for the incremental write path.
//!
//! The per-row `execute_cached` loops in `insert_file_data_impl` dominate a
//! large incremental batch (statement lookup + reset + bind + step per row).
//! This module rewrites the batch inserts table-major: all rows of one table
//! across every [`FileWriteUnit`] go through chunked
//! `INSERT INTO t(...) VALUES (...),(...),…` statements, cutting the
//! per-statement overhead by the chunk factor while keeping byte-identical
//! rows and identical conflict semantics:
//!
//! - Conflict resolution (`OR REPLACE` / `OR IGNORE`) is applied per row by
//!   SQLite inside a multi-row VALUES insert, in row order — the same order
//!   the per-file loops used (units in slice order, rows in outcome order),
//!   so replace/ignore outcomes are unchanged.
//! - Cross-table order is files-first (FK parents), then child tables. Only
//!   within-table order affects conflicts, and that is preserved.
//! - `chunks_fts` keeps its rowid alignment: the base rows are inserted
//!   first, then their fresh rowids are read back by `chunk_id` (PK) and the
//!   FTS rows are multi-row inserted with explicit rowids and the plain text
//!   (the base column may hold a zstd BLOB, so a SELECT-based mirror cannot
//!   work — see `insert_files_literal_fts_batch`).
//! - `files_fts` / `literal_fts` stay deferred to
//!   [`IndexDb::insert_files_literal_fts_batch`], exactly like the per-file
//!   deferred path this replaces.
//!
//! Statement-cache discipline: chunk sizes come from the fixed tier list
//! [`VALUES_TIERS`], so each table contributes at most `TIERS.len()` distinct
//! SQL strings to the prepared-statement cache instead of one per batch
//! length.

use std::collections::HashMap;

use cc_model::CcResult;
use rusqlite::Connection;

use crate::index_db::{compress_chunk_text, FileWriteUnit, IndexDb, PrecompressedChunks};
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

/// Rows-per-statement tiers. Full chunks of 64 rows amortize ~98% of the
/// per-statement overhead; the tail drains through 8-row then single-row
/// statements so every batch length maps onto at most three cached SQL
/// strings per table. Worst-case variable count: 64 rows × 30 columns
/// (call_edges) = 1920, far below SQLITE_MAX_VARIABLE_NUMBER (32766).
const VALUES_TIERS: [usize; 3] = [64, 8, 1];

/// Build `"{head}(?,?,…),(?,?,…),…"` for `rows` rows of `cols` columns.
/// `head` must end with `"VALUES"` (no trailing parenthesis).
fn values_sql(head: &str, cols: usize, rows: usize) -> String {
    let mut row = String::with_capacity(cols * 2 + 1);
    row.push('(');
    for i in 0..cols {
        if i > 0 {
            row.push(',');
        }
        row.push('?');
    }
    row.push(')');
    let mut sql = String::with_capacity(head.len() + (row.len() + 1) * rows);
    sql.push_str(head);
    for i in 0..rows {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str(&row);
    }
    sql
}

/// Drive one table's rows through tiered multi-row INSERT statements.
/// `bind` must bind exactly `cols` parameters at 1-based positions
/// `base+1..=base+cols` for its row.
fn multi_insert<T>(
    conn: &Connection,
    head: &str,
    cols: usize,
    rows: &[T],
    bind: impl Fn(&mut rusqlite::Statement<'_>, usize, &T) -> rusqlite::Result<()>,
) -> CcResult<()> {
    let mut rest = rows;
    for &per_stmt in VALUES_TIERS.iter() {
        while rest.len() >= per_stmt {
            let (chunk, tail) = rest.split_at(per_stmt);
            let sql = values_sql(head, cols, per_stmt);
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            for (i, row) in chunk.iter().enumerate() {
                bind(&mut stmt, i * cols, row).map_err(db_err)?;
            }
            stmt.raw_execute().map_err(db_err)?;
            rest = tail;
        }
    }
    Ok(())
}

/// Bind a fixed list of values at sequential 1-based positions after `base`.
macro_rules! bind_row {
    ($stmt:expr, $base:expr, [ $($v:expr),+ $(,)? ]) => {{
        let mut i: usize = $base;
        $( i += 1; $stmt.raw_bind_parameter(i, $v)?; )+
        let _ = i;
    }};
}

impl IndexDb {
    /// Table-major batched insert of every [`FileWriteUnit`]'s rows, with the
    /// `files_fts` / `literal_fts` mirrors deferred (the caller must run
    /// [`Self::insert_files_literal_fts_batch`] over the same paths in the
    /// same transaction). Batch equivalent of looping
    /// [`Self::insert_file_data_deferred_fts`] per unit — identical rows,
    /// identical conflict semantics, ~1/64 of the statement executions.
    ///
    /// Preconditions (same as the per-file loop it replaces): the previous
    /// rows for every unit's path were deleted earlier in this transaction,
    /// so the `chunks` rowid read-back below sees exactly this batch's rows.
    pub(crate) fn insert_file_units_batch(
        conn: &Connection,
        units: &[FileWriteUnit],
        precompressed: &PrecompressedChunks,
    ) -> CcResult<()> {
        if units.is_empty() {
            return Ok(());
        }

        // files — parents first (chunks/symbols carry FK REFERENCES files).
        let now = chrono::Utc::now().to_rfc3339();
        let files_rows: Vec<(&FileWriteUnit, String)> = units
            .iter()
            .map(|file| {
                let excerpt: String = file
                    .outcome
                    .chunks
                    .iter()
                    .take(3)
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .chars()
                    .take(20000)
                    .collect();
                (file, excerpt)
            })
            .collect();
        multi_insert(
            conn,
            "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) VALUES",
            11,
            &files_rows,
            |stmt, base, (file, excerpt)| {
                let o = &file.outcome;
                bind_row!(stmt, base, [
                    &file.rel_path, file.language.as_str(), &file.content_hash,
                    file.mtime, file.size as i64, &o.summary, excerpt,
                    o.parser_tier.as_str(), o.parser_confidence,
                    o.is_test_file as i32, &now,
                ]);
                Ok(())
            },
        )?;

        // chunks — resolve each chunk's payload (pre-compressed side-car or
        // in-transaction fallback; same policy, identical bytes).
        struct ChunkRow<'a> {
            rec: &'a cc_model::chunk::ChunkRecord,
            zstd: Option<std::borrow::Cow<'a, [u8]>>,
        }
        let chunk_rows: Vec<ChunkRow<'_>> = units
            .iter()
            .flat_map(|file| {
                let blobs = precompressed.get(&file.rel_path).map(Vec::as_slice);
                file.outcome.chunks.iter().enumerate().map(move |(idx, c)| {
                    let zstd = match blobs.and_then(|b| b.get(idx)) {
                        Some(precomputed) => precomputed.as_deref().map(std::borrow::Cow::Borrowed),
                        None => compress_chunk_text(&c.text).map(std::borrow::Cow::Owned),
                    };
                    ChunkRow { rec: c, zstd }
                })
            })
            .collect();
        multi_insert(
            conn,
            "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES",
            14,
            &chunk_rows,
            |stmt, base, row| {
                let c = row.rec;
                bind_row!(stmt, base, [
                    &c.chunk_id, &c.file_path, c.language.as_str(), c.chunk_index,
                    c.start_line, c.end_line, &c.breadcrumb, &c.symbol_name,
                    c.symbol_kind.map(|k| k.as_str()),
                ]);
                match &row.zstd {
                    Some(blob) => {
                        stmt.raw_bind_parameter(base + 10, &blob[..])?;
                        stmt.raw_bind_parameter(base + 11, "zstd")?;
                    }
                    None => {
                        stmt.raw_bind_parameter(base + 10, &c.text)?;
                        stmt.raw_bind_parameter(base + 11, "plain")?;
                    }
                }
                bind_row!(stmt, base + 11, [
                    c.token_estimate, c.parser_tier.as_str(), c.parser_confidence,
                ]);
                Ok(())
            },
        )?;

        // chunks_fts — read back the fresh base rowids (previous rows for
        // these paths were deleted, so file_path IN (...) sees only this
        // batch), then mirror with explicit rowids and the plain text.
        let chunk_paths: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            chunk_rows
                .iter()
                .map(|r| r.rec.file_path.as_str())
                .filter(|p| seen.insert(*p))
                .collect()
        };
        let mut chunk_rowids: HashMap<String, i64> = HashMap::with_capacity(chunk_rows.len());
        for batch in chunk_paths.chunks(IN_BATCH_SIZE) {
            let sql = format!(
                "SELECT chunk_id, rowid FROM chunks WHERE file_path IN ({})",
                sql_in_placeholders(batch.len())
            );
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })
                .map_err(db_err)?;
            for row in rows {
                let (chunk_id, rowid) = row.map_err(db_err)?;
                chunk_rowids.insert(chunk_id, rowid);
            }
        }
        let fts_rows: Vec<(i64, &cc_model::chunk::ChunkRecord)> = chunk_rows
            .iter()
            .map(|row| {
                chunk_rowids
                    .get(&row.rec.chunk_id)
                    .map(|rowid| (*rowid, row.rec))
                    .ok_or_else(|| {
                        cc_model::CcError::Database(format!(
                            "chunks_fts mirror: no base rowid for chunk_id {}",
                            row.rec.chunk_id
                        ))
                    })
            })
            .collect::<CcResult<_>>()?;
        multi_insert(
            conn,
            "INSERT INTO chunks_fts(rowid,chunk_id,file_path,breadcrumb,symbol_name,text) VALUES",
            6,
            &fts_rows,
            |stmt, base, (rowid, c)| {
                bind_row!(
                    stmt,
                    base,
                    [
                        rowid,
                        &c.chunk_id,
                        &c.file_path,
                        &c.breadcrumb,
                        &c.symbol_name,
                        &c.text,
                    ]
                );
                Ok(())
            },
        )?;

        // symbols
        let symbols: Vec<_> = units.iter().flat_map(|f| &f.outcome.symbols).collect();
        Self::insert_symbols_multi(conn, &symbols, true)?;

        // imports
        let imports: Vec<_> = units.iter().flat_map(|f| &f.outcome.imports).collect();
        multi_insert(
            conn,
            "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES",
            8,
            &imports,
            |stmt, base, i| {
                bind_row!(stmt, base, [
                    &i.file_path, &i.import_string, &i.resolved_path,
                    &i.imported_name, &i.alias, i.is_namespace as i32,
                    i.is_default as i32, i.is_reexport as i32,
                ]);
                Ok(())
            },
        )?;

        // symbol_refs
        let refs: Vec<_> = units.iter().flat_map(|f| &f.outcome.symbol_refs).collect();
        Self::insert_symbol_refs_multi(conn, &refs, true)?;

        // call_edges
        let call_edges: Vec<_> = units.iter().flat_map(|f| &f.outcome.call_edges).collect();
        Self::insert_call_edges_multi(conn, &call_edges)?;

        // test_edges
        let test_edges: Vec<_> = units.iter().flat_map(|f| &f.outcome.test_edges).collect();
        multi_insert(
            conn,
            "INSERT OR IGNORE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES",
            5,
            &test_edges,
            |stmt, base, t| {
                bind_row!(stmt, base, [
                    &t.edge_id, &t.test_file_path, &t.code_file_path,
                    &t.reason, t.confidence,
                ]);
                Ok(())
            },
        )?;

        // route_edges
        let route_edges: Vec<_> = units.iter().flat_map(|f| &f.outcome.route_edges).collect();
        Self::insert_route_edges_multi(conn, &route_edges, true)?;

        // http_call_edges
        let http_edges: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.http_call_edges)
            .collect();
        multi_insert(
            conn,
            "INSERT OR REPLACE INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,normalized_path,method,call_kind,line,confidence,parser_tier,broker_type) VALUES",
            11,
            &http_edges,
            |stmt, base, hce| {
                bind_row!(stmt, base, [
                    &hce.edge_id, &hce.file_path, &hce.caller_symbol_uid,
                    &hce.url_or_path, &hce.normalized_path, &hce.method,
                    &hce.call_kind, hce.line, hce.confidence,
                    hce.parser_tier.as_str(), &hce.broker_type,
                ]);
                Ok(())
            },
        )?;

        // literal_index — OR IGNORE keeps first-wins per literal_id; the
        // deferred literal_fts mirror selects from the surviving base rows.
        let literals: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.literal_index)
            .collect();
        multi_insert(
            conn,
            "INSERT OR IGNORE INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES",
            9,
            &literals,
            |stmt, base, l| {
                bind_row!(stmt, base, [
                    &l.literal_id, &l.file_path, &l.literal, &l.literal_kind,
                    l.line, &l.container, l.confidence,
                    &l.enclosing_symbol_uid, &l.key_path,
                ]);
                Ok(())
            },
        )?;

        // semantic_edges
        let sem_edges: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.semantic_edges)
            .collect();
        Self::insert_semantic_edges_multi(conn, &sem_edges)?;

        // data_flow_edges
        let dfe_rows: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.data_flow_edges)
            .collect();
        multi_insert(
            conn,
            "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier,env_key) VALUES",
            9,
            &dfe_rows,
            |stmt, base, dfe| {
                bind_row!(stmt, base, [
                    &dfe.edge_id, &dfe.file_path, &dfe.source_symbol_uid,
                    &dfe.target_symbol_uid, &dfe.flow_kind, dfe.line,
                    dfe.confidence, dfe.parser_tier.as_str(), &dfe.env_key,
                ]);
                Ok(())
            },
        )?;

        // dispatch_sites
        let sites: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.dispatch_sites)
            .collect();
        Self::insert_dispatch_sites_multi(conn, &sites)?;

        Ok(())
    }

    /// Dirty-file (DirtyResolveOnly) edge rewrite for a whole unit set: one
    /// chunked `DELETE … WHERE file_path IN (...)` per re-resolvable table,
    /// then table-major multi-row re-inserts. Does NOT touch: files row,
    /// chunks, FTS, route_nodes, http_call_edges, data_flow_edges, literals,
    /// file_frameworks, co_change_edges, test_edges. Deletes
    /// run for the whole set before any insert — each delete is keyed by its
    /// own unit's path, so no insert of one unit can be affected by another
    /// unit's delete (same reasoning as the normal-unit replacement path).
    pub(crate) fn replace_reresolved_edges_batch(
        conn: &Connection,
        units: &[FileWriteUnit],
    ) -> CcResult<()> {
        if units.is_empty() {
            return Ok(());
        }
        let paths: Vec<&str> = units.iter().map(|u| u.rel_path.as_str()).collect();
        for table in &[
            "call_edges",
            "symbol_refs",
            "symbols",
            "imports",
            "semantic_edges",
            "dispatch_sites",
            "routes",
        ] {
            for batch in paths.chunks(IN_BATCH_SIZE) {
                Self::execute_cached(
                    conn,
                    &format!(
                        "DELETE FROM {} WHERE file_path IN ({})",
                        table,
                        sql_in_placeholders(batch.len())
                    ),
                    rusqlite::params_from_iter(batch.iter()),
                )?;
            }
        }

        let symbols: Vec<_> = units.iter().flat_map(|f| &f.outcome.symbols).collect();
        Self::insert_symbols_multi(conn, &symbols, false)?;

        let imports: Vec<_> = units.iter().flat_map(|f| &f.outcome.imports).collect();
        multi_insert(
            conn,
            "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES",
            8,
            &imports,
            |stmt, base, i| {
                bind_row!(stmt, base, [
                    &i.file_path, &i.import_string, &i.resolved_path,
                    &i.imported_name, &i.alias, i.is_namespace as i32,
                    i.is_default as i32, i.is_reexport as i32,
                ]);
                Ok(())
            },
        )?;

        let refs: Vec<_> = units.iter().flat_map(|f| &f.outcome.symbol_refs).collect();
        Self::insert_symbol_refs_multi(conn, &refs, false)?;

        let call_edges: Vec<_> = units.iter().flat_map(|f| &f.outcome.call_edges).collect();
        Self::insert_call_edges_multi(conn, &call_edges)?;

        let sem_edges: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.semantic_edges)
            .collect();
        Self::insert_semantic_edges_multi(conn, &sem_edges)?;

        let sites: Vec<_> = units
            .iter()
            .flat_map(|f| &f.outcome.dispatch_sites)
            .collect();
        Self::insert_dispatch_sites_multi(conn, &sites)?;

        let route_edges: Vec<_> = units.iter().flat_map(|f| &f.outcome.route_edges).collect();
        Self::insert_route_edges_multi(conn, &route_edges, false)?;

        Ok(())
    }

    // ── shared per-table multi-row inserts ───────────────────────
    // `or_replace` mirrors the two call sites' historical conflict clauses:
    // the full-replacement path used OR REPLACE / the dirty-rewrite path used
    // plain INSERT (its per-file deletes already cleared the keyed rows).

    fn insert_symbols_multi(
        conn: &Connection,
        rows: &[&cc_model::symbol::SymbolRecord],
        or_replace: bool,
    ) -> CcResult<()> {
        let head = if or_replace {
            "INSERT OR REPLACE INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES"
        } else {
            "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES"
        };
        multi_insert(conn, head, 25, rows, |stmt, base, s| {
            bind_row!(
                stmt,
                base,
                [
                    &s.symbol_id,
                    &s.file_path,
                    &s.name,
                    s.kind.as_str(),
                    &s.container,
                    s.start_line,
                    s.end_line,
                    s.start_col,
                    s.end_col,
                    &s.signature,
                    &s.doc,
                    s.parser_tier.as_str(),
                    s.parser_confidence,
                    &s.qname,
                    &s.parent_symbol_id,
                    &s.export_name,
                    s.is_default_export as i32,
                    &s.symbol_uid,
                    &s.framework_role,
                    &s.receiver_type,
                    &s.param_types,
                    &s.return_type,
                    s.param_count,
                    &s.base_types,
                    &s.implements,
                ]
            );
            Ok(())
        })
    }

    fn insert_symbol_refs_multi(
        conn: &Connection,
        rows: &[&cc_model::symbol::SymbolRefRecord],
        or_replace: bool,
    ) -> CcResult<()> {
        let head = if or_replace {
            "INSERT OR REPLACE INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES"
        } else {
            "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES"
        };
        multi_insert(conn, head, 18, rows, |stmt, base, r| {
            bind_row!(
                stmt,
                base,
                [
                    &r.ref_id,
                    &r.file_path,
                    &r.symbol_name,
                    &r.container,
                    &r.ref_kind,
                    r.line,
                    r.column,
                    &r.target_symbol_id,
                    &r.target_file_path,
                    &r.target_symbol_uid,
                    &r.ref_name,
                    r.resolution_kind.as_str(),
                    r.resolution_confidence,
                    &r.resolution_strategy,
                    r.ref_end_line,
                    r.ref_end_col,
                    r.parser_tier.as_str(),
                    r.parser_confidence,
                ]
            );
            Ok(())
        })
    }

    fn insert_call_edges_multi(
        conn: &Connection,
        rows: &[&cc_model::edge::CallEdgeRecord],
    ) -> CcResult<()> {
        multi_insert(
            conn,
            "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES",
            30,
            rows,
            |stmt, base, e| {
                bind_row!(stmt, base, [
                    &e.edge_id, &e.file_path, &e.caller_symbol, &e.callee_symbol,
                    e.line, e.start_col, e.end_line, e.end_col,
                    &e.target_symbol_id, &e.target_file_path,
                    &e.caller_symbol_id, &e.callee_ref_id, &e.caller_symbol_uid,
                    &e.callee_symbol_uid, e.dispatch_kind.as_str(), &e.call_kind,
                    e.resolution_kind.as_str(), e.resolution_confidence,
                    &e.resolution_strategy, &e.receiver_expr,
                    e.arg_count.map(|v| v as i32), e.is_optional_chain as i32,
                    e.is_awaited as i32, e.is_constructor as i32,
                    e.parser_tier.as_str(), e.parser_confidence,
                    &e.synthesized_by, &e.synthesis_key, &e.registered_file,
                    e.registered_line.map(|v| v as i32),
                ]);
                Ok(())
            },
        )
    }

    fn insert_semantic_edges_multi(
        conn: &Connection,
        rows: &[&cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        multi_insert(
            conn,
            "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES",
            10,
            rows,
            |stmt, base, se| {
                bind_row!(stmt, base, [
                    &se.edge_id, &se.file_path, &se.source_symbol,
                    &se.source_symbol_uid, &se.target_symbol,
                    &se.target_symbol_uid, se.relation_kind.as_str(), se.line,
                    se.confidence, se.parser_tier.as_str(),
                ]);
                Ok(())
            },
        )
    }

    fn insert_dispatch_sites_multi(
        conn: &Connection,
        rows: &[&cc_model::dispatch_site::DispatchSiteRecord],
    ) -> CcResult<()> {
        multi_insert(
            conn,
            "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES",
            11,
            rows,
            |stmt, base, ds| {
                bind_row!(stmt, base, [
                    &ds.site_id, &ds.file_path, ds.line, ds.col,
                    &ds.enclosing_symbol_uid, &ds.receiver_expr,
                    ds.site_kind.as_str(), &ds.key, &ds.handler_expr,
                    &ds.handler_symbol_uid, ds.confidence,
                ]);
                Ok(())
            },
        )
    }

    fn insert_route_edges_multi(
        conn: &Connection,
        rows: &[&cc_model::edge::RouteEdgeRecord],
        or_replace: bool,
    ) -> CcResult<()> {
        let head = if or_replace {
            "INSERT OR REPLACE INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES"
        } else {
            "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES"
        };
        multi_insert(conn, head, 19, rows, |stmt, base, r| {
            bind_row!(
                stmt,
                base,
                [
                    &r.edge_id,
                    &r.file_path,
                    &r.route_path,
                    &r.handler_name,
                    &r.method,
                    r.line,
                    r.start_col,
                    r.end_line,
                    r.end_col,
                    &r.handler_symbol_id,
                    &r.handler_symbol_uid,
                    &r.handler_expr,
                    &r.router_symbol_uid,
                    &r.framework,
                    &r.route_kind,
                    r.confidence,
                    r.parser_tier.as_str(),
                    &r.resolution_strategy,
                    r.resolution_confidence,
                ]
            );
            Ok(())
        })
    }
}
