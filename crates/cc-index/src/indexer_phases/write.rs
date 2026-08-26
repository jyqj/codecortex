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

        let mut seed_tokens = None;
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
            seed_tokens = Some(time_step("write", "incremental_batch", || {
                self.db.writes().write_incremental_batch(
                    to_remove,
                    &normal_write_units,
                    &dirty_write_units,
                    route_nodes,
                    hierarchy_edges,
                    chunk_blobs,
                )
            })?);

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
            seed_tokens,
        })
    }

    /// Commit half of a staged full rebuild: atomic swap only. The staging
    /// file was written during prepare (lock-free); this runs under the write
    /// lock and must not rewrite the payload.
    pub(crate) fn commit_full_rebuild_staging(
        &self,
        project_path: &Path,
        generation_floor: cc_db::index_db::IndexGeneration,
        to_remove: &[String],
    ) -> CcResult<WriteResult> {
        time_step("write", "full_staging_swap", || {
            self.db.admin().swap_rebuild_staging(generation_floor)
        })?;
        time_step("write", "frameworks", || {
            self.persist_frameworks(project_path, true, &[], to_remove)
        })?;
        Ok(WriteResult {
            write_units: Vec::new(),
            config_units: Vec::new(),
            seed_tokens: None,
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

#[cfg(test)]
mod phase_write_behavior_tests {
    use super::*;
    use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
    use cc_model::config::IndexingConfig;
    use cc_model::edge::CallEdgeRecord;
    use cc_model::parse::ParseOutcome;
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use cc_model::{ChunkRecord, Language, ParserTier};
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Number of write transactions one incremental `phase_write` with a
    /// non-empty content batch commits, each bumping `index_epoch` once:
    /// 1. the incremental content batch (removals + replacements + dirty
    ///    units + routes + hierarchy edges — ONE atomic transaction),
    /// 2. `replace_file_frameworks` for the batch files,
    /// 3. `replace_repo_frameworks` (unconditional repo-level refresh).
    ///
    /// The constant is asserted below so a regression that splits the batch
    /// into per-file transactions (or adds a spurious bump that evicts
    /// epoch-keyed caches every build) is caught immediately.
    const EPOCH_BUMPS_PER_CONTENT_BATCH: u64 = 3;

    /// Project dir stays empty (phase_write never reads source files); the
    /// DB lives in its own tempdir so the config/infra walks over the
    /// project can never observe DB/WAL files.
    fn setup() -> (TempDir, TempDir, Arc<IndexDb>, Indexer) {
        let project = TempDir::new().unwrap();
        let db_dir = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&db_dir.path().join("write.db")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project.path(), &IndexingConfig::default());
        (project, db_dir, db, indexer)
    }

    fn symbol(file: &str, name: &str, uid: &str) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("{file}:{name}"),
            file_path: file.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
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

    fn call_edge(edge_id: &str, file: &str, caller_uid: &str, callee_uid: &str) -> CallEdgeRecord {
        CallEdgeRecord {
            edge_id: edge_id.to_string(),
            file_path: file.to_string(),
            callee_symbol: callee_uid.to_string(),
            line: 2,
            caller_symbol_uid: Some(caller_uid.to_string()),
            callee_symbol_uid: Some(callee_uid.to_string()),
            ..Default::default()
        }
    }

    fn chunk(file: &str, idx: u32, text: &str) -> ChunkRecord {
        ChunkRecord {
            chunk_id: format!("{file}:{idx}"),
            file_path: file.to_string(),
            language: Language::Rust,
            chunk_index: idx,
            start_line: 1,
            end_line: 5,
            breadcrumb: String::new(),
            text: text.to_string(),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        }
    }

    fn unit(file: &str, hash: &str, outcome: ParseOutcome) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: file.to_string(),
            language: Language::Rust,
            content_hash: hash.to_string(),
            mtime: 1.0,
            size: 1,
            outcome,
        }
    }

    /// Drive the incremental `phase_write` path exactly like `commit_write`
    /// does: side-car compression during prepare, then the write under one
    /// call (empty actions map — no dirty units unless a test builds them).
    fn run_phase_write(
        indexer: &Indexer,
        project: &Path,
        units: Vec<FileWriteUnit>,
        to_remove: &[String],
    ) -> WriteResult {
        let chunk_blobs = Indexer::precompress_chunks(&units);
        let mut build_explain = BuildExplainCollector::new();
        indexer
            .phase_write(
                project,
                false,
                units,
                &HashMap::new(),
                to_remove,
                &[],
                &[],
                &chunk_blobs,
                &mut build_explain,
            )
            .unwrap()
    }

    fn json_rows(db: &IndexDb, sql: &str) -> Vec<serde_json::Value> {
        db.reads().query_json(sql, &[]).unwrap()
    }

    fn count(db: &IndexDb, sql: &str) -> i64 {
        json_rows(db, sql)
            .first()
            .and_then(|row| row.get("cnt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    /// The initial two-file batch shared by the row-content and the
    /// same-batch-removal tests. Returns the observed epoch delta.
    fn write_initial_batch(indexer: &Indexer, project: &Path, db: &IndexDb) -> u64 {
        let before = db.reads().generation().unwrap().index_epoch;
        let units = vec![
            unit(
                "src/a.rs",
                "hash-a-1",
                ParseOutcome {
                    symbols: vec![symbol("src/a.rs", "alpha", "uid_alpha")],
                    call_edges: vec![call_edge("edge-a-1", "src/a.rs", "uid_alpha", "uid_beta")],
                    chunks: vec![chunk("src/a.rs", 0, "fn alpha() {}")],
                    ..Default::default()
                },
            ),
            unit(
                "src/b.rs",
                "hash-b-1",
                ParseOutcome {
                    symbols: vec![symbol("src/b.rs", "beta", "uid_beta")],
                    call_edges: vec![call_edge("edge-b-1", "src/b.rs", "uid_beta", "uid_alpha")],
                    chunks: vec![chunk("src/b.rs", 0, "fn beta() {}")],
                    ..Default::default()
                },
            ),
        ];
        let result = run_phase_write(indexer, project, units, &[]);
        assert_eq!(
            result.write_units.len(),
            2,
            "phase_write must hand the reassembled units to downstream phases"
        );
        assert!(
            result.seed_tokens.is_some(),
            "the incremental path must return the in-transaction seed token span"
        );
        db.reads().generation().unwrap().index_epoch - before
    }

    /// (a) An incremental batch persists files/symbols/call_edges rows with
    /// the exact content of the write units, and the whole batch advances
    /// `index_epoch` by the fixed per-batch constant — the content batch
    /// itself contributes exactly ONE bump because removals, replacements
    /// and edges commit in a single transaction.
    #[test]
    fn incremental_batch_write_persists_rows_with_single_epoch_advance() {
        let (project, _db_dir, db, indexer) = setup();
        let delta = write_initial_batch(&indexer, project.path(), &db);
        assert_eq!(
            delta, EPOCH_BUMPS_PER_CONTENT_BATCH,
            "epoch advance per batch must be the fixed transaction count, \
             independent of file count (batch txn + file/repo frameworks)"
        );

        let files: Vec<(String, String)> = json_rows(
            &db,
            "SELECT file_path, content_hash FROM files ORDER BY file_path",
        )
        .iter()
        .map(|row| {
            (
                row["file_path"].as_str().unwrap().to_string(),
                row["content_hash"].as_str().unwrap().to_string(),
            )
        })
        .collect();
        assert_eq!(
            files,
            vec![
                ("src/a.rs".to_string(), "hash-a-1".to_string()),
                ("src/b.rs".to_string(), "hash-b-1".to_string()),
            ],
            "files rows must mirror the batch units"
        );

        let symbols: Vec<(String, String, String)> = json_rows(
            &db,
            "SELECT file_path, name, symbol_uid FROM symbols ORDER BY file_path",
        )
        .iter()
        .map(|row| {
            (
                row["file_path"].as_str().unwrap().to_string(),
                row["name"].as_str().unwrap().to_string(),
                row["symbol_uid"].as_str().unwrap().to_string(),
            )
        })
        .collect();
        assert_eq!(
            symbols,
            vec![
                (
                    "src/a.rs".to_string(),
                    "alpha".to_string(),
                    "uid_alpha".to_string()
                ),
                (
                    "src/b.rs".to_string(),
                    "beta".to_string(),
                    "uid_beta".to_string()
                ),
            ],
            "symbols rows must mirror the batch units"
        );

        let edges: Vec<(String, String, String)> = json_rows(
            &db,
            "SELECT edge_id, caller_symbol_uid, callee_symbol_uid FROM call_edges ORDER BY edge_id",
        )
        .iter()
        .map(|row| {
            (
                row["edge_id"].as_str().unwrap().to_string(),
                row["caller_symbol_uid"].as_str().unwrap().to_string(),
                row["callee_symbol_uid"].as_str().unwrap().to_string(),
            )
        })
        .collect();
        assert_eq!(
            edges,
            vec![
                (
                    "edge-a-1".to_string(),
                    "uid_alpha".to_string(),
                    "uid_beta".to_string()
                ),
                (
                    "edge-b-1".to_string(),
                    "uid_beta".to_string(),
                    "uid_alpha".to_string()
                ),
            ],
            "call_edges rows must mirror the batch units"
        );
    }

    /// (b) A batch that updates one file and removes another in the SAME
    /// `phase_write` call must clear every row of the removed file (files,
    /// symbols, call_edges, chunks), replace — not accumulate — the updated
    /// file's rows, and advance the epoch by the same per-batch constant as
    /// the initial batch (no extra transaction for the removal).
    #[test]
    fn same_batch_removal_clears_all_rows_of_removed_file() {
        let (project, _db_dir, db, indexer) = setup();
        let initial_delta = write_initial_batch(&indexer, project.path(), &db);

        let before = db.reads().generation().unwrap().index_epoch;
        let units = vec![unit(
            "src/a.rs",
            "hash-a-2",
            ParseOutcome {
                symbols: vec![
                    symbol("src/a.rs", "alpha", "uid_alpha"),
                    symbol("src/a.rs", "alpha_two", "uid_alpha_two"),
                ],
                call_edges: vec![call_edge(
                    "edge-a-2",
                    "src/a.rs",
                    "uid_alpha_two",
                    "uid_alpha",
                )],
                chunks: vec![chunk("src/a.rs", 0, "fn alpha() {}\nfn alpha_two() {}")],
                ..Default::default()
            },
        )];
        run_phase_write(&indexer, project.path(), units, &["src/b.rs".to_string()]);
        let removal_delta = db.reads().generation().unwrap().index_epoch - before;
        assert_eq!(
            removal_delta, initial_delta,
            "update+removal in one batch must commit in the same fixed \
             transaction count as a plain add batch"
        );

        // The removed file's rows are gone from every content table.
        for table in ["files", "symbols", "call_edges", "chunks"] {
            assert_eq!(
                count(
                    &db,
                    &format!("SELECT COUNT(*) AS cnt FROM {table} WHERE file_path = 'src/b.rs'")
                ),
                0,
                "{table} must hold no rows for the removed src/b.rs"
            );
        }

        // The updated file was replaced in place: new hash, new symbol set,
        // and the old edge is gone rather than accumulated next to the new.
        let files = json_rows(&db, "SELECT file_path, content_hash FROM files");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["file_path"].as_str(), Some("src/a.rs"));
        assert_eq!(files[0]["content_hash"].as_str(), Some("hash-a-2"));

        let names: Vec<String> = json_rows(
            &db,
            "SELECT name FROM symbols WHERE file_path = 'src/a.rs' ORDER BY name",
        )
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_string())
        .collect();
        assert_eq!(names, vec!["alpha".to_string(), "alpha_two".to_string()]);

        let edge_ids: Vec<String> = json_rows(&db, "SELECT edge_id FROM call_edges")
            .iter()
            .map(|row| row["edge_id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            edge_ids,
            vec!["edge-a-2".to_string()],
            "replacement must drop edge-a-1 and removal must drop edge-b-1"
        );
    }

    /// (c) The prepare-phase side-car (`precompress_chunks`) must make
    /// exactly the per-chunk decision `compress_chunk_text` declares (keyed
    /// by rel_path, index-aligned, `None` for small/incompressible chunks),
    /// and the blobs written through `phase_write` must read back as the
    /// original text via the production decode path.
    #[test]
    fn precompressed_chunk_sidecar_matches_policy_and_roundtrips() {
        let (project, _db_dir, db, indexer) = setup();

        // Above the 128-byte floor and highly compressible vs. below it.
        let big_text = "// the same compressible payload line\n".repeat(24);
        let small_text = "fn tiny() {}";
        let write_unit = unit(
            "src/big.rs",
            "hash-big-1",
            ParseOutcome {
                chunks: vec![
                    chunk("src/big.rs", 0, &big_text),
                    chunk("src/big.rs", 1, small_text),
                ],
                ..Default::default()
            },
        );

        let blobs = Indexer::precompress_chunks(std::slice::from_ref(&write_unit));
        let expected: Vec<Option<Vec<u8>>> = write_unit
            .outcome
            .chunks
            .iter()
            .map(|c| compress_chunk_text(&c.text))
            .collect();
        assert_eq!(
            blobs.get("src/big.rs"),
            Some(&expected),
            "side-car must be keyed by rel_path and index-aligned with outcome.chunks"
        );
        assert!(
            expected[0].is_some(),
            "large compressible chunk must be zstd-compressed during prepare"
        );
        assert!(
            expected[1].is_none(),
            "chunks at or below the size floor must stay plain text"
        );

        let mut build_explain = BuildExplainCollector::new();
        indexer
            .phase_write(
                project.path(),
                false,
                vec![write_unit],
                &HashMap::new(),
                &[],
                &[],
                &[],
                &blobs,
                &mut build_explain,
            )
            .unwrap();

        // Production decode path (encoding marker + zstd) restores the
        // original text for both storage forms.
        let conn = crate::test_seed::seed_conn(&db);
        let mut stmt = conn
            .prepare(
                "SELECT text, text_encoding FROM chunks \
                 WHERE file_path = 'src/big.rs' ORDER BY chunk_index",
            )
            .unwrap();
        let restored: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    read_chunk_text_with_encoding(row, 0, 1)?,
                    row.get::<_, String>(1)?,
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            restored,
            vec![
                (big_text.clone(), "zstd".to_string()),
                (small_text.to_string(), "plain".to_string()),
            ],
            "stored chunk text must round-trip through the side-car blobs"
        );
    }
}
