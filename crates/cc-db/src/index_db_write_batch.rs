//! Incremental batch write path of [`IndexDb`]: per-file replace of parse
//! products inside ONE transaction with signature-aggregate maintenance and
//! the in-transaction `symbols_seed` token span (see
//! `crate::seed_symbol_cache`). Split from `index_db.rs`; every method is an
//! `impl IndexDb` continuation with unchanged bodies.

use std::collections::HashSet;

use rusqlite::Connection;

use cc_model::edge::RouteNodeRecord;
use cc_model::CcResult;

use crate::index_db::{
    compress_chunk_text, FileWriteUnit, IndexDb, PrecompressedChunks, SeedTokenSpan,
};
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

impl IndexDb {
    // ── Batch write ──────────────────────────────────────────────

    pub(crate) fn replace_files_batch(&self, files: &[FileWriteUnit]) -> CcResult<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let rel_paths: Vec<&str> = files.iter().map(|f| f.rel_path.as_str()).collect();
        let agg_update = crate::signature_agg::begin_path_update(&tx, &rel_paths)?;
        Self::delete_files_fts_batch(&tx, rel_paths.iter().copied())?;
        // Replacement keeps the path, so the path-derived test_edges
        // stay valid (see `delete_files_data_base_keep_test_edges_batch`).
        Self::delete_files_data_base_keep_test_edges_batch(&tx, &rel_paths)?;
        for file in files {
            Self::insert_file_data_deferred_fts(&tx, file, None)?;
        }
        Self::insert_files_literal_fts_batch(&tx, &rel_paths)?;
        crate::signature_agg::finish_path_update(&tx, &rel_paths, agg_update)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Apply one round of config-linker output (incremental path).
    ///
    /// Units whose file already has a `files` row (scanner-visible config
    /// files like yaml/toml, written as parsed units) only replace that
    /// file's config refs — the parsed representation (files row, chunks,
    /// FTS) stays intact. Units without a row are written whole, as before
    /// (non-scanner config files such as .ini/.env).
    ///
    /// `seen_config_files` are the config files covered by this round's scan
    /// (or token cache): a seen file without a unit resolved to ZERO links,
    /// so its leftover config refs from earlier rounds are deleted here —
    /// they would otherwise linger until the next full rebuild.
    pub(crate) fn apply_config_link_units(
        &self,
        units: &[FileWriteUnit],
        seen_config_files: &[String],
    ) -> CcResult<()> {
        if units.is_empty() && seen_config_files.is_empty() {
            return Ok(());
        }
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let linked_now: std::collections::HashSet<&str> =
            units.iter().map(|u| u.rel_path.as_str()).collect();
        // Config refs are not signature-relevant, but the whole-unit branch
        // below (delete_file_data + insert_file_data) can touch signature
        // tables — keep the aggregates in sync over the unit paths.
        let unit_paths: Vec<&str> = units.iter().map(|u| u.rel_path.as_str()).collect();
        let agg_update = crate::signature_agg::begin_path_update(&tx, &unit_paths)?;
        let mut wrote = false;
        for path in seen_config_files {
            if linked_now.contains(path.as_str()) {
                continue;
            }
            // 零链接陈旧行清理：本轮没有替换单元的文件，旧 refs 直接删。
            wrote |= Self::delete_config_link_refs(&tx, path)? > 0;
        }
        for unit in units {
            let parsed_row_exists = {
                let mut stmt = tx
                    .prepare_cached("SELECT 1 FROM files WHERE file_path = ?1")
                    .map_err(db_err)?;
                stmt.exists(rusqlite::params![unit.rel_path])
                    .map_err(db_err)?
            };
            if parsed_row_exists {
                Self::delete_config_link_refs(&tx, &unit.rel_path)?;
                Self::insert_symbol_refs_on(&tx, &unit.outcome.symbol_refs)?;
            } else {
                Self::delete_file_data(&tx, &unit.rel_path)?;
                Self::insert_file_data(&tx, unit)?;
            }
            wrote = true;
        }
        if !wrote {
            // 真正零变化：丢弃事务，不 bump epoch（保持快速路径零写入语义）。
            return Ok(());
        }
        crate::signature_agg::finish_path_update(&tx, &unit_paths, agg_update)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Append a config-link unit's refs without touching the file's parsed
    /// rows. Full-rebuild counterpart of the row-preserving branch in
    /// [`Self::apply_config_link_units`]: inside the rebuild closure the
    /// parsed unit was already inserted, so a second `insert_file_data`
    /// would violate the `files` primary key and lose the parsed chunks.
    pub fn insert_config_link_refs(conn: &Connection, unit: &FileWriteUnit) -> CcResult<()> {
        Self::insert_symbol_refs_on(conn, &unit.outcome.symbol_refs)
    }

    /// Delete the config-linker refs of `rel_path` (parser-produced refs
    /// untouched). Returns the number of deleted rows.
    fn delete_config_link_refs(conn: &Connection, rel_path: &str) -> CcResult<usize> {
        Self::execute_cached(
            conn,
            "DELETE FROM symbol_refs WHERE file_path = ?1 \
             AND ref_kind IN ('config_module','config_file','config_dependency')",
            rusqlite::params![rel_path],
        )
    }

    /// Insert symbol_refs rows (INSERT OR REPLACE on ref_id).
    fn insert_symbol_refs_on(
        conn: &Connection,
        refs: &[cc_model::symbol::SymbolRefRecord],
    ) -> CcResult<()> {
        for r in refs {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
            )?;
        }
        Ok(())
    }

    /// Update only the edge/resolution data for dirty (DirtyResolveOnly) files.
    /// Does NOT delete or modify: files row, chunks, FTS, route_nodes,
    /// http_call_edges, data_flow_edges, literals, file_frameworks,
    /// co_change_edges, test_edges.
    /// Only replaces: symbols, imports, call_edges, symbol_refs, semantic_edges,
    /// dispatch_sites, route_edges.
    pub(crate) fn replace_reresolved_edges_only(&self, units: &[FileWriteUnit]) -> CcResult<()> {
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let rel_paths: Vec<&str> = units.iter().map(|u| u.rel_path.as_str()).collect();
        let agg_update = crate::signature_agg::begin_path_update(&tx, &rel_paths)?;
        for file in units {
            Self::replace_reresolved_edges_for_file(&tx, file)?;
        }
        crate::signature_agg::finish_path_update(&tx, &rel_paths, agg_update)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Per-file body of [`Self::replace_reresolved_edges_only`], usable inside
    /// a caller-owned transaction.
    pub(crate) fn replace_reresolved_edges_for_file(
        tx: &Connection,
        file: &FileWriteUnit,
    ) -> CcResult<()> {
        {
            let rel = file.rel_path.as_str();
            let outcome = &file.outcome;

            // Delete only the re-resolvable tables
            for table in &[
                "call_edges",
                "symbol_refs",
                "symbols",
                "imports",
                "semantic_edges",
                "dispatch_sites",
                "routes",
            ] {
                Self::execute_cached(
                    tx,
                    &format!("DELETE FROM {} WHERE file_path = ?1", table),
                    rusqlite::params![rel],
                )?;
            }

            // Re-insert symbols
            for s in &outcome.symbols {
                Self::execute_cached(
                    tx,
                    "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                    rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
                )?;
            }

            // Re-insert imports
            for i in &outcome.imports {
                Self::execute_cached(
                    tx,
                    "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![i.file_path, i.import_string, i.resolved_path, i.imported_name, i.alias, i.is_namespace as i32, i.is_default as i32, i.is_reexport as i32],
                )?;
            }

            // Re-insert symbol_refs
            for r in &outcome.symbol_refs {
                Self::execute_cached(
                    tx,
                    "INSERT INTO symbol_refs(ref_id,file_path,symbol_name,container,ref_kind,line,column_no,target_symbol_id,target_file_path,target_symbol_uid,ref_name,resolution_kind,resolution_confidence,resolution_strategy,ref_end_line,ref_end_col,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    rusqlite::params![r.ref_id, r.file_path, r.symbol_name, r.container, r.ref_kind, r.line, r.column, r.target_symbol_id, r.target_file_path, r.target_symbol_uid, r.ref_name, r.resolution_kind.as_str(), r.resolution_confidence, r.resolution_strategy, r.ref_end_line, r.ref_end_col, r.parser_tier.as_str(), r.parser_confidence],
                )?;
            }

            // Re-insert call_edges
            for e in &outcome.call_edges {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                    rusqlite::params![e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol, e.line, e.start_col, e.end_line, e.end_col, e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id, e.caller_symbol_uid, e.callee_symbol_uid, e.dispatch_kind.as_str(), e.call_kind, e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy, e.receiver_expr, e.arg_count.map(|v| v as i32), e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32, e.parser_tier.as_str(), e.parser_confidence, e.synthesized_by, e.synthesis_key, e.registered_file, e.registered_line.map(|v| v as i32)],
                )?;
            }

            // Re-insert semantic_edges
            for se in &outcome.semantic_edges {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![se.edge_id, se.file_path, se.source_symbol, se.source_symbol_uid, se.target_symbol, se.target_symbol_uid, se.relation_kind.as_str(), se.line, se.confidence, se.parser_tier.as_str()],
                )?;
            }

            // Re-insert dispatch_sites
            for ds in &outcome.dispatch_sites {
                Self::execute_cached(
                    tx,
                    "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
                )?;
            }

            // Re-insert route_edges
            for r in &outcome.route_edges {
                Self::execute_cached(
                    tx,
                    "INSERT INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str(), r.resolution_strategy, r.resolution_confidence],
                )?;
            }
        }
        Ok(())
    }

    /// Write one incremental index batch atomically: file removals, full file
    /// replacements, dirty-file edge re-resolution, route nodes and the batch
    /// files' hierarchy edges share a single transaction, so a crash cannot
    /// leave files deleted with their edges still present — nor leave a batch
    /// file committed (content_hash persisted, so never re-batched) with its
    /// hierarchy edges missing (and the batch costs one WAL sync instead of
    /// four).
    ///
    /// Signature-aggregate contract: `hierarchy_edges` must belong to batch
    /// files (`file_path` within `normal_units`/`dirty_units`) — the
    /// path-scoped aggregate delta (see `signature_agg`) covers exactly the
    /// batch paths' rows.
    ///
    /// Returns the `symbols_seed` aggregate span observed inside the
    /// transaction, so seed-derived caches above cc-db (the resolver catalog
    /// cache) can prove their fold basis the same way the in-crate seed
    /// cache does.
    pub(crate) fn write_incremental_batch(
        &self,
        to_remove: &[String],
        normal_units: &[FileWriteUnit],
        dirty_units: &[FileWriteUnit],
        route_nodes: &[cc_model::edge::RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        precompressed: &PrecompressedChunks,
    ) -> CcResult<SeedTokenSpan> {
        if to_remove.is_empty()
            && normal_units.is_empty()
            && dirty_units.is_empty()
            && route_nodes.is_empty()
            && hierarchy_edges.is_empty()
        {
            // No-op batch: still make sure the signature-aggregate baseline
            // exists, so the postprocess gates stay O(1) on databases written
            // before the aggregates existed (one-time scan, then never again).
            self.ensure_signature_aggregates_initialized()?;
            return Ok(SeedTokenSpan {
                pre: None,
                post: None,
            });
        }
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        // Signature-aggregate maintenance: capture the touched paths' partial
        // aggregates before any delete (the whole batch is file-scoped, so
        // the delta against the post-state below covers every mutated row,
        // including the batch files' hierarchy edges).
        let touched_paths: Vec<&str> = {
            let mut seen = HashSet::new();
            to_remove
                .iter()
                .map(String::as_str)
                .chain(normal_units.iter().map(|f| f.rel_path.as_str()))
                .chain(dirty_units.iter().map(|f| f.rel_path.as_str()))
                .filter(|p| seen.insert(*p))
                .collect()
        };
        // Cache bases: the `symbols_seed` / `files_state` aggregates before
        // this batch's mutations. Re-read after `finish_path_update` below;
        // each pair lets the post-commit cache update prove its snapshot
        // matches the pre-batch table state (see `seed_symbol_cache` /
        // `file_state_cache`).
        let pre_aggs = crate::signature_agg::load_on(&tx)?;
        let pre_seed_agg = pre_aggs.map(|aggs| aggs.symbols_seed);
        let pre_files_agg = pre_aggs.map(|aggs| aggs.files_state);
        let agg_update = crate::signature_agg::begin_path_update(&tx, &touched_paths)?;
        // Per-section timing: emitted as `tracing::debug!` "sub-phase timing"
        // events (same field style as cc-index's `time_step`) so a slow
        // `write.incremental_batch` aggregate can be attributed from logs.
        fn section_ms(step: &'static str, count: usize, start: std::time::Instant) {
            tracing::debug!(
                phase = "write",
                step,
                count,
                elapsed_ms = start.elapsed().as_millis() as u64,
                "sub-phase timing"
            );
        }
        // One batched scan per FTS table for the whole batch (file_path is
        // UNINDEXED there, so per-file deletes would each scan the table).
        let section_start = std::time::Instant::now();
        Self::delete_files_fts_batch(
            &tx,
            to_remove
                .iter()
                .map(String::as_str)
                .chain(normal_units.iter().map(|f| f.rel_path.as_str())),
        )?;
        section_ms(
            "db_fts_delete",
            to_remove.len() + normal_units.len(),
            section_start,
        );
        let section_start = std::time::Instant::now();
        let remove_paths: Vec<&str> = to_remove.iter().map(String::as_str).collect();
        Self::delete_files_data_base_batch(&tx, &remove_paths)?;
        section_ms("db_remove_files", to_remove.len(), section_start);
        // Replacement keeps the path, so the path-derived test_edges
        // stay valid; only removals above cascade into test_edges.
        // Deletes run batched for the whole replacement set before any
        // insert: no inserted row is keyed by another batch file's path, so
        // the old per-file delete/insert interleaving carried no semantics.
        let section_start = std::time::Instant::now();
        let replace_paths: Vec<&str> = normal_units.iter().map(|f| f.rel_path.as_str()).collect();
        Self::delete_files_data_base_keep_test_edges_batch(&tx, &replace_paths)?;
        section_ms("db_replace_delete", normal_units.len(), section_start);
        let section_start = std::time::Instant::now();
        for file in normal_units {
            Self::insert_file_data_deferred_fts(
                &tx,
                file,
                precompressed.get(&file.rel_path).map(Vec::as_slice),
            )?;
        }
        Self::insert_files_literal_fts_batch(&tx, &replace_paths)?;
        section_ms("db_replace_insert", normal_units.len(), section_start);
        let section_start = std::time::Instant::now();
        for file in dirty_units {
            Self::replace_reresolved_edges_for_file(&tx, file)?;
        }
        section_ms("db_dirty_rewrite", dirty_units.len(), section_start);
        // Hierarchy edges for the batch files, inside the same transaction.
        // Must run after the dirty rewrite above: its per-file delete clears
        // each dirty file's semantic_edges rows, which include the hierarchy
        // edges being re-inserted here.
        let section_start = std::time::Instant::now();
        Self::insert_semantic_edges_batch_on(&tx, hierarchy_edges)?;
        section_ms("db_hierarchy_edges", hierarchy_edges.len(), section_start);
        let section_start = std::time::Instant::now();
        Self::insert_route_nodes_on(&tx, route_nodes)?;
        crate::signature_agg::finish_path_update(&tx, &touched_paths, agg_update)?;
        let post_aggs = crate::signature_agg::load_on(&tx)?;
        let post_seed_agg = post_aggs.map(|aggs| aggs.symbols_seed);
        let post_files_agg = post_aggs.map(|aggs| aggs.files_state);
        Self::bump_index_epoch_on(&tx)?;
        section_ms("db_routes_epoch", route_nodes.len(), section_start);
        let section_start = std::time::Instant::now();
        tx.commit().map_err(db_err)?;
        section_ms("db_commit", 0, section_start);
        // Committed: carry the seed and file-state caches across this batch
        // — the same file-scoped delta the transaction applied to `symbols`
        // and `files`.
        self.seed_cache_apply_batch(
            pre_seed_agg,
            post_seed_agg,
            to_remove,
            normal_units,
            dirty_units,
        );
        self.file_state_cache_apply_batch(pre_files_agg, post_files_agg, to_remove, normal_units);
        Ok(SeedTokenSpan {
            pre: pre_seed_agg,
            post: post_seed_agg,
        })
    }

    /// One-time baseline initialization for the graph-signature aggregates:
    /// when no stored baseline exists (database written before the aggregates
    /// did), recompute it from the table contents so the postprocess gates
    /// read O(1) metadata instead of falling back to full scans every build.
    fn ensure_signature_aggregates_initialized(&self) -> CcResult<()> {
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        if crate::signature_agg::load_on(&conn)?.is_some() {
            return Ok(());
        }
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let aggs = crate::signature_agg::scan_on(&tx)?;
        crate::signature_agg::store_on(&tx, &aggs)?;
        // Metadata only — index content is unchanged, so no epoch bump.
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Recompute and persist the graph-signature aggregate baseline from the
    /// connection's table contents. Full-rebuild writers call this as the
    /// last step inside the rebuild connection (temp-db / DirectWriter), so a
    /// rebuilt snapshot always carries a baseline matching its rows.
    pub fn recompute_graph_signature_aggregates_on(conn: &Connection) -> CcResult<()> {
        let aggs = crate::signature_agg::scan_on(conn)?;
        crate::signature_agg::store_on(conn, &aggs)
    }

    pub(crate) fn remove_files_batch(&self, paths: &[String]) -> CcResult<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;
        let rel_paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        let agg_update = crate::signature_agg::begin_path_update(&tx, &rel_paths)?;
        Self::delete_files_fts_batch(&tx, rel_paths.iter().copied())?;
        Self::delete_files_data_base_batch(&tx, &rel_paths)?;
        crate::signature_agg::finish_path_update(&tx, &rel_paths, agg_update)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(paths.len())
    }

    /// Delete every row owned by `rel_path` across content, FTS and edge tables.
    ///
    /// FTS dual-maintenance model: `symbols_fts` and `file_paths_fts` are kept
    /// in sync by schema triggers (the `DELETE FROM files` in
    /// [`Self::delete_file_data_base`] cascades into `symbols` via
    /// `ON DELETE CASCADE`, firing those triggers), while `chunks_fts`,
    /// `files_fts` and `literal_fts` have no triggers and MUST be deleted at
    /// the application layer — *before* the base rows, in the same
    /// transaction (the rowid-aligned FTS delete resolves rowids through the
    /// still-present base rows). Multi-file callers should batch the FTS half
    /// via [`Self::delete_files_fts_batch`] instead of calling this in a loop.
    pub(crate) fn delete_file_data(conn: &Connection, rel_path: &str) -> CcResult<()> {
        Self::delete_files_fts_batch(conn, std::iter::once(rel_path))?;
        Self::delete_file_data_base(conn, rel_path)
    }

    /// Delete the app-maintained FTS rows (`chunks_fts`, `files_fts`,
    /// `literal_fts`) for a set of files using chunked `IN (...)` statements.
    ///
    /// FTS rowids are aligned with their base-table rowids (schema v5, see
    /// `index_v1.sql`), so each delete resolves the doomed rowids through the
    /// base table's `file_path` index and removes the FTS rows by rowid —
    /// O(log n) per row instead of the full FTS-content-table scan that a
    /// DELETE on the UNINDEXED `file_path` column degrades to. The `IN (...)`
    /// list stays chunked at [`IN_BATCH_SIZE`] to respect
    /// SQLITE_MAX_VARIABLE_NUMBER.
    ///
    /// MUST run before the base-table rows are deleted (the rowid subquery
    /// needs them), in the same transaction as those deletes.
    pub(crate) fn delete_files_fts_batch<'p>(
        conn: &Connection,
        rel_paths: impl IntoIterator<Item = &'p str>,
    ) -> CcResult<()> {
        let rel_paths: Vec<&str> = rel_paths.into_iter().collect();
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            for (fts_table, base_table) in &[
                ("chunks_fts", "chunks"),
                ("files_fts", "files"),
                ("literal_fts", "literal_index"),
            ] {
                Self::execute_cached(
                    conn,
                    &format!(
                        "DELETE FROM {} WHERE rowid IN \
                         (SELECT rowid FROM {} WHERE file_path IN ({}))",
                        fts_table, base_table, placeholders
                    ),
                    rusqlite::params_from_iter(batch.iter()),
                )?;
            }
        }
        Ok(())
    }

    /// Base-table half of [`Self::delete_file_data`]: everything except the
    /// app-maintained FTS mirrors, which multi-file callers batch separately.
    pub(crate) fn delete_file_data_base(conn: &Connection, rel_path: &str) -> CcResult<()> {
        Self::delete_files_data_base_batch(conn, &[rel_path])
    }

    /// Batched [`Self::delete_file_data_base`]: one chunked `IN (...)` DELETE
    /// per table for the whole removal set instead of per-file statements.
    /// The `OR`-predicate tables (`test_edges`, `co_change_edges`) split into
    /// one DELETE per endpoint column so each runs on its own index.
    pub(crate) fn delete_files_data_base_batch(
        conn: &Connection,
        rel_paths: &[&str],
    ) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            for column in &["test_file_path", "code_file_path"] {
                Self::execute_cached(
                    conn,
                    &format!(
                        "DELETE FROM test_edges WHERE {} IN ({})",
                        column, placeholders
                    ),
                    rusqlite::params_from_iter(batch.iter()),
                )?;
            }
            Self::delete_files_data_chunk_keep_test_edges(conn, batch, &placeholders)?;
        }
        Ok(())
    }

    /// [`Self::delete_files_data_base_batch`] minus the test_edges cascade,
    /// for replace-in-place writers. Test edges are path-derived: their
    /// endpoints are file paths and the matching depends only on the path set
    /// plus the `is_test_file` flag, which every parser computes from the
    /// path alone — so deleting and re-inserting a file under the SAME path
    /// cannot change its test edges. New paths have no edges to delete
    /// (postprocess builds them), and removed paths go through
    /// [`Self::delete_files_data_base_batch`]. Chunked at [`IN_BATCH_SIZE`]
    /// like the FTS half (which MUST run first — see
    /// [`Self::delete_files_fts_batch`]).
    pub(crate) fn delete_files_data_base_keep_test_edges_batch(
        conn: &Connection,
        rel_paths: &[&str],
    ) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            Self::delete_files_data_chunk_keep_test_edges(conn, batch, &placeholders)?;
        }
        Ok(())
    }

    /// One `IN (...)` chunk of the keep-test-edges delete: every per-file
    /// DELETE the old loop issued, as one statement per table. The `files`
    /// DELETE still cascades per row into chunks/symbols/imports/symbol_refs/
    /// call_edges/literal_index and fires the `symbols_fts` /
    /// `file_paths_fts` triggers row-by-row, exactly as before — no table's
    /// rows reference another file in the batch, so the per-file interleaving
    /// order carried no semantics.
    fn delete_files_data_chunk_keep_test_edges(
        conn: &Connection,
        batch: &[&str],
        placeholders: &str,
    ) -> CcResult<()> {
        Self::execute_cached(
            conn,
            &format!(
                "DELETE FROM frameworks WHERE scope='file' AND scope_id IN ({})",
                placeholders
            ),
            rusqlite::params_from_iter(batch.iter()),
        )?;
        for table in &[
            "routes",
            "data_flow_edges",
            "http_call_edges",
            "semantic_edges",
            "dispatch_sites",
        ] {
            Self::execute_cached(
                conn,
                &format!(
                    "DELETE FROM {} WHERE file_path IN ({})",
                    table, placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        for column in &["file_a", "file_b"] {
            Self::execute_cached(
                conn,
                &format!(
                    "DELETE FROM co_change_edges WHERE {} IN ({})",
                    column, placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        // Explicitly batch-delete the FK-CASCADE children of `files` BEFORE
        // `DELETE FROM files`. SQLite fires ON DELETE CASCADE once per parent
        // row (one child-table DELETE per files row × per child table), which
        // on a multi-hundred-MB index measures 2–5× slower than one batched
        // `DELETE … WHERE file_path IN (…)` per child table over its
        // file_path index (50k 5% batch `db_replace_delete` ~24s → ~10s).
        // The index-maintenance work is identical either way; the saving is
        // the per-parent-row cascade statement/FK-check overhead.
        //
        // MAINTENANCE: this list must cover every table declared
        // `REFERENCES files(file_path) ON DELETE CASCADE` in index_v1.sql
        // (routes is deleted in the loop above). A future CASCADE child added
        // without an entry here would be orphaned after the files delete —
        // the schema/`epoch_rules` tests are the backstop.
        for table in &[
            "call_edges",
            "symbol_refs",
            "chunks",
            "imports",
            "literal_index",
            "symbols",
        ] {
            Self::execute_cached(
                conn,
                &format!(
                    "DELETE FROM {} WHERE file_path IN ({})",
                    table, placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        Self::execute_cached(
            conn,
            &format!("DELETE FROM files WHERE file_path IN ({})", placeholders),
            rusqlite::params_from_iter(batch.iter()),
        )?;
        Ok(())
    }

    /// Insert a single file's data into the given connection.
    /// Accepts `&Connection` so it works with both `Transaction` (via Deref)
    /// and bare connections (e.g. inside `rebuild_with_temp_db`).
    pub fn insert_file_data(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
        Self::insert_file_data_precompressed(conn, file, None)
    }

    /// [`Self::insert_file_data`] with optional pre-compressed chunk payloads
    /// (index-aligned with `outcome.chunks`, see [`PrecompressedChunks`]).
    /// Chunks without a side-car entry fall back to [`compress_chunk_text`]
    /// inside the transaction — same policy, identical on-disk bytes.
    pub fn insert_file_data_precompressed(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
    ) -> CcResult<()> {
        Self::insert_file_data_impl(conn, file, chunk_blobs, false)
    }

    /// [`Self::insert_file_data_precompressed`] minus the per-row `files_fts`
    /// / `literal_fts` mirror inserts, for multi-file writers that mirror
    /// those tables afterwards in one shot via
    /// [`Self::insert_files_literal_fts_batch`]. `chunks_fts` stays per-row in
    /// both modes: its base column may hold a zstd BLOB while FTS needs the
    /// plain text, so a SELECT-based mirror would require a decompression UDF
    /// for no measurable gain.
    ///
    /// Public for rebuild closures and the write-path micro-benchmark; always
    /// pair with [`Self::insert_files_literal_fts_batch`] in the same
    /// transaction.
    pub fn insert_file_data_deferred_fts(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
    ) -> CcResult<()> {
        Self::insert_file_data_impl(conn, file, chunk_blobs, true)
    }

    /// Mirror freshly inserted `files` / `literal_index` rows into their FTS
    /// tables by selecting straight from the base tables, one chunked
    /// `IN (...)` statement per table — rowid alignment by construction, no
    /// per-row `last_insert_rowid()` round-trips. Selecting from the base
    /// table also inherits the literal `OR IGNORE` first-wins semantics:
    /// only the surviving base rows exist to be mirrored.
    ///
    /// MUST run inside the same transaction after every base-row insert for
    /// `rel_paths`, and only over paths whose previous rows were deleted in
    /// this batch (otherwise pre-existing rows would be mirrored twice).
    pub fn insert_files_literal_fts_batch(conn: &Connection, rel_paths: &[&str]) -> CcResult<()> {
        for batch in rel_paths.chunks(IN_BATCH_SIZE) {
            let placeholders = sql_in_placeholders(batch.len());
            Self::execute_cached(
                conn,
                &format!(
                    "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) \
                     SELECT rowid,file_path,summary,content_excerpt FROM files \
                     WHERE file_path IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
            Self::execute_cached(
                conn,
                &format!(
                    "INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) \
                     SELECT rowid,literal_id,file_path,literal,literal_kind FROM literal_index \
                     WHERE file_path IN ({})",
                    placeholders
                ),
                rusqlite::params_from_iter(batch.iter()),
            )?;
        }
        Ok(())
    }

    fn insert_file_data_impl(
        conn: &Connection,
        file: &FileWriteUnit,
        chunk_blobs: Option<&[Option<Vec<u8>>]>,
        defer_files_literal_fts: bool,
    ) -> CcResult<()> {
        let outcome = &file.outcome;
        let now = chrono::Utc::now().to_rfc3339();
        let excerpt: String = outcome
            .chunks
            .iter()
            .take(3)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(20000)
            .collect();

        // files + files_fts (FTS rowid aligned with files.rowid; SQLite
        // resets last_insert_rowid after the file_paths_fts_ai trigger, so
        // it reliably names the files row here). Batch writers defer the
        // files_fts mirror to `insert_files_literal_fts_batch`.
        Self::execute_cached(
            conn,
            "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![file.rel_path, file.language.as_str(), file.content_hash, file.mtime, file.size as i64, outcome.summary, excerpt, outcome.parser_tier.as_str(), outcome.parser_confidence, outcome.is_test_file as i32, now],
        )?;
        if !defer_files_literal_fts {
            Self::execute_cached(
                conn,
                "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) VALUES(?1,?2,?3,?4)",
                rusqlite::params![conn.last_insert_rowid(), file.rel_path, outcome.summary, excerpt],
            )?;
        }

        // chunks + chunks_fts
        for (chunk_idx, c) in outcome.chunks.iter().enumerate() {
            // Compress chunk text with zstd when it saves space. Prefer the
            // payload pre-compressed during prepare (off the write lock);
            // fall back to compressing here for callers without a side-car.
            let fallback;
            let use_compressed: Option<&[u8]> = match chunk_blobs.and_then(|b| b.get(chunk_idx)) {
                Some(precomputed) => precomputed.as_deref(),
                None => {
                    fallback = compress_chunk_text(&c.text);
                    fallback.as_deref()
                }
            };
            if let Some(blob) = use_compressed {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), blob, "zstd", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            } else {
                Self::execute_cached(
                    conn,
                    "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,symbol_name,symbol_kind,text,text_encoding,token_estimate,parser_tier,parser_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    rusqlite::params![c.chunk_id, c.file_path, c.language.as_str(), c.chunk_index, c.start_line, c.end_line, c.breadcrumb, c.symbol_name, c.symbol_kind.map(|k| k.as_str().to_string()), c.text, "plain", c.token_estimate, c.parser_tier.as_str(), c.parser_confidence],
                )?;
            }
            // FTS always receives uncompressed text (rowid aligned with the
            // chunks row just inserted)
            Self::execute_cached(
                conn,
                "INSERT INTO chunks_fts(rowid,chunk_id,file_path,breadcrumb,symbol_name,text) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![conn.last_insert_rowid(), c.chunk_id, c.file_path, c.breadcrumb, c.symbol_name, c.text],
            )?;
        }

        // symbols
        for s in &outcome.symbols {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
                rusqlite::params![s.symbol_id, s.file_path, s.name, s.kind.as_str(), s.container, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.doc, s.parser_tier.as_str(), s.parser_confidence, s.qname, s.parent_symbol_id, s.export_name, s.is_default_export as i32, s.symbol_uid, s.framework_role, s.receiver_type, s.param_types, s.return_type, s.param_count, s.base_types, s.implements],
            )?;
        }

        // imports
        for i in &outcome.imports {
            Self::execute_cached(conn, "INSERT INTO imports(file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                rusqlite::params![i.file_path, i.import_string, i.resolved_path, i.imported_name, i.alias, i.is_namespace as i32, i.is_default as i32, i.is_reexport as i32],
            )?;
        }

        // symbol_refs
        Self::insert_symbol_refs_on(conn, &outcome.symbol_refs)?;

        // call_edges
        for e in &outcome.call_edges {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,start_col,end_line,end_col,target_symbol_id,target_file_path,caller_symbol_id,callee_ref_id,caller_symbol_uid,callee_symbol_uid,dispatch_kind,call_kind,resolution_kind,resolution_confidence,resolution_strategy,receiver_expr,arg_count,is_optional_chain,is_awaited,is_constructor,parser_tier,parser_confidence,synthesized_by,synthesis_key,registered_file,registered_line) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30)",
                rusqlite::params![e.edge_id, e.file_path, e.caller_symbol, e.callee_symbol, e.line, e.start_col, e.end_line, e.end_col, e.target_symbol_id, e.target_file_path, e.caller_symbol_id, e.callee_ref_id, e.caller_symbol_uid, e.callee_symbol_uid, e.dispatch_kind.as_str(), e.call_kind, e.resolution_kind.as_str(), e.resolution_confidence, e.resolution_strategy, e.receiver_expr, e.arg_count.map(|v| v as i32), e.is_optional_chain as i32, e.is_awaited as i32, e.is_constructor as i32, e.parser_tier.as_str(), e.parser_confidence, e.synthesized_by, e.synthesis_key, e.registered_file, e.registered_line.map(|v| v as i32)],
            )?;
        }

        // test_edges
        for t in &outcome.test_edges {
            Self::execute_cached(conn, "INSERT OR IGNORE INTO test_edges(edge_id,test_file_path,code_file_path,reason,confidence) VALUES(?1,?2,?3,?4,?5)",
                rusqlite::params![t.edge_id, t.test_file_path, t.code_file_path, t.reason, t.confidence],
            )?;
        }

        // route_edges
        for r in &outcome.route_edges {
            Self::execute_cached(conn, "INSERT OR REPLACE INTO routes(edge_id,file_path,route_path,handler_name,method,line,start_col,end_line,end_col,handler_symbol_id,handler_symbol_uid,handler_expr,router_symbol_uid,framework,route_kind,confidence,parser_tier,resolution_strategy,resolution_confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                rusqlite::params![r.edge_id, r.file_path, r.route_path, r.handler_name, r.method, r.line, r.start_col, r.end_line, r.end_col, r.handler_symbol_id, r.handler_symbol_uid, r.handler_expr, r.router_symbol_uid, r.framework, r.route_kind, r.confidence, r.parser_tier.as_str(), r.resolution_strategy, r.resolution_confidence],
            )?;
        }

        // http_call_edges
        for hce in &outcome.http_call_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO http_call_edges(edge_id,file_path,caller_symbol_uid,url_or_path,normalized_path,method,call_kind,line,confidence,parser_tier,broker_type) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![hce.edge_id, hce.file_path, hce.caller_symbol_uid, hce.url_or_path, hce.normalized_path, hce.method, hce.call_kind, hce.line, hce.confidence, hce.parser_tier.as_str(), hce.broker_type],
            )?;
        }

        // literal_index + literal_fts (FTS rowid aligned with the base row).
        // OR IGNORE instead of the previous OR REPLACE: a REPLACE would give
        // the surviving base row a fresh rowid and orphan the FTS row written
        // for the first occurrence. literal_id is derived from
        // (file_path,line,col), so a conflict can only be a duplicate
        // extraction of the same literal within this outcome — first one wins
        // and the duplicate is skipped on both sides. Batch writers defer the
        // FTS mirror; selecting from the base table preserves first-wins.
        for l in &outcome.literal_index {
            let inserted = Self::execute_cached(conn, "INSERT OR IGNORE INTO literal_index(literal_id,file_path,literal,literal_kind,line,container,confidence,enclosing_symbol_uid,key_path) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![l.literal_id, l.file_path, l.literal, l.literal_kind, l.line, l.container, l.confidence, l.enclosing_symbol_uid, l.key_path],
            )?;
            if !defer_files_literal_fts && inserted > 0 {
                Self::execute_cached(conn, "INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4,?5)", rusqlite::params![conn.last_insert_rowid(), l.literal_id, l.file_path, l.literal, l.literal_kind])?;
            }
        }

        // semantic_edges
        for se in &outcome.semantic_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind,line,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![se.edge_id, se.file_path, se.source_symbol, se.source_symbol_uid, se.target_symbol, se.target_symbol_uid, se.relation_kind.as_str(), se.line, se.confidence, se.parser_tier.as_str()],
            )?;
        }

        // data_flow_edges
        for dfe in &outcome.data_flow_edges {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO data_flow_edges(edge_id,file_path,source_symbol_uid,target_symbol_uid,flow_kind,line,confidence,parser_tier,env_key) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![dfe.edge_id, dfe.file_path, dfe.source_symbol_uid, dfe.target_symbol_uid, dfe.flow_kind, dfe.line, dfe.confidence, dfe.parser_tier.as_str(), dfe.env_key],
            )?;
        }

        // dispatch_sites
        for ds in &outcome.dispatch_sites {
            Self::execute_cached(
                conn,
                "INSERT OR REPLACE INTO dispatch_sites(site_id,file_path,line,col,enclosing_symbol_uid,receiver_expr,site_kind,key,handler_expr,handler_symbol_uid,confidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                rusqlite::params![ds.site_id, ds.file_path, ds.line, ds.col, ds.enclosing_symbol_uid, ds.receiver_expr, ds.site_kind.as_str(), ds.key, ds.handler_expr, ds.handler_symbol_uid, ds.confidence],
            )?;
        }

        Ok(())
    }

    /// Insert a single route node into the given connection.
    pub fn insert_route_node_into(conn: &Connection, r: &RouteNodeRecord) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT OR REPLACE INTO routes(route_id,file_path,route_path,method,handler_symbol_uid,handler_name,framework,line,end_line,normalized_path,confidence,parser_tier) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![r.route_id, r.file_path, r.route_path, r.method, r.handler_symbol_uid, r.handler_name, r.framework, r.line, r.end_line, r.normalized_path, r.confidence, r.parser_tier.as_str()],
        )?;
        Ok(())
    }

    /// Set a metadata key=value on the given connection.
    pub fn set_metadata_on(conn: &Connection, key: &str, value: &str) -> CcResult<()> {
        Self::execute_cached(
            conn,
            "INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }
}
