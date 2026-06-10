//! Fixpoint dirty-closure policy for incremental dirty propagation.
//!
//! Single home for the iteration policy and budget semantics: given the set of
//! files whose export fingerprint changed in the current scan, repeatedly find
//! their importers, promote the promotable ones (Skip → DirtyResolveOnly), and
//! re-check whether each promoted file's *effective* export surface changed
//! (re-export chains) so THEIR importers get promoted too. Iterates until
//! convergence, a global file budget, or a hard round cap.
//!
//! The function is pure with respect to I/O: graph lookups and surface-change
//! checks are injected as closures so the policy is unit-testable without a DB.

use std::collections::HashSet;

use cc_model::CcResult;

/// Hard safety cap on fixpoint rounds. Each round performs at least one
/// promotion (otherwise the loop converges), so with the global file budget
/// this can only trip on deep re-export chains; we warn and keep the partial
/// closure rather than bailing.
pub(crate) const DIRTY_CLOSURE_MAX_ROUNDS: usize = 16;

/// Outcome of the fixpoint dirty closure.
#[derive(Debug)]
pub(crate) struct DirtyClosureResult {
    /// Files to promote Skip → DirtyResolveOnly, in promotion order.
    /// Empty when `budget_exceeded` is set (bail = promote nothing,
    /// matching the historical single-round behavior).
    pub(crate) promoted: Vec<String>,
    /// Number of importer-expansion rounds actually run.
    pub(crate) rounds_run: usize,
    /// Round-1 direct importers alone exceeded the budget; propagation
    /// degraded to a no-op (legacy single-hop bail).
    pub(crate) budget_exceeded: bool,
    /// The closure stopped early (round cap hit, or the budget was exceeded
    /// after round 1 and later rounds were dropped); `promoted` is a valid
    /// complete-round prefix of the full closure.
    pub(crate) partial: bool,
}

/// Compute the transitive dirty closure as a fixpoint iteration.
///
/// * `initially_changed` — files whose export fingerprint changed vs the DB
///   (already parsed this build; never promoted themselves).
/// * `max_promoted_files` — GLOBAL budget across all rounds, strictly
///   greater-than like the legacy single-round check. If round 1 alone
///   exceeds it, everything is discarded (`budget_exceeded`, legacy bail);
///   if a LATER round would exceed it, the promotions from completed rounds
///   are kept and the result is marked `partial`.
/// * `max_rounds` — hard cap on iteration rounds (safety valve).
/// * `importers_of` — resolves the importers of a set of files.
/// * `is_promotable` — whether a candidate is currently `Skip` (only those
///   are promoted).
/// * `surface_changed_of(files, changed_so_far)` — per-pass batch hook:
///   returns the subset of `files` (promoted files) whose effective export
///   surface changed given everything that changed so far; those files'
///   importers are expanded next round. Promoted files whose surface did NOT
///   change are re-evaluated whenever the changed set grows (same-round
///   sibling re-export chains); each file can flip at most once, so
///   convergence stays bounded.
pub(crate) fn compute_dirty_closure<ImportersFn, PromotableFn, SurfaceChangedFn>(
    initially_changed: &[String],
    max_promoted_files: usize,
    max_rounds: usize,
    mut importers_of: ImportersFn,
    mut is_promotable: PromotableFn,
    mut surface_changed_of: SurfaceChangedFn,
) -> CcResult<DirtyClosureResult>
where
    ImportersFn: FnMut(&[String]) -> CcResult<Vec<String>>,
    PromotableFn: FnMut(&str) -> bool,
    SurfaceChangedFn: FnMut(&[String], &HashSet<String>) -> CcResult<Vec<String>>,
{
    // Everything whose export surface is known to have changed so far; passed
    // to `export_surface_changed` so re-export chains can be detected against
    // the full changed set, not just the current frontier.
    let mut changed_so_far: HashSet<String> = initially_changed.iter().cloned().collect();
    // Files whose importers still need expanding in the next round.
    let mut frontier: Vec<String> = initially_changed.to_vec();
    let mut promoted: Vec<String> = Vec::new();
    let mut promoted_set: HashSet<String> = HashSet::new();
    // Already-promoted files whose surface has not (yet) changed; re-checked
    // every time `changed_so_far` grows so sibling re-export chains are seen.
    let mut promoted_unchanged: Vec<String> = Vec::new();
    let mut rounds_run = 0usize;
    let mut partial = false;

    while !frontier.is_empty() {
        if rounds_run >= max_rounds {
            partial = true;
            tracing::warn!(
                rounds = rounds_run,
                kept = promoted.len(),
                pending = frontier.len(),
                "dirty propagation: round cap reached before convergence, \
                 keeping partial complete-round closure"
            );
            break;
        }
        rounds_run += 1;

        let importers = importers_of(&frontier)?;
        // Sorted for deterministic promotion order and budget accounting
        // (`find_importers_of` returns set-ordered results).
        let mut newly_promoted: Vec<String> = importers
            .into_iter()
            .filter(|path| {
                !promoted_set.contains(path)
                    && !changed_so_far.contains(path)
                    && is_promotable(path)
            })
            .collect();
        newly_promoted.sort();
        newly_promoted.dedup();

        // GLOBAL budget across all rounds, strictly-greater-than like the
        // legacy single-round check.
        if promoted.len() + newly_promoted.len() > max_promoted_files {
            if rounds_run == 1 {
                // Round 1 alone over budget: even direct importers don't fit,
                // so degrade to no propagation exactly like the legacy
                // single-hop code and let the caller advise a full rebuild.
                tracing::warn!(
                    dirty_count = newly_promoted.len(),
                    max = max_promoted_files,
                    "dirty propagation: too many affected files, skipping (consider full rebuild)"
                );
                return Ok(DirtyClosureResult {
                    promoted: Vec::new(),
                    rounds_run,
                    budget_exceeded: true,
                    partial: false,
                });
            }
            // Later rounds over budget: truncate at the last completed round
            // boundary. A complete k-hop closure is valid (no arbitrary subset
            // of a round is kept) and strictly ≥ the legacy single-hop
            // guarantee, which would have kept round 1.
            partial = true;
            tracing::warn!(
                kept = promoted.len(),
                dropped = newly_promoted.len(),
                completed_rounds = rounds_run - 1,
                max = max_promoted_files,
                "dirty propagation: budget exceeded, keeping partial \
                 complete-round closure (consider full rebuild)"
            );
            break;
        }

        for path in &newly_promoted {
            promoted_set.insert(path.clone());
        }
        promoted.extend(newly_promoted.iter().cloned());

        // Surface evaluation: this round's promotions plus every previously
        // promoted-but-unchanged file, re-checked until no more flips. A file
        // promoted alongside a sibling it re-exports from only flips after
        // the sibling enters `changed_so_far`, hence the inner fixpoint.
        // Flipped files are NOT promoted again — only their importer
        // expansion (next round's frontier) was missing.
        let mut next_frontier: Vec<String> = Vec::new();
        let mut candidates: Vec<String> = newly_promoted;
        candidates.append(&mut promoted_unchanged);
        while !candidates.is_empty() {
            let flipped = surface_changed_of(&candidates, &changed_so_far)?;
            if flipped.is_empty() {
                break;
            }
            let flipped_set: HashSet<&str> = flipped.iter().map(|path| path.as_str()).collect();
            let before = candidates.len();
            candidates.retain(|path| !flipped_set.contains(path.as_str()));
            changed_so_far.extend(flipped.iter().cloned());
            next_frontier.extend(flipped);
            if candidates.len() == before {
                // Defensive: the hook returned files outside `candidates`;
                // nothing shrank, so a further pass cannot make progress.
                break;
            }
        }
        promoted_unchanged = candidates;
        frontier = next_frontier;
    }

    Ok(DirtyClosureResult {
        promoted,
        rounds_run,
        budget_exceeded: false,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    /// Build an `importers_of` closure from a static file → importers map.
    fn importers_from_map(
        graph: HashMap<&'static str, Vec<&'static str>>,
    ) -> impl FnMut(&[String]) -> CcResult<Vec<String>> {
        move |files: &[String]| {
            let mut result: Vec<String> = Vec::new();
            for file in files {
                if let Some(importers) = graph.get(file.as_str()) {
                    result.extend(importers.iter().map(|p| p.to_string()));
                }
            }
            Ok(result)
        }
    }

    fn promotable(set: &'static [&'static str]) -> impl FnMut(&str) -> bool {
        move |path: &str| set.contains(&path)
    }

    fn surface_changes(
        set: &'static [&'static str],
    ) -> impl FnMut(&[String], &HashSet<String>) -> CcResult<Vec<String>> {
        move |files: &[String], _changed: &HashSet<String>| {
            Ok(files
                .iter()
                .filter(|path| set.contains(&path.as_str()))
                .cloned()
                .collect())
        }
    }

    /// Build a `surface_changed_of` hook from a file → re-export targets map:
    /// a file's surface changes iff any of its targets is in the
    /// changed-so-far set (mirrors the production re-export check).
    fn surface_from_reexports(
        targets: HashMap<&'static str, Vec<&'static str>>,
    ) -> impl FnMut(&[String], &HashSet<String>) -> CcResult<Vec<String>> {
        move |files: &[String], changed: &HashSet<String>| {
            Ok(files
                .iter()
                .filter(|path| {
                    targets
                        .get(path.as_str())
                        .is_some_and(|deps| deps.iter().any(|dep| changed.contains(*dep)))
                })
                .cloned()
                .collect())
        }
    }

    /// B's exports change; A imports/re-exports from B (its surface changes
    /// when promoted); C imports A. The closure must reach C in round 2.
    #[test]
    fn fixpoint_promotes_transitive_importers() {
        let graph = HashMap::from([("b.ts", vec!["a.ts"]), ("a.ts", vec!["c.ts"])]);

        let result = compute_dirty_closure(
            &["b.ts".to_string()],
            100,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["a.ts", "c.ts"]),
            surface_changes(&["a.ts"]),
        )
        .unwrap();

        assert!(!result.budget_exceeded);
        assert!(!result.partial);
        assert_eq!(
            result.promoted,
            vec!["a.ts".to_string(), "c.ts".to_string()],
            "C imports A whose export surface changed; C must be promoted too"
        );
        assert_eq!(result.rounds_run, 2);
    }

    /// Round 1 alone exceeding the budget bails to 0 promotions, exactly like
    /// the legacy single-hop code (direct importers over budget → no-op).
    #[test]
    fn fixpoint_bails_when_round_one_exceeds_budget() {
        let graph = HashMap::from([("b.ts", vec!["a1.ts", "a2.ts", "a3.ts"])]);

        let result = compute_dirty_closure(
            &["b.ts".to_string()],
            2,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["a1.ts", "a2.ts", "a3.ts"]),
            surface_changes(&[]),
        )
        .unwrap();

        assert!(
            result.budget_exceeded,
            "3 direct importers must exceed budget 2"
        );
        assert!(
            result.promoted.is_empty(),
            "round-1 budget bail must discard all promotions"
        );
    }

    /// The budget is GLOBAL across rounds, but exceeding it after round 1 must
    /// truncate at the last completed round boundary instead of discarding
    /// everything: a complete k-hop closure is valid and strictly more than
    /// the legacy single-hop guarantee.
    #[test]
    fn fixpoint_truncates_to_complete_rounds_when_budget_exceeded() {
        let graph = HashMap::from([
            ("b.ts", vec!["a1.ts"]),
            ("a1.ts", vec!["a2.ts"]),
            ("a2.ts", vec!["a3.ts"]),
        ]);

        let result = compute_dirty_closure(
            &["b.ts".to_string()],
            2,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["a1.ts", "a2.ts", "a3.ts"]),
            surface_changes(&["a1.ts", "a2.ts", "a3.ts"]),
        )
        .unwrap();

        assert!(
            !result.budget_exceeded,
            "post-round-1 overflow must not degrade to the no-op bail"
        );
        assert_eq!(
            result.promoted,
            vec!["a1.ts".to_string(), "a2.ts".to_string()],
            "rounds 1..N-1 (direct importer a1, then a2) must be kept; round N (a3) dropped"
        );
        assert!(
            result.partial,
            "budget truncation must report a partial closure"
        );
    }

    /// Exactly-at-budget is allowed (legacy check is strictly greater-than).
    #[test]
    fn fixpoint_allows_promotions_exactly_at_budget() {
        let graph = HashMap::from([("b.ts", vec!["a1.ts"]), ("a1.ts", vec!["a2.ts"])]);

        let result = compute_dirty_closure(
            &["b.ts".to_string()],
            2,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["a1.ts", "a2.ts"]),
            surface_changes(&["a1.ts", "a2.ts"]),
        )
        .unwrap();

        assert!(!result.budget_exceeded);
        assert!(!result.partial);
        assert_eq!(result.promoted.len(), 2);
    }

    /// Same-round sibling re-export chains: b changes; z re-exports b;
    /// a imports b AND re-exports z; c imports a. Both a and z are promoted in
    /// round 1, but a sorts BEFORE z, so a's surface check against the
    /// pre-round changed set ({b}) is false — only after z flips must a be
    /// RE-EVALUATED and flip too, so that c (a's importer) gets promoted.
    #[test]
    fn fixpoint_reevaluates_promoted_siblings_for_reexport_chains() {
        let graph = HashMap::from([
            ("b.ts", vec!["z.ts", "a.ts"]),
            ("z.ts", vec!["a.ts"]),
            ("a.ts", vec!["c.ts"]),
        ]);
        let reexports = HashMap::from([("z.ts", vec!["b.ts"]), ("a.ts", vec!["z.ts"])]);

        let result = compute_dirty_closure(
            &["b.ts".to_string()],
            100,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["z.ts", "a.ts", "c.ts"]),
            surface_from_reexports(reexports),
        )
        .unwrap();

        assert!(!result.budget_exceeded);
        assert!(!result.partial);
        assert_eq!(
            result.promoted,
            vec!["a.ts".to_string(), "z.ts".to_string(), "c.ts".to_string()],
            "a's surface depends on sibling z promoted in the same round; \
             once z flips, a must be re-evaluated so its importer c is promoted"
        );
    }

    /// Import cycle x.ts ↔ y.ts (both promotable, both surface-changing):
    /// the iteration must converge instead of looping, promoting each file
    /// exactly once.
    #[test]
    fn fixpoint_converges_on_import_cycles() {
        let graph = HashMap::from([
            ("d.ts", vec!["x.ts", "y.ts"]),
            ("x.ts", vec!["y.ts"]),
            ("y.ts", vec!["x.ts"]),
        ]);

        let result = compute_dirty_closure(
            &["d.ts".to_string()],
            100,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            promotable(&["x.ts", "y.ts"]),
            surface_changes(&["x.ts", "y.ts"]),
        )
        .unwrap();

        assert!(!result.budget_exceeded);
        assert!(!result.partial);
        assert_eq!(
            result.promoted,
            vec!["x.ts".to_string(), "y.ts".to_string()],
            "each cycle member is promoted exactly once"
        );
        assert_eq!(
            result.rounds_run, 2,
            "round 2 re-discovers already-promoted files and converges"
        );
    }

    /// A chain deeper than the round cap stops at the cap with the partial
    /// (still valid) closure and the cap flag set.
    #[test]
    fn fixpoint_stops_at_round_cap_with_partial_closure() {
        const CHAIN_LEN: usize = 20;
        let names: &'static [String] = Box::leak(
            (0..=CHAIN_LEN)
                .map(|i| format!("f{}.ts", i))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let graph: HashMap<&'static str, Vec<&'static str>> = (0..CHAIN_LEN)
            .map(|i| (names[i].as_str(), vec![names[i + 1].as_str()]))
            .collect();

        let result = compute_dirty_closure(
            &[names[0].clone()],
            1000,
            DIRTY_CLOSURE_MAX_ROUNDS,
            importers_from_map(graph),
            |path: &str| path != names[0],
            |files: &[String], _changed: &HashSet<String>| Ok(files.to_vec()),
        )
        .unwrap();

        assert!(result.partial, "deep chain must hit the round cap");
        assert!(!result.budget_exceeded);
        assert_eq!(
            result.promoted.len(),
            DIRTY_CLOSURE_MAX_ROUNDS,
            "one promotion per round up to the cap"
        );
        assert_eq!(result.rounds_run, DIRTY_CLOSURE_MAX_ROUNDS);
    }
}
