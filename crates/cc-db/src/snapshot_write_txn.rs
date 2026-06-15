//! Full-rebuild snapshot write seam: the typed `&Connection` substitute for
//! `write_full_snapshot_contents`.
//!
//! [`SnapshotWriteTxn`] is the cc-db write-transaction seam on the full-rebuild
//! path. It is the mutual-exclusive counterpart of [`crate::unit_of_work::UnitOfWork`]:
//! - `UnitOfWork` serves incremental writes — an `IMMEDIATE` transaction on the
//!   live write connection with a commit-once `index_epoch` bump.
//! - `SnapshotWriteTxn` serves full-rebuild content writes — it holds only a
//!   borrowed `&Connection` into the freshly-built temp DB (or DirectWriter
//!   rebuild connection) supplied by `rebuild_with_temp_db` /
//!   `rebuild_with_direct_writer`, and performs *no* epoch bump.
//!
//! Epoch progression for a full rebuild does not happen inside this seam. The
//! temp DB starts with empty metadata (generation defaults to 0), so any
//! in-transaction bump would be meaningless. Instead
//! `run_rebuild_protocol::finalize_rebuild_generation` stamps both clocks to
//! `max(floor, live) + 1` while holding the write lock, *before* the atomic
//! swap. Adding a `bump_*_on` method to this seam would corrupt that floor
//! computation and is rejected by contract.
//!
//! Like `UnitOfWork`, this seam never exposes the raw `rusqlite::Connection`:
//! `conn` is private and has no getter, so all SQL stays inside cc-db.
//!
//! Epoch-rule audit (`epoch_rules`) does not cover the full-rebuild path (it
//! audits per-table bump call sites); the rebuild epoch invariant is guarded
//! separately by the `mismatch rebuild must not roll index_epoch back` test.

use std::collections::HashSet;

use rusqlite::Connection;

use cc_model::CcResult;

use crate::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};

/// Metadata key for the serialized raw config-token cache written into a
/// rebuilt snapshot. Must stay identical to cc-index's `CONFIG_RAW_CACHE_KEY`
/// (`"config_raw_tokens"`) so the post-rebuild incremental can read it back.
const META_CONFIG_RAW_CACHE_KEY: &str = "config_raw_tokens";
/// Metadata key for the config-linker signature. Mirrors cc-index's
/// `CONFIG_SIG_KEY` (`"last_config_sig"`).
const META_CONFIG_SIG_KEY: &str = "last_config_sig";
/// Metadata key for the config-linker signature algorithm. Mirrors cc-index's
/// `CONFIG_SIG_ALGO_KEY` (`"last_config_sig_algo"`).
const META_CONFIG_SIG_ALGO_KEY: &str = "last_config_sig_algo";
/// Snapshot schema version stamped into every rebuilt index.
const META_INDEX_VERSION: &str = "1.0.0";

/// Typed full-rebuild content-write seam. See the module docs for the
/// epoch-isolation contract and the [`crate::unit_of_work::UnitOfWork`]
/// duality.
pub struct SnapshotWriteTxn<'a> {
    conn: &'a Connection,
}

impl<'a> SnapshotWriteTxn<'a> {
    /// Wrap the connection handed out by a rebuild adapter. The seam borrows
    /// the connection for its whole lifetime; callers never see `conn` again.
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Write per-file data (chunk payloads pre-compressed during prepare;
    /// missing entries fall back to the identical in-transaction policy).
    pub fn write_file_data(
        &self,
        write_units: &[FileWriteUnit],
        chunk_blobs: &PrecompressedChunks,
    ) -> CcResult<()> {
        for unit in write_units {
            IndexDb::insert_file_data_precompressed(
                self.conn,
                unit,
                chunk_blobs.get(&unit.rel_path).map(Vec::as_slice),
            )?;
        }
        Ok(())
    }

    /// Write route nodes.
    pub fn write_route_nodes(
        &self,
        route_nodes: &[cc_model::edge::RouteNodeRecord],
    ) -> CcResult<()> {
        for r in route_nodes {
            IndexDb::insert_route_node_into(self.conn, r)?;
        }
        Ok(())
    }

    /// Write hierarchy edges into the temp DB before the atomic swap.
    ///
    /// The rebuilt snapshot can never become visible without them: writing
    /// them after the swap would leave a crash window where every file's
    /// `content_hash` is committed but its hierarchy edges are missing.
    pub fn write_hierarchy_edges(
        &self,
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        IndexDb::insert_semantic_edges_batch_on(self.conn, hierarchy_edges)
    }

    /// Write config-link units, splitting on whether each path was already
    /// parsed by the scanner.
    ///
    /// Scanner-visible config files (yaml/toml etc.) were already written into
    /// `files` as parsed units — for them only config refs are appended (a
    /// second `insert_file_data` would collide on the `files` primary key and
    /// lose the parsed artifacts). Non-scanner config files (.ini/.env etc.)
    /// are still written wholesale.
    pub fn write_config_units(
        &self,
        config_units: &[FileWriteUnit],
        parsed_paths: &HashSet<&str>,
    ) -> CcResult<()> {
        for unit in config_units {
            if parsed_paths.contains(unit.rel_path.as_str()) {
                IndexDb::insert_config_link_refs(self.conn, unit)?;
            } else {
                IndexDb::insert_file_data(self.conn, unit)?;
            }
        }
        Ok(())
    }

    /// Write snapshot metadata: `last_indexed_at`, schema version, and the
    /// config-linker gate state.
    ///
    /// Order matters and mirrors the incremental `apply_config_link_units`
    /// path: raw cache before signature. Key names must stay identical to the
    /// cc-index config-linker constants so the first post-rebuild incremental
    /// can read them back. The signature algorithm value is supplied by the
    /// caller (cc-index owns the canonical `CONFIG_SIG_ALGORITHM` string).
    pub fn write_snapshot_metadata(
        &self,
        last_indexed_at: &str,
        config_raw_cache: Option<&str>,
        config_sig: u64,
        config_sig_algo: &str,
    ) -> CcResult<()> {
        IndexDb::set_metadata_on(self.conn, "last_indexed_at", last_indexed_at)?;
        IndexDb::set_metadata_on(self.conn, "index_version", META_INDEX_VERSION)?;
        // Cache before signature, same as the incremental path.
        IndexDb::set_metadata_on(
            self.conn,
            META_CONFIG_RAW_CACHE_KEY,
            config_raw_cache.unwrap_or(""),
        )?;
        IndexDb::set_metadata_on(self.conn, META_CONFIG_SIG_KEY, &config_sig.to_string())?;
        IndexDb::set_metadata_on(self.conn, META_CONFIG_SIG_ALGO_KEY, config_sig_algo)?;
        Ok(())
    }

    /// Recompute the graph-signature aggregate baseline from the rebuilt
    /// tables as the LAST table-derived write.
    ///
    /// A rebuilt snapshot can never become visible with a stale baseline, so
    /// the first post-rebuild incremental applies its delta against fresh
    /// state. Must run after every derived table has been written.
    pub fn recompute_graph_signature_baseline(&self) -> CcResult<()> {
        IndexDb::recompute_graph_signature_aggregates_on(self.conn)
    }
}
