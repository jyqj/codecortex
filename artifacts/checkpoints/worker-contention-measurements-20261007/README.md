# P7-015: actual worker contention and complete local-request observations

This checkpoint adds a current, reproducible measurement fixture to the actual
post-index worker and foreground query path. The fixed three-seed run and all
original selected controls passed. It does not complete P7-015: bounded desired
projection into `reconcile_after_rebuild` and the original performance/resource
acceptance obligations remain explicit.

## Frozen source and validation

The base is `65eb87d70bd7bfd10d251b5d1cbb25850958196b`; tested source is
`f95bb87c20e67b8f42c86c6a025f3cc8157ebd60`. The sole crate/Cargo change is
`crates/cc-eval/tests/p7_worker_contention.rs`. No production source, dependency,
old oracle, threshold, task status or source-admission rule was modified.
`source-comparison.json` binds the new file, both original test files and the
complete 769-input manifest to their exact committed and on-disk bytes.

| Executed scope | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| New actual worker contention fixture | 1 | 0 | 0 |
| Original `semantic_lifecycle` target | 2 | 0 | 0 |
| Original `p7_production_fairness_model_review` target | 6 | 0 | 0 |
| Total Rust test functions | 9 | 0 | 0 |

Scoped strict Clippy and the workspace format check also exited 0. The HTTP
target was mistakenly counted as five in the pre-run plan; the unchanged
actual target contains six, including its existing closed-background-owner
physical-exit control. The original plan remains intact and this correction
is recorded in `validation-summary.json`. The 384 request observations are
samples within one test function, not 384 additional tests. There was no
production defect asserted here and no artificial red run was manufactured.

## What the actual fixture establishes

Each seed (7, 19, 43) builds a real CodeIndex containing a stable file, a file
to delete, and 24 mutable files. A real post-index worker calls a FakeProvider
with 10 ms delay. After initial publication, quiet local requests are captured;
then a genuine incremental build changes 24 files and the provider holds four
actual document callbacks. The observed queue has 4 claimed and 20 pending
items, 2 published current documents and 24 uncovered documents. This is a
partial-backfill state with real queued work.

The single DB read-pool connection can still be checked out and queried. It
is released before a separate SQLite connection executes `BEGIN IMMEDIATE;
ROLLBACK;` with the original independent 100 ms busy timeout. Actual read,
writer-acquire/rollback timings and equal generation values are recorded.
The query executor has no admitted or running CPU slots while the callbacks
wait; these internal slot counters are not measurements of OS CPU time.

During the hold, 64 real local requests per seed use `capture_for_request`
and `QueryHandle.search_async`. They cannot start or complete additional
provider calls or change the held queue/attempt snapshot. Actual mutation
and deletion still reach the local graph. An explicit model switch retires
the old runtime while the old callbacks remain held. Releasing those calls
then allows the new model to drain. All three seeds record zero manifest
publications for the exact newly held old-space input digests, maximum active
old-provider callbacks of four, and final pending/claimed/uncovered counts
of zero. Legitimate pre-hold old publications are not falsely counted as
stale work.

The fake provider bypasses the HTTP provider gate; its observed four-wide
whole-attempt boundary is not a claim that the configured HTTP per-project
limit is four. The original complete HTTP target separately exercises real
loopback admission, foreground/background progress, DB/CPU availability,
close/model-switch fencing and physical-exit ownership.

## Request observations and limits

For every seed, quiet and held phases each have concurrency 1 and 4 cells,
with 32 fixed samples per cell: 3 × 2 × 2 × 32 = 384. The measured clock starts
when a request is offered, before task scheduling and admission. Each row
retains caller scheduling, capture/admission, retrieval and offered-to-API
return times, real hit JSON and originating-work/freshness values when present.
The hit assertion requires `stable.rs`; it is not an independent body/source
validator. This new fixture measures the local API, without MCP transport
or separately instrumented backend queue/service durations.

All rows from a completed wave are joined and saved before an error is
asserted. A failed task would retain its ordinal and JoinError; per-stage
timings unavailable after panic/cancellation are explicitly unavailable.
Hard process termination can still leave the current wave incomplete. This
successful run has all 384 rows, no failed request, no best-of selection and
no silent retry. Ordinary cache behavior is retained.

These are descriptive nearest-rank quantiles from 32 samples per cell; the
P99 in each cell is its maximum. Values below are milliseconds:

| Seed | Concurrent requests | Quiet P50 | Quiet P99/max | Held P50 | Held P99/max |
| --- | ---: | ---: | ---: | ---: | ---: |
| 7 | 1 | 6.682 | 26.163 | 5.918 | 23.116 |
| 7 | 4 | 12.026 | 41.692 | 11.173 | 90.064 |
| 19 | 1 | 8.849 | 24.910 | 9.055 | 1119.096 |
| 19 | 4 | 15.401 | 39.045 | 27.184 | 110.638 |
| 43 | 1 | 10.478 | 1351.478 | 8.492 | 27.280 |
| 43 | 4 | 28.123 | 50.885 | 20.288 | 41.326 |

The 1119.096 ms held request at seed 19 and 1351.478 ms quiet request at seed
43 remain in the raw observations. Most of those measured intervals falls
within the retrieval segment; the source of the delay is not established.
Both phases show tail variability. All calls satisfy the original two-second
progress watchdog, but these data do not certify a P99 degradation limit.
No new SLA, adjusted budget or confidence interval was invented. Individual
build/progress waits retain the original five-second watchdog. Reported
`write_delete_us` measures the incremental build after the file writes and
deletion; it does not include those preceding filesystem operations.

Runner, server and server-tree CPU/RSS attribution is explicitly unknown.
The new fixture runs its CodeIndex and fake provider in the test process;
raw sampler/process results are stored separately and are not promoted to
verified ownership or sampled peaks. In this run the process snapshot and
`/proc/PID/exe` observations were null, while `current_exe` identifies the
actual test artifact recorded by Cargo. No unavailable value is filled with
zero. `request-summary.json` independently reproduces counts and quantiles
from all rows and records the exact accepted and unaccepted boundaries.

The existing runtime's bounded worker page/attempt mechanisms do not close
the separate old `reconcile_after_rebuild` desired-projection responsibility.
That original wiring item 11 remains pending.

## Replay and build provenance

The original execution used Rust 1.95.0, the unchanged lockfile, offline mode,
two build jobs, debug information disabled and incremental compilation off.
The actual Rust compiler executable/version/hash and empty wrapper overrides
are recorded. One runner owned the shared target. Before building, it ran:

```sh
cargo +1.95.0 clean --offline --locked -p cc-semantic -p cc-server -p cc-eval
```

This targeted invalidation follows a real preceding cross-worktree stale
cache incident. Its unchanged raw log reports 1151 files and 5.1 GiB removed;
other dependency caches were not deleted. Each of the three relevant
workspace libraries was actually rebuilt (`fresh=false`) from this worktree.
The complete source maps are identical before/after every phase, and each
workspace compiler artifact's source path, features, profile, file hashes and
executable hashes are in the receipts. This scoped binding is not a hermetic
build or dependency-trust certification.

```sh
export CODECORTEX_WORKER_CONTENTION_EVIDENCE_DIR=/path/to/new/worker-observations
export CODECORTEX_LIFECYCLE_RECEIPT_DIR=/path/to/new/original-lifecycle
cargo +1.95.0 test --offline --locked -j2 -p cc-eval --features semantic --test p7_worker_contention --test semantic_lifecycle -- --nocapture
cargo +1.95.0 test --offline --locked -j2 -p cc-server --features semantic-http --test p7_production_fairness_model_review -- --nocapture
cargo +1.95.0 clippy --offline --locked -j2 -p cc-eval --features semantic --test p7_worker_contention --test semantic_lifecycle -- -D warnings
cargo +1.95.0 fmt --all -- --check
```

Actual command arrays, including Cargo JSON output flags, environment,
durations, raw-log hashes and source/production/test artifact identities are
in the phase receipts. `capture-validation.py` is the executed local capture
script with its original absolute paths; it is evidence, not a portable
installer. No external provider, whole-workspace rerun or source guard was
executed. Combining P7-019 or other later changes requires separate evidence.

## Storage

Six large request arrays and two raw Cargo logs are stored losslessly in
`raw-observations.tar.gz` under their original relative paths. Small per-seed
protocols, queue snapshots and summaries, original lifecycle observations,
source maps and phase receipts remain directly readable. The original-file
index and read-only verifier preserve all raw bytes and validate any matching
untracked mirrors without extraction or payload execution. See `STORAGE.md`.
Storage success does not upgrade the incomplete overall P7-015 acceptance.
