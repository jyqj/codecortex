//! Replay the existing dirty closure against a durable invalidation basis.
//! Limited dependent lookups are work windows, never the persisted remainder.
use crate::{
    dirty_closure::{self, DirtyPropagationOutcome, DirtyPropagationStatus},
    indexer::{FileAction, Indexer},
};
use cc_db::index_db::FileWriteUnit;
use cc_model::{freshness::*, resolution::ResolutionDependency, CcResult};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The initial lookup materializes the first budget+1 distinct paths after
/// excluding the old completed set. Before commit, this prepare only adds at
/// most `budget` promoted paths to that set. Thus a larger dependency set keeps
/// a witness in the window; an exhausted window covered the entire smaller set.
/// This owns no read lease and is never reused by a later prepare. Concurrent
/// writes remain subject to the existing prepared-index-epoch commit fence;
/// this is not a SQLite transaction snapshot spanning the separate reads.
fn dependency_window_pending(
    window: &[String],
    completed: &BTreeSet<String>,
    admitted: usize,
    budget: usize,
) -> Option<bool> {
    if admitted > budget {
        // Keep the original query as a conservative fallback if the closure's
        // admission contract changes in the future.
        return None;
    }
    Some(window.iter().any(|path| !completed.contains(path)))
}

/// A positive-binding window is reusable only for the exact seed query that
/// produced it, before dependency/root candidates are appended. The first
/// closure lookup retains min(all distinct eligible paths, 2*budget+1); this
/// prepare adds at most budget promoted paths to completed. Consequently an
/// unobserved suffix cannot lose every retained witness. A changed seed basis
/// (including a newly propagated facade) still requires the original query.
/// Like the dependency window, this is local to this prepare, owns no read
/// lease and relies on the unchanged prepared-index-epoch commit fence.
fn surface_window_pending(
    window: Option<&[String]>,
    queried_seeds: &[String],
    final_seeds: &[String],
    completed: &BTreeSet<String>,
    admitted: usize,
    budget: usize,
) -> Option<bool> {
    if queried_seeds != final_seeds {
        return None;
    }
    dependency_window_pending(window?, completed, admitted, budget)
}

impl Indexer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn plan_dirty_reconciliation(
        &self,
        actions: &mut HashMap<String, FileAction>,
        units: &[FileWriteUnit],
        removed: &[String],
        roots: Vec<String>,
        events: BTreeSet<ResolutionDependency>,
        reasons: BTreeSet<ChangeKind>,
        epoch: u64,
    ) -> CcResult<DirtyPropagationOutcome> {
        let previous = self.db.reads().resolution_frontier()?;
        let resumed = previous.is_some();
        let rebase = resumed && (!roots.is_empty() || !events.is_empty());
        let mut state = previous
            .clone()
            .unwrap_or_else(|| ReconcileState::new(epoch));
        if rebase {
            // Completion belongs to an exact input basis. Keep all old causes,
            // but do not let an earlier processed root skip a new invalidation.
            state.completed.clear();
            state.propagated.clear();
            state.basis_epoch = epoch;
            state.reasons.insert(ChangeKind::Rebased);
        }
        state.roots.extend(roots);
        state.events.extend(events);
        state.reasons.extend(reasons);
        if resumed {
            state.reasons.insert(ChangeKind::Resumed);
        }
        state
            .completed
            .extend(units.iter().map(|u| u.rel_path.clone()));
        state.completed.extend(removed.iter().cloned());
        let mut explanation = DirtyPlanExplanation {
            dependency_sql: Default::default(),
            parsed_files: units.len(),
            selected_dependents: 0,
            file_budget: self.dirty_propagation_max_files,
            reasons: state.reasons.clone(),
            resumed,
            rebased: rebase,
        };
        if state.roots.is_empty() {
            return Ok(DirtyPropagationOutcome {
                marked: 0,
                status: if self.dirty_propagation {
                    DirtyPropagationStatus::Normal
                } else {
                    DirtyPropagationStatus::Disabled
                },
                reconcile: None,
                explanation,
            });
        }
        let initial_seeds: Vec<String> = state.roots.union(&state.propagated).cloned().collect();
        let seed_set: HashSet<_> = initial_seeds.iter().cloned().collect();
        let fresh: HashSet<_> = state.completed.iter().cloned().collect();
        let excluded: Vec<_> = state.completed.iter().cloned().collect();
        let (dependency_window, initial_work) = self.db.reads().resolution_dependents_with_work(
            &state.events,
            self.dirty_propagation_max_files,
            &excluded,
        )?;
        explanation.dependency_sql.merge(initial_work);
        let mut additional = dependency_window.clone();
        // A prior root may itself depend on a newly changed provider.
        additional.extend(state.roots.iter().filter(|p| !fresh.contains(*p)).cloned());
        let mut target_cache = HashMap::new();
        let budget = if self.dirty_propagation {
            self.dirty_propagation_max_files
        } else {
            0
        };
        let mut surface_window = None;
        let closure = dirty_closure::compute_dirty_closure_resuming(
            &initial_seeds,
            &fresh,
            budget,
            dirty_closure::DIRTY_CLOSURE_MAX_ROUNDS,
            |paths| {
                // At most budget earlier promotions can be filtered within this
                // invocation; retain that slack and a genuine overflow witness.
                let (mut result, work) = self.db.reads().surface_dependents_bounded(
                    paths,
                    budget.saturating_mul(2),
                    &excluded,
                )?;
                explanation.dependency_sql.merge(work);
                if surface_window.is_none() && paths == initial_seeds.as_slice() {
                    // Capture only rows returned by this exact positive-binding
                    // query, before adding the separate dependency/root sets.
                    surface_window = Some(result.clone());
                }
                if paths.iter().any(|p| seed_set.contains(p)) {
                    result.extend(additional.iter().cloned());
                }
                result.sort();
                result.dedup();
                Ok(result)
            },
            |path| matches!(actions.get(path), Some(FileAction::Skip)),
            |paths, changed| {
                self.promoted_export_surfaces_changed(paths, changed, &mut target_cache)
            },
        )?;
        tracing::debug!(rounds=closure.rounds_run, status=?closure.status(), resumed,
            "bounded dirty closure evaluated");
        // The budget bailout precedes surface evaluation in the pure policy.
        // Explicitly retain forwarding contributions of every applied prefix,
        // so a deep chain resumes beyond the previously processed component.
        let propagated =
            self.promoted_export_surfaces_changed(&closure.promoted, &seed_set, &mut target_cache)?;
        state.propagated.extend(propagated);
        state.completed.extend(closure.promoted.iter().cloned());
        let seeds: Vec<_> = state.roots.union(&state.propagated).cloned().collect();
        let excluded: Vec<_> = state.completed.iter().cloned().collect();
        let pending_dependency = if let Some(pending) = dependency_window_pending(
            &dependency_window,
            &state.completed,
            closure.promoted.len(),
            self.dirty_propagation_max_files,
        ) {
            pending
        } else {
            let (pending, work) =
                self.db
                    .reads()
                    .resolution_dependents_with_work(&state.events, 0, &excluded)?;
            explanation.dependency_sql.merge(work);
            !pending.is_empty()
        };
        let pending_surface = if let Some(pending) = surface_window_pending(
            surface_window.as_deref(),
            &initial_seeds,
            &seeds,
            &state.completed,
            closure.promoted.len(),
            budget,
        ) {
            pending
        } else {
            let (pending, work) = self
                .db
                .reads()
                .surface_dependents_bounded(&seeds, 0, &excluded)?;
            explanation.dependency_sql.merge(work);
            !pending.is_empty()
        };
        // Keep an unadmitted persisted dependent conservative, never silently ready.
        let pending_surface = pending_surface
            || state
                .roots
                .iter()
                .any(|p| !state.completed.contains(p) && actions.contains_key(p));
        let pending = pending_dependency || pending_surface;
        let status = if !self.dirty_propagation {
            DirtyPropagationStatus::Disabled
        } else if !pending {
            DirtyPropagationStatus::Normal
        } else if closure.budget_exceeded {
            DirtyPropagationStatus::BudgetExceeded
        } else {
            DirtyPropagationStatus::PartialClosure
        };
        let next = if pending {
            state.stop = match status {
                DirtyPropagationStatus::Disabled => ReconcileStop::Disabled,
                DirtyPropagationStatus::BudgetExceeded => ReconcileStop::BudgetExceeded,
                _ => ReconcileStop::PartialClosure,
            };
            state.payload()?; // fail before committing a source hash if storage is insufficient
            Some(state)
        } else {
            None
        };
        let reconcile = if previous.is_some() || next.is_some() {
            Some(ReconcileUpdate {
                expected_index_epoch: epoch,
                next,
            })
        } else {
            None
        };
        explanation.selected_dependents = closure.promoted.len();
        for path in &closure.promoted {
            if let Some(action) = actions.get_mut(path) {
                *action = FileAction::DirtyResolveOnly;
            }
        }
        Ok(DirtyPropagationOutcome {
            marked: closure.promoted.len(),
            status,
            reconcile,
            explanation,
        })
    }
}

#[cfg(test)]
mod dependency_window_tests {
    use super::dependency_window_pending;
    use cc_db::index_db::{FileWriteUnit, IndexDb};
    use cc_model::{
        resolution::{DependencyKind, ResolutionDependency, ResolutionManifest},
        Language, ParseOutcome,
    };
    use std::collections::BTreeSet;

    fn unit(path: &str, key: &str) -> FileWriteUnit {
        let mut outcome = ParseOutcome {
            resolution: ResolutionManifest::new(),
            ..Default::default()
        };
        outcome
            .resolution
            .dependency(DependencyKind::NameBucket, key);
        outcome
            .resolution
            .dependency(DependencyKind::SymbolInventory, "*");
        FileWriteUnit {
            rel_path: path.into(),
            language: Language::Python,
            content_hash: key.into(),
            mtime: 1.0,
            size: 1,
            outcome,
        }
    }

    #[test]
    fn materialized_dependency_window_matches_original_pending_sql() {
        let directory = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
            .unwrap()
            .0;
        let units: Vec<_> = (0..40)
            .map(|index| unit(&format!("f{index:03}.py"), &format!("key-{}", index % 7)))
            .collect();
        db.writes().replace_files_batch(&units).unwrap();
        // More than one IN batch, two kinds and overlapping consumers exercise
        // the original global sorted/distinct cap, not just one SQL statement.
        let mut events: BTreeSet<_> = (0..401)
            .map(|index| {
                ResolutionDependency::new(DependencyKind::NameBucket, format!("key-{index}"))
            })
            .collect();
        events.insert(ResolutionDependency::new(
            DependencyKind::SymbolInventory,
            "*",
        ));
        let mut observations = Vec::new();
        for budget in [0, 1, 2, 8, 40, usize::MAX] {
            for excluded in [
                Vec::new(),
                vec!["f000.py".into(), "f003.py".into(), "absent.py".into()],
                (0..39).map(|index| format!("f{index:03}.py")).collect(),
            ] {
                let (window, initial_work) = db
                    .reads()
                    .resolution_dependents_with_work(&events, budget, &excluded)
                    .unwrap();
                for alternating in [false, true] {
                    let promoted: Vec<_> = window
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !alternating || index.is_multiple_of(2))
                        .map(|(_, path)| path.clone())
                        .take(budget)
                        .collect();
                    let completed: BTreeSet<_> =
                        excluded.iter().chain(&promoted).cloned().collect();
                    let (original, tail_work) = db
                        .reads()
                        .resolution_dependents_with_work(
                            &events,
                            0,
                            &completed.iter().cloned().collect::<Vec<_>>(),
                        )
                        .unwrap();
                    assert_eq!(
                        dependency_window_pending(&window, &completed, promoted.len(), budget),
                        Some(!original.is_empty()),
                        "budget={budget}, excluded={excluded:?}, promoted={promoted:?}"
                    );
                    let mut previous_work = initial_work;
                    previous_work.merge(tail_work);
                    assert!(tail_work.statements > 0);
                    observations.push(serde_json::json!({
                        "budget":budget,"excluded":excluded.len(),"admitted":promoted.len(),
                        "pending":!original.is_empty(),"original_two_lookup_work":previous_work,
                        "retained_single_lookup_work":initial_work,
                        "scope":"actual SQL statement counters; no whole-build latency claim"
                    }));
                }
            }
        }
        if let Ok(output) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(
                std::path::Path::new(&output).join("p8-prepare-dependency-window-work.json"),
                serde_json::to_vec_pretty(&observations).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn dependency_window_reloads_after_replacement_removal_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("index.db");
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        let events = BTreeSet::from([ResolutionDependency::new(DependencyKind::NameBucket, "hot")]);
        db.writes()
            .replace_files_batch(&[unit("b.py", "hot"), unit("c.py", "hot")])
            .unwrap();
        let first = db.reads().resolution_dependents(&events, 1, &[]).unwrap();
        assert_eq!(first, ["b.py", "c.py"]);
        // Read-pool size one: each materialized window has released its lease
        // before another read or write. No object survives the prepare scope.
        db.writes()
            .replace_files_batch(&[unit("a.py", "hot"), unit("b.py", "different")])
            .unwrap();
        db.writes().remove_files_batch(&["c.py".into()]).unwrap();
        let next = db.reads().resolution_dependents(&events, 1, &[]).unwrap();
        assert_eq!(next, ["a.py"]);
        assert_eq!(
            dependency_window_pending(&next, &BTreeSet::from(["a.py".into()]), 1, 1),
            Some(false)
        );
        drop(db);
        let reopened = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        assert_eq!(
            reopened
                .reads()
                .resolution_dependents(&events, 1, &[])
                .unwrap(),
            next
        );
    }

    #[test]
    fn consuming_more_than_the_registered_window_budget_requires_fresh_sql() {
        let window = vec!["a.py".into(), "b.py".into()];
        let completed = BTreeSet::from(["a.py".into(), "b.py".into()]);
        // There could be another dependent after these two rows. An admission
        // contract violation must request the original SQL, not report ready.
        assert_eq!(dependency_window_pending(&window, &completed, 2, 1), None);
        assert_eq!(
            dependency_window_pending(&window, &BTreeSet::from(["a.py".into()]), 1, 1),
            Some(true)
        );
    }
}

#[cfg(test)]
mod surface_window_tests {
    use super::{surface_window_pending, FileAction, Indexer};
    use crate::dirty_closure::DirtyPropagationStatus;
    use cc_db::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
    use cc_model::{
        config::IndexingConfig, public_surface::PublicSurface, ImportRecord, Language, ParseOutcome,
    };
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    fn unit(path: &str, targets: &[&str]) -> FileWriteUnit {
        let mut outcome = ParseOutcome {
            public_surface: PublicSurface::new("python", path, "surface-window-test-v1"),
            ..Default::default()
        };
        for target in targets {
            outcome.imports.push(ImportRecord {
                context: Default::default(),
                file_path: path.into(),
                import_string: (*target).into(),
                resolved_path: Some((*target).into()),
                imported_name: Some("item".into()),
                alias: None,
                is_namespace: false,
                is_default: false,
                is_reexport: false,
            });
        }
        FileWriteUnit {
            rel_path: path.into(),
            language: Language::Python,
            content_hash: "surface-window-test".into(),
            mtime: 1.0,
            size: 1,
            outcome,
        }
    }

    #[test]
    fn materialized_surface_window_matches_original_pending_sql() {
        let directory = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
            .unwrap()
            .0;
        let units: Vec<_> = (0..40)
            .map(|index| {
                unit(
                    &format!("f{index:03}.py"),
                    &[
                        &format!("target{:03}.py", index % 7),
                        &format!("target{:03}.py", 399 + index % 2),
                    ],
                )
            })
            .collect();
        db.writes().replace_files_batch(&units).unwrap();
        // Three IN batches and overlapping import consumers exercise the
        // original global sorted/distinct cap. No SQL implementation changes.
        let seeds: Vec<_> = (0..401)
            .map(|index| format!("target{index:03}.py"))
            .collect();
        let mut observations = Vec::new();
        for budget in [0, 1, 2, 8, 40, usize::MAX] {
            for excluded in [
                Vec::new(),
                vec!["f000.py".into(), "f003.py".into(), "absent.py".into()],
                (0..39).map(|index| format!("f{index:03}.py")).collect(),
            ] {
                let (window, initial_work) = db
                    .reads()
                    .surface_dependents_bounded(&seeds, budget.saturating_mul(2), &excluded)
                    .unwrap();
                for selection in 0..3 {
                    let mut candidates: Vec<_> = window
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| selection != 1 || index.is_multiple_of(2))
                        .map(|(_, path)| path.clone())
                        .collect();
                    if selection == 2 {
                        candidates.reverse();
                    }
                    let promoted: Vec<_> = candidates.into_iter().take(budget).collect();
                    let completed: BTreeSet<_> =
                        excluded.iter().chain(&promoted).cloned().collect();
                    let (original, tail_work) = db
                        .reads()
                        .surface_dependents_bounded(
                            &seeds,
                            0,
                            &completed.iter().cloned().collect::<Vec<_>>(),
                        )
                        .unwrap();
                    assert_eq!(
                        surface_window_pending(
                            Some(&window),
                            &seeds,
                            &seeds,
                            &completed,
                            promoted.len(),
                            budget,
                        ),
                        Some(!original.is_empty()),
                        "budget={budget}, excluded={excluded:?}, promoted={promoted:?}"
                    );
                    assert!(window.windows(2).all(|pair| pair[0] < pair[1]));
                    assert!(window.len() <= budget.saturating_mul(2).saturating_add(1));
                    assert!(tail_work.statements > 0);
                    let mut original_work = initial_work;
                    original_work.merge(tail_work);
                    observations.push(serde_json::json!({
                        "budget":budget,"excluded":excluded.len(),"admitted":promoted.len(),
                        "pending":!original.is_empty(),"original_two_lookup_work":original_work,
                        "retained_single_lookup_work":initial_work,
                        "scope":"actual SQL statement counters; no whole-build latency claim"
                    }));
                }
            }
        }
        if let Ok(output) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
            std::fs::create_dir_all(&output).unwrap();
            std::fs::write(
                std::path::Path::new(&output).join("p8-prepare-surface-window-work.json"),
                serde_json::to_vec_pretty(&observations).unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn surface_window_requires_exact_queried_basis_and_bounded_admission() {
        let directory = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
            .unwrap()
            .0;
        db.writes()
            .replace_files_batch(&[
                unit("b.py", &["a.py"]),
                unit("c.py", &["b.py"]),
            ])
            .unwrap();
        let queried = vec!["a.py".into()];
        let (window, _) = db
            .reads()
            .surface_dependents_bounded(&queried, 2, &[])
            .unwrap();
        assert_eq!(window, ["b.py"]);
        let completed = BTreeSet::from(["b.py".into()]);
        assert_eq!(
            surface_window_pending(Some(&window), &queried, &queried, &completed, 1, 1),
            Some(false)
        );
        let expanded = vec!["a.py".into(), "b.py".into()];
        assert_eq!(
            surface_window_pending(Some(&window), &queried, &expanded, &completed, 1, 1),
            None
        );
        let (original, _) = db
            .reads()
            .surface_dependents_bounded(&expanded, 0, &["b.py".into()])
            .unwrap();
        assert_eq!(original, ["c.py"]);
        assert_eq!(
            surface_window_pending(None, &queried, &queried, &completed, 1, 1),
            None
        );
        assert_eq!(
            surface_window_pending(Some(&window), &queried, &queried, &completed, 2, 1),
            None
        );
        // Equality is intentionally conservative: subsets could retain a
        // witness belonging only to a removed target; reordered or duplicate
        // query lists also fall back instead of guessing their equivalence.
        for changed in [
            vec!["a.py".into()],
            vec!["b.py".into(), "a.py".into()],
            vec!["a.py".into(), "a.py".into()],
        ] {
            assert_eq!(
                surface_window_pending(Some(&window), &expanded, &changed, &completed, 1, 1),
                None
            );
        }
    }

    #[test]
    fn surface_window_reloads_after_replacement_removal_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("index.db");
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        let seeds = vec!["api.py".into()];
        db.writes()
            .replace_files_batch(&[
                unit("b.py", &["api.py"]),
                unit("c.py", &["api.py"]),
            ])
            .unwrap();
        let first = db
            .reads()
            .surface_dependents_bounded(&seeds, 2, &[])
            .unwrap()
            .0;
        assert_eq!(first, ["b.py", "c.py"]);
        db.writes()
            .replace_files_batch(&[
                unit("a.py", &["api.py"]),
                unit("b.py", &["other.py"]),
            ])
            .unwrap();
        db.writes().remove_files_batch(&["c.py".into()]).unwrap();
        // A new prepare must execute a new lookup. The old window is neither
        // persisted in the frontier nor retained in Indexer/IndexDb.
        let next = db
            .reads()
            .surface_dependents_bounded(&seeds, 2, &[])
            .unwrap()
            .0;
        assert_eq!(next, ["a.py"]);
        assert_eq!(
            surface_window_pending(
                Some(&next),
                &seeds,
                &seeds,
                &BTreeSet::from(["a.py".into()]),
                1,
                1,
            ),
            Some(false)
        );
        drop(db);
        let reopened = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        assert_eq!(
            reopened
                .reads()
                .surface_dependents_bounded(&seeds, 2, &[])
                .unwrap()
                .0,
            next
        );
    }

    #[test]
    fn prepare_reuses_surface_window_without_changing_promotions() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
                .unwrap()
                .0,
        );
        let root = unit("api.py", &[]);
        db.writes()
            .replace_files_batch(&[
                root.clone(),
                unit("b.py", &["api.py"]),
                unit("c.py", &["api.py"]),
            ])
            .unwrap();
        let epoch = db.reads().generation().unwrap().index_epoch;
        for budget in [0, 1, 2] {
            let config = IndexingConfig {
                dirty_propagation: true,
                dirty_propagation_max_files: budget,
                ..Default::default()
            };
            let indexer = Indexer::new(db.clone(), directory.path(), &config);
            let mut actions = HashMap::from([
                ("api.py".into(), FileAction::Update),
                ("b.py".into(), FileAction::Skip),
                ("c.py".into(), FileAction::Skip),
            ]);
            let outcome = indexer
                .plan_dirty_reconciliation(
                    &mut actions,
                    std::slice::from_ref(&root),
                    &[],
                    vec!["api.py".into()],
                    BTreeSet::new(),
                    BTreeSet::new(),
                    epoch,
                )
                .unwrap();
            assert_eq!(outcome.marked, budget);
            assert_eq!(outcome.explanation.dependency_sql.statements, 3);
            let expected = if budget < 2 {
                DirtyPropagationStatus::BudgetExceeded
            } else {
                DirtyPropagationStatus::Normal
            };
            assert_eq!(outcome.status, expected);
            for (index, file) in ["b.py", "c.py"].iter().enumerate() {
                assert_eq!(
                    matches!(actions[*file], FileAction::DirtyResolveOnly),
                    index < budget
                );
            }
            assert_eq!(db.reads().generation().unwrap().index_epoch, epoch);
        }
    }

    #[test]
    fn prepare_requeries_surface_when_propagation_adds_a_seed() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
                .unwrap()
                .0,
        );
        let root = unit("a.py", &[]);
        let mut facade = unit("b.py", &["a.py"]);
        // Unknown surfaces conservatively propagate; c.py is visible only
        // after b.py joins the seed basis.
        facade.outcome.public_surface = PublicSurface::default();
        db.writes()
            .replace_files_batch(&[root.clone(), facade, unit("c.py", &["b.py"])])
            .unwrap();
        let config = IndexingConfig {
            dirty_propagation: true,
            dirty_propagation_max_files: 1,
            ..Default::default()
        };
        let indexer = Indexer::new(db.clone(), directory.path(), &config);
        let mut actions = HashMap::from([
            ("a.py".into(), FileAction::Update),
            ("b.py".into(), FileAction::Skip),
            ("c.py".into(), FileAction::Skip),
        ]);
        let epoch = db.reads().generation().unwrap().index_epoch;
        let outcome = indexer
            .plan_dirty_reconciliation(
                &mut actions,
                &[root],
                &[],
                vec!["a.py".into()],
                BTreeSet::new(),
                BTreeSet::new(),
                epoch,
            )
            .unwrap();
        assert_eq!(outcome.marked, 1);
        assert_eq!(outcome.status, DirtyPropagationStatus::PartialClosure);
        // Initial a.py, next-round b.py, and the original final union query.
        assert_eq!(outcome.explanation.dependency_sql.statements, 9);
        let state = outcome.reconcile.unwrap().next.unwrap();
        assert_eq!(state.propagated, BTreeSet::from(["b.py".into()]));
        assert!(state.completed.contains("b.py"));
        assert!(!state.completed.contains("c.py"));
        assert!(matches!(actions["c.py"], FileAction::Skip));
    }

    #[test]
    fn reused_surface_window_keeps_external_write_epoch_fence() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&directory.path().join("index.db"), 1)
                .unwrap()
                .0,
        );
        let root = unit("api.py", &[]);
        db.writes()
            .replace_files_batch(&[
                root.clone(),
                unit("b.py", &["api.py"]),
                unit("c.py", &["api.py"]),
            ])
            .unwrap();
        let config = IndexingConfig {
            dirty_propagation: true,
            dirty_propagation_max_files: 1,
            ..Default::default()
        };
        let indexer = Indexer::new(db.clone(), directory.path(), &config);
        let mut actions = HashMap::from([
            ("api.py".into(), FileAction::Update),
            ("b.py".into(), FileAction::Skip),
            ("c.py".into(), FileAction::Skip),
        ]);
        let epoch = db.reads().generation().unwrap().index_epoch;
        let outcome = indexer
            .plan_dirty_reconciliation(
                &mut actions,
                &[root],
                &[],
                vec!["api.py".into()],
                BTreeSet::new(),
                BTreeSet::new(),
                epoch,
            )
            .unwrap();
        assert_eq!(outcome.explanation.dependency_sql.statements, 3);
        let update = outcome.reconcile.unwrap();
        assert_eq!(update.expected_index_epoch, epoch);
        // A separate write after prepare invalidates this plan. The local
        // window cannot be committed as a new cross-generation freshness proof.
        db.writes()
            .replace_files_batch(&[unit("new.py", &["api.py"])])
            .unwrap();
        let after_write = db.reads().generation().unwrap();
        assert!(after_write.index_epoch > epoch);
        assert!(matches!(
            db.writes().write_reconciled_batch(
                &[],
                &[],
                &[],
                &[],
                &[],
                &PrecompressedChunks::new(),
                Some(&update),
            ),
            Err(cc_model::CcError::StalePreparedBuild { .. })
        ));
        assert_eq!(db.reads().generation().unwrap(), after_write);
        assert!(db.reads().resolution_frontier().unwrap().is_none());
    }
}
