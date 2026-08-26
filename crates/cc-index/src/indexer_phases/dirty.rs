use std::collections::{HashMap, HashSet};

use cc_db::index_db::FileWriteUnit;
use cc_model::CcResult;

use crate::dirty_closure::{DirtyPropagationOutcome, DirtyPropagationStatus};
use crate::indexer::{FileAction, Indexer};

impl Indexer {
    /// Dirty propagation: detect export signature changes and mark importers
    /// as `DirtyResolveOnly` so their cross-file references get re-resolved
    /// against the updated symbol catalog. The returned outcome carries the
    /// closure status so degradations (budget bail, partial closure) surface
    /// on the index report instead of only in logs.
    pub(crate) fn run_dirty_propagation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        write_units: &[FileWriteUnit],
        removed_files: &[String],
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

        // Nothing changed and nothing removed: the closure is trivially
        // converged.
        if changed_files.is_empty() && removed_files.is_empty() {
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

        // Removed (or renamed-away) files: their export surface went to nothing,
        // so any file that still imports them must re-resolve — otherwise its
        // call edges keep a `target_symbol_id` pointing at now-deleted symbols
        // and its `IMPORTS` edge points at a file that no longer exists. The
        // closure seeds from these paths (still present in importers' stored
        // `imports.resolved_path` at this point) and promotes the importers.
        export_changed_files.extend(removed_files.iter().cloned());

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

        // Budget overflow (warn already emitted inside the closure): the
        // closure returns a budget-sized partial promotion set instead of a
        // no-op, so it is applied like any other result below; the
        // BudgetExceeded status (from `closure_result.status()`) still tells
        // the caller to consider a full rebuild for the dropped remainder.

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
    /// through such files promote their importers. The Rust extractor sets it
    /// for visibility-qualified `use` (`pub use`, `pub(crate) use`, …);
    /// since no non-JS/TS parser sets `export_name`, Rust export
    /// fingerprints are constant and the flag currently matters for
    /// removal-seeded closures (a re-exported crate file deleted/renamed
    /// promotes the facade's importers transitively).
    ///
    /// Known remaining gaps: CommonJS forwarding
    /// (`const { x } = require('./b'); module.exports = { x }` or mixed
    /// `export { x }`) is still stored as a plain import, and the remaining
    /// language extractors never set the flag (e.g. Python
    /// `from b import *` / `__init__.py` star re-exports), so equivalent
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

        let mut scan = indexer.phase_scan_and_diff(project, false, None, None).unwrap();
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
            .run_dirty_propagation(&mut actions, &parse.write_units, &scan.to_remove)
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

        let mut scan = indexer.phase_scan_and_diff(project, false, None, None).unwrap();
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
            .run_dirty_propagation(&mut actions, &parse.write_units, &scan.to_remove)
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
    /// (Rust surface *edits* still don't seed the closure: no non-JS/TS
    /// parser sets `export_name`, so Rust export fingerprints are constant.
    /// Removal-seeded closures are the path this fix makes work end-to-end.)
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
        write(
            "crate_a/src/lib.rs",
            "pub fn alpha() -> i32 {\n    1\n}\n",
        );
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

        let mut scan = indexer.phase_scan_and_diff(project, false, None, None).unwrap();
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
            .run_dirty_propagation(&mut actions, &parse.write_units, &scan.to_remove)
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

        let mut scan = indexer.phase_scan_and_diff(project, false, None, None).unwrap();
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
            .run_dirty_propagation(&mut actions, &parse.write_units, &scan.to_remove)
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
            .run_dirty_propagation(&mut HashMap::new(), &[], &[])
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Disabled);
        assert_eq!(outcome.marked, 0);

        let enabled_indexer = Indexer::new(db, project, &IndexingConfig::default());
        let outcome = enabled_indexer
            .run_dirty_propagation(&mut HashMap::new(), &[], &[])
            .unwrap();
        assert_eq!(outcome.status, DirtyPropagationStatus::Normal);
        assert_eq!(outcome.marked, 0);
    }
}
