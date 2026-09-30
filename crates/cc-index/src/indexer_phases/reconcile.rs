//! Replay the existing dirty closure against a durable invalidation basis.
//! Limited dependent lookups are work windows, never the persisted remainder.
use crate::{
    dirty_closure::{self, DirtyPropagationOutcome, DirtyPropagationStatus},
    indexer::{FileAction, Indexer},
};
use cc_db::index_db::FileWriteUnit;
use cc_model::{freshness::*, resolution::ResolutionDependency, CcResult};
use std::collections::{BTreeSet, HashMap, HashSet};

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
        let (mut additional, initial_work) = self.db.reads().resolution_dependents_with_work(
            &state.events,
            self.dirty_propagation_max_files,
            &excluded,
        )?;
        explanation.dependency_sql.merge(initial_work);
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
        let (pending, work) =
            self.db
                .reads()
                .resolution_dependents_with_work(&state.events, 0, &excluded)?;
        explanation.dependency_sql.merge(work);
        let pending_dependency = !pending.is_empty();
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
