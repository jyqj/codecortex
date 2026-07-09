use std::collections::HashSet;
use std::path::Path;

use rayon::prelude::*;

use cc_db::index_db::FileWriteUnit;
use cc_model::edge::RouteNodeRecord;
use cc_model::parse::ParseOutcome;
use cc_model::symbol::SymbolRecord;
use cc_model::{CcResult, StableId};

use crate::indexer::{Indexer, ResolveResult, MIN_FILES_FOR_PARALLEL};
use crate::resolver::catalog_cache::{self, CatalogCarry};
use crate::resolver::{ResolutionContext, SymbolCatalog};

/// Phase 4a output: the [`SymbolCatalog`] seeded with persisted + freshly
/// parsed symbols, the persisted symbols themselves (consumed again by the
/// hierarchy sub-phase; empty when the catalog was reused from the
/// cross-build cache), and one pre-built [`ResolutionContext`] per write
/// unit (index-aligned with the write units they were built from).
struct ResolutionCatalog {
    catalog: SymbolCatalog,
    persisted_symbols: Vec<SymbolRecord>,
    resolution_contexts: Vec<ResolutionContext>,
    /// The `symbols_seed` token the persisted part corresponds to, when this
    /// build is eligible to fold the catalog back into the cross-build cache
    /// (incremental, non-empty batch, aggregate baseline present).
    cache_basis: Option<cc_db::RowAgg>,
    /// `true` when the catalog (its `TypeCatalog` included) came from the
    /// cross-build cache: phase 4b must delta-add the batch's type
    /// contributions instead of rebuilding from a persisted snapshot.
    type_catalog_reused: bool,
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
            cache_basis,
            type_catalog_reused,
        } = super::time_step("resolve", "build_catalog", || {
            self.build_resolution_catalog(full, write_units, to_remove)
        })?;

        // Phase 4a / 4a-2: semantic edge UIDs + backfill, USES_TYPE derivation.
        super::time_step("resolve", "semantic_edges", || {
            Self::resolve_semantic_edges(&catalog, write_units, &resolution_contexts)
        });

        // Phase 4b: type catalog (dispatch) + hierarchy edges.
        let hierarchy_edges = super::time_step("resolve", "hierarchy", || {
            Self::resolve_hierarchy(
                &mut catalog,
                &persisted_symbols,
                write_units,
                type_catalog_reused,
            )
        });

        // Phase 4c: call edges, symbol refs, route edges.
        super::time_step("resolve", "call_edges", || {
            Self::resolve_call_edges(&catalog, write_units, &resolution_contexts)
        });

        // Phase 4d: cross-file framework resolution (post-catalog).
        super::time_step("resolve", "framework_cross_file", || {
            Self::resolve_framework_cross_file(&catalog, write_units, fw_context)
        });

        Ok(ResolveResult {
            hierarchy_edges,
            catalog_carry: cache_basis.map(|basis| CatalogCarry { basis, catalog }),
        })
    }

    /// Phase 4a (input construction): seed the [`SymbolCatalog`] with symbols
    /// persisted in the DB (incremental builds only — excluding files being
    /// re-parsed or removed) plus the freshly parsed symbols, and pre-build
    /// one [`ResolutionContext`] per write unit.
    ///
    /// Incremental non-empty batches first try the cross-build catalog cache
    /// (see [`crate::resolver::catalog_cache`]): on a token-validated hit the
    /// excluded files' entries are removed from the reused catalog instead of
    /// reloading and re-registering every persisted symbol.
    fn build_resolution_catalog(
        &self,
        full: bool,
        write_units: &[FileWriteUnit],
        to_remove: &[String],
    ) -> CcResult<ResolutionCatalog> {
        let resolution_contexts = |units: &[FileWriteUnit]| -> Vec<ResolutionContext> {
            units
                .iter()
                .map(|unit| SymbolCatalog::build_resolution_context(&unit.outcome, &unit.rel_path))
                .collect()
        };

        // Full builds seed from the batch alone. Empty incremental batches
        // (nothing parsed, nothing dirty) have nothing to resolve either:
        // every resolution sub-phase iterates `write_units`, so the
        // persisted snapshot would feed no consumer — skip the load (and
        // leave any parked cross-build catalog in place).
        if full || write_units.is_empty() {
            let mut catalog = SymbolCatalog::new();
            for unit in write_units.iter() {
                catalog.add_symbols(&unit.outcome.symbols);
            }
            return Ok(ResolutionCatalog {
                catalog,
                persisted_symbols: Vec::new(),
                resolution_contexts: resolution_contexts(write_units),
                cache_basis: None,
                type_catalog_reused: false,
            });
        }

        let resolver_excluded_files: Vec<String> = write_units
            .iter()
            .map(|u| u.rel_path.clone())
            .chain(to_remove.iter().cloned())
            .collect();

        if let Some((basis, mut catalog)) = catalog_cache::take_validated(&self.db) {
            let excluded_set: HashSet<String> =
                resolver_excluded_files.iter().cloned().collect();
            catalog.remove_files(&excluded_set);
            catalog.reset_type_assigns();
            for unit in write_units.iter() {
                catalog.add_symbols(&unit.outcome.symbols);
            }
            tracing::debug!(
                phase = "resolve",
                step = "catalog_cache",
                live = catalog.live_len(),
                "reused cross-build resolver catalog"
            );
            return Ok(ResolutionCatalog {
                catalog,
                persisted_symbols: Vec::new(),
                resolution_contexts: resolution_contexts(write_units),
                cache_basis: Some(basis),
                type_catalog_reused: true,
            });
        }

        let (seed_token, persisted_symbols) = self
            .db
            .reads()
            .resolver_seed_symbols_with_token_excluding(&resolver_excluded_files)?;

        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&persisted_symbols);
        for unit in write_units.iter() {
            catalog.add_symbols(&unit.outcome.symbols);
        }

        Ok(ResolutionCatalog {
            resolution_contexts: resolution_contexts(write_units),
            catalog,
            persisted_symbols,
            cache_basis: seed_token,
            type_catalog_reused: false,
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
    ///
    /// When the catalog was reused from the cross-build cache
    /// (`type_catalog_reused`), its TypeCatalog already holds every persisted
    /// file's contributions (excluded files removed in 4a), so only the
    /// batch's contributions are delta-added — at the same pipeline point as
    /// the fresh rebuild, so both paths see the 4a-backfilled symbols.
    fn resolve_hierarchy(
        catalog: &mut SymbolCatalog,
        persisted_symbols: &[SymbolRecord],
        write_units: &[FileWriteUnit],
        type_catalog_reused: bool,
    ) -> Vec<cc_model::edge::SemanticEdgeRecord> {
        // Borrow persisted ++ batch instead of materializing an owned
        // concatenation: both consumers iterate references, so the full
        // snapshot is never deep-cloned (it scales with the whole repo).
        let batch_symbols = || write_units.iter().flat_map(|u| u.outcome.symbols.iter());
        if type_catalog_reused {
            catalog.type_catalog_add_symbols(batch_symbols());
        } else {
            catalog.build_type_catalog(persisted_symbols.iter().chain(batch_symbols()));
        }
        catalog.add_type_assigns_from_outcomes(write_units);

        let file_paths: Vec<String> = write_units.iter().map(|u| u.rel_path.clone()).collect();
        crate::hierarchy::generate_hierarchy_edges(batch_symbols(), &file_paths)
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
}

#[cfg(test)]
mod phase_resolve_subphase_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use cc_model::config::IndexingConfig;
    use cc_model::edge::{CallEdgeRecord, SemanticEdgeRecord, SemanticRelation};
    use cc_model::symbol::{SymbolKind, SymbolRecord};
    use cc_model::{Language, ParserTier};
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

        let conn = crate::test_seed::seed_conn(&db);
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

        let edges = Indexer::resolve_hierarchy(&mut catalog, &[], &units, false);

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
mod hierarchy_incremental_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
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
        std::fs::write(
            project.join("src/main.py"),
            "def main_handler():\n    return 2\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/solo/only.py"),
            "def solo_handler():\n    return 3\n",
        )
        .unwrap();

        // 索引库放在项目树之外，避免 db 文件影响扫描结果的可比性。
        let db_dir = TempDir::new().unwrap();
        let db = Arc::new(
            IndexDb::open(&db_dir.path().join("index.sqlite3"))
                .unwrap()
                .0,
        );
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
            incremental
                .iter()
                .any(|row| row.contains("dir::src/newdir")),
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
