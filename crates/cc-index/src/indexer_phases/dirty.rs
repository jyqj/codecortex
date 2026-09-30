use std::collections::{HashMap, HashSet};

use cc_db::index_db::FileWriteUnit;
use cc_model::{freshness::ChangeKind, CcResult};

use crate::dirty_closure::DirtyPropagationOutcome;
#[cfg(test)]
use crate::dirty_closure::DirtyPropagationStatus;
use crate::indexer::{FileAction, Indexer};

impl Indexer {
    /// Dirty propagation: detect export signature changes and mark importers
    /// as `DirtyResolveOnly` so their cross-file references get re-resolved
    /// against the updated symbol catalog. The returned outcome carries the
    /// closure status so degradations (budget bail, partial closure) surface
    /// on the index report instead of only in logs.
    #[cfg(test)]
    pub(crate) fn run_dirty_propagation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
        removed_files: &[String],
        basis_epoch: u64,
    ) -> CcResult<DirtyPropagationOutcome> {
        self.run_dirty_propagation_with_inputs(
            actions,
            write_units,
            removed_files,
            basis_epoch,
            &[],
        )
    }

    pub(crate) fn run_dirty_propagation_with_inputs(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
        removed_files: &[String],
        basis_epoch: u64,
        input_changes: &[String],
    ) -> CcResult<DirtyPropagationOutcome> {
        // Step 1: Collect all Add/Update files (the ones that were freshly parsed)
        let changed_files: Vec<String> = actions
            .iter()
            .filter(|(_, a)| matches!(a, FileAction::Add | FileAction::Update))
            .map(|(p, _)| p.clone())
            .collect();

        // A no-source-change scan can still owe durable resolution work.
        let mut reasons = std::collections::BTreeSet::new();
        if !changed_files.is_empty() {
            reasons.insert(ChangeKind::Body);
        }

        // Step 2: Compare old vs new export fingerprints to find files whose
        //         public API surface actually changed. Fetch all old
        //         fingerprints in one batched query to avoid N+1 round trips.
        let old_surfaces = self.db.reads().public_surfaces(&changed_files)?;

        // Build a HashMap index over write_units for O(1) lookup per file,
        // avoiding the previous O(changed_files × write_units) linear scan.
        let write_unit_index: HashMap<&str, &FileWriteUnit> = write_units
            .iter()
            .map(|u| (u.rel_path.as_str(), u))
            .collect();

        let mut export_changed_files = Vec::new();
        for file_path in &changed_files {
            // Unknown/missing evidence is not proof that an interface stayed
            // empty. A shared canonical model owns the comparison on both sides.
            let changed = write_unit_index.get(file_path.as_str()).is_none_or(|unit| {
                unit.outcome
                    .public_surface
                    .changed_from(old_surfaces.get(file_path))
            });
            if changed {
                export_changed_files.push(file_path.clone());
            }
        }

        if !export_changed_files.is_empty() {
            reasons.insert(ChangeKind::PublicSurface);
        }
        // Declared interfaces intentionally ignore source positions. Stored
        // cross-file bindings also carry a location-derived symbol_id, however:
        // a comment/body edit can move a still-identical UID to a new address.
        // Re-resolve affected dependencies without polluting the API fingerprint
        // or treating every private/local body change as an interface change.
        let changed_surface_set: HashSet<&str> =
            export_changed_files.iter().map(String::as_str).collect();
        let unchanged_surfaces: Vec<String> = changed_files
            .iter()
            .filter(|p| !changed_surface_set.contains(p.as_str()))
            .cloned()
            .collect();
        let bound_addresses = self
            .db
            .reads()
            .surface_bound_addresses(&unchanged_surfaces)?;
        for path in &unchanged_surfaces {
            let Some(required) = bound_addresses.get(path) else {
                continue;
            };
            let Some(unit) = write_unit_index.get(path.as_str()) else {
                continue;
            };
            let current: HashSet<_> = unit
                .outcome
                .symbols
                .iter()
                .filter_map(|s| {
                    s.symbol_uid
                        .as_deref()
                        .map(|uid| (s.symbol_id.as_str(), uid))
                })
                .collect();
            if required
                .iter()
                .any(|(id, uid)| !current.contains(&(id.as_str(), uid.as_str())))
            {
                tracing::debug!(file = %path, "dirty propagation: bound symbol address changed");
                reasons.insert(ChangeKind::BoundAddress);
                export_changed_files.push(path.clone());
            }
        }

        // Removed (or renamed-away) files: their export surface went to nothing,
        // so any file that still imports them must re-resolve — otherwise its
        // call edges keep a `target_symbol_id` pointing at now-deleted symbols
        // and its `IMPORTS` edge points at a file that no longer exists. The
        // closure seeds from these paths (still present in importers' stored
        // `imports.resolved_path` at this point) and promotes the importers.
        export_changed_files.extend(removed_files.iter().cloned());

        let mut events = self.resolution_dependency_events(
            actions,
            write_units,
            removed_files,
            &export_changed_files,
        )?;
        if !input_changes.is_empty() {
            use cc_model::resolution::{DependencyKind, ResolutionDependency};
            export_changed_files.extend(input_changes.iter().cloned());
            events.insert(ResolutionDependency::new(DependencyKind::ModuleConfig, "*"));
            for path in input_changes {
                events.insert(ResolutionDependency::new(
                    DependencyKind::ModuleConfig,
                    path.clone(),
                ));
            }
            reasons.insert(ChangeKind::Configuration);
        }
        if export_changed_files.is_empty() && !events.is_empty() {
            let already_parsed: Vec<_> = write_units
                .iter()
                .map(|u| u.rel_path.clone())
                .chain(removed_files.iter().cloned())
                .collect();
            // A new private name is not an interface change. Only seed the
            // closure for candidate/config events that have actual consumers.
            if !self
                .db
                .reads()
                .resolution_dependents(&events, 0, &already_parsed)?
                .is_empty()
            {
                export_changed_files.extend(changed_files.iter().cloned());
                export_changed_files.extend(removed_files.iter().cloned());
            }
        }
        for event in &events {
            use cc_model::resolution::DependencyKind::*;
            let kind = match event.kind {
                ModuleConfig => ChangeKind::Configuration,
                MissingPath | FileInventory | PackageFiles => ChangeKind::Inventory,
                NameBucket | SymbolInventory => ChangeKind::CandidateSet,
                TargetSurface => ChangeKind::PublicSurface,
            };
            reasons.insert(kind);
        }
        self.plan_dirty_reconciliation(
            actions,
            write_units,
            removed_files,
            export_changed_files,
            events,
            reasons,
            basis_epoch,
        )
    }

    /// Which of the given promoted (DirtyResolveOnly) files' *effective*
    /// export surfaces changed, given the set of files whose exports changed
    /// so far (batch hook for `compute_dirty_closure`).
    ///
    /// Promoted files are content-unchanged and reloaded without parsing. Their
    /// persisted file-local declaration evidence stays intact. A changed
    /// forwarding target can nevertheless affect their importers. Unknown or
    /// missing surfaces conservatively continue the existing bounded closure;
    /// known surfaces use resolved forwarding routes. No recursive hashes.
    ///
    /// Re-export targets are fetched via one batched
    /// `reexport_targets_for_files` query per pass (only for files not yet in
    /// `targets_cache`, which memoizes them across rounds and re-evaluation
    /// passes), replacing the previous per-file N+1 query.
    ///
    /// P2-A records ES direct/two-step forwarding, static CommonJS bindings,
    /// Rust visibility-qualified use and Python imports. It reuses existing
    /// resolved import rows; unresolved routes remain a project-model limit.
    /// Python star interfaces are explicitly Unknown. Dependency events and
    /// durable remainder reuse this finite-set expansion through reconcile.rs.
    pub(super) fn promoted_export_surfaces_changed(
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
        let surfaces = self.db.reads().public_surfaces(files)?;
        Ok(files
            .iter()
            .filter(|path| {
                if surfaces
                    .get(path.as_str())
                    .is_none_or(|s| s.fingerprint().is_none() || !s.forwards.is_empty())
                {
                    // A facade can acquire a previously missing route. Treat its
                    // contribution conservatively as changed even when the old
                    // resolved-import table has no route yet. Finite file sets,
                    // not recursive hashes, stabilize cycles.
                    return true;
                }
                targets_cache
                    .get(path.as_str())
                    .is_some_and(|targets| targets.iter().any(|t| changed_so_far.contains(t)))
            })
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod export_fingerprint_contract_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use cc_model::parse::ParseOutcome;
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

    /// The parser/model and compatibility DB accessor consume ONE encoding.
    /// Legacy symbol export flags cannot select a competing hash algorithm.
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

        let mut unit = write_unit(symbols);
        unit.outcome.public_surface = cc_parsers::ParserRegistry::new()
            .parse(
                "src/lib.rs",
                "pub fn alpha() {}\nfn private_fn() {}",
                Language::Rust,
            )
            .unwrap()
            .public_surface;

        // Persist canonical declared surface, independent of export-name hints.
        let tmp = tempfile::TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("contract.db")).unwrap().0;
        db.writes()
            .replace_files_batch(std::slice::from_ref(&unit))
            .unwrap();
        let db_fp = db.reads().get_export_fingerprint("src/lib.rs").unwrap();

        // Compute the in-memory fingerprint from the same write_unit.
        let mem_fp = unit.outcome.public_surface.fingerprint();

        assert!(db_fp.is_some(), "expected a non-empty DB fingerprint");
        assert_eq!(
            mem_fp, db_fp,
            "in-memory and DB export fingerprints must be identical"
        );
    }

    /// Missing interface evidence is Unknown, not inferred from symbol exports.
    #[test]
    fn both_return_none_without_surface_evidence() {
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

        let mem_fp = unit.outcome.public_surface.fingerprint();

        assert_eq!(db_fp, None);
        assert_eq!(mem_fp, None);
    }
}

#[cfg(test)]
mod dirty_propagation_fixpoint_tests {
    use super::*;
    use cc_db::index_db::IndexDb;
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

        let mut scan = indexer
            .phase_scan_and_diff(project, false, None, None)
            .unwrap();
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
            .run_dirty_propagation(
                &mut actions,
                &parse.write_units,
                &scan.to_remove,
                indexer.db.reads().generation().unwrap().index_epoch,
            )
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

        let mut scan = indexer
            .phase_scan_and_diff(project, false, None, None)
            .unwrap();
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
            .run_dirty_propagation(
                &mut actions,
                &parse.write_units,
                &scan.to_remove,
                indexer.db.reads().generation().unwrap().index_epoch,
            )
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

    /// Rust `pub use` re-export chain: workspace crate_b's lib.rs forwards
    /// crate_a's surface (`pub use crate_a::alpha;`), crate_c imports
    /// through crate_b. Removing crate_a's lib.rs must promote BOTH
    /// crate_b/src/lib.rs (direct importer) and crate_c/src/lib.rs
    /// (transitive, through the re-export flag). Before the parser fix the
    /// `pub use` row kept the literal string `pub use crate_a::alpha` —
    /// unresolvable against the workspace alias map — so neither promotion
    /// ever happened.
    ///
    /// P2-A additionally seeds edits from PublicSurface; this historical case
    /// specifically guards deletion-seeded forwarding. Signature mutation
    /// parity is covered by cc-eval's p2a_incremental integration tests.
    #[test]
    fn rust_pub_use_chain_promotes_transitive_importer_on_removal() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        let write = |rel: &str, content: &str| {
            let path = project.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        };
        write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crate_a\", \"crate_b\", \"crate_c\"]\n",
        );
        write(
            "crate_a/Cargo.toml",
            "[package]\nname = \"crate_a\"\nversion = \"0.1.0\"\n",
        );
        write("crate_a/src/lib.rs", "pub fn alpha() -> i32 {\n    1\n}\n");
        write(
            "crate_b/Cargo.toml",
            "[package]\nname = \"crate_b\"\nversion = \"0.1.0\"\n",
        );
        write("crate_b/src/lib.rs", "pub use crate_a::alpha;\n");
        write(
            "crate_c/Cargo.toml",
            "[package]\nname = \"crate_c\"\nversion = \"0.1.0\"\n",
        );
        write(
            "crate_c/src/lib.rs",
            "use crate_b::alpha;\n\npub fn gamma() -> i32 {\n    alpha() + 1\n}\n",
        );

        let db = Arc::new(IndexDb::open(&project.join("index.sqlite3")).unwrap().0);
        let config = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), project, &config);
        indexer.build_index(project, true).unwrap();

        // Premise: the pub use row must be persisted as a resolved re-export
        // pointing at crate_a's entry point.
        let reexports = db
            .reads()
            .query_json(
                "SELECT import_string, resolved_path FROM imports \
                 WHERE file_path = 'crate_b/src/lib.rs' AND is_reexport = 1",
                &[],
            )
            .unwrap();
        assert!(
            reexports.iter().any(|row| {
                row.get("import_string").and_then(|v| v.as_str()) == Some("crate_a::alpha")
                    && row.get("resolved_path").and_then(|v| v.as_str())
                        == Some("crate_a/src/lib.rs")
            }),
            "pub use must persist as a resolved re-export; got {reexports:?}"
        );

        // Remove the re-export target and run the incremental pipeline.
        std::fs::remove_file(project.join("crate_a/src/lib.rs")).unwrap();

        let mut scan = indexer
            .phase_scan_and_diff(project, false, None, None)
            .unwrap();
        assert!(
            scan.to_remove.contains(&"crate_a/src/lib.rs".to_string()),
            "deleted lib.rs must land in to_remove; got {:?}",
            scan.to_remove
        );
        let to_parse = std::mem::take(&mut scan.to_parse);
        let parse = indexer.phase_parse(project, to_parse).unwrap();
        let mut actions =
            indexer.build_actions_map(&parse.write_units, &scan.existing, &scan.scanned_paths);

        indexer
            .run_dirty_propagation(
                &mut actions,
                &parse.write_units,
                &scan.to_remove,
                indexer.db.reads().generation().unwrap().index_epoch,
            )
            .unwrap();

        assert!(
            matches!(
                actions.get("crate_b/src/lib.rs"),
                Some(FileAction::DirtyResolveOnly)
            ),
            "crate_b re-exports the removed crate_a and must be promoted; got {:?}",
            actions.get("crate_b/src/lib.rs")
        );
        assert!(
            matches!(
                actions.get("crate_c/src/lib.rs"),
                Some(FileAction::DirtyResolveOnly)
            ),
            "crate_c imports through crate_b's re-export and must be promoted; got {:?}",
            actions.get("crate_c/src/lib.rs")
        );
    }

    /// Removing a dependency must promote its importers for re-resolution:
    /// `a.ts` imports `beta` from `b.ts`; deleting `b.ts` has to mark `a.ts`
    /// `DirtyResolveOnly` so its now-dangling call/import edges get cleared and
    /// re-resolved against a catalog that no longer contains `b.ts`.
    #[test]
    fn removed_dependency_promotes_importer() {
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
        let config = IndexingConfig::default();
        let indexer = Indexer::new(db.clone(), project, &config);
        indexer.build_index(project, true).unwrap();

        // Premise: a.ts's import must resolve to b.ts so the importer lookup
        // (keyed on imports.resolved_path) can find it after removal.
        let imports = db
            .reads()
            .query_json(
                "SELECT resolved_path FROM imports WHERE file_path = 'a.ts'",
                &[],
            )
            .unwrap();
        assert!(
            imports
                .iter()
                .any(|row| row.get("resolved_path").and_then(|v| v.as_str()) == Some("b.ts")),
            "a.ts must import a resolved b.ts; got {:?}",
            imports
        );

        // Delete b.ts and run the incremental diff/parse/propagation pipeline.
        std::fs::remove_file(project.join("b.ts")).unwrap();

        let mut scan = indexer
            .phase_scan_and_diff(project, false, None, None)
            .unwrap();
        assert!(
            scan.to_remove.contains(&"b.ts".to_string()),
            "deleted b.ts must land in to_remove; got {:?}",
            scan.to_remove
        );
        let to_parse = std::mem::take(&mut scan.to_parse);
        let parse = indexer.phase_parse(project, to_parse).unwrap();
        let mut actions =
            indexer.build_actions_map(&parse.write_units, &scan.existing, &scan.scanned_paths);
        assert!(
            matches!(actions.get("a.ts"), Some(FileAction::Skip)),
            "unchanged a.ts starts as Skip; got {:?}",
            actions.get("a.ts")
        );

        let outcome = indexer
            .run_dirty_propagation(
                &mut actions,
                &parse.write_units,
                &scan.to_remove,
                indexer.db.reads().generation().unwrap().index_epoch,
            )
            .unwrap();

        assert!(
            matches!(actions.get("a.ts"), Some(FileAction::DirtyResolveOnly)),
            "a.ts imports the removed b.ts and must be promoted; got {:?}",
            actions.get("a.ts")
        );
        assert_eq!(outcome.marked, 1, "exactly a.ts is promoted");
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
            .run_dirty_propagation(&mut HashMap::new(), &[], &[], 0)
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Disabled);
        assert_eq!(outcome.marked, 0);

        let enabled_indexer = Indexer::new(db, project, &IndexingConfig::default());
        let outcome = enabled_indexer
            .run_dirty_propagation(&mut HashMap::new(), &[], &[], 0)
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Normal);
        assert_eq!(outcome.marked, 0);
    }
}
