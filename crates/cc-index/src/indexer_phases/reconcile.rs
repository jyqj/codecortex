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
        let seeds: Vec<String> = state.roots.union(&state.propagated).cloned().collect();
        let seed_set: HashSet<_> = seeds.iter().cloned().collect();
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
        let closure = dirty_closure::compute_dirty_closure_resuming(
            &seeds,
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
        let (pending, work) = self
            .db
            .reads()
            .surface_dependents_bounded(&seeds, 0, &excluded)?;
        explanation.dependency_sql.merge(work);
        // Keep an unadmitted persisted dependent conservative, never silently ready.
        let pending_surface = !pending.is_empty()
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
