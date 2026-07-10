//! Commutative graph-signature aggregates: O(batch) maintenance of the
//! postprocess gate inputs.
//!
//! The dispatch/interface/community signature gates (cc-index `pass_gate`)
//! historically recomputed their input signatures by scanning four whole
//! tables every build — an O(repo) decision cost paid even when the batch
//! changed a single file and the gate ultimately decided to skip. This module
//! replaces the scan with a multiset-homomorphic aggregate per input group:
//!
//! - per row: a SipHash (`DefaultHasher`, fixed key — deterministic across
//!   processes) over the row's signature columns;
//! - per group: `(count, wrapping sum of row hashes)` — a commutative monoid,
//!   so rows can be added and removed in any order and two row multisets are
//!   equal iff their aggregates are equal (up to 64-bit collision odds, same
//!   class of guarantee the previous sequential hash provided).
//!
//! The first six groups mirror exactly what the gates hash (filters
//! included); `symbols_seed` validates the cross-build resolver seed cache
//! (see `crate::seed_symbol_cache`); `files_state` validates the
//! cross-build file-state snapshot (see `crate::file_state_cache`):
//!
//! | group              | table          | rows                              | columns                                              |
//! |--------------------|----------------|-----------------------------------|------------------------------------------------------|
//! | `symbols_full`     | symbols        | `symbol_uid IS NOT NULL`          | symbol_uid, name, kind, container                    |
//! | `symbols_community`| symbols        | `symbol_uid IS NOT NULL`          | symbol_uid, name, kind                               |
//! | `call_real`        | call_edges     | both uids set, not synthesized    | caller_symbol_uid, callee_symbol_uid                 |
//! | `call_synthetic`   | call_edges     | both uids set, synthesized        | caller_symbol_uid, callee_symbol_uid                 |
//! | `semantic_real`    | semantic_edges | `edge_id NOT LIKE 'synth:%'`      | source_symbol_uid, target_symbol_uid, relation_kind  |
//! | `dispatch_sites`   | dispatch_sites | all                               | site_kind, key, file_path, enclosing/handler uid, line |
//! | `symbols_seed`     | symbols        | all                               | the 15 resolver-seed columns (`SEED_COLUMNS` in `index_db_query`) |
//! | `files_state`      | files          | all                               | file_path, content_hash, mtime, size (the scan-diff projection) |
//!
//! NULL (or non-TEXT) values hash as `""` in the gate groups, matching the
//! previous scans' `as_str().unwrap_or("")` extraction. `symbols_seed`
//! hashes `Option` values instead (NULL distinct from `''`), because its
//! consumer caches actual row content, not just a change signal.
//!
//! Maintenance contract — every write path that mutates the columns above
//! must keep the stored aggregates in sync, inside the same transaction:
//!
//! - file-scoped writers (incremental batch, file replace/remove, dirty
//!   re-resolution, config-link units, per-file semantic/dispatch rewrites)
//!   use [`begin_path_update`]/[`finish_path_update`]: capture the touched
//!   paths' partial aggregates before and after the mutation and apply the
//!   difference — O(batch rows) via the `file_path` indexes, immune to
//!   `INSERT OR REPLACE` collisions within the touched path set;
//! - kind-scoped synthetic call-edge writers adjust `call_synthetic`
//!   directly ([`synthetic_kind_agg_on`] before a delete-by-kind,
//!   [`adjust_call_edge_upsert`] per inserted edge);
//! - full rebuilds recompute the baseline from the final table contents
//!   ([`scan_on`] + [`store_on`]) as the last step inside the rebuild
//!   connection, so a rebuilt snapshot can never carry a stale baseline.
//!
//! When no stored aggregates exist (database written before this module, or
//! fixtures seeded with raw SQL), maintenance no-ops / rebuilds the baseline
//! and readers fall back to [`scan_on`] — the historical O(repo) cost, never
//! a wrong value. The serialized format carries [`FORMAT_VERSION`]; bumping
//! it invalidates stored aggregates and forces one baseline rebuild.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use rusqlite::{Connection, OptionalExtension};

use cc_model::CcResult;

use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

/// Metadata key holding the serialized aggregates.
pub(crate) const GRAPH_SIG_AGG_KEY: &str = "graph_sig_aggregates";

/// Serialized-format version. Bump when the row-hash formula, column sets or
/// group layout change; stored aggregates from other versions read as absent.
/// Version "2": added the `symbols_seed` group.
/// Version "3": added the `files_state` group.
const FORMAT_VERSION: &str = "3";

/// One group's aggregate: row count plus wrapping sum of per-row hashes.
/// Equal multisets of rows produce equal aggregates; add/remove commute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RowAgg {
    pub count: u64,
    pub sum: u64,
}

impl RowAgg {
    pub fn add_row(&mut self, row_hash: u64) {
        self.count = self.count.wrapping_add(1);
        self.sum = self.sum.wrapping_add(row_hash);
    }

    pub fn remove_row(&mut self, row_hash: u64) {
        self.count = self.count.wrapping_sub(1);
        self.sum = self.sum.wrapping_sub(row_hash);
    }

    /// Multiset union of two aggregates.
    pub fn merged(&self, other: &RowAgg) -> RowAgg {
        RowAgg {
            count: self.count.wrapping_add(other.count),
            sum: self.sum.wrapping_add(other.sum),
        }
    }

    /// Multiset difference (`other` must be a sub-multiset for the result to
    /// be meaningful; wrapping arithmetic keeps the operation total).
    pub fn minus(&self, other: &RowAgg) -> RowAgg {
        RowAgg {
            count: self.count.wrapping_sub(other.count),
            sum: self.sum.wrapping_sub(other.sum),
        }
    }

    /// Feed this aggregate into a gate-signature hasher.
    pub fn hash_into(&self, hasher: &mut DefaultHasher) {
        self.count.hash(hasher);
        self.sum.hash(hasher);
    }
}

/// The eight maintained aggregates (see module docs for the group table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GraphSignatureAggregates {
    pub symbols_full: RowAgg,
    pub symbols_community: RowAgg,
    pub call_real: RowAgg,
    pub call_synthetic: RowAgg,
    pub semantic_real: RowAgg,
    pub dispatch_sites: RowAgg,
    pub symbols_seed: RowAgg,
    pub files_state: RowAgg,
}

impl GraphSignatureAggregates {
    fn groups(&self) -> [RowAgg; 8] {
        [
            self.symbols_full,
            self.symbols_community,
            self.call_real,
            self.call_synthetic,
            self.semantic_real,
            self.dispatch_sites,
            self.symbols_seed,
            self.files_state,
        ]
    }

    /// `stored - pre + post` per group: the path-scoped delta application.
    fn apply_delta(&mut self, pre: &Self, post: &Self) {
        let apply = |stored: &mut RowAgg, pre: RowAgg, post: RowAgg| {
            *stored = stored.minus(&pre).merged(&post);
        };
        apply(&mut self.symbols_full, pre.symbols_full, post.symbols_full);
        apply(
            &mut self.symbols_community,
            pre.symbols_community,
            post.symbols_community,
        );
        apply(&mut self.call_real, pre.call_real, post.call_real);
        apply(
            &mut self.call_synthetic,
            pre.call_synthetic,
            post.call_synthetic,
        );
        apply(
            &mut self.semantic_real,
            pre.semantic_real,
            post.semantic_real,
        );
        apply(
            &mut self.dispatch_sites,
            pre.dispatch_sites,
            post.dispatch_sites,
        );
        apply(&mut self.symbols_seed, pre.symbols_seed, post.symbols_seed);
        apply(&mut self.files_state, pre.files_state, post.files_state);
    }

    fn serialize(&self) -> String {
        let mut out = String::from(FORMAT_VERSION);
        for group in self.groups() {
            out.push('|');
            out.push_str(&group.count.to_string());
            out.push(',');
            out.push_str(&group.sum.to_string());
        }
        out
    }

    fn deserialize(value: &str) -> Option<Self> {
        let mut parts = value.split('|');
        if parts.next()? != FORMAT_VERSION {
            return None;
        }
        let mut group = || -> Option<RowAgg> {
            let (count, sum) = parts.next()?.split_once(',')?;
            Some(RowAgg {
                count: count.parse().ok()?,
                sum: sum.parse().ok()?,
            })
        };
        let aggs = Self {
            symbols_full: group()?,
            symbols_community: group()?,
            call_real: group()?,
            call_synthetic: group()?,
            semantic_real: group()?,
            dispatch_sites: group()?,
            symbols_seed: group()?,
            files_state: group()?,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(aggs)
    }
}

// ── Per-row hash functions ───────────────────────────────────────────

fn text(value: Option<&str>) -> &str {
    value.unwrap_or("")
}

fn hash_symbol_full(
    uid: Option<&str>,
    name: Option<&str>,
    kind: Option<&str>,
    container: Option<&str>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    text(uid).hash(&mut hasher);
    text(name).hash(&mut hasher);
    text(kind).hash(&mut hasher);
    text(container).hash(&mut hasher);
    hasher.finish()
}

fn hash_symbol_community(uid: Option<&str>, name: Option<&str>, kind: Option<&str>) -> u64 {
    let mut hasher = DefaultHasher::new();
    text(uid).hash(&mut hasher);
    text(name).hash(&mut hasher);
    text(kind).hash(&mut hasher);
    hasher.finish()
}

/// Per-row hash over the 15 resolver-seed columns, in `SEED_COLUMNS` order
/// (see `index_db_query::resolver_seed_symbols_excluding`). Text columns
/// hash as `Option` (NULL distinct from `''`) because the seed cache serves
/// row *content*: two states this hash cannot distinguish must materialize
/// identical seed rows.
fn hash_symbol_seed(texts: &[Option<&str>; 11], ints: &[i64; 3], param_count: Option<i64>) -> u64 {
    let mut hasher = DefaultHasher::new();
    for value in texts {
        value.hash(&mut hasher);
    }
    for value in ints {
        value.hash(&mut hasher);
    }
    param_count.hash(&mut hasher);
    hasher.finish()
}

/// Per-row hash over the `files` table's scan-diff projection. `mtime`
/// hashes by IEEE bit pattern (the write path binds an `f64` and the read
/// path compares `f64`s, so bit equality is the correct equivalence).
/// Crate-visible so the file-state cache projects written units identically.
pub(crate) fn hash_file_state(
    file_path: &str,
    content_hash: Option<&str>,
    mtime: f64,
    size: i64,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    text(content_hash).hash(&mut hasher);
    mtime.to_bits().hash(&mut hasher);
    size.hash(&mut hasher);
    hasher.finish()
}

/// Per-row hash of a call edge's `(caller_symbol_uid, callee_symbol_uid)`
/// pair. Public so cc-index can fold a staged synthesis round's in-memory
/// inserts into the projected community aggregate.
pub fn hash_call_uid_pair(caller: &str, callee: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    caller.hash(&mut hasher);
    callee.hash(&mut hasher);
    hasher.finish()
}

fn hash_semantic_row(
    source_uid: Option<&str>,
    target_uid: Option<&str>,
    relation_kind: Option<&str>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    text(source_uid).hash(&mut hasher);
    text(target_uid).hash(&mut hasher);
    text(relation_kind).hash(&mut hasher);
    hasher.finish()
}

#[allow(clippy::too_many_arguments)]
fn hash_dispatch_row(
    site_kind: Option<&str>,
    key: Option<&str>,
    file_path: Option<&str>,
    enclosing_uid: Option<&str>,
    handler_uid: Option<&str>,
    line: i64,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    text(site_kind).hash(&mut hasher);
    text(key).hash(&mut hasher);
    text(file_path).hash(&mut hasher);
    text(enclosing_uid).hash(&mut hasher);
    text(handler_uid).hash(&mut hasher);
    line.hash(&mut hasher);
    hasher.finish()
}

// ── Scans (full + path-scoped) ───────────────────────────────────────

/// Read a column as TEXT: NULL or non-text storage reads as `None`, mirroring
/// the historical `query_json` + `as_str()` extraction.
fn col_text(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    Ok(match row.get_ref(idx)? {
        rusqlite::types::ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        _ => None,
    })
}

fn col_i64(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<i64> {
    Ok(match row.get_ref(idx)? {
        rusqlite::types::ValueRef::Integer(n) => n,
        _ => 0,
    })
}

/// Read a REAL column leniently (INTEGER-affinity rows read as their float
/// value, matching how the file-state consumer reads `mtime` via `f64`).
fn col_f64(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<f64> {
    Ok(match row.get_ref(idx)? {
        rusqlite::types::ValueRef::Real(r) => r,
        rusqlite::types::ValueRef::Integer(n) => n as f64,
        _ => 0.0,
    })
}

/// Read an integer column preserving NULL: the seed projection distinguishes
/// `param_count` NULL from 0.
fn col_opt_i64(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<i64>> {
    Ok(match row.get_ref(idx)? {
        rusqlite::types::ValueRef::Integer(n) => Some(n),
        _ => None,
    })
}

/// Fold the rows of one table (optionally scoped to a path chunk) into the
/// aggregates. `scope` is `Some((placeholders, params))` for path-scoped
/// captures, `None` for full scans.
fn fold_table(
    conn: &Connection,
    aggs: &mut GraphSignatureAggregates,
    table: Table,
    paths: Option<&[&str]>,
) -> CcResult<()> {
    let (base_sql, scoped_clause) = table.sql();
    match paths {
        None => fold_query(conn, aggs, table, &base_sql.replace("{scope}", ""), &[]),
        Some(paths) => {
            for chunk in paths.chunks(IN_BATCH_SIZE) {
                let clause = scoped_clause.replace("{in}", &sql_in_placeholders(chunk.len()));
                fold_query(
                    conn,
                    aggs,
                    table,
                    &base_sql.replace("{scope}", &clause),
                    chunk,
                )?;
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
enum Table {
    Symbols,
    CallEdges,
    SemanticEdges,
    DispatchSites,
    Files,
}

impl Table {
    /// `(base SQL with a `{scope}` hole, scope clause with an `{in}` hole)`.
    fn sql(self) -> (&'static str, &'static str) {
        match self {
            Table::Symbols => (
                // All rows, all seed columns: the `symbols_seed` group covers
                // every row; the gate groups keep their historical
                // `symbol_uid IS NOT NULL` filter, applied in the fold.
                "SELECT symbol_uid, name, kind, container, symbol_id, file_path, qname, \
                 export_name, receiver_type, base_types, implements, start_line, end_line, \
                 is_default_export, param_count FROM symbols{scope}",
                " WHERE file_path IN ({in})",
            ),
            Table::CallEdges => (
                "SELECT caller_symbol_uid, callee_symbol_uid, synthesized_by FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL{scope}",
                " AND file_path IN ({in})",
            ),
            Table::SemanticEdges => (
                "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
                 WHERE edge_id NOT LIKE 'synth:%'{scope}",
                " AND file_path IN ({in})",
            ),
            Table::DispatchSites => (
                "SELECT site_kind, key, file_path, enclosing_symbol_uid, handler_symbol_uid, line \
                 FROM dispatch_sites{scope}",
                " WHERE file_path IN ({in})",
            ),
            Table::Files => (
                "SELECT file_path, content_hash, mtime, size FROM files{scope}",
                " WHERE file_path IN ({in})",
            ),
        }
    }
}

fn fold_query(
    conn: &Connection,
    aggs: &mut GraphSignatureAggregates,
    table: Table,
    sql: &str,
    params: &[&str],
) -> CcResult<()> {
    let mut stmt = conn.prepare_cached(sql).map_err(db_err)?;
    let mut rows = stmt
        .query(rusqlite::params_from_iter(params.iter()))
        .map_err(db_err)?;
    while let Some(row) = rows.next().map_err(db_err)? {
        match table {
            Table::Symbols => {
                let uid = col_text(row, 0).map_err(db_err)?;
                let name = col_text(row, 1).map_err(db_err)?;
                let kind = col_text(row, 2).map_err(db_err)?;
                let container = col_text(row, 3).map_err(db_err)?;
                // Gate groups keep the historical `symbol_uid IS NOT NULL`
                // row filter (previously in the SQL).
                if uid.is_some() {
                    aggs.symbols_full.add_row(hash_symbol_full(
                        uid.as_deref(),
                        name.as_deref(),
                        kind.as_deref(),
                        container.as_deref(),
                    ));
                    aggs.symbols_community.add_row(hash_symbol_community(
                        uid.as_deref(),
                        name.as_deref(),
                        kind.as_deref(),
                    ));
                }
                let symbol_id = col_text(row, 4).map_err(db_err)?;
                let file_path = col_text(row, 5).map_err(db_err)?;
                let qname = col_text(row, 6).map_err(db_err)?;
                let export_name = col_text(row, 7).map_err(db_err)?;
                let receiver_type = col_text(row, 8).map_err(db_err)?;
                let base_types = col_text(row, 9).map_err(db_err)?;
                let implements = col_text(row, 10).map_err(db_err)?;
                let start_line = col_i64(row, 11).map_err(db_err)?;
                let end_line = col_i64(row, 12).map_err(db_err)?;
                let is_default_export = col_i64(row, 13).map_err(db_err)?;
                let param_count = col_opt_i64(row, 14).map_err(db_err)?;
                aggs.symbols_seed.add_row(hash_symbol_seed(
                    &[
                        symbol_id.as_deref(),
                        file_path.as_deref(),
                        name.as_deref(),
                        kind.as_deref(),
                        container.as_deref(),
                        qname.as_deref(),
                        export_name.as_deref(),
                        uid.as_deref(),
                        receiver_type.as_deref(),
                        base_types.as_deref(),
                        implements.as_deref(),
                    ],
                    &[start_line, end_line, is_default_export],
                    param_count,
                ));
            }
            Table::CallEdges => {
                let caller = col_text(row, 0).map_err(db_err)?;
                let callee = col_text(row, 1).map_err(db_err)?;
                let synthesized = col_text(row, 2).map_err(db_err)?;
                let hash = hash_call_uid_pair(text(caller.as_deref()), text(callee.as_deref()));
                if synthesized.is_some() {
                    aggs.call_synthetic.add_row(hash);
                } else {
                    aggs.call_real.add_row(hash);
                }
            }
            Table::SemanticEdges => {
                let source = col_text(row, 0).map_err(db_err)?;
                let target = col_text(row, 1).map_err(db_err)?;
                let kind = col_text(row, 2).map_err(db_err)?;
                aggs.semantic_real.add_row(hash_semantic_row(
                    source.as_deref(),
                    target.as_deref(),
                    kind.as_deref(),
                ));
            }
            Table::DispatchSites => {
                let site_kind = col_text(row, 0).map_err(db_err)?;
                let key = col_text(row, 1).map_err(db_err)?;
                let file_path = col_text(row, 2).map_err(db_err)?;
                let enclosing = col_text(row, 3).map_err(db_err)?;
                let handler = col_text(row, 4).map_err(db_err)?;
                let line = col_i64(row, 5).map_err(db_err)?;
                aggs.dispatch_sites.add_row(hash_dispatch_row(
                    site_kind.as_deref(),
                    key.as_deref(),
                    file_path.as_deref(),
                    enclosing.as_deref(),
                    handler.as_deref(),
                    line,
                ));
            }
            Table::Files => {
                let file_path = col_text(row, 0).map_err(db_err)?;
                let content_hash = col_text(row, 1).map_err(db_err)?;
                let mtime = col_f64(row, 2).map_err(db_err)?;
                let size = col_i64(row, 3).map_err(db_err)?;
                aggs.files_state.add_row(hash_file_state(
                    text(file_path.as_deref()),
                    content_hash.as_deref(),
                    mtime,
                    size,
                ));
            }
        }
    }
    Ok(())
}

/// Recompute all aggregates from the table contents (the ground truth).
/// O(repo); used for rebuild baselines, missing-baseline initialization and
/// the read-side fallback.
pub(crate) fn scan_on(conn: &Connection) -> CcResult<GraphSignatureAggregates> {
    let mut aggs = GraphSignatureAggregates::default();
    for table in [
        Table::Symbols,
        Table::CallEdges,
        Table::SemanticEdges,
        Table::DispatchSites,
        Table::Files,
    ] {
        fold_table(conn, &mut aggs, table, None)?;
    }
    Ok(aggs)
}

/// Partial aggregates over the rows owned by `paths` (indexed `file_path`
/// lookups, chunked IN lists). O(rows of those paths).
fn capture_paths_on(conn: &Connection, paths: &[&str]) -> CcResult<GraphSignatureAggregates> {
    let mut aggs = GraphSignatureAggregates::default();
    if paths.is_empty() {
        return Ok(aggs);
    }
    for table in [
        Table::Symbols,
        Table::CallEdges,
        Table::SemanticEdges,
        Table::DispatchSites,
        Table::Files,
    ] {
        fold_table(conn, &mut aggs, table, Some(paths))?;
    }
    Ok(aggs)
}

/// `call_synthetic` partial aggregate for the given `synthesized_by` kinds.
/// Used before delete-by-kind and by the community gate's overlay projection.
pub(crate) fn synthetic_kind_agg_on(conn: &Connection, kinds: &[&str]) -> CcResult<RowAgg> {
    let mut agg = RowAgg::default();
    if kinds.is_empty() {
        return Ok(agg);
    }
    for chunk in kinds.chunks(IN_BATCH_SIZE) {
        let sql = format!(
            "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
             WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
             AND synthesized_by IN ({})",
            sql_in_placeholders(chunk.len())
        );
        let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(chunk.iter()))
            .map_err(db_err)?;
        while let Some(row) = rows.next().map_err(db_err)? {
            let caller = col_text(row, 0).map_err(db_err)?;
            let callee = col_text(row, 1).map_err(db_err)?;
            agg.add_row(hash_call_uid_pair(
                text(caller.as_deref()),
                text(callee.as_deref()),
            ));
        }
    }
    Ok(agg)
}

// ── Persistence ──────────────────────────────────────────────────────

/// Stored aggregates, or `None` when absent / unparseable / from another
/// format version (all three read as "no baseline"). A failed READ is an
/// `Err`, not "no baseline": maintenance call sites run inside the writing
/// transaction, so propagating rolls the row writes back together with the
/// skipped aggregate update instead of silently misclassifying the failure
/// as a missing baseline (which would trigger a full rebuild or a wrong
/// no-op skip).
pub(crate) fn load_on(conn: &Connection) -> CcResult<Option<GraphSignatureAggregates>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            rusqlite::params![GRAPH_SIG_AGG_KEY],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_err)?;
    Ok(value
        .as_deref()
        .and_then(GraphSignatureAggregates::deserialize))
}

pub(crate) fn store_on(conn: &Connection, aggs: &GraphSignatureAggregates) -> CcResult<()> {
    crate::index_db::IndexDb::set_metadata_on(conn, GRAPH_SIG_AGG_KEY, &aggs.serialize())
}

// ── Path-scoped update protocol ──────────────────────────────────────

/// Pre-state of a path-scoped write. `Rebuild` when no stored baseline
/// exists: the writer then recomputes the whole baseline at finish time
/// instead of applying a delta against nothing.
pub(crate) enum PathScopedUpdate {
    Delta(GraphSignatureAggregates),
    Rebuild,
}

/// Call before mutating any rows of `paths`. Must run inside the same
/// transaction as the mutation (the capture must observe pre-delete state).
///
/// Writers should open that transaction with
/// `TransactionBehavior::Immediate`: the baseline read here precedes the
/// first write, and a DEFERRED transaction's later read→write upgrade can
/// fail with `SQLITE_BUSY_SNAPSHOT` under a cross-process writer (the
/// in-process `write_conn` mutex only serializes this process).
pub(crate) fn begin_path_update(conn: &Connection, paths: &[&str]) -> CcResult<PathScopedUpdate> {
    match load_on(conn)? {
        None => Ok(PathScopedUpdate::Rebuild),
        Some(_) => Ok(PathScopedUpdate::Delta(capture_paths_on(conn, paths)?)),
    }
}

/// Call after the mutation, before the transaction commits. The mutation must
/// not have touched signature-relevant rows outside `paths` (file-scoped
/// writers satisfy this by construction; `INSERT OR REPLACE` within the path
/// set is covered because both captures read final table state).
pub(crate) fn finish_path_update(
    conn: &Connection,
    paths: &[&str],
    update: PathScopedUpdate,
) -> CcResult<()> {
    match update {
        PathScopedUpdate::Rebuild => store_on(conn, &scan_on(conn)?),
        PathScopedUpdate::Delta(pre) => {
            let post = capture_paths_on(conn, paths)?;
            // The baseline existed at begin time and only this transaction
            // mutates it; a vanished baseline degrades to a fresh scan.
            match load_on(conn)? {
                Some(mut stored) => {
                    stored.apply_delta(&pre, &post);
                    store_on(conn, &stored)
                }
                None => store_on(conn, &scan_on(conn)?),
            }
        }
    }
}

// ── Row-scoped adjustments (synthetic call edges, semantic upserts) ──

/// Adjust in-memory aggregates for one `INSERT OR REPLACE` call edge: the row
/// currently stored under the same `edge_id` (if any) leaves its bucket, the
/// incoming row enters its bucket. Call BEFORE executing the insert.
pub(crate) fn adjust_call_edge_upsert(
    conn: &Connection,
    aggs: &mut GraphSignatureAggregates,
    edge: &cc_model::CallEdgeRecord,
) -> CcResult<()> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT caller_symbol_uid, callee_symbol_uid, synthesized_by FROM call_edges \
             WHERE edge_id = ?1",
        )
        .map_err(db_err)?;
    let old = stmt
        .query_row(rusqlite::params![edge.edge_id], |row| {
            Ok((col_text(row, 0)?, col_text(row, 1)?, col_text(row, 2)?))
        })
        .optional()
        .map_err(db_err)?;
    if let Some((caller, callee, synthesized)) = old {
        if let (Some(caller), Some(callee)) = (caller, callee) {
            let hash = hash_call_uid_pair(&caller, &callee);
            if synthesized.is_some() {
                aggs.call_synthetic.remove_row(hash);
            } else {
                aggs.call_real.remove_row(hash);
            }
        }
    }
    if let (Some(caller), Some(callee)) = (&edge.caller_symbol_uid, &edge.callee_symbol_uid) {
        let hash = hash_call_uid_pair(caller, callee);
        if edge.synthesized_by.is_some() {
            aggs.call_synthetic.add_row(hash);
        } else {
            aggs.call_real.add_row(hash);
        }
    }
    Ok(())
}

/// Adjust in-memory aggregates for one `INSERT OR REPLACE` semantic edge.
/// Synthetic ids (`synth:%`) are outside every aggregate and skip entirely
/// (a synthetic id can never replace a real row — the id IS the key). Call
/// BEFORE executing the insert.
pub(crate) fn adjust_semantic_edge_upsert(
    conn: &Connection,
    aggs: &mut GraphSignatureAggregates,
    edge: &cc_model::edge::SemanticEdgeRecord,
) -> CcResult<()> {
    if edge.edge_id.starts_with("synth:") {
        return Ok(());
    }
    let mut stmt = conn
        .prepare_cached(
            "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
             WHERE edge_id = ?1",
        )
        .map_err(db_err)?;
    if let Some((source, target, kind)) = stmt
        .query_row(rusqlite::params![edge.edge_id], |row| {
            Ok((col_text(row, 0)?, col_text(row, 1)?, col_text(row, 2)?))
        })
        .optional()
        .map_err(db_err)?
    {
        aggs.semantic_real.remove_row(hash_semantic_row(
            source.as_deref(),
            target.as_deref(),
            kind.as_deref(),
        ));
    }
    aggs.semantic_real.add_row(hash_semantic_row(
        edge.source_symbol_uid.as_deref(),
        edge.target_symbol_uid.as_deref(),
        Some(edge.relation_kind.as_str()),
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
    use tempfile::TempDir;

    fn open_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("sig_agg.db")).unwrap().0;
        (tmp, db)
    }

    /// Stored aggregates must equal a fresh full recompute over the final
    /// table contents — the convergence invariant every maintained write
    /// path has to preserve.
    fn assert_converged(db: &IndexDb, label: &str) {
        let conn = db.read_conn().unwrap();
        let stored = load_on(&conn)
            .unwrap()
            .unwrap_or_else(|| panic!("{label}: stored baseline must exist"));
        let scanned = scan_on(&conn).unwrap();
        assert_eq!(
            stored, scanned,
            "{label}: stored aggregates diverged from full recompute"
        );
    }

    fn symbol(
        file: &str,
        name: &str,
        uid: &str,
        container: Option<&str>,
    ) -> cc_model::SymbolRecord {
        cc_model::SymbolRecord {
            symbol_id: format!("{file}:{name}"),
            file_path: file.to_string(),
            name: name.to_string(),
            kind: cc_model::symbol::SymbolKind::Function,
            container: container.map(str::to_string),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: cc_model::ParserTier::TreeSitter,
            parser_confidence: 1.0,
            qname: Some(format!("{file}.{name}")),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid.to_string()),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    fn call_edge(
        id: &str,
        file: &str,
        caller_uid: &str,
        callee_uid: &str,
        synthesized_by: Option<&str>,
    ) -> cc_model::CallEdgeRecord {
        cc_model::CallEdgeRecord {
            edge_id: id.to_string(),
            file_path: file.to_string(),
            callee_symbol: "callee".to_string(),
            caller_symbol_uid: Some(caller_uid.to_string()),
            callee_symbol_uid: Some(callee_uid.to_string()),
            synthesized_by: synthesized_by.map(str::to_string),
            ..Default::default()
        }
    }

    fn semantic_edge(
        id: &str,
        file: &str,
        source_uid: &str,
        target_uid: &str,
    ) -> cc_model::edge::SemanticEdgeRecord {
        cc_model::edge::SemanticEdgeRecord {
            edge_id: id.to_string(),
            file_path: file.to_string(),
            source_symbol: "Src".to_string(),
            source_symbol_uid: Some(source_uid.to_string()),
            target_symbol: "Dst".to_string(),
            target_symbol_uid: Some(target_uid.to_string()),
            relation_kind: cc_model::edge::SemanticRelation::Implements,
            line: 1,
            confidence: 0.9,
            parser_tier: cc_model::ParserTier::TreeSitter,
        }
    }

    fn dispatch_site(id: &str, file: &str, key: &str) -> cc_model::DispatchSiteRecord {
        cc_model::DispatchSiteRecord {
            site_id: id.to_string(),
            file_path: file.to_string(),
            line: 3,
            col: 0,
            enclosing_symbol_uid: None,
            receiver_expr: None,
            site_kind: cc_model::DispatchSiteKind::EventOn,
            key: key.to_string(),
            handler_expr: None,
            handler_symbol_uid: None,
            confidence: 0.8,
        }
    }

    fn unit(file: &str, outcome: cc_model::ParseOutcome) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: file.to_string(),
            language: cc_model::Language::Rust,
            content_hash: format!("hash-{file}-{}", outcome.symbols.len()),
            mtime: 1.0,
            size: 1,
            outcome,
        }
    }

    fn write_batch(
        db: &IndexDb,
        to_remove: &[String],
        normal: &[FileWriteUnit],
        dirty: &[FileWriteUnit],
        hierarchy: &[cc_model::edge::SemanticEdgeRecord],
    ) {
        db.write_incremental_batch(
            to_remove,
            normal,
            dirty,
            &[],
            hierarchy,
            &PrecompressedChunks::new(),
        )
        .unwrap();
    }

    /// Add / modify / remove / dirty-rewrite through the incremental batch:
    /// the maintained aggregates must converge to a full recompute after
    /// every step.
    #[test]
    fn incremental_batch_add_modify_remove_converges() {
        let (_tmp, db) = open_db();

        // Add two files (symbols with and without container, real call
        // edges, a semantic edge, a dispatch site) plus a hierarchy edge.
        let unit_a = unit(
            "src/a.rs",
            cc_model::ParseOutcome {
                symbols: vec![
                    symbol("src/a.rs", "alpha", "uid_alpha", None),
                    symbol("src/a.rs", "beta", "uid_beta", Some("Alpha")),
                ],
                call_edges: vec![call_edge(
                    "src/a.rs:e1",
                    "src/a.rs",
                    "uid_alpha",
                    "uid_beta",
                    None,
                )],
                semantic_edges: vec![semantic_edge(
                    "src/a.rs:se1",
                    "src/a.rs",
                    "uid_alpha",
                    "uid_iface",
                )],
                dispatch_sites: vec![dispatch_site("src/a.rs:ds1", "src/a.rs", "click")],
                ..Default::default()
            },
        );
        let unit_b = unit(
            "src/b.rs",
            cc_model::ParseOutcome {
                symbols: vec![symbol("src/b.rs", "gamma", "uid_gamma", None)],
                call_edges: vec![call_edge(
                    "src/b.rs:e1",
                    "src/b.rs",
                    "uid_gamma",
                    "uid_alpha",
                    None,
                )],
                ..Default::default()
            },
        );
        let hier = vec![semantic_edge(
            "hier:src/a.rs:1",
            "src/a.rs",
            "file::src/a.rs",
            "uid_alpha",
        )];
        write_batch(&db, &[], &[unit_a, unit_b], &[], &hier);
        assert_converged(&db, "after add batch");

        // Modify: replace src/a.rs with a different symbol/edge set.
        let unit_a2 = unit(
            "src/a.rs",
            cc_model::ParseOutcome {
                symbols: vec![symbol(
                    "src/a.rs",
                    "alpha",
                    "uid_alpha",
                    Some("NewContainer"),
                )],
                dispatch_sites: vec![
                    dispatch_site("src/a.rs:ds1", "src/a.rs", "click"),
                    dispatch_site("src/a.rs:ds2", "src/a.rs", "submit"),
                ],
                ..Default::default()
            },
        );
        write_batch(&db, &[], &[unit_a2], &[], &[]);
        assert_converged(&db, "after modify batch");

        // Dirty rewrite: re-resolve src/a.rs's edges only.
        let dirty_a = unit(
            "src/a.rs",
            cc_model::ParseOutcome {
                symbols: vec![symbol("src/a.rs", "alpha", "uid_alpha", None)],
                call_edges: vec![call_edge(
                    "src/a.rs:e9",
                    "src/a.rs",
                    "uid_alpha",
                    "uid_gamma",
                    None,
                )],
                ..Default::default()
            },
        );
        write_batch(&db, &[], &[], &[dirty_a], &[]);
        assert_converged(&db, "after dirty rewrite batch");

        // Remove src/b.rs.
        write_batch(&db, &["src/b.rs".to_string()], &[], &[], &[]);
        assert_converged(&db, "after remove batch");
    }

    /// A fully empty batch initializes the baseline on databases that were
    /// written before the aggregates existed (raw-SQL seeded here).
    #[test]
    fn empty_batch_initializes_missing_baseline() {
        let (_tmp, db) = open_db();
        {
            let conn = db.write_conn.lock().unwrap();
            conn.execute_batch(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                     VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
                 INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                     VALUES('s1','src/x.rs','A','function',1,1,'uA');",
            )
            .unwrap();
            assert!(
                load_on(&conn).unwrap().is_none(),
                "no baseline before the batch"
            );
        }
        write_batch(&db, &[], &[], &[], &[]);
        assert_converged(&db, "after no-op batch init");
    }

    /// Synthetic call-edge insert (incl. an OR REPLACE collision) and
    /// delete-by-kind keep `call_synthetic` converged.
    #[test]
    fn synthetic_call_edge_lifecycle_converges() {
        let (_tmp, db) = open_db();
        // Baseline + a files row for the FK, through maintained writers.
        write_batch(
            &db,
            &[],
            &[unit("src/a.rs", cc_model::ParseOutcome::default())],
            &[],
            &[],
        );

        db.insert_synthetic_call_edges(&[
            call_edge(
                "synth:ee:1",
                "src/a.rs",
                "uid_a",
                "uid_b",
                Some("event_emitter"),
            ),
            call_edge(
                "synth:ee:2",
                "src/a.rs",
                "uid_a",
                "uid_c",
                Some("event_emitter"),
            ),
        ])
        .unwrap();
        assert_converged(&db, "after synthetic insert");

        // OR REPLACE: same edge_id, different callee.
        db.insert_synthetic_call_edges(&[call_edge(
            "synth:ee:1",
            "src/a.rs",
            "uid_a",
            "uid_d",
            Some("event_emitter"),
        )])
        .unwrap();
        assert_converged(&db, "after synthetic upsert collision");

        db.delete_synthetic_call_edges("event_emitter").unwrap();
        assert_converged(&db, "after delete by kind");
    }

    /// The synthesis unit of work (semantic upserts incl. real-id rows and a
    /// within-batch duplicate, synthetic call edges, prefix delete) keeps
    /// every aggregate converged.
    #[test]
    fn unit_of_work_synthesis_writes_converge() {
        let (_tmp, db) = open_db();
        write_batch(
            &db,
            &[],
            &[unit(
                "src/a.rs",
                cc_model::ParseOutcome {
                    semantic_edges: vec![semantic_edge("real:1", "src/a.rs", "uid_a", "uid_b")],
                    ..Default::default()
                },
            )],
            &[],
            &[],
        );

        let uow = db.begin_unit_of_work().unwrap();
        uow.insert_semantic_edges_batch(&[
            semantic_edge("synth:jsx:1", "src/a.rs", "uid_a", "uid_x"),
            // Replaces the committed real:1 row.
            semantic_edge("real:1", "src/a.rs", "uid_a", "uid_changed"),
            // Within-batch duplicate id: replaces the row inserted above.
            semantic_edge("real:1", "src/a.rs", "uid_a", "uid_final"),
        ])
        .unwrap();
        uow.insert_synthetic_call_edges(&[call_edge(
            "synth:ee:1",
            "src/a.rs",
            "uid_a",
            "uid_b",
            Some("event_emitter"),
        )])
        .unwrap();
        uow.delete_synthetic_semantic_edges("synth:jsx:").unwrap();
        uow.commit().unwrap();
        assert_converged(&db, "after synthesis unit of work");
    }

    /// The remaining file-scoped writers (dispatch-site replace, semantic
    /// removal by file, whole-file replace) keep the aggregates converged.
    #[test]
    fn file_scoped_writers_converge() {
        let (_tmp, db) = open_db();
        db.replace_files_batch(&[unit(
            "src/a.rs",
            cc_model::ParseOutcome {
                symbols: vec![symbol("src/a.rs", "alpha", "uid_alpha", None)],
                semantic_edges: vec![semantic_edge("real:1", "src/a.rs", "uid_a", "uid_b")],
                ..Default::default()
            },
        )])
        .unwrap();
        assert_converged(&db, "after replace_files_batch");

        db.replace_dispatch_sites("src/a.rs", &[dispatch_site("ds1", "src/a.rs", "click")])
            .unwrap();
        assert_converged(&db, "after replace_dispatch_sites");
        db.replace_dispatch_sites("src/a.rs", &[]).unwrap();
        assert_converged(&db, "after dispatch site clear");

        db.remove_semantic_edges_by_file("src/a.rs").unwrap();
        assert_converged(&db, "after remove_semantic_edges_by_file");

        db.remove_files_batch(&["src/a.rs".to_string()]).unwrap();
        assert_converged(&db, "after remove_files_batch");
    }

    #[test]
    fn row_agg_add_remove_round_trips() {
        let mut agg = RowAgg::default();
        agg.add_row(7);
        agg.add_row(11);
        agg.remove_row(7);
        let mut expected = RowAgg::default();
        expected.add_row(11);
        assert_eq!(agg, expected);
    }

    #[test]
    fn row_agg_distinguishes_duplicate_multisets() {
        // {a, a, b} vs {b, b, a}: XOR-style aggregates collapse these; the
        // wrapping-sum aggregate must not.
        let (a, b) = (0x1234_5678_9abc_def0u64, 0x0fed_cba9_8765_4321u64);
        let mut left = RowAgg::default();
        left.add_row(a);
        left.add_row(a);
        left.add_row(b);
        let mut right = RowAgg::default();
        right.add_row(b);
        right.add_row(b);
        right.add_row(a);
        assert_ne!(left, right);
    }

    #[test]
    fn serialize_round_trips_and_rejects_other_versions() {
        let mut aggs = GraphSignatureAggregates::default();
        aggs.symbols_full.add_row(u64::MAX);
        aggs.call_synthetic.add_row(42);
        let serialized = aggs.serialize();
        assert_eq!(
            GraphSignatureAggregates::deserialize(&serialized),
            Some(aggs)
        );
        let foreign_version = format!(
            "0|{}",
            serialized.split_once('|').expect("versioned payload").1
        );
        assert_eq!(
            GraphSignatureAggregates::deserialize(&foreign_version),
            None,
            "foreign format versions must read as absent"
        );
        assert_eq!(GraphSignatureAggregates::deserialize("garbage"), None);
    }
}
