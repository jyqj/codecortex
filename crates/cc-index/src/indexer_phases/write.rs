use std::collections::{HashMap, HashSet};
use std::path::Path;

use rayon::prelude::*;

use cc_db::index_db::{compress_chunk_text, FileWriteUnit, PrecompressedChunks};
use cc_model::edge::RouteNodeRecord;
use cc_model::{BuildExplainCollector, CcResult};

use crate::framework_registry;
use crate::indexer::{FileAction, Indexer, WriteResult, MIN_FILES_FOR_PARALLEL};

use super::time_step;

impl Indexer {
    /// Phase 5.5 (prepare, lock-free): compress chunk payloads with the
    /// shared deterministic policy so the write transaction only binds
    /// pre-computed blobs instead of running zstd while holding the write
    /// connection. Keyed by rel_path because `phase_write` re-partitions the
    /// units (normal vs dirty) before writing.
    pub(crate) fn precompress_chunks(write_units: &[FileWriteUnit]) -> PrecompressedChunks {
        let compress_unit = |unit: &FileWriteUnit| {
            (
                unit.rel_path.clone(),
                unit.outcome
                    .chunks
                    .iter()
                    .map(|c| compress_chunk_text(&c.text))
                    .collect::<Vec<_>>(),
            )
        };
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units.par_iter().map(compress_unit).collect()
        } else {
            write_units.iter().map(compress_unit).collect()
        }
    }

    /// Phase 6: Batch write to SQLite (dual path: full rebuild vs incremental).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn phase_write(
        &self,
        project_path: &Path,
        full: bool,
        write_units: Vec<FileWriteUnit>,
        actions: &HashMap<String, FileAction>,
        to_remove: &[String],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        chunk_blobs: &PrecompressedChunks,
        build_explain: &mut BuildExplainCollector,
    ) -> CcResult<WriteResult> {
        // Separate dirty write units from normal ones before write.
        let dirty_set: HashSet<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::DirtyResolveOnly))
            .map(|(p, _)| p.clone())
            .collect();
        let (dirty_write_units, normal_write_units): (Vec<_>, Vec<_>) = write_units
            .into_iter()
            .partition(|u| dirty_set.contains(&u.rel_path));

        let config_units = if full {
            // Full rebuild: temp-db + atomic swap
            if self.use_direct_writer {
                match self.write_full_snapshot_via_direct_writer(
                    project_path,
                    &normal_write_units,
                    route_nodes,
                    hierarchy_edges,
                    chunk_blobs,
                ) {
                    Ok(config_units) => {
                        tracing::info!("full rebuild completed via direct writer");
                        config_units
                    }
                    Err(e) => {
                        tracing::warn!(
                            err = %e,
                            "direct writer failed, falling back to standard rebuild"
                        );
                        self.write_full_snapshot_via_temp_db(
                            project_path,
                            &normal_write_units,
                            route_nodes,
                            hierarchy_edges,
                            chunk_blobs,
                        )?
                    }
                }
            } else {
                self.write_full_snapshot_via_temp_db(
                    project_path,
                    &normal_write_units,
                    route_nodes,
                    hierarchy_edges,
                    chunk_blobs,
                )?
            }
        } else {
            // Incremental: removals, replacements, dirty re-resolution, route
            // nodes and the batch files' hierarchy edges commit atomically —
            // a crash cannot leave files deleted with their edges still
            // present, nor a batch file committed (content_hash persisted, so
            // never re-batched) with its hierarchy edges missing. The batch
            // deletes every batch/removed file's semantic_edges rows, so the
            // in-transaction insert is the sole owner of the batch files'
            // hierarchy edges; unchanged files keep theirs from earlier
            // builds (the edges are file-local).
            let batch_empty = to_remove.is_empty()
                && normal_write_units.is_empty()
                && dirty_write_units.is_empty();
            time_step("write", "incremental_batch", || {
                self.db.writes().write_incremental_batch(
                    to_remove,
                    &normal_write_units,
                    &dirty_write_units,
                    route_nodes,
                    hierarchy_edges,
                    chunk_blobs,
                )
            })?;

            // Config links read the just-committed snapshot (separate read
            // connection), so they stay outside the batch transaction. The
            // gate skips the config scan (and, for no-op batches, the whole
            // pass) when the config-file set is unchanged.
            let config_units = match time_step("write", "config_link_compute", || {
                self.build_config_link_units_gated(project_path, batch_empty, build_explain)
            })? {
                // 快速路径保持零写入：签名未变且批次为空，链接不可能变化。
                None => Vec::new(),
                // resolve 半程跑过就必须走 apply：除了写入新链接，还要清掉
                // “上一轮有链接、本轮解析为零链接”的文件的陈旧 refs（这类
                // 文件不再产出替换单元，否则会一直挂到下次 full build）。
                Some(round) => {
                    time_step("write", "config_link_apply", || {
                        self.db
                            .writes()
                            .apply_config_link_units(&round.units, &round.seen_config_files)
                    })?;
                    round.units
                }
            };

            // Update metadata (for incremental only; full path sets it inside temp-db)
            time_step("write", "metadata", || {
                let now = chrono::Utc::now().to_rfc3339();
                self.db.writes().set_metadata("last_indexed_at", &now)?;
                self.db.writes().set_metadata("index_version", "1.0.0")
            })?;

            // Long incremental-only sessions never hit the full-rebuild
            // checkpoint, so reclaim the WAL here once it grows too large.
            const MAX_INCREMENTAL_WAL_BYTES: u64 = 16 * 1024 * 1024;
            time_step("write", "wal_checkpoint", || {
                if let Err(e) = self
                    .db
                    .admin()
                    .checkpoint_wal_if_large(MAX_INCREMENTAL_WAL_BYTES)
                {
                    tracing::warn!(err = %e, "incremental WAL checkpoint failed");
                }
            });

            config_units
        };

        let framework_file_paths: Vec<String> = normal_write_units
            .iter()
            .map(|u| u.rel_path.clone())
            .collect();

        // Reassemble write_units for downstream phases that need the full list
        let write_units: Vec<FileWriteUnit> = normal_write_units
            .into_iter()
            .chain(dirty_write_units)
            .collect();

        // Hierarchy edges were written inside the incremental batch
        // transaction / the full-rebuild temp-db (before the swap), so a
        // crash can never separate a committed file from its edges.
        if !hierarchy_edges.is_empty() {
            tracing::info!(count = hierarchy_edges.len(), "generated hierarchy edges");
        }

        // Post-processing passes run on the live DB after both paths.
        // Framework detection only needs the files that were actually parsed on
        // incremental builds; full rebuilds still rescan the whole project.
        time_step("write", "frameworks", || {
            self.persist_frameworks(project_path, full, &framework_file_paths, to_remove)
        })?;

        Ok(WriteResult {
            write_units,
            config_units,
        })
    }

    fn persist_frameworks(
        &self,
        project_path: &Path,
        full: bool,
        changed_files: &[String],
        removed_files: &[String],
    ) -> CcResult<()> {
        if full {
            return framework_registry::detect_and_persist_frameworks(&self.db, project_path);
        }
        if changed_files.is_empty() && !removed_files.is_empty() {
            return framework_registry::refresh_repo_frameworks(&self.db, project_path);
        }
        let changed_files: Vec<&str> = changed_files.iter().map(String::as_str).collect();
        framework_registry::detect_and_persist_frameworks_incremental(
            &self.db,
            project_path,
            &changed_files,
        )
    }
}
