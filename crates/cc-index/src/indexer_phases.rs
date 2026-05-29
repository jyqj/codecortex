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
use cc_model::symbol::SymbolRefRecord;
use cc_model::{CcResult, Language, ParserTier, StableId};

use crate::community::{build_community_labels, louvain_communities};
use crate::config_linker::{extract_config_links, ConfigLinkKind};
use crate::framework_registry;
use crate::resolver::{ResolutionContext, SymbolCatalog};

use super::indexer::{FileAction, Indexer, ResolveResult, WriteResult, MIN_FILES_FOR_PARALLEL};

impl Indexer {
    /// Phase 4: Symbol resolution (semantic edges, type catalog, call edges, cross-file).
    pub(crate) fn phase_resolve(
        &self,
        _project_path: &Path,
        full: bool,
        write_units: &mut Vec<FileWriteUnit>,
        to_remove: &[String],
        fw_context: &crate::framework_resolvers::ProjectFrameworkContext,
    ) -> CcResult<ResolveResult> {
        let resolver_excluded_files: Vec<String> = write_units
            .iter()
            .map(|u| u.rel_path.clone())
            .chain(to_remove.iter().cloned())
            .collect();
        let persisted_symbols = if full {
            Vec::new()
        } else {
            self.db
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

        // Phase 4a: Resolve semantic edge UIDs and backfill base_types/implements
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

        // Phase 4b: Build TypeCatalog for type-aware method dispatch resolution
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

        // Phase 4b-1: Feed type_assigns into TypeCatalog for variable type inference
        catalog.add_type_assigns_from_outcomes(write_units);

        // Phase 4b-2: Generate hierarchy edges (Defines, DefinesMethod, ContainsFile)
        let file_paths: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let hierarchy_edges = crate::hierarchy::generate_hierarchy_edges(&all_symbols, &file_paths);

        drop(all_symbols);

        // Phase 4c: Resolve call edges, symbol refs, route edges
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

        // Phase 4d: Cross-file framework resolution (post-catalog).
        //
        // Resolvers need `&mut [(String, ParseOutcome)]`. Previously every
        // outcome was deep-cloned (symbols/edges/refs/chunks) just to hand the
        // resolvers a mutable view, then a partial subset of edges was merged
        // back. Instead we *move* each outcome out of its write_unit (leaving a
        // cheap default in place), let resolvers mutate it in place, and move it
        // straight back. This eliminates the full-graph deep copy and also
        // faithfully preserves in-place edge mutations (e.g. route prefix
        // propagation / handler UID binding) that the old length-only merge
        // silently dropped.
        {
            let registry = crate::framework_resolvers::default_registry();
            let active = registry.active_resolvers(fw_context);
            if !active.is_empty() {
                let mut owned_pairs: Vec<(String, ParseOutcome)> = write_units
                    .iter_mut()
                    .map(|u| (u.rel_path.clone(), std::mem::take(&mut u.outcome)))
                    .collect();
                for resolver in &active {
                    resolver.resolve_cross_file(&catalog, &mut owned_pairs, fw_context);
                }
                // Move the (possibly mutated) outcomes back into their units.
                for (unit, (_, outcome)) in write_units.iter_mut().zip(owned_pairs) {
                    unit.outcome = outcome;
                }
            }
        }

        Ok(ResolveResult { hierarchy_edges })
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
            // Incremental: keep existing path
            if !to_remove.is_empty() {
                self.db.remove_files_batch(to_remove)?;
            }
            if !normal_write_units.is_empty() {
                self.db.replace_files_batch(&normal_write_units)?;
            }
            if !dirty_write_units.is_empty() {
                self.db.replace_reresolved_edges_only(&dirty_write_units)?;
            }
            if !route_nodes.is_empty() {
                self.db.insert_route_nodes_batch(route_nodes)?;
            }

            let config_units = self.build_config_link_units(project_path)?;
            if !config_units.is_empty() {
                self.db.replace_files_batch(&config_units)?;
            }

            // Update metadata (for incremental only; full path sets it inside temp-db)
            let now = chrono::Utc::now().to_rfc3339();
            self.db.set_metadata("last_indexed_at", &now)?;
            self.db.set_metadata("index_version", "1.0.0")?;

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
            self.db.insert_semantic_edges_batch(hierarchy_edges)?;
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
            self.db.rebuild_test_edges()?;
        } else if !changed_paths.is_empty() {
            self.db.rebuild_test_edges_for_files(&changed_paths)?;
        }

        // Phase 7b: Dynamic dispatch synthesis (event emitter -> handler)
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
            let mut stats = crate::dispatch_synthesis::run_event_emitter_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            if stats.event_emitter_edges > 0 {
                tracing::info!(
                    edges = stats.event_emitter_edges,
                    skipped_generic = stats.skipped_generic,
                    skipped_fanout = stats.skipped_fanout,
                    "event emitter synthesis complete"
                );
            }

            // Phase 7c: JSX component synthesis
            let jsx_count = crate::dispatch_synthesis::run_jsx_synthesis(&self.db)?;
            stats.jsx_edges = jsx_count;
            if jsx_count > 0 {
                tracing::info!(edges = jsx_count, "JSX component synthesis complete");
            }

            // Phase 7d: State setter synthesis
            let setter_count = crate::dispatch_synthesis::run_state_setter_synthesis(&self.db)?;
            stats.setter_edges = setter_count;
            if setter_count > 0 {
                tracing::info!(edges = setter_count, "state setter synthesis complete");
            }

            // Phase 7e: Field-backed observer synthesis
            let observer_count = crate::dispatch_synthesis::run_field_observer_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            stats.field_observer_edges = observer_count;
            if observer_count > 0 {
                tracing::info!(edges = observer_count, "field observer synthesis complete");
            }

            // Phase 7f: React re-render chain synthesis
            let rerender_count =
                crate::dispatch_synthesis::run_react_rerender_chain_synthesis(&self.db)?;
            stats.react_rerender_edges = rerender_count;
            if rerender_count > 0 {
                tracing::info!(
                    edges = rerender_count,
                    "React re-render chain synthesis complete"
                );
            }

            // Phase 7g: Vue template synthesis (child components + event handlers)
            let vue_count = crate::dispatch_synthesis::run_vue_template_synthesis(&self.db)?;
            if vue_count > 0 {
                tracing::info!(edges = vue_count, "Vue template synthesis complete");
            }

            // Phase 7h: Interface/abstract method dispatch synthesis
            let interface_count = crate::dispatch_synthesis::run_interface_dispatch_synthesis(
                &self.db,
                &synthesis_config,
            )?;
            if interface_count > 0 {
                tracing::info!(
                    edges = interface_count,
                    "Interface dispatch synthesis complete"
                );
            }
        } else {
            // If synthesis was enabled in a previous run and is disabled now,
            // proactively remove stale synthetic edges.
            let removed_event = self.db.delete_synthetic_call_edges("event_emitter")?;
            let removed_setter = self.db.delete_synthetic_call_edges("react_state_setter")?;
            let removed_jsx = self.db.delete_synthetic_semantic_edges("synth:jsx:")?;
            let removed_observer = self.db.delete_synthetic_call_edges("field_observer")?;
            let removed_rerender = self.db.delete_synthetic_call_edges("react_rerender")?;
            let removed_vue_semantic = self.db.delete_synthetic_semantic_edges("synth:vue:")?;
            let removed_vue_handler = self.db.delete_synthetic_call_edges("vue_event_handler")?;
            let removed_interface = self.db.delete_synthetic_call_edges("interface_dispatch")?;
            if removed_event > 0
                || removed_setter > 0
                || removed_jsx > 0
                || removed_observer > 0
                || removed_rerender > 0
                || removed_vue_semantic > 0
                || removed_vue_handler > 0
                || removed_interface > 0
            {
                tracing::info!(
                    event_edges = removed_event,
                    setter_edges = removed_setter,
                    jsx_edges = removed_jsx,
                    observer_edges = removed_observer,
                    rerender_edges = removed_rerender,
                    vue_semantic_edges = removed_vue_semantic,
                    vue_handler_edges = removed_vue_handler,
                    interface_edges = removed_interface,
                    "dispatch synthesis disabled; removed stale synthetic edges"
                );
            }
        }

        self.rebuild_communities()?;

        Ok(())
    }

    /// Phase 8-11: Git co-change, infrastructure, resolver feedback, and ADR indexing.
    pub(crate) fn phase_analysis(
        &self,
        project_path: &Path,
        write_units: &[FileWriteUnit],
        route_nodes: &[RouteNodeRecord],
    ) -> CcResult<()> {
        // Phase 8: Git co-change analysis
        self.analyze_git_cochanges(project_path)?;

        // Phase 9: Infrastructure pass
        let (mut infra_nodes, mut infra_edges) = crate::infra_pass::run_infra_pass(project_path);
        if !infra_nodes.is_empty() || !infra_edges.is_empty() {
            // Bind infra nodes to code symbols before persisting
            let bind_symbols: Vec<_> = write_units
                .iter()
                .flat_map(|u| u.outcome.symbols.iter().cloned())
                .collect();
            crate::infra_pass::bind_infra_to_symbols(&mut infra_nodes, &bind_symbols);

            // Match binding target URLs to known route nodes
            crate::infra_pass::match_bindings_to_routes(&mut infra_edges, route_nodes);

            self.db.replace_infra_data(&infra_nodes, &infra_edges)?;
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

        // Phase 10: Resolver quality feedback
        let unresolved_count = self.db.rebuild_resolution_attempts()?;
        if unresolved_count > 0 {
            tracing::info!(
                count = unresolved_count,
                "rebuilt unresolved reference backlog"
            );
        }

        // Phase 11: Architecture Decision Records (ADR) indexing
        {
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
                self.db.set_metadata(
                    "adr_documents",
                    &serde_json::to_string(&adr_docs).unwrap_or_default(),
                )?;
            }
        }

        Ok(())
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
        let symbol_targets = self.db.list_symbol_targets()?;
        let indexed_files = self.db.list_file_paths()?;
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
        // Pre-collect snapshot data for config links before entering temp-db closure.
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
        )?;

        let now = chrono::Utc::now().to_rfc3339();

        // Clone config_units for return value (originals move into the closure).
        let config_units_ret = config_units.clone();

        self.db.rebuild_with_temp_db(|conn| {
            // Write main file data
            for unit in write_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write route nodes
            for r in route_nodes {
                IndexDb::insert_route_node_into(conn, r)?;
            }

            // Write config link units
            for unit in &config_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write metadata
            IndexDb::set_metadata_on(conn, "last_indexed_at", &now)?;
            IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

            Ok(())
        })?;

        Ok(config_units_ret)
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
        // Pre-collect snapshot data (same as temp_db path)
        let symbol_targets = Self::collect_symbol_targets(write_units);
        let indexed_files: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        let config_units = Self::build_config_link_units_from_snapshot(
            project_path,
            symbol_targets,
            &indexed_files,
        )?;

        let now = chrono::Utc::now().to_rfc3339();
        let config_units_ret = config_units.clone();

        self.db.rebuild_with_direct_writer(|conn| {
            // Write main file data
            for unit in write_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write route nodes
            for r in route_nodes {
                IndexDb::insert_route_node_into(conn, r)?;
            }

            // Write config link units
            for unit in &config_units {
                IndexDb::insert_file_data(conn, unit)?;
            }

            // Write metadata
            IndexDb::set_metadata_on(conn, "last_indexed_at", &now)?;
            IndexDb::set_metadata_on(conn, "index_version", "1.0.0")?;

            Ok(())
        })?;

        Ok(config_units_ret)
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

    fn analyze_git_cochanges(&self, project_path: &Path) -> CcResult<()> {
        match crate::git_cochange::analyze_cochanges(project_path, 2, 0.2, 500) {
            Ok(co_changes) => {
                if !co_changes.is_empty() {
                    self.db.insert_co_change_edges_batch(&co_changes)?;
                    tracing::info!(count = co_changes.len(), "indexed git co-change edges");
                }
            }
            Err(err) => {
                // Non-fatal: git may not be available or the project may not be a git repo
                tracing::warn!(error = %err, "skipping git co-change analysis");
            }
        }
        Ok(())
    }

    fn rebuild_communities(&self) -> CcResult<()> {
        let edges = self.db.call_uid_edges()?;
        let assignments = louvain_communities(&edges, 20);
        let symbol_names = self.db.symbol_names_by_uid()?;
        let labels = build_community_labels(&assignments, &symbol_names);
        self.db.update_communities(&assignments, &labels)
    }

    /// Dirty propagation: detect export signature changes and mark importers
    /// as `DirtyResolveOnly` so their cross-file references get re-resolved
    /// against the updated symbol catalog.
    pub(crate) fn run_dirty_propagation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
    ) -> CcResult<usize> {
        if !self.dirty_propagation {
            return Ok(0);
        }

        // Step 1: Collect all Add/Update files (the ones that were freshly parsed)
        let changed_files: Vec<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::Add | FileAction::Update))
            .map(|(p, _)| p.clone())
            .collect();

        if changed_files.is_empty() {
            return Ok(0);
        }

        // Step 2: Compare old vs new export fingerprints to find files whose
        //         public API surface actually changed. Fetch all old
        //         fingerprints in one batched query to avoid N+1 round trips.
        let old_fingerprints = self.db.get_export_fingerprints(&changed_files)?;
        let mut export_changed_files = Vec::new();
        for file_path in &changed_files {
            // Files with no exported symbols are absent from the map (== None),
            // matching the single-file query's None return.
            let old_fp = old_fingerprints.get(file_path).cloned();
            let new_fp = Self::compute_new_export_fingerprint(write_units, file_path);
            if old_fp != new_fp {
                export_changed_files.push(file_path.clone());
            }
        }

        if export_changed_files.is_empty() {
            return Ok(0);
        }

        // Step 3: Find all files that import at least one of the changed files
        let importers = self.db.find_importers_of(&export_changed_files)?;

        // Step 4: Count how many Skip files would be promoted. If the count
        //         exceeds the configured limit, bail out to avoid runaway
        //         propagation (the user should do a full rebuild instead).
        let dirty_count = importers
            .iter()
            .filter(|p| matches!(actions.get(*p), Some(FileAction::Skip)))
            .count();

        if dirty_count > self.dirty_propagation_max_files {
            tracing::warn!(
                dirty_count,
                max = self.dirty_propagation_max_files,
                "dirty propagation: too many affected files, skipping (consider full rebuild)"
            );
            return Ok(0);
        }

        // Step 5: Promote Skip → DirtyResolveOnly
        let mut marked = 0;
        for importer in importers {
            if let Some(action) = actions.get_mut(&importer) {
                if matches!(action, FileAction::Skip) {
                    *action = FileAction::DirtyResolveOnly;
                    marked += 1;
                }
            }
        }

        if marked > 0 {
            tracing::info!(
                marked,
                export_changed = export_changed_files.len(),
                "dirty propagation: marked files for re-resolution"
            );
        }

        Ok(marked)
    }

    /// Compute the export fingerprint from freshly-parsed write_units.
    ///
    /// The algorithm matches `IndexDb::get_export_fingerprint()`:
    ///   1. Select exported symbols (export_name IS NOT NULL or is_default_export)
    ///   2. Format each as "uid|name|signature|export_name"
    ///   3. Sort by uid (first field)
    ///   4. Join with "\n" and hash with blake3
    pub(crate) fn compute_new_export_fingerprint(
        write_units: &[FileWriteUnit],
        file_path: &str,
    ) -> Option<String> {
        let unit = write_units.iter().find(|u| u.rel_path == file_path)?;
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
        db.replace_files_batch(std::slice::from_ref(&unit)).unwrap();
        let db_fp = db.get_export_fingerprint("src/lib.rs").unwrap();

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
        db.replace_files_batch(std::slice::from_ref(&unit)).unwrap();
        let db_fp = db.get_export_fingerprint("src/lib.rs").unwrap();

        let mem_fp =
            Indexer::compute_new_export_fingerprint(std::slice::from_ref(&unit), "src/lib.rs");

        assert_eq!(db_fp, None);
        assert_eq!(mem_fp, None);
    }
}
