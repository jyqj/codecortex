//! Indexer phase implementations (Phase 3.6 – Phase 11).
//!
//! Split from `indexer.rs` for maintainability. All methods are on `impl Indexer`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use cc_db::index_db::{
    compress_chunk_text, FileState, FileWriteUnit, IndexDb, PrecompressedChunks, SymbolTargetRow,
};
use cc_model::edge::{CoChangeEdgeRecord, ResolutionKind, RouteNodeRecord};
use cc_model::infra::{InfraEdge, InfraNode};
use cc_model::parse::ParseOutcome;
use cc_model::symbol::{SymbolRecord, SymbolRefRecord};
use cc_model::{CcError, CcResult, Language, ParserTier, StableId};

use crate::community::{build_community_labels, louvain_communities};
use crate::config_linker::{
    config_files_signature, resolve_config_links, scan_config_tokens, ConfigLinkKind,
    RawConfigToken,
};
use crate::dirty_closure::{DirtyPropagationOutcome, DirtyPropagationStatus};
use crate::framework_registry;
use crate::pass_gate::{
    log_gate_decision, DbSignatureGate, DeferredSignatureRecord, FileSignatureGate, PairGate,
    PassGate, StringCacheGate,
};
use crate::resolver::{ResolutionContext, SymbolCatalog};
use crate::synthesis_pipeline::SynthesisRound;

use super::indexer::{FileAction, Indexer, ResolveResult, WriteResult, MIN_FILES_FOR_PARALLEL};

/// Signature algorithm versions, persisted next to each recorded signature
/// (see `pass_gate`). Bump a version when its signature's column set or hash
/// formula changes, so a stale recorded value forces exactly one recompute
/// instead of a wrong skip. Signatures recorded before the version keys
/// existed read as version "1".
const DISPATCH_SIG_ALGORITHM: &str = "1";
const INTERFACE_SIG_ALGORITHM: &str = "1";
const COMMUNITY_SIG_ALGORITHM: &str = "1";
const INFRA_SIG_ALGORITHM: &str = "1";
const CONFIG_SIG_ALGORITHM: &str = "1";

/// Metadata keys for the config-linker gate: the config-file-set signature
/// (paths + mtime + size, mirroring `last_infra_sig`), its algorithm version,
/// and the cached raw token extraction the signature validates.
const CONFIG_SIG_KEY: &str = "last_config_sig";
const CONFIG_SIG_ALGO_KEY: &str = "last_config_sig_algo";
const CONFIG_RAW_CACHE_KEY: &str = "config_raw_tokens";

/// Upper bound for the persisted raw-token cache. Projects whose config scan
/// produces more serialized tokens than this (huge lock files) simply skip
/// the cache and rescan each build — the pre-gate behavior.
const CONFIG_RAW_CACHE_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Metadata key for the git co-change HEAD-skip gate. Shared by the gate
/// construction (compute) and the deferred record (apply).
const COCHANGE_HEAD_KEY: &str = "last_cochange_head";

/// Typed signature-scan row: each selected column read as TEXT (NULL or
/// non-text storage → `None`), mirroring `query_json`'s
/// `as_str().unwrap_or("")` extraction without the per-row
/// `serde_json::Map` materialization that dominated signature scans.
type SignatureTextRow<const COLS: usize> = [Option<String>; COLS];

/// Stream a parameter-less `sql` through one pooled read connection into
/// typed text rows. Hash-value-compatible with the previous
/// `query_json`-based scans: TEXT yields the string, everything else
/// (NULL/INTEGER/REAL/BLOB) yields `None`, exactly like `as_str()` on the
/// corresponding JSON value.
fn signature_text_rows<const COLS: usize>(
    db: &IndexDb,
    sql: &str,
) -> CcResult<Vec<SignatureTextRow<COLS>>> {
    let conn = db.reads().read_conn()?;
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| CcError::Database(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            let mut cols: SignatureTextRow<COLS> = std::array::from_fn(|_| None);
            for (i, slot) in cols.iter_mut().enumerate() {
                *slot = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Text(text) => {
                        Some(String::from_utf8_lossy(text).into_owned())
                    }
                    _ => None,
                };
            }
            Ok(cols)
        })
        .map_err(|e| CcError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CcError::Database(e.to_string()))?;
    Ok(rows)
}

/// Per-build memo of the symbols scan shared by the dispatch, interface, and
/// community signatures (identical row set and ordering; community hashes a
/// column subset), so a build pays for the symbols table scan once instead of
/// once per signature. Builds are single-threaded through the postprocess
/// phase, hence plain interior mutability.
#[derive(Default)]
struct SymbolRowsCache {
    rows: std::cell::RefCell<Option<std::rc::Rc<Vec<SignatureTextRow<4>>>>>,
}

impl SymbolRowsCache {
    fn get(&self, db: &IndexDb) -> CcResult<std::rc::Rc<Vec<SignatureTextRow<4>>>> {
        if let Some(rows) = self.rows.borrow().as_ref() {
            return Ok(std::rc::Rc::clone(rows));
        }
        let rows = std::rc::Rc::new(signature_text_rows::<4>(
            db,
            "SELECT symbol_uid, name, kind, container FROM symbols \
             WHERE symbol_uid IS NOT NULL ORDER BY symbol_uid",
        )?);
        *self.rows.borrow_mut() = Some(std::rc::Rc::clone(&rows));
        Ok(rows)
    }
}

/// Time one postprocess/analysis sub-step and emit a `tracing::debug!` event
/// with the elapsed milliseconds, so a slow `postprocess_ms`/`analysis_ms`
/// aggregate can be attributed to a specific sub-step from logs without a
/// profiler.
pub(crate) fn time_step<T>(phase: &'static str, step: &'static str, f: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let result = f();
    tracing::debug!(
        phase,
        step,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "sub-phase timing"
    );
    result
}

/// Phase 4a output: the [`SymbolCatalog`] seeded with persisted + freshly
/// parsed symbols, the persisted symbols themselves (consumed again by the
/// hierarchy sub-phase), and one pre-built [`ResolutionContext`] per write
/// unit (index-aligned with the write units they were built from).
struct ResolutionCatalog {
    catalog: SymbolCatalog,
    persisted_symbols: Vec<SymbolRecord>,
    resolution_contexts: Vec<ResolutionContext>,
}

/// One non-skipped round of the incremental config-link pass: the units to
/// write plus the config files the scan (or token cache) covered. A seen
/// file without a unit resolved to zero links this round — apply uses the
/// list to clear such files' stale refs from earlier rounds.
struct ConfigLinkRound {
    units: Vec<FileWriteUnit>,
    seen_config_files: Vec<String>,
}

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

// ── Staged postprocess/analysis plans (compute → apply seam) ────────────
//
// Phase 7 (postprocess) and Phase 8-11 (analysis) are split into a COMPUTE
// half (pure reads through the read pool, heavy work: signature scans,
// synthesis passes, Louvain, git log, infra walk) and an APPLY half (short
// DB transactions only). The plan structs below are the typed deltas that
// travel between the two halves; the caller decides how much locking each
// half needs (see `build_plan` for the staging contract).

/// 测试边重建指令：计算本身在 cc-db 的 SQL 里完成，compute 阶段只决定
/// apply 阶段执行哪一种重建。
enum TestEdgeRebuild {
    Skip,
    Full,
    Files(Vec<String>),
}

/// What the apply stage executes for the dispatch-synthesis round.
enum SynthesisAction {
    /// Normal round: one atomic batch write (see `synthesis_pipeline`).
    Round(SynthesisRound),
    /// Synthesis disabled after being enabled previously: delete every
    /// synthetic edge kind/prefix declared by the pass registry.
    DisableCleanup,
}

struct SynthesisStage {
    action: SynthesisAction,
    /// Dispatch + interface signatures, persisted only after the community
    /// apply completed (the historical `RecordTiming::Deferred` semantics: a
    /// later community failure leaves no synthesis signature recorded).
    records: Vec<DeferredSignatureRecord>,
}

enum CommunityAction {
    /// 边数超限的降级路径：未分配社区的符号全部归入 community 0。
    Degraded,
    Update {
        assignments: HashMap<String, u32>,
        labels: HashMap<u32, String>,
    },
}

struct CommunityStage {
    action: CommunityAction,
    record: DeferredSignatureRecord,
}

/// Phase 7 deltas: test edges, dispatch synthesis, community detection.
/// `None` stage fields mean the pass's gate decided to skip this build.
pub(crate) struct PostprocessPlan {
    test_edges: TestEdgeRebuild,
    synthesis: Option<SynthesisStage>,
    community: Option<CommunityStage>,
}

struct CoChangeStage {
    co_changes: Vec<CoChangeEdgeRecord>,
    /// HEAD sha to record after the apply; `None` when git was unavailable or
    /// the analysis degraded — nothing is recorded so the next build retries.
    record_head: Option<String>,
}

struct InfraStage {
    nodes: Vec<InfraNode>,
    edges: Vec<InfraEdge>,
    record: DeferredSignatureRecord,
}

/// Phase 8-11 deltas: git co-change, infrastructure, ADR documents.
pub(crate) struct AnalysisPlan {
    cochange: Option<CoChangeStage>,
    infra: Option<InfraStage>,
    /// ADR docs are re-scanned unconditionally; an empty list writes nothing.
    adr_docs: Vec<serde_json::Value>,
}

impl Indexer {
    /// Phase 4: Symbol resolution (semantic edges, type catalog, call edges, cross-file).
    ///
    /// Thin orchestration over four sub-phases, each of which owns only the
    /// inputs it actually consumes so it can be exercised directly in tests:
    /// [`Self::build_resolution_catalog`] → [`Self::resolve_semantic_edges`] →
    /// [`Self::resolve_hierarchy`] → [`Self::resolve_call_edges`] →
    /// [`Self::resolve_framework_cross_file`].
    pub(crate) fn phase_resolve(
        &self,
        _project_path: &Path,
        full: bool,
        write_units: &mut [FileWriteUnit],
        to_remove: &[String],
        fw_context: &crate::framework_resolvers::ProjectFrameworkContext,
    ) -> CcResult<ResolveResult> {
        let ResolutionCatalog {
            mut catalog,
            persisted_symbols,
            resolution_contexts,
        } = self.build_resolution_catalog(full, write_units, to_remove)?;

        // Phase 4a / 4a-2: semantic edge UIDs + backfill, USES_TYPE derivation.
        Self::resolve_semantic_edges(&catalog, write_units, &resolution_contexts);

        // Phase 4b: type catalog (dispatch) + hierarchy edges.
        let hierarchy_edges =
            Self::resolve_hierarchy(&mut catalog, &persisted_symbols, write_units);

        // Phase 4c: call edges, symbol refs, route edges.
        Self::resolve_call_edges(&catalog, write_units, &resolution_contexts);

        // Phase 4d: cross-file framework resolution (post-catalog).
        Self::resolve_framework_cross_file(&catalog, write_units, fw_context);

        Ok(ResolveResult { hierarchy_edges })
    }

    /// Phase 4a (input construction): seed the [`SymbolCatalog`] with symbols
    /// persisted in the DB (incremental builds only — excluding files being
    /// re-parsed or removed) plus the freshly parsed symbols, and pre-build
    /// one [`ResolutionContext`] per write unit.
    fn build_resolution_catalog(
        &self,
        full: bool,
        write_units: &[FileWriteUnit],
        to_remove: &[String],
    ) -> CcResult<ResolutionCatalog> {
        let resolver_excluded_files: Vec<String> = write_units
            .iter()
            .map(|u| u.rel_path.clone())
            .chain(to_remove.iter().cloned())
            .collect();
        let persisted_symbols = if full {
            Vec::new()
        } else {
            self.db
                .reads()
                .resolver_seed_symbols_excluding(&resolver_excluded_files)?
        };

        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&persisted_symbols);
        for unit in write_units.iter() {
            catalog.add_symbols(&unit.outcome.symbols);
        }

        let resolution_contexts: Vec<ResolutionContext> = write_units
            .iter()
            .map(|unit| SymbolCatalog::build_resolution_context(&unit.outcome, &unit.rel_path))
            .collect();

        Ok(ResolutionCatalog {
            catalog,
            persisted_symbols,
            resolution_contexts,
        })
    }

    /// Phase 4a: resolve semantic edge UIDs and backfill base_types/implements,
    /// then (4a-2) derive USES_TYPE edges from type annotations. Mutates each
    /// unit's outcome in place. `resolution_contexts` must be index-aligned
    /// with `write_units` (as produced by [`Self::build_resolution_catalog`]).
    fn resolve_semantic_edges(
        catalog: &SymbolCatalog,
        write_units: &mut [FileWriteUnit],
        resolution_contexts: &[ResolutionContext],
    ) {
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units
                .par_iter_mut()
                .zip(resolution_contexts.par_iter())
                .for_each(|(unit, context)| {
                    let file_path = unit.rel_path.clone();
                    catalog.resolve_semantic_edges_and_backfill_with_context(
                        &file_path,
                        &mut unit.outcome,
                        context,
                    );
                });
        } else {
            for (unit, context) in write_units.iter_mut().zip(resolution_contexts.iter()) {
                let file_path = unit.rel_path.clone();
                catalog.resolve_semantic_edges_and_backfill_with_context(
                    &file_path,
                    &mut unit.outcome,
                    context,
                );
            }
        }

        // Phase 4a-2: Derive USES_TYPE edges from type annotations
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units.par_iter_mut().for_each(|unit| {
                let file_path = unit.rel_path.clone();
                catalog.derive_uses_type_edges(&file_path, &mut unit.outcome);
            });
        } else {
            for unit in write_units.iter_mut() {
                let file_path = unit.rel_path.clone();
                catalog.derive_uses_type_edges(&file_path, &mut unit.outcome);
            }
        }
    }

    /// Phase 4b: build the TypeCatalog for type-aware method dispatch
    /// resolution (4b), feed it the parsed type_assigns for variable type
    /// inference (4b-1), and generate hierarchy edges — Defines,
    /// DefinesMethod, ContainsFile (4b-2). The type catalog consumes the full
    /// snapshot of all symbols (persisted + freshly parsed); hierarchy edges
    /// are file-local (every rule in [`crate::hierarchy`] keys on the
    /// symbol's/file's own path), so they are generated for the batch files
    /// only — unchanged files keep their stored edges, and the per-file
    /// deletes in the write batch already cover replaced/dirty/removed files
    /// (see `dirty_reload_policy` for the reload-side declaration). On full
    /// builds the batch is the whole project, so this degenerates to the
    /// historical full regeneration.
    fn resolve_hierarchy(
        catalog: &mut SymbolCatalog,
        persisted_symbols: &[SymbolRecord],
        write_units: &[FileWriteUnit],
    ) -> Vec<cc_model::edge::SemanticEdgeRecord> {
        let all_symbols: Vec<_> = persisted_symbols
            .iter()
            .cloned()
            .chain(
                write_units
                    .iter()
                    .flat_map(|u| u.outcome.symbols.iter().cloned()),
            )
            .collect();
        catalog.build_type_catalog(&all_symbols);
        catalog.add_type_assigns_from_outcomes(write_units);

        // `all_symbols` is persisted ++ batch in that order, so the freshly
        // parsed batch symbols are exactly the tail slice.
        let batch_symbols = &all_symbols[persisted_symbols.len()..];
        let file_paths: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        crate::hierarchy::generate_hierarchy_edges(batch_symbols, &file_paths)
    }

    /// Phase 4c: resolve call edges, symbol refs and route edges against the
    /// catalog (type-catalog assisted once [`Self::resolve_hierarchy`] has
    /// run). `resolution_contexts` must be index-aligned with `write_units`.
    fn resolve_call_edges(
        catalog: &SymbolCatalog,
        write_units: &mut [FileWriteUnit],
        resolution_contexts: &[ResolutionContext],
    ) {
        if write_units.len() >= MIN_FILES_FOR_PARALLEL {
            write_units
                .par_iter_mut()
                .zip(resolution_contexts.par_iter())
                .for_each(|(unit, context)| {
                    let file_path = unit.rel_path.clone();
                    catalog.resolve_outcome_with_context(&file_path, &mut unit.outcome, context);
                });
        } else {
            for (unit, context) in write_units.iter_mut().zip(resolution_contexts.iter()) {
                let file_path = unit.rel_path.clone();
                catalog.resolve_outcome_with_context(&file_path, &mut unit.outcome, context);
            }
        }
    }

    /// Phase 4d: cross-file framework resolution (post-catalog).
    ///
    /// Resolvers need `&mut [(String, ParseOutcome)]`. Previously every
    /// outcome was deep-cloned (symbols/edges/refs/chunks) just to hand the
    /// resolvers a mutable view, then a partial subset of edges was merged
    /// back. Instead we *move* each outcome out of its write_unit (leaving a
    /// cheap default in place), let resolvers mutate it in place, and move it
    /// straight back. This eliminates the full-graph deep copy and also
    /// faithfully preserves in-place edge mutations (e.g. route prefix
    /// propagation / handler UID binding) that the old length-only merge
    /// silently dropped.
    fn resolve_framework_cross_file(
        catalog: &SymbolCatalog,
        write_units: &mut [FileWriteUnit],
        fw_context: &crate::framework_resolvers::ProjectFrameworkContext,
    ) {
        let registry = crate::framework_resolvers::default_registry();
        let active = registry.active_resolvers(fw_context);
        if active.is_empty() {
            return;
        }
        let mut owned_pairs: Vec<(String, ParseOutcome)> = write_units
            .iter_mut()
            .map(|u| (u.rel_path.clone(), std::mem::take(&mut u.outcome)))
            .collect();
        for resolver in &active {
            resolver.resolve_cross_file(catalog, &mut owned_pairs, fw_context);
        }
        // Move the (possibly mutated) outcomes back into their units.
        for (unit, (_, outcome)) in write_units.iter_mut().zip(owned_pairs) {
            unit.outcome = outcome;
        }
    }

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
                self.build_config_link_units_gated(project_path, batch_empty)
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

    /// Phase 7 (compute half): test edges, dispatch synthesis, community
    /// detection. Pure reads through the read pool — the heavy work
    /// (signature table scans, synthesis passes, Louvain) all happens here,
    /// so callers may run it without holding any index lock. The signature
    /// gates decide in this stage; their records travel inside the plan and
    /// are persisted by [`Self::phase_postprocess_apply`].
    pub(crate) fn phase_postprocess_compute(
        &self,
        full: bool,
        write_units: &[FileWriteUnit],
        config_units: &[FileWriteUnit],
        to_remove: &[String],
        pre_batch_files: &HashMap<String, FileState>,
    ) -> CcResult<PostprocessPlan> {
        // Test edges for changed files: the rebuild itself is a cc-db SQL
        // operation, so compute only decides WHICH rebuild apply runs.
        //
        // Update-only batches skip the rebuild outright: test edges are
        // path-derived (endpoints are file paths; matching depends only on
        // the path set plus the path-derived `is_test_file` flag), and the
        // write batch no longer cascades test_edges deletes for in-place
        // replacements — so when the batch removed nothing and every written
        // path already existed before the batch (`pre_batch_files` is the
        // scan-time files snapshot, covering dirty-closure and config units
        // too), the committed edges are already exactly the rebuilt ones.
        let mut changed_paths: Vec<String> =
            write_units.iter().map(|u| u.rel_path.clone()).collect();
        changed_paths.extend(config_units.iter().map(|u| u.rel_path.clone()));
        changed_paths.extend(to_remove.iter().cloned());
        let path_set_unchanged = to_remove.is_empty()
            && write_units
                .iter()
                .chain(config_units.iter())
                .all(|u| pre_batch_files.contains_key(&u.rel_path));
        let test_edges = if full {
            TestEdgeRebuild::Full
        } else if !changed_paths.is_empty() && !path_set_unchanged {
            TestEdgeRebuild::Files(changed_paths)
        } else {
            TestEdgeRebuild::Skip
        };

        // Per-pass signature gates: instead of a single graph_signature that
        // hashes all 4 tables, each pass group carries its own input
        // signature. This avoids re-running all 7 synthesis passes + Louvain
        // when only one input changed (e.g. a new dispatch site does not need
        // interface dispatch recomputation, and vice versa).
        //
        // The dispatch and interface signatures share one symbols scan per
        // build via `SymbolRowsCache`, and the synthesis round's records are
        // persisted only after the community apply completed (deferred) — a
        // mid-build failure never records a signature for work that did not
        // finish.
        let forced = if full { Some("full rebuild") } else { None };
        let symbol_rows = SymbolRowsCache::default();

        let dispatch_gate = DbSignatureGate::new(
            "dispatch_synthesis",
            &self.db,
            "last_dispatch_sig",
            "last_dispatch_sig_algo",
            DISPATCH_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "dispatch_signature", || {
                    self.dispatch_synthesis_signature_from(&symbol_rows)
                })
            },
        );
        let interface_gate = DbSignatureGate::new(
            "interface_dispatch",
            &self.db,
            "last_interface_sig",
            "last_interface_sig_algo",
            INTERFACE_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "interface_signature", || {
                    self.interface_dispatch_signature_from(&symbol_rows)
                })
            },
        );
        // The two signatures gate one synthesis round: the round runs when
        // either input changed, and the individual decisions route work to
        // the dispatch- vs interface-gated sub-passes inside the round (see
        // `dispatch_synthesis::SynthesisPassSpec`).
        let synthesis_gate = PairGate::new("synthesis_round", &dispatch_gate, &interface_gate);
        let synthesis_decision = synthesis_gate.should_run()?;
        log_gate_decision(&synthesis_gate, synthesis_decision);

        // Phase 7b–7h: Dynamic dispatch synthesis. Compute every pass delta
        // against the committed snapshot; the apply stage writes all deltas
        // in one short atomic unit of work. See `crate::synthesis_pipeline`
        // for the cross-pass overlay and the concurrency notes.
        let synthesis = if synthesis_decision.run {
            let action = if self.dispatch_synthesis {
                let synthesis_config = crate::dispatch_synthesis::SynthesisConfig {
                    enabled: true,
                    event_fanout_cap: self.event_fanout_cap,
                    generic_event_denylist: if self.event_denylist.is_empty() {
                        crate::dispatch_synthesis::SynthesisConfig::default().generic_event_denylist
                    } else {
                        self.event_denylist.iter().cloned().collect()
                    },
                };
                let round = time_step("postprocess", "synthesis_round", || {
                    crate::synthesis_pipeline::compute_synthesis_round(
                        &self.db,
                        &synthesis_config,
                        synthesis_gate.first_changed(),
                        synthesis_gate.second_changed(),
                    )
                })?;
                SynthesisAction::Round(round)
            } else {
                // Synthesis disabled after a previous enabled run: the apply
                // stage removes stale synthetic edges (deletion set derived
                // from each pass's declared owned kinds/prefixes).
                SynthesisAction::DisableCleanup
            };
            Some(SynthesisStage {
                action,
                records: vec![
                    dispatch_gate.deferred_record()?,
                    interface_gate.deferred_record()?,
                ],
            })
        } else {
            None
        };

        // Community detection conceptually runs AFTER synthesis: its inputs
        // include synthetic edges. The staged round has not been applied yet,
        // so the committed call graph is projected forward in memory (see
        // `community_edges_with_overlay`) — both the gate signature and the
        // Louvain input therefore match the post-apply DB state. When the
        // round was skipped the synthetic edges are unchanged and the
        // committed state is already the post-round state.
        let community_edges = time_step("postprocess", "community_edges", || {
            self.community_edges_with_overlay(synthesis.as_ref().map(|s| &s.action))
        })?;
        let community_gate = DbSignatureGate::new(
            "community",
            &self.db,
            "last_community_sig",
            "last_community_sig_algo",
            COMMUNITY_SIG_ALGORITHM,
            forced,
            || {
                time_step("postprocess", "community_signature", || {
                    self.community_signature_from_edges(&community_edges, &symbol_rows)
                })
            },
        );
        let community_decision = community_gate.should_run()?;
        log_gate_decision(&community_gate, community_decision);
        let community = if community_decision.run {
            Some(CommunityStage {
                action: time_step("postprocess", "louvain", || {
                    self.compute_community_action(&community_edges)
                })?,
                record: community_gate.deferred_record()?,
            })
        } else {
            None
        };

        Ok(PostprocessPlan {
            test_edges,
            synthesis,
            community,
        })
    }

    /// Phase 7 (apply half): short DB transactions only — test-edge rebuild,
    /// synthesis round apply, community update, then the deferred signature
    /// records. Record ordering preserves the historical `RecordTiming`
    /// semantics: community records before the synthesis signatures, so a
    /// community failure leaves no synthesis signature recorded.
    pub(crate) fn phase_postprocess_apply(&self, plan: &PostprocessPlan) -> CcResult<()> {
        time_step("postprocess", "test_edges_apply", || {
            match &plan.test_edges {
                TestEdgeRebuild::Full => self.db.writes().rebuild_test_edges()?,
                TestEdgeRebuild::Files(paths) => {
                    self.db.writes().rebuild_test_edges_for_files(paths)?
                }
                TestEdgeRebuild::Skip => {}
            }
            CcResult::Ok(())
        })?;

        if let Some(stage) = &plan.synthesis {
            match &stage.action {
                // All deltas land in one short atomic unit of work; the apply
                // is all-or-nothing.
                SynthesisAction::Round(round) => {
                    time_step("postprocess", "synthesis_apply", || {
                        crate::synthesis_pipeline::apply_synthesis_round(&self.db, round)
                    })?;
                }
                SynthesisAction::DisableCleanup => {
                    // If synthesis was enabled in a previous run and is
                    // disabled now, proactively remove stale synthetic edges.
                    // The deletion set is derived from each pass's declared
                    // owned kinds/prefixes, so a new pass is covered here the
                    // moment its spec is registered.
                    let mut removed_edges = 0usize;
                    for spec in crate::dispatch_synthesis::registry() {
                        for kind in spec.owned_call_kinds {
                            removed_edges += self.db.writes().delete_synthetic_call_edges(kind)?;
                        }
                        for prefix in spec.owned_semantic_prefixes {
                            removed_edges +=
                                self.db.writes().delete_synthetic_semantic_edges(prefix)?;
                        }
                    }
                    if removed_edges > 0 {
                        tracing::info!(
                            removed_edges,
                            "dispatch synthesis disabled; removed stale synthetic edges"
                        );
                    }
                }
            }
        }

        if let Some(stage) = &plan.community {
            time_step("postprocess", "community_apply", || {
                match &stage.action {
                    CommunityAction::Degraded => {
                        self.db.writes().assign_all_symbols_to_community(0)?;
                    }
                    CommunityAction::Update {
                        assignments,
                        labels,
                    } => {
                        self.db.writes().update_communities(assignments, labels)?;
                    }
                }
                CcResult::Ok(())
            })?;
            stage.record.record(&self.db)?;
        }
        if let Some(stage) = &plan.synthesis {
            for record in &stage.records {
                record.record(&self.db)?;
            }
        }
        Ok(())
    }

    /// Louvain (or the OOM-degradation decision) over the projected
    /// post-apply edge set.
    fn compute_community_action(&self, edges: &[(String, String)]) -> CcResult<CommunityAction> {
        // Guard: cap the edge count before running Louvain to prevent OOM.
        let max_community_edges: usize = std::env::var("CODECORTEX_COMMUNITY_MAX_EDGES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2_000_000);

        if edges.len() > max_community_edges {
            tracing::warn!(
                edge_count = edges.len(),
                max_community_edges,
                "community detection: edge count exceeds limit, assigning all symbols to community 0"
            );
            return Ok(CommunityAction::Degraded);
        }

        let assignments = louvain_communities(edges, 20);
        let symbol_names = self.db.reads().symbol_names_by_uid()?;
        let labels = build_community_labels(&assignments, &symbol_names);
        Ok(CommunityAction::Update {
            assignments,
            labels,
        })
    }

    /// Project the committed call graph forward across a staged synthesis
    /// action: committed (caller_uid, callee_uid) pairs minus the synthetic
    /// kinds the action deletes, plus the round's in-memory inserts (both-UID
    /// edges only, mirroring the SQL `NOT NULL` filter). Once the action is
    /// applied, the DB edge set equals this projection — community detection
    /// can therefore compute against post-apply state before the apply runs.
    fn community_edges_with_overlay(
        &self,
        action: Option<&SynthesisAction>,
    ) -> CcResult<Vec<(String, String)>> {
        let deleted_kinds: Vec<&'static str> = match action {
            None => Vec::new(),
            Some(SynthesisAction::Round(round)) => round
                .deltas
                .iter()
                .flat_map(|delta| delta.delete_call_kinds.iter().copied())
                .collect(),
            Some(SynthesisAction::DisableCleanup) => crate::dispatch_synthesis::registry()
                .iter()
                .flat_map(|spec| spec.owned_call_kinds.iter().copied())
                .collect(),
        };

        let mut edges = if deleted_kinds.is_empty() {
            self.db.reads().call_uid_edges()?
        } else {
            let placeholders = (1..=deleted_kinds.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
                 AND (synthesized_by IS NULL OR synthesized_by NOT IN ({placeholders}))"
            );
            let params: Vec<String> = deleted_kinds.iter().map(|kind| kind.to_string()).collect();
            self.db
                .reads()
                .query_json(&sql, &params)?
                .into_iter()
                .filter_map(|row| {
                    let caller = row.get("caller_symbol_uid")?.as_str()?.to_string();
                    let callee = row.get("callee_symbol_uid")?.as_str()?.to_string();
                    Some((caller, callee))
                })
                .collect()
        };

        if let Some(SynthesisAction::Round(round)) = action {
            for delta in &round.deltas {
                for edge in &delta.insert_call_edges {
                    if let (Some(caller), Some(callee)) =
                        (&edge.caller_symbol_uid, &edge.callee_symbol_uid)
                    {
                        edges.push((caller.clone(), callee.clone()));
                    }
                }
            }
        }
        Ok(edges)
    }

    /// Phase 8-11 (compute half): git co-change, infrastructure, and ADR
    /// indexing. Reads git, the filesystem, and the read pool only — no index
    /// writes, so callers may run it without holding any index lock.
    pub(crate) fn phase_analysis_compute(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<AnalysisPlan> {
        // Phase 8: Git co-change analysis. HEAD-skip: co-change edges only
        // depend on commit history. If HEAD has not advanced since the last
        // successful analysis, the result is unchanged (the `--since=1.year`
        // window drifts but produces equivalent output while HEAD is fixed),
        // so the git log + parse + write can be skipped.
        let cochange_gate =
            StringCacheGate::new("git_cochange", &self.db, COCHANGE_HEAD_KEY, || {
                crate::git_cochange::current_git_head(project_path)
            });
        let cochange_decision = cochange_gate.should_run()?;
        log_gate_decision(&cochange_gate, cochange_decision);
        let cochange = if cochange_decision.run {
            match time_step("analysis", "cochange_scan", || {
                crate::git_cochange::analyze_cochanges(project_path, 2, 0.2, 500)
            }) {
                Ok(co_changes) => Some(CoChangeStage {
                    co_changes,
                    record_head: cochange_gate.record_key(),
                }),
                Err(err) => {
                    // Non-fatal: git may not be available or the project may
                    // not be a git repo. The HEAD marker stays unrecorded so a
                    // transient failure never poisons the skip cache.
                    tracing::warn!(error = %err, "skipping git co-change analysis");
                    Some(CoChangeStage {
                        co_changes: Vec::new(),
                        record_head: None,
                    })
                }
            }
        } else {
            None
        };

        // Phase 9: Infrastructure pass.
        //
        // The infra pass scans the whole project (Dockerfiles, compose, K8s,
        // terraform, compile_commands) independently of the language parser
        // pipeline — so infra files generally never appear in `write_units` and
        // their changes cannot be inferred from it. To stay strictly correct
        // *and* skip when unchanged, gate the pass on a signature over the infra
        // candidate set (paths + mtime + size); see `infra_pass::infra_signature`.
        let infra_gate = FileSignatureGate::new(
            "infra",
            &self.db,
            "last_infra_sig",
            "last_infra_sig_algo",
            INFRA_SIG_ALGORITHM,
            || {
                time_step("analysis", "infra_signature", || {
                    crate::infra_pass::infra_signature(project_path)
                })
            },
        );
        let infra_decision = infra_gate.should_run()?;
        log_gate_decision(&infra_gate, infra_decision);
        let infra = if infra_decision.run {
            let (mut infra_nodes, mut infra_edges) = time_step("analysis", "infra_scan", || {
                crate::infra_pass::run_infra_pass(project_path)
            });
            if !infra_nodes.is_empty() || !infra_edges.is_empty() {
                // Bind infra nodes to code symbols before persisting
                let bind_symbols: Vec<_> = write_units
                    .iter()
                    .flat_map(|u| u.outcome.symbols.iter().cloned())
                    .collect();
                crate::infra_pass::bind_infra_to_symbols(&mut infra_nodes, &bind_symbols);

                // Match binding target URLs to known route nodes
                crate::infra_pass::match_bindings_to_routes(&mut infra_edges, route_nodes);
            }
            Some(InfraStage {
                nodes: infra_nodes,
                edges: infra_edges,
                record: infra_gate.deferred_record(),
            })
        } else {
            None
        };

        // Phase 10: Architecture Decision Records (ADR) indexing — no skip
        // condition, rescanned every build.
        let adr_docs = time_step("analysis", "adr_scan", || {
            Self::collect_adr_docs(project_path)
        });

        Ok(AnalysisPlan {
            cochange,
            infra,
            adr_docs,
        })
    }

    /// Phase 8-11 (apply half): short DB transactions only. Per-pass record
    /// ordering matches the historical immediate-record loop: each pass's
    /// marker is persisted right after its own write, so a later pass failure
    /// never unrecords an earlier completed pass.
    pub(crate) fn phase_analysis_apply(&self, plan: &AnalysisPlan) -> CcResult<()> {
        if let Some(stage) = &plan.cochange {
            if !stage.co_changes.is_empty() {
                self.db
                    .writes()
                    .insert_co_change_edges_batch(&stage.co_changes)?;
                tracing::info!(
                    count = stage.co_changes.len(),
                    "indexed git co-change edges"
                );
            }
            if let Some(head) = &stage.record_head {
                self.db.writes().set_metadata(COCHANGE_HEAD_KEY, head)?;
            }
        }

        if let Some(stage) = &plan.infra {
            if !stage.nodes.is_empty() || !stage.edges.is_empty() {
                self.db
                    .writes()
                    .replace_infra_data(&stage.nodes, &stage.edges)?;
                let bound_count = stage
                    .nodes
                    .iter()
                    .filter(|n| n.bound_symbol_uid.is_some())
                    .count();
                let binding_count = stage
                    .edges
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.kind,
                            cc_model::infra::InfraEdgeKind::BindsTopic
                                | cc_model::infra::InfraEdgeKind::ConsumesQueue
                        )
                    })
                    .count();
                tracing::info!(
                    nodes = stage.nodes.len(),
                    edges = stage.edges.len(),
                    bound = bound_count,
                    bindings = binding_count,
                    "indexed infra graph"
                );
            }
            stage.record.record(&self.db)?;
        }

        if !plan.adr_docs.is_empty() {
            tracing::info!(count = plan.adr_docs.len(), "indexed ADR documents");
            self.db.writes().set_metadata(
                "adr_documents",
                &serde_json::to_string(&plan.adr_docs).unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    /// Scan the conventional ADR directories and extract MADR-format headers.
    /// Pure filesystem read.
    fn collect_adr_docs(project_path: &Path) -> Vec<serde_json::Value> {
        let adr_dirs = [
            "docs/adr",
            "docs/decisions",
            "doc/architecture/decisions",
            "doc/adr",
        ];
        let mut adr_docs = Vec::new();

        for dir in &adr_dirs {
            let adr_path = project_path.join(dir);
            if adr_path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&adr_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().is_some_and(|e| e == "md") {
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                // Extract MADR-format header
                                let mut title = None;
                                let mut status = None;
                                let mut date = None;
                                for line in content.lines().take(20) {
                                    if title.is_none() && line.starts_with("# ") {
                                        title = Some(line.trim_start_matches("# ").to_string());
                                    }
                                    if line.to_lowercase().starts_with("status:") {
                                        status = Some(
                                            line.split(':').nth(1).unwrap_or("").trim().to_string(),
                                        );
                                    }
                                    if line.to_lowercase().starts_with("date:") {
                                        date = Some(
                                            line.split(':').nth(1).unwrap_or("").trim().to_string(),
                                        );
                                    }
                                }
                                if let Some(t) = title {
                                    let rel = path
                                        .strip_prefix(project_path)
                                        .unwrap_or(&path)
                                        .to_string_lossy()
                                        .to_string();
                                    adr_docs.push(serde_json::json!({
                                        "file": rel,
                                        "title": t,
                                        "status": status,
                                        "date": date,
                                    }));
                                }
                            }
                        }
                    }
                }
            }
        }
        adr_docs
    }

    pub(crate) fn collect_route_nodes(
        &self,
        write_units: &[FileWriteUnit],
    ) -> Vec<RouteNodeRecord> {
        let mut route_nodes = Vec::new();
        for unit in write_units {
            for route in &unit.outcome.route_edges {
                route_nodes.push(RouteNodeRecord {
                    route_id: StableId::edge_id(
                        "route_node",
                        &route.file_path,
                        route.line,
                        route.start_col,
                    ),
                    file_path: route.file_path.clone(),
                    route_path: route.route_path.clone(),
                    method: route.method.clone(),
                    handler_symbol_uid: route.handler_symbol_uid.clone(),
                    handler_name: route.handler_name.clone(),
                    framework: route.framework.clone(),
                    line: route.line,
                    end_line: route.end_line,
                    normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                        &route.route_path,
                    )),
                    confidence: route.confidence,
                    parser_tier: route.parser_tier,
                });
            }
        }
        route_nodes
    }

    /// Pure function: build config link units from pre-collected snapshot data
    /// plus pre-scanned raw config tokens (see [`scan_config_tokens`]).
    /// Does not query the database, suitable for use inside temp-db write closure.
    fn build_config_link_units_from_snapshot(
        project_path: &Path,
        symbol_targets: Vec<SymbolTargetRow>,
        indexed_files: &[String],
        raw_tokens: &[RawConfigToken],
    ) -> CcResult<Vec<FileWriteUnit>> {
        let mut known_symbols = HashSet::new();
        let mut qname_lookup: HashMap<String, (String, Option<String>, String)> = HashMap::new();
        let mut basename_lookup: HashMap<String, Vec<(String, Option<String>, String)>> =
            HashMap::new();
        for sym in symbol_targets {
            if let Some(qname) = sym.qname.clone() {
                known_symbols.insert(qname.clone());
                qname_lookup.insert(
                    qname,
                    (
                        sym.symbol_id.clone(),
                        sym.symbol_uid.clone(),
                        sym.file_path.clone(),
                    ),
                );
            }
            basename_lookup.entry(sym.name.clone()).or_default().push((
                sym.symbol_id,
                sym.symbol_uid,
                sym.file_path,
            ));
        }

        let known_files: HashSet<String> = indexed_files.iter().cloned().collect();
        let mut file_basename_lookup: HashMap<String, Vec<String>> = HashMap::new();
        for file in indexed_files {
            if let Some(base) = Path::new(file).file_name().and_then(|n| n.to_str()) {
                file_basename_lookup
                    .entry(base.to_string())
                    .or_default()
                    .push(file.clone());
            }
        }
        let links = resolve_config_links(raw_tokens, &known_symbols, &known_files);
        if links.is_empty() {
            return Ok(Vec::new());
        }

        let mut grouped: HashMap<String, Vec<_>> = HashMap::new();
        for link in links {
            grouped
                .entry(link.config_file.clone())
                .or_default()
                .push(link);
        }

        let mut units = Vec::new();
        for (config_file, links) in grouped {
            let abs_path = project_path.join(&config_file);
            let content = match std::fs::read_to_string(&abs_path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let metadata = match abs_path.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mut symbol_refs = Vec::new();
            for link in &links {
                let (
                    target_symbol_id,
                    target_symbol_uid,
                    target_file_path,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                ) = match link.link_kind {
                    ConfigLinkKind::ModulePath => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_module_exact".to_string(),
                            )
                        } else {
                            let tail = link
                                .referenced_value
                                .rsplit('.')
                                .next()
                                .unwrap_or(&link.referenced_value);
                            match basename_lookup.get(tail) {
                                Some(candidates) if candidates.len() == 1 => {
                                    let (sid, suid, fpath) = &candidates[0];
                                    (
                                        Some(sid.clone()),
                                        suid.clone(),
                                        Some(fpath.clone()),
                                        ResolutionKind::Heuristic,
                                        link.confidence,
                                        "config_module_suffix".to_string(),
                                    )
                                }
                                _ => (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                ),
                            }
                        }
                    }
                    ConfigLinkKind::FilePath => {
                        let resolved_path = if known_files.contains(&link.referenced_value) {
                            Some(link.referenced_value.clone())
                        } else {
                            Path::new(&link.referenced_value)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .and_then(|base| file_basename_lookup.get(base))
                                .filter(|paths| paths.len() == 1)
                                .and_then(|paths| paths.first().cloned())
                        };
                        match resolved_path {
                            Some(path) => (
                                None,
                                None,
                                Some(path),
                                if known_files.contains(&link.referenced_value) {
                                    ResolutionKind::Exact
                                } else {
                                    ResolutionKind::Heuristic
                                },
                                link.confidence,
                                if known_files.contains(&link.referenced_value) {
                                    "config_file_exact".to_string()
                                } else {
                                    "config_file_basename".to_string()
                                },
                            ),
                            None => (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            ),
                        }
                    }
                    ConfigLinkKind::DependencyImport => {
                        if let Some((sid, suid, fpath)) = qname_lookup.get(&link.referenced_value) {
                            (
                                Some(sid.clone()),
                                suid.clone(),
                                Some(fpath.clone()),
                                ResolutionKind::Exact,
                                link.confidence,
                                "config_dependency_exact".to_string(),
                            )
                        } else if let Some(candidates) = basename_lookup.get(&link.referenced_value)
                        {
                            if candidates.len() == 1 {
                                let (sid, suid, fpath) = &candidates[0];
                                (
                                    Some(sid.clone()),
                                    suid.clone(),
                                    Some(fpath.clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_symbol".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else if let Some(paths) = file_basename_lookup.get(&link.referenced_value)
                        {
                            if paths.len() == 1 {
                                (
                                    None,
                                    None,
                                    Some(paths[0].clone()),
                                    ResolutionKind::Heuristic,
                                    link.confidence,
                                    "config_dependency_file".to_string(),
                                )
                            } else {
                                (
                                    None,
                                    None,
                                    None,
                                    ResolutionKind::Unresolved,
                                    0.0,
                                    "unresolved".to_string(),
                                )
                            }
                        } else {
                            (
                                None,
                                None,
                                None,
                                ResolutionKind::Unresolved,
                                0.0,
                                "unresolved".to_string(),
                            )
                        }
                    }
                };

                symbol_refs.push(SymbolRefRecord {
                    ref_id: StableId::ref_id(&config_file, &link.referenced_value, link.line, 0),
                    file_path: config_file.clone(),
                    symbol_name: link.referenced_value.clone(),
                    container: Some(link.config_key.clone()),
                    ref_kind: match link.link_kind {
                        ConfigLinkKind::ModulePath => "config_module".to_string(),
                        ConfigLinkKind::FilePath => "config_file".to_string(),
                        ConfigLinkKind::DependencyImport => "config_dependency".to_string(),
                    },
                    line: link.line,
                    column: 0,
                    target_symbol_id,
                    target_file_path,
                    target_symbol_uid,
                    ref_name: Some(link.referenced_value.clone()),
                    scope_id: None,
                    resolution_kind,
                    resolution_confidence,
                    resolution_strategy,
                    ref_end_line: Some(link.line),
                    ref_end_col: None,
                    parser_tier: ParserTier::Heuristic,
                    parser_confidence: link.confidence.max(0.70),
                });
            }

            let excerpt: String = links
                .iter()
                .take(6)
                .map(|link| format!("{} -> {}", link.config_key, link.referenced_value))
                .collect::<Vec<_>>()
                .join("; ");

            let outcome = ParseOutcome {
                summary: format!(
                    "Configuration file with {} code link(s){}",
                    symbol_refs.len(),
                    if excerpt.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", excerpt)
                    }
                ),
                symbol_refs,
                parser_tier: ParserTier::Heuristic,
                parser_confidence: 0.85,
                ..Default::default()
            };

            let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            units.push(FileWriteUnit {
                rel_path: config_file,
                language: Language::Unknown,
                content_hash,
                mtime,
                size: metadata.len(),
                outcome,
            });
        }

        Ok(units)
    }

    /// Incremental config-link pass behind a file-set signature gate.
    ///
    /// The expensive half (project walk + read + tokenize, see
    /// [`scan_config_tokens`]) only depends on the config files themselves, so
    /// it is skipped when the config signature (paths + mtime + size) is
    /// unchanged — the raw tokens are then served from the metadata cache.
    /// The cheap half (resolving tokens against the symbol/file catalog) must
    /// still run whenever this build wrote index content, because links
    /// appear/disappear with the catalog (a removed symbol must not keep its
    /// link). When the signature is unchanged AND the batch wrote nothing,
    /// the catalog provably did not change either and the whole pass —
    /// including the catalog reads — is skipped (`None`).
    ///
    /// Second skip (zero-token fast path): when the signature is unchanged
    /// and the cached raw tokens deserialize to an EMPTY list, the resolve
    /// half is a provable no-op — zero tokens resolve to zero links AND an
    /// empty seen-file list, so `apply_config_link_units` would write
    /// nothing — and the catalog reads are skipped even for non-empty
    /// batches. This cannot swallow stale-row clearing: a previous round
    /// that produced links did so from non-empty tokens, and the cache is
    /// recorded together with the signature, so an unchanged signature
    /// serves those same non-empty tokens and the fast path does not fire.
    ///
    /// `Some` means the resolve half ran; the caller must hand the round to
    /// `apply_config_link_units` even when `units` is empty, so that files
    /// which resolved to zero links this round get their stale refs cleared.
    fn build_config_link_units_gated(
        &self,
        project_path: &Path,
        batch_empty: bool,
    ) -> CcResult<Option<ConfigLinkRound>> {
        let sig = time_step("write", "config_sig_walk", || {
            config_files_signature(project_path)
        });
        let recorded_algo = self
            .db
            .reads()
            .get_metadata(CONFIG_SIG_ALGO_KEY)?
            .unwrap_or_else(|| "1".to_string());
        let recorded_sig = self.db.reads().get_metadata(CONFIG_SIG_KEY)?;
        let unchanged = recorded_algo == CONFIG_SIG_ALGORITHM
            && recorded_sig.and_then(|s| s.parse::<u64>().ok()) == Some(sig);

        if unchanged && batch_empty {
            tracing::debug!("config linker: signature unchanged and batch empty, skipping");
            return Ok(None);
        }

        let raw_tokens = if unchanged {
            // 签名未变：原始 token 与上次一致，优先用缓存，缓存缺失/损坏则重扫。
            match time_step("write", "config_token_cache", || {
                self.db
                    .reads()
                    .get_metadata(CONFIG_RAW_CACHE_KEY)
                    .map(|cached| {
                        cached
                            .and_then(|json| serde_json::from_str::<Vec<RawConfigToken>>(&json).ok())
                    })
            })? {
                Some(tokens) if tokens.is_empty() => {
                    // 零 token 快路径：零 token ⇒ 零链接且 seen 为空 ⇒ apply
                    // 必为 no-op，连 catalog 读取一起跳过。上轮若有链接，其
                    // token 非空且与签名一同落盘，签名未变时缓存命中的就是
                    // 那批非空 token —— 不会走到这里，陈旧行清理不受影响。
                    tracing::debug!(
                        "config linker: signature unchanged and cached tokens empty, skipping"
                    );
                    return Ok(None);
                }
                Some(tokens) => {
                    tracing::debug!(
                        tokens = tokens.len(),
                        "config linker: scan skipped, resolving cached raw tokens"
                    );
                    tokens
                }
                None => self.scan_and_record_config_tokens(project_path, sig)?,
            }
        } else {
            self.scan_and_record_config_tokens(project_path, sig)?
        };

        // 本轮扫描（或缓存）覆盖到的配置文件：没有产出单元的即为零链接，
        // apply 时按此清理它们的陈旧 refs。
        let mut seen_config_files: Vec<String> = raw_tokens
            .iter()
            .map(|token| token.config_file.clone())
            .collect();
        seen_config_files.sort();
        seen_config_files.dedup();

        let symbol_targets = time_step("write", "config_symbol_targets", || {
            self.db.reads().list_symbol_targets()
        })?;
        let indexed_files = time_step("write", "config_file_paths", || {
            self.db.reads().list_file_paths()
        })?;
        let units = time_step("write", "config_resolve", || {
            Self::build_config_link_units_from_snapshot(
                project_path,
                symbol_targets,
                &indexed_files,
                &raw_tokens,
            )
        })?;
        Ok(Some(ConfigLinkRound {
            units,
            seen_config_files,
        }))
    }

    /// Run the config scan and persist the gate state. The cache is written
    /// before the signature so a mid-write failure can only leave a stale/
    /// missing signature — which forces a rescan, never a wrong skip.
    fn scan_and_record_config_tokens(
        &self,
        project_path: &Path,
        sig: u64,
    ) -> CcResult<Vec<RawConfigToken>> {
        let raw_tokens = time_step("write", "config_token_scan", || {
            scan_config_tokens(project_path)
        })?;
        match Self::serialize_raw_token_cache(&raw_tokens) {
            // 超出缓存上限：清掉旧缓存，避免新签名配上陈旧 token。
            None => self.db.writes().set_metadata(CONFIG_RAW_CACHE_KEY, "")?,
            Some(serialized) => self
                .db
                .writes()
                .set_metadata(CONFIG_RAW_CACHE_KEY, &serialized)?,
        }
        self.db
            .writes()
            .set_metadata(CONFIG_SIG_KEY, &sig.to_string())?;
        self.db
            .writes()
            .set_metadata(CONFIG_SIG_ALGO_KEY, CONFIG_SIG_ALGORITHM)?;
        Ok(raw_tokens)
    }

    /// Serialize raw tokens for the metadata cache; `None` when over the cap.
    fn serialize_raw_token_cache(raw_tokens: &[RawConfigToken]) -> Option<String> {
        let serialized = serde_json::to_string(raw_tokens).ok()?;
        if serialized.len() > CONFIG_RAW_CACHE_MAX_BYTES {
            tracing::debug!(
                bytes = serialized.len(),
                "config linker: raw token cache over cap, scan will rerun next build"
            );
            return None;
        }
        Some(serialized)
    }

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
    ) -> CcResult<FullSnapshotPayload> {
        // Pre-collect snapshot data for config links before entering the
        // rebuild closure (the closure must not query the live DB).
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        // Full builds always scan. Signature first, scan second: a config
        // file changing in between leaves a stale signature behind, which
        // forces a rescan next build — never a wrong skip.
        let config_sig = config_files_signature(project_path);
        let raw_tokens = scan_config_tokens(project_path)?;
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
    /// units and metadata into the connection handed out by either rebuild
    /// adapter.
    fn write_full_snapshot_contents(
        conn: &rusqlite::Connection,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        payload: &FullSnapshotPayload,
        chunk_blobs: &PrecompressedChunks,
    ) -> CcResult<()> {
        // Write main file data (chunk payloads pre-compressed during prepare;
        // missing entries fall back to the identical in-transaction policy).
        for unit in write_units {
            IndexDb::insert_file_data_precompressed(
                conn,
                unit,
                chunk_blobs.get(&unit.rel_path).map(Vec::as_slice),
            )?;
        }

        // Write route nodes
        for r in route_nodes {
            IndexDb::insert_route_node_into(conn, r)?;
        }

        // Hierarchy edges go into the temp-db before the atomic swap, so the
        // rebuilt snapshot can never become visible without them (writing
        // them after the swap would leave a crash window where every file's
        // content_hash is committed but its hierarchy edges are missing).
        IndexDb::insert_semantic_edges_batch_on(conn, hierarchy_edges)?;

        // Write config link units. Scanner 可见的配置文件（yaml/toml 等）已经
        // 作为解析单元写入过 files —— 对它们只追加 config refs（二次
        // insert_file_data 会撞 files 主键，且会丢失解析产物）；其余配置
        // 文件（.ini/.env 等非 scanner 文件）仍整体写入。
        let parsed_paths: HashSet<&str> = write_units.iter().map(|u| u.rel_path.as_str()).collect();
        for unit in &payload.config_units {
            if parsed_paths.contains(unit.rel_path.as_str()) {
                IndexDb::insert_config_link_refs(conn, unit)?;
            } else {
                IndexDb::insert_file_data(conn, unit)?;
            }
        }

        // Write metadata
        IndexDb::set_metadata_on(conn, "last_indexed_at", &payload.recorded_at)?;
        IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

        // Config-linker gate state goes into the rebuilt snapshot (the swap
        // replaces the whole DB file), so the next incremental can skip the
        // config scan. Cache before signature, same as the incremental path.
        IndexDb::set_metadata_on(
            conn,
            CONFIG_RAW_CACHE_KEY,
            payload.config_raw_cache.as_deref().unwrap_or(""),
        )?;
        IndexDb::set_metadata_on(conn, CONFIG_SIG_KEY, &payload.config_sig.to_string())?;
        IndexDb::set_metadata_on(conn, CONFIG_SIG_ALGO_KEY, CONFIG_SIG_ALGORITHM)?;

        Ok(())
    }

    /// Write all index data via temp-db + atomic swap (full rebuild only).
    /// All main data (files, route_nodes, config_units, metadata) is written
    /// inside the temp-db transaction. Post-processing passes (frameworks,
    /// communities, test_edges, git co-changes, infra) run after the swap
    /// against the live DB.
    fn write_full_snapshot_via_temp_db(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        chunk_blobs: &PrecompressedChunks,
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = time_step("write", "full_prepare_payload", || {
            self.prepare_full_snapshot_payload(project_path, write_units)
        })?;
        time_step("write", "full_rebuild_temp_db", || {
            self.db.admin().rebuild_with_temp_db(|conn| {
                Self::write_full_snapshot_contents(
                    conn,
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
    fn write_full_snapshot_via_direct_writer(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        hierarchy_edges: &[cc_model::edge::SemanticEdgeRecord],
        chunk_blobs: &PrecompressedChunks,
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = time_step("write", "full_prepare_payload", || {
            self.prepare_full_snapshot_payload(project_path, write_units)
        })?;
        time_step("write", "full_rebuild_direct_writer", || {
            self.db.admin().rebuild_with_direct_writer(|conn| {
                Self::write_full_snapshot_contents(
                    conn,
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

    /// Deterministic signature over the *inputs* of dispatch synthesis and
    /// community detection: the real (non-synthetic) call graph plus the symbol
    /// structure (uid/name/kind/container).
    ///
    /// Synthesis is a pure function of the real call edges + symbols, so its
    /// output (synthetic edges) is fully determined by them; community detection
    /// then runs over real + synthetic edges, which is therefore also determined
    /// by the same inputs. Hashing real edges only (excluding `synthesized_by IS
    /// NOT NULL`) is both sufficient and necessary: necessary because synthesis
    /// writes synthetic edges back into `call_edges`, so a signature that
    /// included them would drift every run and never match.
    ///
    /// Signature covering dispatch synthesis inputs (dispatch_sites + symbols).
    /// Used to gate the 6 dispatch synthesis passes (event_emitter, jsx,
    /// state_setter, field_observer, react_rerender, vue_template).
    ///
    /// `DefaultHasher` (SipHash with a fixed key) is deterministic across
    /// processes, so persisting the resulting u64 across runs is sound.
    #[cfg(test)]
    fn dispatch_synthesis_signature(&self) -> CcResult<u64> {
        self.dispatch_synthesis_signature_from(&SymbolRowsCache::default())
    }

    /// Same as `dispatch_synthesis_signature`, reading the symbols
    /// scan through a shared per-build cache.
    fn dispatch_synthesis_signature_from(&self, symbol_rows: &SymbolRowsCache) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Dispatch sites (input to 6 synthesis passes). Typed scan: the five
        // text columns are hashed in select order, then the line number —
        // value-compatible with the previous `query_json` scan.
        let conn = self.db.reads().read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT site_kind, key, file_path, enclosing_symbol_uid, handler_symbol_uid, \
                 line FROM dispatch_sites ORDER BY site_id",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let site_rows = stmt
            .query_map([], |row| {
                let mut cols: SignatureTextRow<5> = std::array::from_fn(|_| None);
                for (i, slot) in cols.iter_mut().enumerate() {
                    *slot = match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Text(text) => {
                            Some(String::from_utf8_lossy(text).into_owned())
                        }
                        _ => None,
                    };
                }
                let line = match row.get_ref(5)? {
                    rusqlite::types::ValueRef::Integer(n) => n,
                    _ => 0,
                };
                Ok((cols, line))
            })
            .map_err(|e| CcError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcError::Database(e.to_string()))?;

        // Release the pooled connection before `symbol_rows.get` checks out
        // its own — holding both would deadlock a size-1 read pool.
        drop(stmt);
        drop(conn);

        site_rows.len().hash(&mut hasher);
        for (cols, line) in &site_rows {
            for col in cols {
                col.as_deref().unwrap_or("").hash(&mut hasher);
            }
            line.hash(&mut hasher);
        }

        // Symbol structure (all synthesis passes read symbols)
        Self::hash_symbol_rows(&symbol_rows.get(&self.db)?, &mut hasher);

        Ok(hasher.finish())
    }

    /// Signature covering interface dispatch synthesis inputs
    /// (real call_edges + symbols + real semantic_edges).
    #[cfg(test)]
    fn interface_dispatch_signature(&self) -> CcResult<u64> {
        self.interface_dispatch_signature_from(&SymbolRowsCache::default())
    }

    /// Same as `interface_dispatch_signature`, reading the symbols
    /// scan through a shared per-build cache.
    fn interface_dispatch_signature_from(&self, symbol_rows: &SymbolRowsCache) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Real call edges (synthetic excluded)
        let edge_rows = signature_text_rows::<2>(
            &self.db,
            "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
             WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
             AND synthesized_by IS NULL \
             ORDER BY caller_symbol_uid, callee_symbol_uid",
        )?;
        edge_rows.len().hash(&mut hasher);
        for cols in &edge_rows {
            for col in cols {
                col.as_deref().unwrap_or("").hash(&mut hasher);
            }
        }

        // Symbols
        Self::hash_symbol_rows(&symbol_rows.get(&self.db)?, &mut hasher);

        // Real semantic edges (synthetic 'synth:%' excluded)
        let sem_rows = signature_text_rows::<3>(
            &self.db,
            "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
             WHERE edge_id NOT LIKE 'synth:%' ORDER BY edge_id",
        )?;
        sem_rows.len().hash(&mut hasher);
        for cols in &sem_rows {
            for col in cols {
                col.as_deref().unwrap_or("").hash(&mut hasher);
            }
        }

        Ok(hasher.finish())
    }

    /// Signature covering community detection inputs over the committed DB
    /// state (no overlay). Production goes through
    /// [`Self::community_signature_from_edges`] with the staged synthesis
    /// overlay; this wrapper feeds the signature-coverage tests.
    #[cfg(test)]
    fn community_signature(&self) -> CcResult<u64> {
        let edges = self.db.reads().call_uid_edges()?;
        self.community_signature_from_edges(&edges, &SymbolRowsCache::default())
    }

    /// Community signature over an explicit (caller_uid, callee_uid) edge set
    /// — ALL call edges including synthetic ones, conceptually computed AFTER
    /// the synthesis round since synthetic edges affect community structure.
    ///
    /// Value-compatible with the historical DB-scan formula: pairs are hashed
    /// in `(caller, callee)` order — SQLite's default BINARY collation and
    /// Rust's byte-wise `String` ordering agree — so signatures recorded by
    /// older builds still match and never force a spurious Louvain rerun.
    fn community_signature_from_edges(
        &self,
        edges: &[(String, String)],
        symbol_rows: &SymbolRowsCache,
    ) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        let mut ordered: Vec<&(String, String)> = edges.iter().collect();
        ordered.sort();
        edges.len().hash(&mut hasher);
        for (caller, callee) in ordered {
            caller.as_str().hash(&mut hasher);
            callee.as_str().hash(&mut hasher);
        }

        // Symbols (uid + name + kind). `container` is intentionally excluded:
        // community output is Louvain over call-edge uid pairs plus labels
        // built from symbol names by uid, so container is not an input — a
        // container-only change must not force a Louvain rerun. Locked by
        // `community_signature_ignores_container_unlike_synthesis_signatures`.
        // The rows come from the shared per-build cache (same row set and
        // ordering); only the first three columns are hashed, so the value
        // stays identical to the previous dedicated 3-column scan.
        let rows = symbol_rows.get(&self.db)?;
        rows.len().hash(&mut hasher);
        for cols in rows.iter() {
            for col in &cols[..3] {
                col.as_deref().unwrap_or("").hash(&mut hasher);
            }
        }

        Ok(hasher.finish())
    }

    /// Hash symbol structure (uid/name/kind/container) into the given hasher.
    /// Shared by `dispatch_synthesis_signature` and `interface_dispatch_signature`;
    /// the rows come from a per-build [`SymbolRowsCache`] so the symbols table
    /// is scanned once even when both signatures are computed.
    fn hash_symbol_rows(
        rows: &[SignatureTextRow<4>],
        hasher: &mut std::collections::hash_map::DefaultHasher,
    ) {
        use std::hash::Hash;

        rows.len().hash(hasher);
        for cols in rows {
            for col in cols {
                col.as_deref().unwrap_or("").hash(hasher);
            }
        }
    }

    /// Dirty propagation: detect export signature changes and mark importers
    /// as `DirtyResolveOnly` so their cross-file references get re-resolved
    /// against the updated symbol catalog. The returned outcome carries the
    /// closure status so degradations (budget bail, partial closure) surface
    /// on the index report instead of only in logs.
    pub(crate) fn run_dirty_propagation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
    ) -> CcResult<DirtyPropagationOutcome> {
        if !self.dirty_propagation {
            return Ok(DirtyPropagationOutcome {
                marked: 0,
                status: DirtyPropagationStatus::Disabled,
            });
        }

        // Step 1: Collect all Add/Update files (the ones that were freshly parsed)
        let changed_files: Vec<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::Add | FileAction::Update))
            .map(|(p, _)| p.clone())
            .collect();

        // Nothing changed: the closure is trivially converged.
        if changed_files.is_empty() {
            return Ok(DirtyPropagationOutcome {
                marked: 0,
                status: DirtyPropagationStatus::Normal,
            });
        }

        // Step 2: Compare old vs new export fingerprints to find files whose
        //         public API surface actually changed. Fetch all old
        //         fingerprints in one batched query to avoid N+1 round trips.
        let old_fingerprints = self.db.reads().get_export_fingerprints(&changed_files)?;

        // Build a HashMap index over write_units for O(1) lookup per file,
        // avoiding the previous O(changed_files × write_units) linear scan.
        let write_unit_index: HashMap<&str, &FileWriteUnit> = write_units
            .iter()
            .map(|u| (u.rel_path.as_str(), u))
            .collect();

        let mut export_changed_files = Vec::new();
        for file_path in &changed_files {
            // Files with no exported symbols are absent from the map (== None),
            // matching the single-file query's None return.
            let old_fp = old_fingerprints.get(file_path).cloned();
            let new_fp = write_unit_index
                .get(file_path.as_str())
                .and_then(|unit| Self::compute_fingerprint_for_unit(unit));
            if old_fp != new_fp {
                export_changed_files.push(file_path.clone());
            }
        }

        if export_changed_files.is_empty() {
            return Ok(DirtyPropagationOutcome {
                marked: 0,
                status: DirtyPropagationStatus::Normal,
            });
        }

        // Step 3: Fixpoint closure over importers. Round 1 promotes direct
        //         importers of export-changed files; if a promoted file's own
        //         effective export surface changed (re-export chains), its
        //         importers are promoted in the next round, until convergence.
        //         The iteration policy, global budget, and round cap all live
        //         in `compute_dirty_closure`.
        // Per-file resolved re-export targets, memoized across rounds and
        // re-evaluation passes so each file's targets are fetched at most
        // once (one batched query per pass for the not-yet-cached files).
        let mut reexport_targets_cache: HashMap<String, Vec<String>> = HashMap::new();
        let closure_result = crate::dirty_closure::compute_dirty_closure(
            &export_changed_files,
            self.dirty_propagation_max_files,
            crate::dirty_closure::DIRTY_CLOSURE_MAX_ROUNDS,
            |files| self.db.reads().find_importers_of(files),
            |path| matches!(actions.get(path), Some(FileAction::Skip)),
            |files, changed_so_far| {
                self.promoted_export_surfaces_changed(
                    files,
                    changed_so_far,
                    &mut reexport_targets_cache,
                )
            },
        )?;

        // Budget bail (warn already emitted inside the closure): degrade to no
        // propagation, the user should do a full rebuild instead.
        if closure_result.budget_exceeded {
            return Ok(DirtyPropagationOutcome {
                marked: 0,
                status: DirtyPropagationStatus::BudgetExceeded,
            });
        }

        // Step 4: Promote Skip → DirtyResolveOnly
        let marked = closure_result.promoted.len();
        for importer in &closure_result.promoted {
            if let Some(action) = actions.get_mut(importer) {
                *action = FileAction::DirtyResolveOnly;
            }
        }

        if marked > 0 {
            tracing::info!(
                marked,
                export_changed = export_changed_files.len(),
                rounds = closure_result.rounds_run,
                partial = closure_result.partial,
                "dirty propagation: marked files for re-resolution"
            );
        }

        Ok(DirtyPropagationOutcome {
            marked,
            status: closure_result.status(),
        })
    }

    /// Which of the given promoted (DirtyResolveOnly) files' *effective*
    /// export surfaces changed, given the set of files whose exports changed
    /// so far (batch hook for `compute_dirty_closure`).
    ///
    /// Promoted files are reloaded verbatim from the DB (`phase_dirty_reload`
    /// does not re-parse), so their own export fingerprint provably cannot
    /// change within this build — the in-memory and DB fingerprint formulas
    /// are locked together by `in_memory_and_db_fingerprints_match`. What CAN
    /// change is the surface contributed by re-exports (`export * from './b'`,
    /// `export { x } from './b'`): when a promoted file re-exports from a
    /// changed file, its own importers observe a changed surface and must be
    /// re-resolved too.
    ///
    /// Re-export targets are fetched via one batched
    /// `reexport_targets_for_files` query per pass (only for files not yet in
    /// `targets_cache`, which memoizes them across rounds and re-evaluation
    /// passes), replacing the previous per-file N+1 query.
    ///
    /// Coverage: the jsts extractor sets `is_reexport = 1` for
    /// single-statement re-exports (`export * from './b'`,
    /// `export { x } from './b'`) AND for two-step forwarding via ES imports
    /// (`import { x } from './b'; export { x };`, including `as` aliasing and
    /// `export default x` of an imported binding), so surface changes flowing
    /// through such files promote their importers.
    ///
    /// Known remaining gaps: CommonJS forwarding
    /// (`const { x } = require('./b'); module.exports = { x }` or mixed
    /// `export { x }`) is still stored as a plain import, and other language
    /// extractors never set the flag (e.g. Python `from b import *` /
    /// `__init__.py` star re-exports, Rust `pub use`), so equivalent
    /// forwarding in those languages is still missed.
    fn promoted_export_surfaces_changed(
        &self,
        files: &[String],
        changed_so_far: &HashSet<String>,
        targets_cache: &mut HashMap<String, Vec<String>>,
    ) -> CcResult<Vec<String>> {
        let missing: Vec<&str> = files
            .iter()
            .filter(|path| !targets_cache.contains_key(path.as_str()))
            .map(|path| path.as_str())
            .collect();
        if !missing.is_empty() {
            let mut fetched = self.db.reads().reexport_targets_for_files(&missing)?;
            for path in missing {
                // Files with no resolved re-exports are absent from the batch
                // result; cache an empty target list so they are not refetched.
                let targets = fetched.remove(path).unwrap_or_default();
                targets_cache.insert(path.to_string(), targets);
            }
        }
        Ok(files
            .iter()
            .filter(|path| {
                targets_cache
                    .get(path.as_str())
                    .is_some_and(|targets| targets.iter().any(|t| changed_so_far.contains(t)))
            })
            .cloned()
            .collect())
    }

    /// Compute the export fingerprint from freshly-parsed write_units.
    ///
    /// The algorithm matches `IndexDb::get_export_fingerprint()`:
    ///   1. Select exported symbols (export_name IS NOT NULL or is_default_export)
    ///   2. Format each as "uid|name|signature|export_name"
    ///   3. Sort by uid (first field)
    ///   4. Join with "\n" and hash with blake3
    ///
    /// Note: For hot-path usage (e.g. looping over many files), prefer building
    /// a HashMap index over `write_units` and calling `compute_fingerprint_for_unit`
    /// directly to avoid O(n) linear scan per call.
    #[cfg(test)]
    pub(crate) fn compute_new_export_fingerprint(
        write_units: &[FileWriteUnit],
        file_path: &str,
    ) -> Option<String> {
        let unit = write_units.iter().find(|u| u.rel_path == file_path)?;
        Self::compute_fingerprint_for_unit(unit)
    }

    /// Compute the export fingerprint for a single pre-found `FileWriteUnit`.
    ///
    /// This is the inner computation extracted from `compute_new_export_fingerprint`
    /// so callers that already have a reference to the unit (e.g. via a HashMap
    /// index) can skip the linear search.
    fn compute_fingerprint_for_unit(unit: &FileWriteUnit) -> Option<String> {
        let mut parts: Vec<String> = unit
            .outcome
            .symbols
            .iter()
            .filter(|s| s.export_name.is_some() || s.is_default_export)
            .map(|s| {
                format!(
                    "{}|{}|{}|{}",
                    s.symbol_uid.as_deref().unwrap_or(""),
                    s.name,
                    s.signature.as_deref().unwrap_or(""),
                    s.export_name.as_deref().unwrap_or(""),
                )
            })
            .collect();
        // Sort by the uid prefix (whole string sort gives the same result
        // because uid is the first field, matching the DB's ORDER BY symbol_uid).
        parts.sort();

        if parts.is_empty() {
            return None;
        }

        let combined = parts.join("\n");
        Some(blake3::hash(combined.as_bytes()).to_hex().to_string())
    }
}

#[cfg(test)]
mod export_fingerprint_contract_tests {
    use super::*;
    use cc_model::symbol::SymbolKind;
    use cc_model::symbol::SymbolRecord;
    use cc_model::{Language, ParserTier};

    fn symbol(
        uid: &str,
        name: &str,
        signature: Option<&str>,
        export_name: Option<&str>,
        is_default_export: bool,
    ) -> SymbolRecord {
        SymbolRecord {
            symbol_id: uid.to_string(),
            file_path: "src/lib.rs".to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            container: None,
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: signature.map(String::from),
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.9,
            qname: Some(name.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: export_name.map(String::from),
            is_default_export,
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

    fn write_unit(symbols: Vec<SymbolRecord>) -> FileWriteUnit {
        let outcome = ParseOutcome {
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.9,
            symbols,
            ..Default::default()
        };
        FileWriteUnit {
            rel_path: "src/lib.rs".to_string(),
            language: Language::Rust,
            content_hash: "hash-contract".to_string(),
            mtime: 1.0,
            size: 100,
            outcome,
        }
    }

    /// Contract: `compute_new_export_fingerprint` (cc-index, in-memory) and
    /// `IndexDb::get_export_fingerprint` (cc-db, SQL) are two independent blake3
    /// implementations whose hashes MUST be byte-for-byte identical for the same
    /// symbols. This test locks that contract so the two can never silently drift.
    #[test]
    fn in_memory_and_db_fingerprints_match() {
        let symbols = vec![
            // Out-of-order uids to exercise the sort/ORDER BY contract.
            symbol(
                "uid_zeta",
                "zeta",
                Some("fn zeta() -> u8"),
                Some("zeta"),
                false,
            ),
            symbol(
                "uid_alpha",
                "alpha",
                Some("fn alpha()"),
                Some("alpha"),
                false,
            ),
            // Default export with no explicit export_name.
            symbol("uid_default", "Widget", Some("struct Widget"), None, true),
            // A non-exported symbol must be ignored by BOTH implementations.
            symbol(
                "uid_priv",
                "private_fn",
                Some("fn private_fn()"),
                None,
                false,
            ),
        ];

        let unit = write_unit(symbols);

        // Persist into a real IndexDb and read the DB-side fingerprint.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("contract.db")).unwrap().0;
        db.writes()
            .replace_files_batch(std::slice::from_ref(&unit))
            .unwrap();
        let db_fp = db.reads().get_export_fingerprint("src/lib.rs").unwrap();

        // Compute the in-memory fingerprint from the same write_unit.
        let mem_fp =
            Indexer::compute_new_export_fingerprint(std::slice::from_ref(&unit), "src/lib.rs");

        assert!(db_fp.is_some(), "expected a non-empty DB fingerprint");
        assert_eq!(
            mem_fp, db_fp,
            "in-memory and DB export fingerprints must be identical"
        );
    }

    /// Contract for the no-exports case: both implementations must return None
    /// when a file has zero exported symbols.
    #[test]
    fn both_return_none_without_exports() {
        let symbols = vec![symbol(
            "uid_priv",
            "helper",
            Some("fn helper()"),
            None,
            false,
        )];
        let unit = write_unit(symbols);

        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("contract_none.db"))
            .unwrap()
            .0;
        db.writes()
            .replace_files_batch(std::slice::from_ref(&unit))
            .unwrap();
        let db_fp = db.reads().get_export_fingerprint("src/lib.rs").unwrap();

        let mem_fp =
            Indexer::compute_new_export_fingerprint(std::slice::from_ref(&unit), "src/lib.rs");

        assert_eq!(db_fp, None);
        assert_eq!(mem_fp, None);
    }
}

#[cfg(test)]
mod graph_signature_coverage_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_indexer() -> (TempDir, Indexer) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("sig_cov.db")).unwrap().0);
        let cfg = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), tmp.path(), &cfg);

        let conn = db.reads().read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                 VALUES('s1','src/x.rs','A','function',1,1,'uA');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) \
                 VALUES('e1','src/x.rs','B',1,'uA','uB');",
        )
        .unwrap();

        (tmp, indexer)
    }

    /// dispatch_synthesis_signature must change when dispatch_sites change,
    /// but NOT when call_edges or semantic_edges change.
    #[test]
    fn dispatch_synthesis_signature_covers_sites_and_symbols() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.dispatch_synthesis_signature().unwrap();

        // A new dispatch site must change the dispatch signature.
        let conn = db.reads().read_conn().unwrap();
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo')",
            [],
        )
        .unwrap();
        let sig_after_site = indexer.dispatch_synthesis_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_site,
            "a new dispatch site must change dispatch_synthesis_signature"
        );

        // A new semantic edge must NOT change the dispatch signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('se1','src/x.rs','A','uA','I','uI','implements')",
            [],
        )
        .unwrap();
        let sig_after_sem = indexer.dispatch_synthesis_signature().unwrap();
        assert_eq!(
            sig_after_site, sig_after_sem,
            "semantic edges must NOT affect dispatch_synthesis_signature"
        );
    }

    /// interface_dispatch_signature must change when semantic_edges or real
    /// call_edges change, but NOT when dispatch_sites change.
    #[test]
    fn interface_dispatch_signature_covers_edges_and_semantics() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.interface_dispatch_signature().unwrap();

        // A new dispatch site must NOT change the interface signature.
        let conn = db.reads().read_conn().unwrap();
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo')",
            [],
        )
        .unwrap();
        let sig_after_site = indexer.interface_dispatch_signature().unwrap();
        assert_eq!(
            sig_base, sig_after_site,
            "dispatch sites must NOT affect interface_dispatch_signature"
        );

        // A real semantic edge must change the interface signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('se1','src/x.rs','A','uA','I','uI','implements')",
            [],
        )
        .unwrap();
        let sig_after_sem = indexer.interface_dispatch_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_sem,
            "a real semantic edge must change interface_dispatch_signature"
        );

        // A synthetic semantic edge ('synth:%') must NOT change the interface
        // signature.
        conn.execute(
            "INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
             VALUES('synth:jsx:1','src/x.rs','A','uA','Foo','uFoo','renders_component')",
            [],
        )
        .unwrap();
        let sig_after_synth = indexer.interface_dispatch_signature().unwrap();
        assert_eq!(
            sig_after_sem, sig_after_synth,
            "synthetic semantic edges must be excluded from interface_dispatch_signature"
        );
    }

    /// community_signature must include ALL call edges (including synthetic),
    /// but must NOT depend on dispatch_sites.
    #[test]
    fn community_signature_includes_all_edges() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let sig_base = indexer.community_signature().unwrap();

        // A synthetic call edge must change the community signature.
        let conn = db.reads().read_conn().unwrap();
        conn.execute(
            "INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
             VALUES('se1','src/x.rs','C',1,'uA','uC','event_emitter')",
            [],
        )
        .unwrap();
        let sig_after_synth = indexer.community_signature().unwrap();
        assert_ne!(
            sig_base, sig_after_synth,
            "a synthetic call edge must change community_signature"
        );
    }

    /// `community_signature` intentionally excludes `container`: community
    /// output is Louvain over call-edge uid pairs plus labels built from
    /// symbol names by uid, so container is not an input and a container-only
    /// change must not force a Louvain rerun. The dispatch/interface
    /// signatures DO hash container (synthesis passes resolve methods through
    /// their containers), so the same change must move both of them.
    #[test]
    fn community_signature_ignores_container_unlike_synthesis_signatures() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let community_before = indexer.community_signature().unwrap();
        let dispatch_before = indexer.dispatch_synthesis_signature().unwrap();
        let interface_before = indexer.interface_dispatch_signature().unwrap();

        let conn = db.reads().read_conn().unwrap();
        conn.execute(
            "UPDATE symbols SET container = 'NewContainer' WHERE symbol_id = 's1'",
            [],
        )
        .unwrap();

        assert_eq!(
            community_before,
            indexer.community_signature().unwrap(),
            "a container-only change must NOT affect community_signature"
        );
        assert_ne!(
            dispatch_before,
            indexer.dispatch_synthesis_signature().unwrap(),
            "a container change must affect dispatch_synthesis_signature"
        );
        assert_ne!(
            interface_before,
            indexer.interface_dispatch_signature().unwrap(),
            "a container change must affect interface_dispatch_signature"
        );
    }

    /// Sharing one symbols scan between the dispatch and interface signatures
    /// must not change their values: computing through a shared
    /// `SymbolRowsCache` yields the same u64s as independent scans.
    #[test]
    fn shared_symbol_rows_cache_preserves_signature_values() {
        let (_tmp, indexer) = setup_indexer();
        let shared = SymbolRowsCache::default();

        assert_eq!(
            indexer.dispatch_synthesis_signature().unwrap(),
            indexer.dispatch_synthesis_signature_from(&shared).unwrap(),
            "dispatch signature must be identical through the shared cache"
        );
        assert_eq!(
            indexer.interface_dispatch_signature().unwrap(),
            indexer.interface_dispatch_signature_from(&shared).unwrap(),
            "interface signature must be identical through the shared cache"
        );
    }

    /// The typed signature scans must stay value-compatible with the
    /// historical `query_json`-based formula: signatures recorded by older
    /// builds must still match after the streaming rewrite, so an upgrade
    /// never forces a spurious synthesis/Louvain rerun (and never wrongly
    /// skips). The historical formula is reproduced verbatim here.
    #[test]
    fn typed_signature_scans_match_historical_json_formula() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        // One row per scanned table, including NULL text columns (s1 has no
        // container; the site has no enclosing/handler uid) so the NULL → ""
        // mapping participates in the comparison.
        let conn = db.reads().read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
                 VALUES('ds1','src/x.rs',3,0,'jsx_tag','Foo');\
             INSERT INTO semantic_edges(edge_id,file_path,source_symbol,source_symbol_uid,target_symbol,target_symbol_uid,relation_kind) \
                 VALUES('se1','src/x.rs','A','uA','I','uI','implements');",
        )
        .unwrap();

        let json_str = |row: &serde_json::Value, col: &str| -> String {
            row.get(col)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };

        let symbols_json = db
            .reads()
            .query_json(
                "SELECT symbol_uid, name, kind, container FROM symbols \
                 WHERE symbol_uid IS NOT NULL ORDER BY symbol_uid",
                &[],
            )
            .unwrap();
        let hash_symbols_json =
            |hasher: &mut DefaultHasher, cols: &[&str]| {
                symbols_json.len().hash(hasher);
                for row in &symbols_json {
                    for col in cols {
                        json_str(row, col).hash(hasher);
                    }
                }
            };

        // Historical dispatch formula.
        let mut hasher = DefaultHasher::new();
        let site_rows = db
            .reads()
            .query_json(
                "SELECT site_kind, key, file_path, line, enclosing_symbol_uid, handler_symbol_uid \
                 FROM dispatch_sites ORDER BY site_id",
                &[],
            )
            .unwrap();
        site_rows.len().hash(&mut hasher);
        for row in &site_rows {
            for col in [
                "site_kind",
                "key",
                "file_path",
                "enclosing_symbol_uid",
                "handler_symbol_uid",
            ] {
                json_str(row, col).hash(&mut hasher);
            }
            row.get("line")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .hash(&mut hasher);
        }
        hash_symbols_json(&mut hasher, &["symbol_uid", "name", "kind", "container"]);
        assert_eq!(
            indexer.dispatch_synthesis_signature().unwrap(),
            hasher.finish(),
            "typed dispatch signature must match the historical json formula"
        );

        // Historical interface formula.
        let mut hasher = DefaultHasher::new();
        let edge_rows = db
            .reads()
            .query_json(
                "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
                 AND synthesized_by IS NULL \
                 ORDER BY caller_symbol_uid, callee_symbol_uid",
                &[],
            )
            .unwrap();
        edge_rows.len().hash(&mut hasher);
        for row in &edge_rows {
            json_str(row, "caller_symbol_uid").hash(&mut hasher);
            json_str(row, "callee_symbol_uid").hash(&mut hasher);
        }
        hash_symbols_json(&mut hasher, &["symbol_uid", "name", "kind", "container"]);
        let sem_rows = db
            .reads()
            .query_json(
                "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
                 WHERE edge_id NOT LIKE 'synth:%' ORDER BY edge_id",
                &[],
            )
            .unwrap();
        sem_rows.len().hash(&mut hasher);
        for row in &sem_rows {
            for col in ["source_symbol_uid", "target_symbol_uid", "relation_kind"] {
                json_str(row, col).hash(&mut hasher);
            }
        }
        assert_eq!(
            indexer.interface_dispatch_signature().unwrap(),
            hasher.finish(),
            "typed interface signature must match the historical json formula"
        );

        // Historical community formula (edges part unchanged in production;
        // the symbols part previously came from a dedicated 3-column scan).
        let mut hasher = DefaultHasher::new();
        let edges = db.reads().call_uid_edges().unwrap();
        let mut ordered: Vec<&(String, String)> = edges.iter().collect();
        ordered.sort();
        edges.len().hash(&mut hasher);
        for (caller, callee) in ordered {
            caller.as_str().hash(&mut hasher);
            callee.as_str().hash(&mut hasher);
        }
        hash_symbols_json(&mut hasher, &["symbol_uid", "name", "kind"]);
        assert_eq!(
            indexer.community_signature().unwrap(),
            hasher.finish(),
            "typed community signature must match the historical json formula"
        );
    }

    /// Per-pass signatures must be independent: changing dispatch_sites only
    /// affects dispatch_synthesis_signature, not interface_dispatch_signature.
    #[test]
    fn per_pass_signatures_are_independent() {
        let (_tmp, indexer) = setup_indexer();
        let db = &indexer.db;

        let dispatch_before = indexer.dispatch_synthesis_signature().unwrap();
        let interface_before = indexer.interface_dispatch_signature().unwrap();

        // Modify only dispatch_sites
        let conn = db.reads().read_conn().unwrap();
        conn.execute(
            "INSERT INTO dispatch_sites(site_id,file_path,line,col,site_kind,key) \
             VALUES('ds2','src/x.rs',5,0,'event_emit','click')",
            [],
        )
        .unwrap();

        let dispatch_after = indexer.dispatch_synthesis_signature().unwrap();
        let interface_after = indexer.interface_dispatch_signature().unwrap();

        assert_ne!(
            dispatch_before, dispatch_after,
            "dispatch_synthesis_signature must change when dispatch_sites change"
        );
        assert_eq!(
            interface_before, interface_after,
            "interface_dispatch_signature must NOT change when only dispatch_sites change"
        );
    }
}

#[cfg(test)]
mod community_overlay_tests {
    use super::*;
    use crate::synthesis_pipeline::{apply_synthesis_round, EdgeDelta, SynthesisRound};
    use cc_model::config::IndexingConfig;
    use cc_model::edge::CallEdgeRecord;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Fixture: one real call edge plus one stale synthetic edge of a kind a
    /// later round replaces.
    fn setup_indexer() -> (TempDir, Indexer) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("overlay.db")).unwrap().0);
        let indexer = Indexer::new(db.clone(), tmp.path(), &IndexingConfig::default());

        let conn = db.reads().read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/x.rs','Rust','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) \
                 VALUES('s1','src/x.rs','A','function',1,1,'uA');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) \
                 VALUES('e1','src/x.rs','B',1,'uA','uB');\
             INSERT INTO call_edges(edge_id,file_path,callee_symbol,line,caller_symbol_uid,callee_symbol_uid,synthesized_by) \
                 VALUES('synth:old','src/x.rs','Old',2,'uA','uOld','event_emitter');",
        )
        .unwrap();

        (tmp, indexer)
    }

    fn synthetic_edge(edge_id: &str, callee_uid: &str) -> CallEdgeRecord {
        CallEdgeRecord {
            edge_id: edge_id.to_string(),
            file_path: "src/x.rs".to_string(),
            callee_symbol: callee_uid.to_string(),
            line: 3,
            caller_symbol_uid: Some("uA".to_string()),
            callee_symbol_uid: Some(callee_uid.to_string()),
            synthesized_by: Some("event_emitter".to_string()),
            ..Default::default()
        }
    }

    /// The staged community overlay (computed BEFORE the synthesis apply)
    /// must equal the committed edge set AFTER the apply — both as a multiset
    /// of uid pairs and through the community signature, so the marker the
    /// apply stage records matches what the next build recomputes from the DB
    /// (no spurious Louvain rerun, no wrong skip).
    #[test]
    fn community_overlay_matches_post_apply_state() {
        let (_tmp, indexer) = setup_indexer();

        let round = SynthesisRound {
            deltas: vec![EdgeDelta {
                delete_call_kinds: vec!["event_emitter"],
                delete_semantic_prefixes: vec![],
                insert_call_edges: vec![
                    synthetic_edge("synth:new1", "uNew1"),
                    synthetic_edge("synth:new2", "uNew2"),
                    // No-UID edges are excluded from the community input by
                    // the SQL NOT NULL filter; the overlay must skip them too.
                    CallEdgeRecord {
                        edge_id: "synth:nouid".to_string(),
                        file_path: "src/x.rs".to_string(),
                        callee_symbol: "Anon".to_string(),
                        line: 4,
                        synthesized_by: Some("event_emitter".to_string()),
                        ..Default::default()
                    },
                ],
                insert_semantic_edges: vec![],
            }],
        };
        let action = SynthesisAction::Round(round);

        let mut overlay = indexer.community_edges_with_overlay(Some(&action)).unwrap();
        let overlay_sig = indexer
            .community_signature_from_edges(&overlay, &SymbolRowsCache::default())
            .unwrap();

        // The stale 'event_emitter' edge is projected out, the new edges in.
        overlay.sort();
        assert_eq!(
            overlay,
            vec![
                ("uA".to_string(), "uB".to_string()),
                ("uA".to_string(), "uNew1".to_string()),
                ("uA".to_string(), "uNew2".to_string()),
            ],
            "overlay must replace the deleted kind with the round's inserts"
        );

        let SynthesisAction::Round(round) = action else {
            unreachable!()
        };
        apply_synthesis_round(&indexer.db, &round).unwrap();

        let mut committed = indexer.db.reads().call_uid_edges().unwrap();
        committed.sort();
        assert_eq!(
            overlay, committed,
            "pre-apply overlay must equal the post-apply committed edge set"
        );
        assert_eq!(
            overlay_sig,
            indexer.community_signature().unwrap(),
            "overlay signature must equal the post-apply DB signature"
        );
    }

    /// With no staged synthesis action the overlay is exactly the committed
    /// edge set (synthetic edges included).
    #[test]
    fn community_overlay_without_round_is_committed_state() {
        let (_tmp, indexer) = setup_indexer();
        let mut overlay = indexer.community_edges_with_overlay(None).unwrap();
        let mut committed = indexer.db.reads().call_uid_edges().unwrap();
        overlay.sort();
        committed.sort();
        assert_eq!(overlay, committed);
    }
}

#[cfg(test)]
mod phase_resolve_subphase_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use cc_model::edge::{CallEdgeRecord, SemanticEdgeRecord, SemanticRelation};
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn symbol(uid: &str, name: &str, file_path: &str, kind: SymbolKind) -> SymbolRecord {
        SymbolRecord {
            symbol_id: uid.to_string(),
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: None,
            start_line: 1,
            end_line: 2,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.9,
            qname: Some(name.to_string()),
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

    fn write_unit(rel_path: &str, outcome: ParseOutcome) -> FileWriteUnit {
        FileWriteUnit {
            rel_path: rel_path.to_string(),
            language: Language::Python,
            content_hash: "h".to_string(),
            mtime: 1.0,
            size: 1,
            outcome,
        }
    }

    fn contexts_for(units: &[FileWriteUnit]) -> Vec<ResolutionContext> {
        units
            .iter()
            .map(|u| SymbolCatalog::build_resolution_context(&u.outcome, &u.rel_path))
            .collect()
    }

    /// Phase 4a (input construction): the persisted seed must exclude files
    /// being re-parsed (present in write_units) and files being removed, and
    /// full builds must never seed from the DB at all.
    #[test]
    fn build_resolution_catalog_seeds_persisted_and_excludes_reparsed() {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("catalog.db")).unwrap().0);
        let cfg = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), tmp.path(), &cfg);

        let conn = db.reads().read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES \
                 ('src/persisted.py','Python','h',1.0,1,'2024-01-01'), \
                 ('src/changed.py','Python','h',1.0,1,'2024-01-01'), \
                 ('src/gone.py','Python','h',1.0,1,'2024-01-01');\
             INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES \
                 ('sp','src/persisted.py','persisted_fn','function',1,1,'uPersist'), \
                 ('sc','src/changed.py','stale_fn','function',1,1,'uStale'), \
                 ('sg','src/gone.py','gone_fn','function',1,1,'uGone');",
        )
        .unwrap();
        drop(conn);

        let units = vec![write_unit(
            "src/changed.py",
            ParseOutcome {
                symbols: vec![symbol(
                    "uNew",
                    "new_fn",
                    "src/changed.py",
                    SymbolKind::Function,
                )],
                ..Default::default()
            },
        )];
        let to_remove = vec!["src/gone.py".to_string()];

        let incremental = indexer
            .build_resolution_catalog(false, &units, &to_remove)
            .unwrap();
        let persisted_names: Vec<&str> = incremental
            .persisted_symbols
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            persisted_names,
            vec!["persisted_fn"],
            "re-parsed and removed files must be excluded from the persisted seed"
        );
        assert_eq!(
            incremental.resolution_contexts.len(),
            units.len(),
            "one pre-built context per write unit"
        );

        // The catalog must contain BOTH the persisted seed and the freshly
        // parsed symbols: call edges to either must resolve through it.
        let mut probe_units = vec![write_unit(
            "src/changed.py",
            ParseOutcome {
                call_edges: vec![
                    CallEdgeRecord {
                        edge_id: "ce1".to_string(),
                        file_path: "src/changed.py".to_string(),
                        callee_symbol: "persisted_fn".to_string(),
                        line: 3,
                        ..Default::default()
                    },
                    CallEdgeRecord {
                        edge_id: "ce2".to_string(),
                        file_path: "src/changed.py".to_string(),
                        callee_symbol: "new_fn".to_string(),
                        line: 4,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        )];
        let probe_contexts = contexts_for(&probe_units);
        Indexer::resolve_call_edges(&incremental.catalog, &mut probe_units, &probe_contexts);
        let edges = &probe_units[0].outcome.call_edges;
        assert_eq!(edges[0].callee_symbol_uid.as_deref(), Some("uPersist"));
        assert_eq!(edges[1].callee_symbol_uid.as_deref(), Some("uNew"));

        let full = indexer
            .build_resolution_catalog(true, &units, &to_remove)
            .unwrap();
        assert!(
            full.persisted_symbols.is_empty(),
            "full builds must not seed persisted symbols from the DB"
        );
    }

    /// Phase 4c: an unresolved call edge whose callee is a unique catalog
    /// symbol must be bound to that symbol's UID and file.
    #[test]
    fn resolve_call_edges_binds_callee_uid_cross_file() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[symbol(
            "uHelper",
            "helper",
            "src/lib.py",
            SymbolKind::Function,
        )]);

        let mut units = vec![write_unit(
            "src/main.py",
            ParseOutcome {
                call_edges: vec![CallEdgeRecord {
                    edge_id: "ce1".to_string(),
                    file_path: "src/main.py".to_string(),
                    callee_symbol: "helper".to_string(),
                    line: 3,
                    ..Default::default()
                }],
                ..Default::default()
            },
        )];
        let contexts = contexts_for(&units);

        Indexer::resolve_call_edges(&catalog, &mut units, &contexts);

        let edge = &units[0].outcome.call_edges[0];
        assert_eq!(edge.callee_symbol_uid.as_deref(), Some("uHelper"));
        assert_eq!(edge.target_file_path.as_deref(), Some("src/lib.py"));
        assert!(
            !edge.resolution_strategy.is_empty(),
            "resolution strategy must be recorded"
        );
    }

    /// Phase 4a: semantic edge source UIDs resolve same-file, target UIDs
    /// resolve cross-file via the catalog (unique global class name).
    #[test]
    fn resolve_semantic_edges_fills_source_and_target_uids() {
        let mut catalog = SymbolCatalog::new();
        let base = symbol("uBase", "Base", "src/base.py", SymbolKind::Class);
        let child = symbol("uChild", "Child", "src/child.py", SymbolKind::Class);
        catalog.add_symbols(&[base, child.clone()]);

        let mut units = vec![write_unit(
            "src/child.py",
            ParseOutcome {
                symbols: vec![child],
                semantic_edges: vec![SemanticEdgeRecord {
                    edge_id: "se1".to_string(),
                    file_path: "src/child.py".to_string(),
                    source_symbol: "Child".to_string(),
                    source_symbol_uid: None,
                    target_symbol: "Base".to_string(),
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Inherits,
                    line: 1,
                    confidence: 0.9,
                    parser_tier: ParserTier::TreeSitter,
                }],
                ..Default::default()
            },
        )];
        let contexts = contexts_for(&units);

        Indexer::resolve_semantic_edges(&catalog, &mut units, &contexts);

        let edge = &units[0].outcome.semantic_edges[0];
        assert_eq!(edge.source_symbol_uid.as_deref(), Some("uChild"));
        assert_eq!(
            edge.target_symbol_uid.as_deref(),
            Some("uBase"),
            "a unique global class name must resolve cross-file"
        );
    }

    /// Phase 4b: a method with a class container produces a DefinesMethod
    /// edge from the class UID to the method UID, and the unit's file path
    /// yields a ContainsFile edge.
    #[test]
    fn resolve_hierarchy_generates_defines_method_edges() {
        let mut catalog = SymbolCatalog::new();
        let class_sym = symbol("uAcc", "Accumulator", "src/lib.py", SymbolKind::Class);
        let mut method_sym = symbol("uAdd", "add", "src/lib.py", SymbolKind::Method);
        method_sym.container = Some("Accumulator".to_string());

        let units = vec![write_unit(
            "src/lib.py",
            ParseOutcome {
                symbols: vec![class_sym, method_sym],
                ..Default::default()
            },
        )];

        let edges = Indexer::resolve_hierarchy(&mut catalog, &[], &units);

        assert!(
            edges.iter().any(|e| {
                e.relation_kind == SemanticRelation::DefinesMethod
                    && e.source_symbol_uid.as_deref() == Some("uAcc")
                    && e.target_symbol_uid.as_deref() == Some("uAdd")
            }),
            "expected class→method DefinesMethod edge; got {:?}",
            edges
        );
        assert!(
            edges
                .iter()
                .any(|e| e.relation_kind == SemanticRelation::ContainsFile),
            "expected folder→file ContainsFile edge; got {:?}",
            edges
        );
    }
}

#[cfg(test)]
mod config_linker_gate_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    const INI_LIB: &str = "script = src/lib.py\n";
    /// Same byte length as [`INI_LIB`], so a swap keeps size (and, with the
    /// mtime restored, the config-file signature) unchanged.
    const INI_WIN: &str = "script = src/win.py\n";

    fn setup_project(ini_content: &str) -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/lib.py"),
            "def lib_handler():\n    return 1\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/win.py"),
            "def win_handler():\n    return 2\n",
        )
        .unwrap();
        std::fs::write(project.join("settings.ini"), ini_content).unwrap();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    /// Resolved config-link targets recorded for `settings.ini`, line order.
    fn config_ref_targets(db: &IndexDb) -> Vec<String> {
        db.reads()
            .query_json(
                "SELECT target_file_path FROM symbol_refs \
                 WHERE file_path = 'settings.ini' AND target_file_path IS NOT NULL \
                 ORDER BY line",
                &[],
            )
            .unwrap()
            .iter()
            .filter_map(|row| {
                row.get("target_file_path")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .collect()
    }

    /// Rewrite a file in place, restoring its original mtime so the
    /// stat-based config signature cannot observe the change (same length
    /// content keeps the size component identical too).
    fn rewrite_preserving_mtime(path: &Path, content: &str) {
        let original_mtime = std::fs::metadata(path).unwrap().modified().unwrap();
        std::fs::write(path, content).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(original_mtime))
            .unwrap();
    }

    /// (a) When the config-file set signature is unchanged, an incremental
    /// build must skip the config scan and re-resolve the cached raw tokens:
    /// rewriting the config in a stat-invisible way must NOT be picked up
    /// (proving the file was never re-read), while the recorded gate
    /// metadata stays put.
    #[test]
    fn incremental_build_with_unchanged_signature_resolves_from_cache() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);
        let sig = db
            .reads()
            .get_metadata(CONFIG_SIG_KEY)
            .unwrap()
            .expect("config signature recorded");
        assert!(
            !db.reads()
                .get_metadata(CONFIG_RAW_CACHE_KEY)
                .unwrap()
                .expect("raw token cache recorded")
                .is_empty(),
            "raw token cache must be persisted"
        );

        // Stat-invisible rewrite + a source edit so the batch is non-empty
        // (re-resolution must still run against the current catalog).
        rewrite_preserving_mtime(&project.join("settings.ini"), INI_WIN);
        std::fs::write(
            project.join("src/lib.py"),
            "def lib_handler():\n    return 1\n\n\ndef lib_extra():\n    return 3\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py"],
            "scan skipped: links must reflect the cached tokens, not the rewritten file"
        );
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap().as_deref(),
            Some(sig.as_str()),
            "unchanged signature must stay recorded"
        );
    }

    /// (b) A visibly modified config file (size change) must be rescanned
    /// and its links rebuilt from the new content.
    #[test]
    fn modified_config_file_is_rescanned_and_relinked() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);

        // Different size → signature changes regardless of mtime resolution.
        std::fs::write(
            project.join("settings.ini"),
            "script = src/win.py\nextra_flag = 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/win.py"],
            "changed config file must be re-scanned and re-linked"
        );
    }

    /// (c) Full builds always scan, even when the recorded signature matches
    /// the (stat-invisible) on-disk state.
    #[test]
    fn full_build_always_rescans_config_files() {
        let (tmp, db, indexer) = setup_project(INI_LIB);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(config_ref_targets(&db), vec!["src/lib.py"]);

        rewrite_preserving_mtime(&project.join("settings.ini"), INI_WIN);
        indexer.build_index(project, true).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/win.py"],
            "full build must rescan config files unconditionally"
        );
    }

    /// (d) Removing a file referenced by a config link must drop that link on
    /// the next incremental build even though the config scan is skipped —
    /// re-resolution of the cached raw tokens against the current catalog is
    /// what keeps links from dangling.
    #[test]
    fn removed_link_target_does_not_leave_dangling_config_link() {
        let (tmp, db, indexer) = setup_project("script = src/lib.py\nhelper = src/win.py\n");
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py", "src/win.py"],
            "both links must resolve initially"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/win.py")).unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            config_ref_targets(&db),
            vec!["src/lib.py"],
            "the link to the removed file must be gone"
        );
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files did not change: the scan must have been skipped"
        );
    }
}

#[cfg(test)]
mod config_link_write_path_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// app.yaml 是 scanner 可见的配置文件（Language::Yaml）：同时走解析通道
    /// （generic chunker 产出 chunks）和 config-link 通道（file-path ref）。
    fn setup_yaml_project() -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/only.py"),
            "def only_handler():\n    return 1\n",
        )
        .unwrap();
        std::fs::write(project.join("app.yaml"), "script: src/only.py\n").unwrap();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    fn count(db: &IndexDb, sql: &str) -> i64 {
        db.reads()
            .query_json(sql, &[])
            .unwrap()
            .first()
            .and_then(|row| row.get("cnt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    }

    /// app.yaml 上的 config-link refs 行数。
    fn config_ref_count(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM symbol_refs WHERE file_path = 'app.yaml' \
             AND ref_kind IN ('config_module','config_file','config_dependency')",
        )
    }

    fn yaml_files_rows(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM files WHERE file_path = 'app.yaml'",
        )
    }

    fn yaml_chunks(db: &IndexDb) -> i64 {
        count(
            db,
            "SELECT COUNT(*) AS cnt FROM chunks WHERE file_path = 'app.yaml'",
        )
    }

    /// 缺陷 A / 变体 (a)：配置文件集未变（签名不变 → cached-token 路径）。
    /// 删除被引用文件后本轮解析为零链接，不再产出替换单元 —— 旧 refs 必须
    /// 被显式清理，且 app.yaml 的 files 行保持存在且唯一。
    #[test]
    fn zero_link_resolution_clears_stale_refs_via_cached_tokens() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/only.py")).unwrap();
        indexer.build_index(project, false).unwrap();

        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files unchanged: this run must take the cached-token path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "zero-link resolution must clear the stale config refs"
        );
        assert_eq!(
            yaml_files_rows(&db),
            1,
            "app.yaml keeps exactly one files row"
        );
    }

    /// 缺陷 A / 变体 (b)：另一个配置文件被改动（签名变化 → fresh-scan 路径），
    /// 而 app.yaml 本身未变、不会被重新解析 —— 陈旧 refs 同样必须清理。
    #[test]
    fn zero_link_resolution_clears_stale_refs_via_fresh_scan() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        std::fs::remove_file(project.join("src/only.py")).unwrap();
        // 触碰另一个配置文件改变配置集签名，强制 fresh scan。
        std::fs::write(project.join("settings.ini"), "flag = 1\n").unwrap();
        indexer.build_index(project, false).unwrap();

        assert_ne!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config set changed: this run must take the fresh-scan path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "zero-link resolution must clear the stale config refs"
        );
        assert_eq!(
            yaml_files_rows(&db),
            1,
            "app.yaml keeps exactly one files row"
        );
    }

    /// 缺陷 B：full build 下 scanner 可见的 yaml 既在解析集又产出 config 单元，
    /// 必须恰好落库一次：解析产物（chunks、yaml language）与 config refs 共存；
    /// 同一棵树的增量构建收敛到同一状态。
    #[test]
    fn full_build_writes_linked_yaml_once_with_parsed_and_config_data() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer
            .build_index(project, true)
            .expect("full build over a linked yaml config must succeed");

        let snapshot = |db: &IndexDb| {
            (
                yaml_files_rows(db),
                yaml_chunks(db),
                config_ref_count(db),
                db.reads()
                    .query_json(
                        "SELECT language FROM files WHERE file_path = 'app.yaml'",
                        &[],
                    )
                    .unwrap()
                    .first()
                    .and_then(|row| row.get("language"))
                    .and_then(|v| v.as_str())
                    .map(String::from),
            )
        };
        let full_state = snapshot(&db);
        assert_eq!(full_state.0, 1, "exactly one files row for app.yaml");
        assert!(full_state.1 > 0, "parsed chunks must be preserved");
        assert!(full_state.2 > 0, "config refs must be present");
        assert_eq!(
            full_state.3.as_deref(),
            Some("yaml"),
            "parsed language must be preserved"
        );

        // 同一 DB 上的增量重建必须收敛（不破坏已合并状态）。
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            snapshot(&db),
            full_state,
            "incremental rebuild on the same db must converge"
        );

        // 同一棵树、全新 DB 的纯增量构建也必须得到同一状态。
        let db2 = Arc::new(IndexDb::open(&project.join("index2.sqlite3")).unwrap().0);
        let indexer2 = Indexer::new(db2.clone(), project, &IndexingConfig::default());
        indexer2.build_index(project, false).unwrap();
        assert_eq!(
            snapshot(&db2),
            full_state,
            "fresh incremental build must match the full-build state"
        );
    }

    /// C4 边界：上轮有链接 → 本轮配置内容被清空（token 归零）。归零必然改变
    /// 配置签名，走 fresh-scan 路径清除旧链接（快路径条件 unchanged=false，
    /// 不可能触发）；其后签名稳定、缓存 token 为空的增量轮走零 token 快路径，
    /// 既不得遗留也不得复活任何 config refs。
    #[test]
    fn zero_token_fast_path_does_not_swallow_link_clearing() {
        let (tmp, db, indexer) = setup_yaml_project();
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(
            config_ref_count(&db) > 0,
            "premise: initial build links app.yaml -> src/only.py"
        );
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();

        // 清空链接内容：签名变化 → fresh scan → 零 token，旧链接必须被清除。
        std::fs::write(project.join("app.yaml"), "note: nothing here\n").unwrap();
        indexer.build_index(project, false).unwrap();
        assert_ne!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config content changed: this run must take the fresh-scan path"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "links must be cleared when the config tokens go to zero"
        );
        assert_eq!(
            db.reads()
                .get_metadata(CONFIG_RAW_CACHE_KEY)
                .unwrap()
                .as_deref(),
            Some("[]"),
            "premise: the recorded token cache is the empty list (fast-path trigger)"
        );

        // 快路径轮：签名未变 + 缓存 token 为空 + 批次非空（源码编辑）。
        let sig = db.reads().get_metadata(CONFIG_SIG_KEY).unwrap();
        std::fs::write(
            project.join("src/other.py"),
            "def other_handler():\n    return 2\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            db.reads().get_metadata(CONFIG_SIG_KEY).unwrap(),
            sig,
            "config files unchanged: the zero-token fast path round keeps the signature"
        );
        assert_eq!(
            config_ref_count(&db),
            0,
            "the zero-token fast path must leave zero config refs in place"
        );
    }
}

#[cfg(test)]
mod hierarchy_incremental_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// 全部 hierarchy 边的稳定序列化（edge_id 与 UID 均为内容确定的），
    /// 排序后用于"增量边集 == 全量重建边集"的等价断言。
    fn hierarchy_edges(db: &IndexDb) -> Vec<String> {
        db.reads()
            .query_json(
                "SELECT edge_id || '|' || relation_kind || '|' || file_path || '|' || \
                 source_symbol || '|' || COALESCE(source_symbol_uid,'') || '|' || \
                 target_symbol || '|' || COALESCE(target_symbol_uid,'') AS row \
                 FROM semantic_edges \
                 WHERE relation_kind IN ('defines','defines_method','contains_file') \
                 ORDER BY row",
                &[],
            )
            .unwrap()
            .iter()
            .filter_map(|r| r.get("row").and_then(|v| v.as_str()).map(String::from))
            .collect()
    }

    /// C1 不变量：增量构建后的 hierarchy 边集必须等于同内容全量重建的边集。
    /// 变更场景覆盖：新增文件进新目录（目录"节点"出现）、删除目录最后一个
    /// 文件（目录"节点"消失）、重命名（旧路径边消失、新路径边出现）。
    #[test]
    fn incremental_hierarchy_edges_match_full_rebuild() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::create_dir_all(project.join("src/solo")).unwrap();
        std::fs::write(
            project.join("src/lib.py"),
            "class Accumulator:\n    def add(self):\n        return 1\n",
        )
        .unwrap();
        std::fs::write(project.join("src/main.py"), "def main_handler():\n    return 2\n")
            .unwrap();
        std::fs::write(
            project.join("src/solo/only.py"),
            "def solo_handler():\n    return 3\n",
        )
        .unwrap();

        // 索引库放在项目树之外，避免 db 文件影响扫描结果的可比性。
        let db_dir = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&db_dir.path().join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        indexer.build_index(project, false).unwrap();
        let initial = hierarchy_edges(&db);
        assert!(
            initial.iter().any(|row| row.contains("dir::src/solo")),
            "premise: the initial build materializes the src/solo dir edge; got {initial:?}"
        );
        assert!(
            initial
                .iter()
                .any(|row| row.contains("defines_method") && row.contains("Accumulator")),
            "premise: class->method DefinesMethod edge exists; got {initial:?}"
        );

        // 变更：新目录新文件 + 删除目录最后一个文件 + 重命名。
        std::fs::create_dir_all(project.join("src/newdir")).unwrap();
        std::fs::write(
            project.join("src/newdir/extra.py"),
            "def extra_handler():\n    return 4\n",
        )
        .unwrap();
        std::fs::remove_file(project.join("src/solo/only.py")).unwrap();
        std::fs::rename(project.join("src/main.py"), project.join("src/renamed.py")).unwrap();

        indexer.build_index(project, false).unwrap();
        let incremental = hierarchy_edges(&db);

        // 同内容全量重建作为基准边集。
        let db_full = Arc::new(
            IndexDb::open(&db_dir.path().join("index_full.sqlite3"))
                .unwrap()
                .0,
        );
        let indexer_full = Indexer::new(db_full.clone(), project, &IndexingConfig::default());
        indexer_full.build_index(project, true).unwrap();
        let full = hierarchy_edges(&db_full);

        assert_eq!(
            incremental, full,
            "incremental hierarchy edge set must equal a same-content full rebuild"
        );
        assert!(
            incremental.iter().any(|row| row.contains("dir::src/newdir")),
            "new directory edge must appear; got {incremental:?}"
        );
        assert!(
            !incremental.iter().any(|row| row.contains("src/solo")),
            "emptied directory must leave no edges behind; got {incremental:?}"
        );
        assert!(
            !incremental.iter().any(|row| row.contains("src/main.py")),
            "renamed-away path must leave no edges behind; got {incremental:?}"
        );
        assert!(
            incremental.iter().any(|row| row.contains("src/renamed.py")),
            "renamed-to path must own its edges; got {incremental:?}"
        );
    }
}

#[cfg(test)]
mod dirty_propagation_fixpoint_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// End-to-end fixpoint propagation over a TS re-export chain:
    /// `c.ts` imports from `a.ts`, `a.ts` does `export * from './b'`, and an
    /// edit to `b.ts` adds a new exported function. The incremental pass must
    /// promote BOTH `a.ts` (direct importer) and `c.ts` (importer of the
    /// re-exporting file) to `DirtyResolveOnly`.
    #[test]
    fn reexport_chain_promotes_transitive_importer_incrementally() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n",
        )
        .unwrap();
        std::fs::write(project.join("a.ts"), "export * from './b';\n").unwrap();
        std::fs::write(
            project.join("c.ts"),
            "import { beta } from './a';\nexport function useBeta(): number { return beta(); }\n",
        )
        .unwrap();

        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let config = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), project, &config);
        indexer.build_index(project, true).unwrap();

        // Premise check: the re-export in a.ts must be persisted with a
        // resolved path to b.ts, otherwise round 2 has nothing to chain on.
        let reexports = db
            .reads()
            .query_json(
                "SELECT resolved_path FROM imports \
                 WHERE file_path = 'a.ts' AND is_reexport = 1",
                &[],
            )
            .unwrap();
        assert!(
            reexports
                .iter()
                .any(|row| row.get("resolved_path").and_then(|v| v.as_str()) == Some("b.ts")),
            "jsts must persist `export * from './b'` as a resolved re-export import; got {:?}",
            reexports
        );

        // Edit b.ts: add a new exported function so its export fingerprint changes.
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n\
             export function gamma(): number { return 2; }\n",
        )
        .unwrap();

        let mut scan = indexer.phase_scan_and_diff(project, false, None).unwrap();
        let to_parse = std::mem::take(&mut scan.to_parse);
        let parse = indexer.phase_parse(project, to_parse).unwrap();
        let mut actions =
            indexer.build_actions_map(&parse.write_units, &scan.existing, &scan.scanned_paths);
        assert!(
            matches!(actions.get("b.ts"), Some(FileAction::Update)),
            "edited b.ts must be re-parsed as Update; got {:?}",
            actions.get("b.ts")
        );

        let outcome = indexer
            .run_dirty_propagation(&mut actions, &parse.write_units)
            .unwrap();

        assert!(
            matches!(actions.get("a.ts"), Some(FileAction::DirtyResolveOnly)),
            "a.ts directly imports b.ts and must be promoted; got {:?}",
            actions.get("a.ts")
        );
        assert!(
            matches!(actions.get("c.ts"), Some(FileAction::DirtyResolveOnly)),
            "c.ts imports a.ts whose re-exported surface changed; got {:?}",
            actions.get("c.ts")
        );
        assert_eq!(outcome.marked, 2, "exactly a.ts and c.ts are promoted");
        assert_eq!(
            outcome.status,
            DirtyPropagationStatus::Normal,
            "a converged closure must classify as normal"
        );
    }

    /// Same chain as `reexport_chain_promotes_transitive_importer_incrementally`,
    /// but the middle file forwards via two steps
    /// (`import { beta } from './b'; export { beta };`) instead of a
    /// single-statement re-export. The jsts extractor must mark the
    /// originating import as `is_reexport = 1` so dirty propagation promotes
    /// the transitive importer `c.ts` as well.
    #[test]
    fn two_step_forwarding_chain_promotes_transitive_importer_incrementally() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n",
        )
        .unwrap();
        std::fs::write(
            project.join("a.ts"),
            "import { beta } from './b';\nexport { beta };\n",
        )
        .unwrap();
        std::fs::write(
            project.join("c.ts"),
            "import { beta } from './a';\nexport function useBeta(): number { return beta(); }\n",
        )
        .unwrap();

        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let config = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), project, &config);
        indexer.build_index(project, true).unwrap();

        // Premise check: the forwarded import in a.ts must be persisted as a
        // resolved re-export, otherwise round 2 has nothing to chain on.
        let reexports = db
            .reads()
            .query_json(
                "SELECT resolved_path FROM imports \
                 WHERE file_path = 'a.ts' AND is_reexport = 1",
                &[],
            )
            .unwrap();
        assert!(
            reexports
                .iter()
                .any(|row| row.get("resolved_path").and_then(|v| v.as_str()) == Some("b.ts")),
            "jsts must persist two-step forwarding (`import {{ beta }} from './b'; \
             export {{ beta }};`) as a resolved re-export import; got {:?}",
            reexports
        );

        // Edit b.ts: add a new exported function so its export fingerprint changes.
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n\
             export function gamma(): number { return 2; }\n",
        )
        .unwrap();

        let mut scan = indexer.phase_scan_and_diff(project, false, None).unwrap();
        let to_parse = std::mem::take(&mut scan.to_parse);
        let parse = indexer.phase_parse(project, to_parse).unwrap();
        let mut actions =
            indexer.build_actions_map(&parse.write_units, &scan.existing, &scan.scanned_paths);
        assert!(
            matches!(actions.get("b.ts"), Some(FileAction::Update)),
            "edited b.ts must be re-parsed as Update; got {:?}",
            actions.get("b.ts")
        );

        let outcome = indexer
            .run_dirty_propagation(&mut actions, &parse.write_units)
            .unwrap();

        assert!(
            matches!(actions.get("a.ts"), Some(FileAction::DirtyResolveOnly)),
            "a.ts directly imports b.ts and must be promoted; got {:?}",
            actions.get("a.ts")
        );
        assert!(
            matches!(actions.get("c.ts"), Some(FileAction::DirtyResolveOnly)),
            "c.ts imports a.ts whose forwarded surface changed; got {:?}",
            actions.get("c.ts")
        );
        assert_eq!(outcome.marked, 2, "exactly a.ts and c.ts are promoted");
    }

    /// A round-1 budget bail must surface as `budget_exceeded` on the
    /// incremental `IndexReport` instead of being a silent no-op; the full
    /// build that precedes it must carry no propagation status at all.
    #[test]
    fn budget_bail_surfaces_on_incremental_index_report() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n",
        )
        .unwrap();
        std::fs::write(
            project.join("a.ts"),
            "import { beta } from './b';\nexport function useBeta(): number { return beta(); }\n",
        )
        .unwrap();

        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let config = IndexingConfig {
            dirty_propagation_max_files: 0,
            ..IndexingConfig::default()
        };
        let indexer = Indexer::new(db, project, &config);
        let full_report = indexer.build_index(project, true).unwrap();
        assert_eq!(
            full_report.dirty_propagation, None,
            "full builds must not carry a propagation status"
        );

        // Edit b.ts so its export fingerprint changes; its single importer
        // a.ts already exceeds the zero budget, so round 1 bails.
        std::fs::write(
            project.join("b.ts"),
            "export function beta(): number { return 1; }\n\
             export function gamma(): number { return 2; }\n",
        )
        .unwrap();

        let report = indexer.build_index(project, false).unwrap();
        assert_eq!(
            report.dirty_propagation,
            Some(DirtyPropagationStatus::BudgetExceeded),
            "round-1 budget bail must be surfaced on the report"
        );
    }

    /// Config-off propagation classifies as `disabled`; an enabled run with
    /// nothing changed is a trivially converged `normal`.
    #[test]
    fn disabled_and_trivially_converged_statuses() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);

        let disabled_config = IndexingConfig {
            dirty_propagation: false,
            ..IndexingConfig::default()
        };
        let disabled_indexer = Indexer::new(db.clone(), project, &disabled_config);
        let outcome = disabled_indexer
            .run_dirty_propagation(&mut HashMap::new(), &[])
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Disabled);
        assert_eq!(outcome.marked, 0);

        let enabled_indexer = Indexer::new(db, project, &IndexingConfig::default());
        let outcome = enabled_indexer
            .run_dirty_propagation(&mut HashMap::new(), &[])
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Normal);
        assert_eq!(outcome.marked, 0);
    }
}

#[cfg(test)]
mod test_edge_invariant_tests {
    use super::*;
    use cc_model::config::IndexingConfig;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> (TempDir, Arc<IndexDb>, Indexer) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        for (rel, content) in files {
            let path = project.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let indexer = Indexer::new(db.clone(), project, &IndexingConfig::default());
        (tmp, db, indexer)
    }

    /// Sorted (test_file_path, code_file_path, reason) triples.
    fn edges(db: &IndexDb) -> Vec<(String, String, String)> {
        let mut rows: Vec<(String, String, String)> = db
            .reads()
            .query_json(
                "SELECT test_file_path, code_file_path, reason FROM test_edges",
                &[],
            )
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["test_file_path"].as_str().unwrap().to_string(),
                    row["code_file_path"].as_str().unwrap().to_string(),
                    row["reason"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        rows.sort();
        rows
    }

    /// 不变量：仅修改文件内容（路径集合不变）的增量构建跳过 test_edges
    /// 重建，但边集必须与之后的全量重建完全一致。
    #[test]
    fn content_only_incremental_keeps_test_edges_consistent_with_full() {
        let (tmp, db, indexer) = setup(&[
            ("src/foo.py", "def foo():\n    return 1\n"),
            ("tests/foo_test.py", "def check_foo():\n    return 1\n"),
        ]);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        let initial = edges(&db);
        assert!(
            initial
                .iter()
                .any(|(t, c, _)| t == "tests/foo_test.py" && c == "src/foo.py"),
            "fixture must link tests/foo_test.py to src/foo.py, got {:?}",
            initial
        );

        // Content-only edits to both files: the batch adds/removes no paths,
        // so the rebuild is skipped — edges must survive unchanged.
        std::fs::write(
            project.join("src/foo.py"),
            "def foo():\n    return 2  # edited\n",
        )
        .unwrap();
        std::fs::write(
            project.join("tests/foo_test.py"),
            "def check_foo():\n    return 2  # edited\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert_eq!(
            edges(&db),
            initial,
            "update-only incremental must leave test_edges identical"
        );

        // Cross-check against a from-scratch full rebuild.
        indexer.build_index(project, true).unwrap();
        assert_eq!(
            edges(&db),
            initial,
            "full rebuild must agree with the incrementally preserved edges"
        );
    }

    /// 不变量：新增 / 删除 test 或 source 文件的增量构建仍重建相关边，
    /// 边随路径集合变化正确出现与消失。
    #[test]
    fn added_and_removed_paths_update_test_edges_incrementally() {
        let (tmp, db, indexer) = setup(&[("src/foo.py", "def foo():\n    return 1\n")]);
        let project = tmp.path();
        indexer.build_index(project, false).unwrap();
        assert!(edges(&db).is_empty(), "no test files yet, no edges");

        // Add a test file: the edge must appear in the same incremental build.
        std::fs::create_dir_all(project.join("tests")).unwrap();
        std::fs::write(
            project.join("tests/foo_test.py"),
            "def check_foo():\n    return 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            edges(&db)
                .iter()
                .any(|(t, c, _)| t == "tests/foo_test.py" && c == "src/foo.py"),
            "adding a test file must create its edge, got {:?}",
            edges(&db)
        );

        // Add a second source file matched by a new test file in one batch.
        std::fs::write(project.join("src/bar.py"), "def bar():\n    return 1\n").unwrap();
        std::fs::write(
            project.join("tests/bar_test.py"),
            "def check_bar():\n    return 1\n",
        )
        .unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            edges(&db)
                .iter()
                .any(|(t, c, _)| t == "tests/bar_test.py" && c == "src/bar.py"),
            "adding source+test in one batch must create the edge, got {:?}",
            edges(&db)
        );

        // Remove the test file: its edges must disappear.
        std::fs::remove_file(project.join("tests/foo_test.py")).unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            !edges(&db).iter().any(|(t, _, _)| t == "tests/foo_test.py"),
            "removing a test file must drop its edges, got {:?}",
            edges(&db)
        );

        // Remove a source file: edges pointing at it must disappear too.
        std::fs::remove_file(project.join("src/bar.py")).unwrap();
        indexer.build_index(project, false).unwrap();
        assert!(
            !edges(&db).iter().any(|(_, c, _)| c == "src/bar.py"),
            "removing a source file must drop edges pointing at it, got {:?}",
            edges(&db)
        );
    }
}
