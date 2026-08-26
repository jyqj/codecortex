use std::collections::HashSet;
use std::path::Path;

use cc_db::index_db::{FileWriteUnit, PrecompressedChunks, SymbolTargetRow};
use cc_db::SnapshotWriteTxn;
use cc_model::edge::RouteNodeRecord;
use cc_model::CcResult;

use crate::config_linker::{config_files_signature, scan_config_tokens};
use crate::indexer::Indexer;

use super::{time_step, CONFIG_SIG_ALGORITHM};

/// Owned output of the common front half of a full-snapshot write, shared by
/// the temp-db and DirectWriter paths: the derived config-link units plus the
/// `last_indexed_at` timestamp recorded inside the rebuilt snapshot, and the
/// config-linker gate state (signature + raw-token cache) so post-rebuild
/// incrementals can skip the config scan.
struct FullSnapshotPayload {
    config_units: Vec<FileWriteUnit>,
    recorded_at: String,
    config_sig: u64,
    /// Serialized raw tokens, or `None` when over the cache size cap.
    config_raw_cache: Option<String>,
}

impl Indexer {
    /// Collect symbol targets from write_units for config link snapshot.
    fn collect_symbol_targets(write_units: &[FileWriteUnit]) -> Vec<SymbolTargetRow> {
        let mut targets = Vec::new();
        for unit in write_units {
            for s in &unit.outcome.symbols {
                targets.push(SymbolTargetRow {
                    symbol_id: s.symbol_id.clone(),
                    symbol_uid: s.symbol_uid.clone(),
                    name: s.name.clone(),
                    qname: s.qname.clone(),
                    file_path: s.file_path.clone(),
                });
            }
        }
        targets
    }

    /// Common front half of both full-snapshot write paths: derive the
    /// config-link units from the freshly parsed write units and stamp the
    /// rebuild time. Returns an owned payload so the two paths only differ in
    /// their rebuild adapter (`rebuild_with_temp_db` vs
    /// `rebuild_with_direct_writer`).
    fn prepare_full_snapshot_payload(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        walk_manifest: Option<&crate::scanner::WalkManifest>,
    ) -> CcResult<FullSnapshotPayload> {
        // Pre-collect snapshot data for config links before entering the
        // rebuild closure (the closure must not query the live DB).
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        // Full builds always scan. Signature first, scan second: a config
        // file changing in between leaves a stale signature behind, which
        // forces a rescan next build — never a wrong skip. With a shared-walk
        // manifest both halves come from the same snapshot, no extra walks.
        let (config_sig, raw_tokens) = match walk_manifest {
            Some(manifest) => (
                crate::config_linker::config_files_signature_from_manifest(manifest),
                crate::config_linker::scan_config_tokens_from_manifest(project_path, manifest)?,
            ),
            None => (
                config_files_signature(project_path),
                scan_config_tokens(project_path)?,
            ),
        };
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
            &raw_tokens,
        )?;

        Ok(FullSnapshotPayload {
            config_units,
            recorded_at: chrono::Utc::now().to_rfc3339(),
            config_sig,
            config_raw_cache: Self::serialize_raw_token_cache(&raw_tokens),
        })
    }

    /// Shared rebuild-closure body: writes file data, route nodes, config-link
    /// units and metadata into the snapshot write seam handed out by either
    /// rebuild adapter. The raw `&Connection` never crosses this interface —
    /// both wrappers wrap it in a `SnapshotWriteTxn` at the closure entry.
    fn write_full_snapshot_contents(
        txn: &SnapshotWriteTxn,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        payload: &FullSnapshotPayload,
        chunk_blobs: &PrecompressedChunks,
    ) -> CcResult<()> {
        // Write main file data (chunk payloads pre-compressed during prepare).
        txn.write_file_data(write_units, chunk_blobs)?;

        // Route nodes.
        txn.write_route_nodes(route_nodes)?;

        // Hierarchy edges go into the temp-db before the atomic swap, so the
        // rebuilt snapshot can never become visible without them (writing
        // them after the swap would leave a crash window where every file's
        // content_hash is committed but its hierarchy edges are missing).
        txn.write_hierarchy_edges(hierarchy_edges)?;

        // Config-link units: parsed scanner files get refs-only, the rest are
        // written wholesale (see SnapshotWriteTxn::write_config_units).
        let parsed_paths: HashSet<&str> = write_units.iter().map(|u| u.rel_path.as_str()).collect();
        txn.write_config_units(&payload.config_units, &parsed_paths)?;

        // Metadata: schema version + config-linker gate state. Cache before
        // signature, same as the incremental path.
        txn.write_snapshot_metadata(
            &payload.recorded_at,
            payload.config_raw_cache.as_deref(),
            payload.config_sig,
            CONFIG_SIG_ALGORITHM,
        )?;

        // Graph-signature baseline: the LAST table-derived write so a rebuilt
        // snapshot never becomes visible with a stale baseline.
        txn.recompute_graph_signature_baseline()?;

        Ok(())
    }

    /// Write all index data via temp-db + atomic swap (full rebuild only).
    /// All main data (files, route_nodes, config_units, metadata) is written
    /// inside the temp-db transaction. Post-processing passes (frameworks,
    /// communities, test_edges, git co-changes, infra) run after the swap
    /// against the live DB.
    pub(super) fn write_full_snapshot_via_temp_db(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        chunk_blobs: &PrecompressedChunks,
        walk_manifest: Option<&crate::scanner::WalkManifest>,
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = time_step("write", "full_prepare_payload", || {
            self.prepare_full_snapshot_payload(project_path, write_units, walk_manifest)
        })?;
        time_step("write", "full_rebuild_temp_db", || {
            self.db.admin().rebuild_with_temp_db(|conn| {
                let txn = SnapshotWriteTxn::new(conn);
                Self::write_full_snapshot_contents(
                    &txn,
                    write_units,
                    route_nodes,
                    hierarchy_edges,
                    &payload,
                    chunk_blobs,
                )
            })
        })?;
        Ok(payload.config_units)
    }

    /// Write all index data via DirectWriter (high-speed path) + atomic swap.
    /// Same data flow as `write_full_snapshot_via_temp_db` but uses aggressive
    /// PRAGMAs (journal OFF, synchronous OFF, 64KB pages) for faster writes.
    pub(super) fn write_full_snapshot_via_direct_writer(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        chunk_blobs: &PrecompressedChunks,
        walk_manifest: Option<&crate::scanner::WalkManifest>,
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = time_step("write", "full_prepare_payload", || {
            self.prepare_full_snapshot_payload(project_path, write_units, walk_manifest)
        })?;
        time_step("write", "full_rebuild_direct_writer", || {
            self.db.admin().rebuild_with_direct_writer(|conn| {
                let txn = SnapshotWriteTxn::new(conn);
                Self::write_full_snapshot_contents(
                    &txn,
                    write_units,
                    route_nodes,
                    hierarchy_edges,
                    &payload,
                    chunk_blobs,
                )
            })
        })?;
        Ok(payload.config_units)
    }
}
