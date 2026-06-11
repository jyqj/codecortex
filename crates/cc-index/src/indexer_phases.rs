//! Indexer phase implementations (Phase 3.6 – Phase 11).
//!
//! Split from `indexer.rs` for maintainability. All methods are on `impl Indexer`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rayon::prelude::*;
use sha2::{Digest, Sha256};

use cc_db::index_db::{FileWriteUnit, IndexDb, SymbolTargetRow};
use cc_model::edge::{ResolutionKind, RouteNodeRecord};
use cc_model::parse::ParseOutcome;
use cc_model::symbol::{SymbolRecord, SymbolRefRecord};
use cc_model::{CcResult, Language, ParserTier, StableId};

use crate::community::{build_community_labels, louvain_communities};
use crate::config_linker::{extract_config_links, ConfigLinkKind};
use crate::dirty_closure::{DirtyPropagationOutcome, DirtyPropagationStatus};
use crate::framework_registry;
use crate::pass_gate::{
    run_gated_passes, DbSignatureGate, FileSignatureGate, GatedPass, PairGate, RecordTiming,
    StringCacheGate, Unconditional,
};
use crate::resolver::{ResolutionContext, SymbolCatalog};

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

/// Per-build memo of the symbols scan shared by the dispatch and interface
/// signatures (identical column set and ordering), so a build pays for the
/// symbols table scan once instead of once per signature. Builds are
/// single-threaded through the postprocess phase, hence plain interior
/// mutability.
#[derive(Default)]
struct SymbolRowsCache {
    rows: std::cell::RefCell<Option<std::rc::Rc<Vec<serde_json::Value>>>>,
}

impl SymbolRowsCache {
    fn get(&self, db: &IndexDb) -> CcResult<std::rc::Rc<Vec<serde_json::Value>>> {
        if let Some(rows) = self.rows.borrow().as_ref() {
            return Ok(std::rc::Rc::clone(rows));
        }
        let rows = std::rc::Rc::new(db.reads().query_json(
            "SELECT symbol_uid, name, kind, container FROM symbols \
             WHERE symbol_uid IS NOT NULL ORDER BY symbol_uid",
            &[],
        )?);
        *self.rows.borrow_mut() = Some(std::rc::Rc::clone(&rows));
        Ok(rows)
    }
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

/// Owned output of the common front half of a full-snapshot write, shared by
/// the temp-db and DirectWriter paths: the derived config-link units plus the
/// `last_indexed_at` timestamp recorded inside the rebuilt snapshot.
struct FullSnapshotPayload {
    config_units: Vec<FileWriteUnit>,
    recorded_at: String,
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
    /// DefinesMethod, ContainsFile (4b-2). The catalog feed and the hierarchy
    /// edges consume the same snapshot of all symbols (persisted + freshly
    /// parsed), which is why they form one sub-phase.
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

        let file_paths: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        crate::hierarchy::generate_hierarchy_edges(&all_symbols, &file_paths)
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
                        )?
                    }
                }
            } else {
                self.write_full_snapshot_via_temp_db(
                    project_path,
                    &normal_write_units,
                    route_nodes,
                )?
            }
        } else {
            // Incremental: removals, replacements, dirty re-resolution and
            // route nodes commit atomically — a crash cannot leave files
            // deleted with their edges still present.
            self.db.writes().write_incremental_batch(
                to_remove,
                &normal_write_units,
                &dirty_write_units,
                route_nodes,
            )?;

            // Config links read the just-committed snapshot (separate read
            // connection), so they stay outside the batch transaction.
            let config_units = self.build_config_link_units(project_path)?;
            if !config_units.is_empty() {
                self.db.writes().replace_files_batch(&config_units)?;
            }

            // Update metadata (for incremental only; full path sets it inside temp-db)
            let now = chrono::Utc::now().to_rfc3339();
            self.db.writes().set_metadata("last_indexed_at", &now)?;
            self.db.writes().set_metadata("index_version", "1.0.0")?;

            // Long incremental-only sessions never hit the full-rebuild
            // checkpoint, so reclaim the WAL here once it grows too large.
            const MAX_INCREMENTAL_WAL_BYTES: u64 = 16 * 1024 * 1024;
            if let Err(e) = self
                .db
                .admin()
                .checkpoint_wal_if_large(MAX_INCREMENTAL_WAL_BYTES)
            {
                tracing::warn!(err = %e, "incremental WAL checkpoint failed");
            }

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

        // Write hierarchy edges (appends to semantic_edges, does not replace)
        if !hierarchy_edges.is_empty() {
            self.db
                .writes()
                .insert_semantic_edges_batch(hierarchy_edges)?;
            tracing::info!(count = hierarchy_edges.len(), "generated hierarchy edges");
        }

        // Post-processing passes run on the live DB after both paths.
        // Framework detection only needs the files that were actually parsed on
        // incremental builds; full rebuilds still rescan the whole project.
        self.persist_frameworks(project_path, full, &framework_file_paths, to_remove)?;

        Ok(WriteResult {
            write_units,
            config_units,
        })
    }

    /// Phase 7: Post-processing (test edges, dispatch synthesis, community detection).
    pub(crate) fn phase_postprocess(
        &self,
        _project_path: &Path,
        full: bool,
        write_units: &[FileWriteUnit],
        config_units: &[FileWriteUnit],
        to_remove: &[String],
    ) -> CcResult<()> {
        // Rebuild test edges for changed files
        let mut changed_paths: Vec<String> =
            write_units.iter().map(|u| u.rel_path.clone()).collect();
        changed_paths.extend(config_units.iter().map(|u| u.rel_path.clone()));
        changed_paths.extend(to_remove.iter().cloned());
        if full {
            self.db.writes().rebuild_test_edges()?;
        } else if !changed_paths.is_empty() {
            self.db
                .writes()
                .rebuild_test_edges_for_files(&changed_paths)?;
        }

        // Per-pass signature gates: instead of a single graph_signature that
        // hashes all 4 tables, each pass group carries its own input
        // signature. This avoids re-running all 7 synthesis passes + Louvain
        // when only one input changed (e.g. a new dispatch site does not need
        // interface dispatch recomputation, and vice versa).
        //
        // The dispatch and interface signatures share one symbols scan per
        // build via `SymbolRowsCache`, and the synthesis round records its
        // signatures only after community detection completed
        // (`RecordTiming::Deferred`) — a mid-build failure never records a
        // signature for work that did not finish.
        let forced = if full { Some("full rebuild") } else { None };
        let symbol_rows = SymbolRowsCache::default();

        let dispatch_gate = DbSignatureGate::new(
            "dispatch_synthesis",
            &self.db,
            "last_dispatch_sig",
            "last_dispatch_sig_algo",
            DISPATCH_SIG_ALGORITHM,
            forced,
            || self.dispatch_synthesis_signature_from(&symbol_rows),
        );
        let interface_gate = DbSignatureGate::new(
            "interface_dispatch",
            &self.db,
            "last_interface_sig",
            "last_interface_sig_algo",
            INTERFACE_SIG_ALGORITHM,
            forced,
            || self.interface_dispatch_signature_from(&symbol_rows),
        );
        // The two signatures gate one synthesis round: the round runs when
        // either input changed, and the individual decisions route work to
        // the dispatch- vs interface-gated sub-passes inside the round (see
        // `dispatch_synthesis::SynthesisPassSpec`).
        let synthesis_gate = PairGate::new("synthesis_round", &dispatch_gate, &interface_gate);

        // Community detection runs AFTER synthesis: its signature includes
        // synthetic edges, so the loop evaluates this gate only once the
        // synthesis round has been applied (or skipped — in which case the
        // synthetic edges are unchanged too and the pre-round state is
        // already the post-round state).
        let community_gate = DbSignatureGate::new(
            "community",
            &self.db,
            "last_community_sig",
            "last_community_sig_algo",
            COMMUNITY_SIG_ALGORITHM,
            forced,
            || self.community_signature(),
        );

        // Phase 7b–7h: Dynamic dispatch synthesis
        let run_synthesis = || -> CcResult<bool> {
            if self.dispatch_synthesis {
                let synthesis_config = crate::dispatch_synthesis::SynthesisConfig {
                    enabled: true,
                    event_fanout_cap: self.event_fanout_cap,
                    generic_event_denylist: if self.event_denylist.is_empty() {
                        crate::dispatch_synthesis::SynthesisConfig::default().generic_event_denylist
                    } else {
                        self.event_denylist.iter().cloned().collect()
                    },
                };

                // Compute every pass delta against the committed snapshot (no
                // write lock held), then apply all deltas in one short atomic
                // unit of work. A mid-pass failure leaves the database
                // untouched; the apply itself is all-or-nothing. See
                // `crate::synthesis_pipeline` for the cross-pass overlay and
                // the concurrency notes.
                let round = crate::synthesis_pipeline::compute_synthesis_round(
                    &self.db,
                    &synthesis_config,
                    synthesis_gate.first_changed(),
                    synthesis_gate.second_changed(),
                )?;
                crate::synthesis_pipeline::apply_synthesis_round(&self.db, &round)?;
            } else {
                // If synthesis was enabled in a previous run and is disabled now,
                // proactively remove stale synthetic edges. The deletion set is
                // derived from each pass's declared owned kinds/prefixes, so a
                // new pass is covered here the moment its spec is registered.
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
            Ok(true)
        };

        let run_community = || -> CcResult<bool> {
            self.rebuild_communities()?;
            Ok(true)
        };

        run_gated_passes(&[
            GatedPass {
                gate: &synthesis_gate,
                timing: RecordTiming::Deferred,
                run: &run_synthesis,
            },
            GatedPass {
                gate: &community_gate,
                timing: RecordTiming::Immediate,
                run: &run_community,
            },
        ])
    }

    /// Phase 8-11: Git co-change, infrastructure, resolver feedback, and ADR indexing.
    pub(crate) fn phase_analysis(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<()> {
        // Phase 8: Git co-change analysis. HEAD-skip: co-change edges only
        // depend on commit history. If HEAD has not advanced since the last
        // successful analysis, the result is unchanged (the `--since=1.year`
        // window drifts but produces equivalent output while HEAD is fixed),
        // so the git log + parse + write can be skipped.
        let cochange_gate =
            StringCacheGate::new("git_cochange", &self.db, "last_cochange_head", || {
                crate::git_cochange::current_git_head(project_path)
            });
        let run_cochange = || -> CcResult<bool> {
            match crate::git_cochange::analyze_cochanges(project_path, 2, 0.2, 500) {
                Ok(co_changes) => {
                    if !co_changes.is_empty() {
                        self.db.writes().insert_co_change_edges_batch(&co_changes)?;
                        tracing::info!(count = co_changes.len(), "indexed git co-change edges");
                    }
                    Ok(true)
                }
                Err(err) => {
                    // Non-fatal: git may not be available or the project may
                    // not be a git repo. The HEAD marker stays unrecorded so a
                    // transient failure never poisons the skip cache.
                    tracing::warn!(error = %err, "skipping git co-change analysis");
                    Ok(false)
                }
            }
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
            || crate::infra_pass::infra_signature(project_path),
        );
        let run_infra = || -> CcResult<bool> {
            let (mut infra_nodes, mut infra_edges) =
                crate::infra_pass::run_infra_pass(project_path);
            if !infra_nodes.is_empty() || !infra_edges.is_empty() {
                // Bind infra nodes to code symbols before persisting
                let bind_symbols: Vec<_> = write_units
                    .iter()
                    .flat_map(|u| u.outcome.symbols.iter().cloned())
                    .collect();
                crate::infra_pass::bind_infra_to_symbols(&mut infra_nodes, &bind_symbols);

                // Match binding target URLs to known route nodes
                crate::infra_pass::match_bindings_to_routes(&mut infra_edges, route_nodes);

                self.db
                    .writes()
                    .replace_infra_data(&infra_nodes, &infra_edges)?;
                let bound_count = infra_nodes
                    .iter()
                    .filter(|n| n.bound_symbol_uid.is_some())
                    .count();
                let binding_count = infra_edges
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
                    nodes = infra_nodes.len(),
                    edges = infra_edges.len(),
                    bound = bound_count,
                    bindings = binding_count,
                    "indexed infra graph"
                );
            }
            Ok(true)
        };

        // Phase 10: Architecture Decision Records (ADR) indexing
        let adr_gate = Unconditional::new("adr");
        let run_adr = || -> CcResult<bool> {
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
                                                line.split(':')
                                                    .nth(1)
                                                    .unwrap_or("")
                                                    .trim()
                                                    .to_string(),
                                            );
                                        }
                                        if line.to_lowercase().starts_with("date:") {
                                            date = Some(
                                                line.split(':')
                                                    .nth(1)
                                                    .unwrap_or("")
                                                    .trim()
                                                    .to_string(),
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

            if !adr_docs.is_empty() {
                tracing::info!(count = adr_docs.len(), "indexed ADR documents");
                self.db.writes().set_metadata(
                    "adr_documents",
                    &serde_json::to_string(&adr_docs).unwrap_or_default(),
                )?;
            }
            Ok(true)
        };

        run_gated_passes(&[
            GatedPass {
                gate: &cochange_gate,
                timing: RecordTiming::Immediate,
                run: &run_cochange,
            },
            GatedPass {
                gate: &infra_gate,
                timing: RecordTiming::Immediate,
                run: &run_infra,
            },
            GatedPass {
                gate: &adr_gate,
                timing: RecordTiming::Immediate,
                run: &run_adr,
            },
        ])
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

    /// Pure function: build config link units from pre-collected snapshot data.
    /// Does not query the database, suitable for use inside temp-db write closure.
    fn build_config_link_units_from_snapshot(
        project_path: &Path,
        symbol_targets: Vec<SymbolTargetRow>,
        indexed_files: &[String],
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
        let links = extract_config_links(project_path, &known_symbols, &known_files)?;
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

    /// Compatibility wrapper: queries DB for symbol targets / file paths,
    /// then delegates to the pure function.
    fn build_config_link_units(&self, project_path: &Path) -> CcResult<Vec<FileWriteUnit>> {
        let symbol_targets = self.db.reads().list_symbol_targets()?;
        let indexed_files = self.db.reads().list_file_paths()?;
        Self::build_config_link_units_from_snapshot(project_path, symbol_targets, &indexed_files)
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
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
        )?;

        Ok(FullSnapshotPayload {
            config_units,
            recorded_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Shared rebuild-closure body: writes file data, route nodes, config-link
    /// units and metadata into the connection handed out by either rebuild
    /// adapter.
    fn write_full_snapshot_contents(
        conn: &rusqlite::Connection,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
        payload: &FullSnapshotPayload,
    ) -> CcResult<()> {
        // Write main file data
        for unit in write_units {
            IndexDb::insert_file_data(conn, unit)?;
        }

        // Write route nodes
        for r in route_nodes {
            IndexDb::insert_route_node_into(conn, r)?;
        }

        // Write config link units
        for unit in &payload.config_units {
            IndexDb::insert_file_data(conn, unit)?;
        }

        // Write metadata
        IndexDb::set_metadata_on(conn, "last_indexed_at", &payload.recorded_at)?;
        IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

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
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = self.prepare_full_snapshot_payload(project_path, write_units)?;
        self.db.admin().rebuild_with_temp_db(|conn| {
            Self::write_full_snapshot_contents(conn, write_units, route_nodes, &payload)
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
    ) -> CcResult<Vec<FileWriteUnit>> {
        let payload = self.prepare_full_snapshot_payload(project_path, write_units)?;
        self.db.admin().rebuild_with_direct_writer(|conn| {
            Self::write_full_snapshot_contents(conn, write_units, route_nodes, &payload)
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

        // Dispatch sites (input to 6 synthesis passes)
        let site_rows = self.db.reads().query_json(
            "SELECT site_kind, key, file_path, line, enclosing_symbol_uid, handler_symbol_uid \
             FROM dispatch_sites ORDER BY site_id",
            &[],
        )?;
        site_rows.len().hash(&mut hasher);
        for row in &site_rows {
            for col in [
                "site_kind",
                "key",
                "file_path",
                "enclosing_symbol_uid",
                "handler_symbol_uid",
            ] {
                row.get(col)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(&mut hasher);
            }
            row.get("line")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                .hash(&mut hasher);
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
        let edge_rows = self.db.reads().query_json(
            "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
             WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
             AND synthesized_by IS NULL \
             ORDER BY caller_symbol_uid, callee_symbol_uid",
            &[],
        )?;
        edge_rows.len().hash(&mut hasher);
        for row in &edge_rows {
            row.get("caller_symbol_uid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .hash(&mut hasher);
            row.get("callee_symbol_uid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .hash(&mut hasher);
        }

        // Symbols
        Self::hash_symbol_rows(&symbol_rows.get(&self.db)?, &mut hasher);

        // Real semantic edges (synthetic 'synth:%' excluded)
        let sem_rows = self.db.reads().query_json(
            "SELECT source_symbol_uid, target_symbol_uid, relation_kind FROM semantic_edges \
             WHERE edge_id NOT LIKE 'synth:%' ORDER BY edge_id",
            &[],
        )?;
        sem_rows.len().hash(&mut hasher);
        for row in &sem_rows {
            for col in ["source_symbol_uid", "target_symbol_uid", "relation_kind"] {
                row.get(col)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(&mut hasher);
            }
        }

        Ok(hasher.finish())
    }

    /// Signature covering community detection inputs.
    /// Must be computed AFTER synthesis passes, since synthetic edges affect
    /// community structure.
    fn community_signature(&self) -> CcResult<u64> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // ALL call edges (including synthetic)
        let edge_rows = self.db.reads().query_json(
            "SELECT caller_symbol_uid, callee_symbol_uid FROM call_edges \
             WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL \
             ORDER BY caller_symbol_uid, callee_symbol_uid",
            &[],
        )?;
        edge_rows.len().hash(&mut hasher);
        for row in &edge_rows {
            row.get("caller_symbol_uid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .hash(&mut hasher);
            row.get("callee_symbol_uid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .hash(&mut hasher);
        }

        // Symbols (uid + name + kind). `container` is intentionally excluded:
        // community output is Louvain over call-edge uid pairs plus labels
        // built from symbol names by uid, so container is not an input — a
        // container-only change must not force a Louvain rerun. Locked by
        // `community_signature_ignores_container_unlike_synthesis_signatures`.
        let symbol_rows = self.db.reads().query_json(
            "SELECT symbol_uid, name, kind FROM symbols \
             WHERE symbol_uid IS NOT NULL ORDER BY symbol_uid",
            &[],
        )?;
        symbol_rows.len().hash(&mut hasher);
        for row in &symbol_rows {
            for col in ["symbol_uid", "name", "kind"] {
                row.get(col)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(&mut hasher);
            }
        }

        Ok(hasher.finish())
    }

    /// Hash symbol structure (uid/name/kind/container) into the given hasher.
    /// Shared by `dispatch_synthesis_signature` and `interface_dispatch_signature`;
    /// the rows come from a per-build [`SymbolRowsCache`] so the symbols table
    /// is scanned once even when both signatures are computed.
    fn hash_symbol_rows(
        rows: &[serde_json::Value],
        hasher: &mut std::collections::hash_map::DefaultHasher,
    ) {
        use std::hash::Hash;

        rows.len().hash(hasher);
        for row in rows {
            for col in ["symbol_uid", "name", "kind", "container"] {
                row.get(col)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .hash(hasher);
            }
        }
    }

    fn rebuild_communities(&self) -> CcResult<()> {
        // Guard: check edge count before loading full graph to prevent OOM
        let edge_count = self
            .db
            .reads()
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges \
                 WHERE caller_symbol_uid IS NOT NULL AND callee_symbol_uid IS NOT NULL",
                &[],
            )?
            .first()
            .and_then(|r| r.get("cnt"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let max_community_edges: i64 = std::env::var("CODECORTEX_COMMUNITY_MAX_EDGES")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(2_000_000);

        if edge_count > max_community_edges {
            tracing::warn!(
                edge_count,
                max_community_edges,
                "community detection: edge count exceeds limit, assigning all symbols to community 0"
            );
            // Degraded: assign all symbols to a single community
            self.db.writes().assign_all_symbols_to_community(0)?;
            return Ok(());
        }

        let edges = self.db.reads().call_uid_edges()?;
        let assignments = louvain_communities(&edges, 20);
        let symbol_names = self.db.reads().symbol_names_by_uid()?;
        let labels = build_community_labels(&assignments, &symbol_names);
        self.db.writes().update_communities(&assignments, &labels)
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
