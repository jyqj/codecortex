# Same-prepare positive-binding window draft

Status: fixed unreferenced source draft, awaiting independent review and actual Rust validation. No product source ref, task state, workflow, threshold or prior measurement was changed.

The sole proposed source path is `crates/cc-index/src/indexer_phases/reconcile.rs`. Its exact base is G2 `0272a1fb152fd76a7cfb22386a580629d4038a64`, blob `9617e0f241c624ce35db3c6db8e5eba568fb7594`; the actual P3 `90838d6c890b12f031fa0be9d1956cfbdef9e8d6` has the same complete product tree and this same blob. The candidate is not a new measured source commit.

## Change and eligible case

The dirty closure already queries positive binding dependents with limit `budget.saturating_mul(2)`, so its result retains up to `2*budget+1` distinct sorted paths, using the existing saturating arithmetic. The draft keeps a copy only when the actual callback query list equals the initial seed list. Capture occurs before dependency and root candidates are appended, sorted or deduplicated.

The final pending check reuses that window only if:

1. A matching query actually produced a window in this prepare.
2. Its exact ordered seed list equals the final root/propagated union.
3. The number of promotions added to completed is no larger than the original closure budget.

A missing window, new or removed seed, reordered or duplicate seed list, or violated admission bound takes the original final `surface_dependents_bounded(..., 0, completed)` query with its original error propagation and SqlWork accounting. Comparing exact lists is deliberately stronger than mere subset coverage: a window for a strict superset could contain a witness belonging only to a removed target.

The lookup itself, SQL parameters, global sorted/distinct result, exclusions, budget, round cap, deterministic promotions, propagation, dependency-window logic, root-pending check, durable frontier payload and status selection remain unchanged. SqlWork includes only statements actually run; skipped SQL is not assigned invented zero-cost observations.

## Finite-set proof

Fix one query basis S and the original completed set E. Let D be all distinct matching positive-binding paths outside E, in the original SQL/global order. The actual initial query materializes W, the prefix of D limited by the unchanged `2*budget+1` window. During this prepare the only added completed paths are P, the actual promoted paths, with at most budget entries.

If W contains a path outside the new completed set, a pending dependent exists. If it contains none and D had an unmaterialized suffix, the retained overflow prefix would contain more than budget distinct paths; at most budget newly completed paths cannot remove every witness. If there is no unmaterialized suffix, W was the whole eligible set. Thus the pending boolean equals the original final query for the same seed basis. Saturating limits keep the existing representable/global cap; the SQL and helper's existing upper-bound contract are not redefined.

This argument counts all promotions, including paths from the separate dependency/root candidates. Such paths can consume the admission budget without consuming a window witness. Equality of actual queried seeds with the final seed list is necessary for this local proof; the draft never infers coverage from a guessed frontier.

An author-only JavaScript enumeration checked 1,048,576 eligible cases over all dependency, initial-completed and promotion subsets of a six-element universe and budgets 0 through 6. This is a finite-set sanity check, not Rust, SQLite, planner, Cargo or performance execution.

## Lifetimes, writes and fences

The owned window is a local variable in this single `plan_dirty_reconciliation` call. No read lease survives the lookup; no cache, Indexer field, durable frontier field or cross-prepare cursor is added. Every new prepare queries again, including after replacement, deletion or reopen. Extra retained memory is one clone of at most the already bounded positive-window rows and their path strings. This is a row-count bound, not a measured fixed-byte RSS allowance.

The unchanged full prepared-build fence also covers `previous=None`, `pending=false`, `reconcile=None`:

- Exact `build_plan.rs` blob `ffe62dd2827b37461c8db914e11b8279a03b8f15`, lines 224–228, captures prepared_index_epoch before the first DB read.
- Lines 260–265 pass that epoch into DirtyClosed/reconciliation.
- `commit_write` lines 432–433 independently unpack prepared_index_epoch and the optional reconcile update.
- Lines 451–457 unconditionally compare the current index epoch and return typed StalePreparedBuild on mismatch, with no condition on the optional update.
- Only later, lines 470–480, does phase_write receive reconcile.as_ref().

For a Some update, the unchanged `WriteOps::write_reconciled_batch` delegates to `IndexDb::write_incremental_batch`. That writer begins an immediate transaction and checks update.expected_index_epoch before any mutations; a reconcile update prevents the empty-batch early return. The exact writer blob is `2758481da50748be45f9c35a35832d2f2bc9b851`.

These are the existing prepared-build/caller-write-lock contracts. The optimization does not add a read transaction spanning the separate reads, does not claim a new cross-process atomic check-and-write guarantee, and does not make a stale read valid. An external index write during the observed prepare period must still fail the existing commit fence. Evidence-only epoch movement remains outside index-content invalidation as before.

Executed-query errors still propagate. An eligible case no longer performs the redundant final read and therefore cannot encounter an error specific to that omitted read; it does not swallow errors from any query that still executes.

## Regression draft

The original three dependency-window tests and all their assertions are byte-identical. Six new Rust tests are appended in `indexer_phases::reconcile::surface_window_tests`:

- `materialized_surface_window_matches_original_pending_sql`: actual SQLite, pool size one, 401 seed targets spanning three original IN batches, overlapping consumers, budgets 0/1/2/8/40/usize::MAX, empty/scattered/nonexistent/39-file completed prefixes, prefix/alternating/reversed promotions. Compares the helper with the original final SQL and records real statement counters if the established observations directory is supplied.
- `surface_window_requires_exact_queried_basis_and_bounded_admission`: a real a→b→c chain proves that adding b to the seed basis exposes c outside the old window; checks fallback for missing window, excess admissions, subset/reordered/duplicate seed lists.
- `surface_window_reloads_after_replacement_removal_and_reopen`: real replacement/deletion/reopen with pool size one and a fresh window.
- `prepare_reuses_surface_window_without_changing_promotions`: actual planner with budgets 0/1/2 checks deterministic actions/status, unchanged DB epoch and the real three-statement positive query count for an unchanged seed basis.
- `prepare_requeries_surface_when_propagation_adds_a_seed`: actual unknown facade propagation checks the original final SQL still runs, keeps the c.py remainder, and records three original lookup groups/nine statements.
- `reused_surface_window_keeps_external_write_epoch_fence`: actual planner plus an intervening DB write verifies the transaction rejects the original update specifically with StalePreparedBuild and leaves generation/frontier unchanged.

These six tests have not been compiled or run. Their fixture records are contract tests, not new workload measurements. The original whole-build stale-prepare test in build_plan.rs, original DB stale-frontier test, dirty-closure controls and complete parity gates remain unchanged and remain relevant.

Suggested future engineering commands, not executed by this draft:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo test --release --locked -p cc-index --lib indexer_phases::reconcile::surface_window_tests -- --nocapture` (require all six exact names)
- `cargo test --release --locked -p cc-index --lib indexer_phases::reconcile::dependency_window_tests -- --nocapture` (require the original three exact names)
- `cargo test --release --locked -p cc-index --lib build_plan::tests::stale_prepared_build_is_rejected_at_commit -- --exact --nocapture`
- `cargo test --release --locked -p cc-db --test p2c_freshness_store`
- `cargo test --release --locked -p cc-db --test p2d_dependency_cost`

Any integration still needs the ordinary source review, actual tests and original workload/parity gates appropriate to its own source. This draft does not retag G2, P3, a23 or any prior receipt.

## Performance scope and deliberate non-goals

The motivation is the existing SQL's repeated ordered prefix scan while excluding an increasing completed set. This proposal removes only the redundant final positive-binding lookup in the proven unchanged-seed case. It does not eliminate the initial lookup or the remaining prefix work on a later prepare; it does not optimize the dependency-store SQL.

No speedup ratio, wall-time improvement, RSS trend, benchmark completion or TODO closure is claimed. The window clone adds bounded memory/copy work whose net impact still requires actual measurement. The prior observed large row/VM counts motivate investigation; they are not measurements of this draft.

The draft does not disable the query_only read pool, inject temporary excluded tables, use an unsafe max(excluded) cursor, or persist cursors through index epochs. Those alternatives need different proofs and are outside this candidate.
