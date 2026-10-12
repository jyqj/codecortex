# P8 release-evidence JSON number repair — 2026-10-12

## Scope and decision

Base: `8fba6ef15a586eceed6c089b136cd0e3d44448b8` (`main`). This is a narrow, preparatory repair for P8-020 / failure-exit integrity, not original-task or G8 acceptance. Keep the resulting PR Draft until ordinary CI and independent source admission/review are complete. No workflow, source-proof guard, threshold, original task definition, dependency, historical measurement, or existing test file is modified.

## Reproduced defect

`scripts/p8_release_evidence.py:read_json` rejects named NaN/Infinity constants but previously admitted numeric JSON tokens such as `1e999` as Python infinity. Candidate/archive verification then raised an uncaught `ValueError` during canonical checksum calculation. The actual CLI exited 1 with a traceback instead of emitting the specified `invalid_input` JSON receipt and exiting 2.

The repair rejects non-finite parsed floats at the JSON input boundary using `parse_float` and `math.isfinite`. Finite floats (including signed zero), integer/bool/string types, duplicate-key rejection and named-constant rejection retain their behavior. It does not turn any failed gate into a pass.

## Actual validation

- Before the repair, the new regression module ran 7 test methods and recorded 7 failing subcases. The separate baseline existing release-evidence suite passed 42 tests.
- After the repair, 86 selected tests passed: 42 existing release-evidence, 19 existing release-review, 18 existing judge-evidence, and 7 new JSON-number tests. The three existing test files are byte-identical to the base commit.
- These were actual Python 3.9.6 executions in a scratch source copy containing exactly those four test modules, the corresponding three implementation modules, and immutable definition/task fixtures. They are not the full repository test suite, Rust compilation, complete source-proof validation, a live judge run, or GitHub CI.
- The exact local invocation was `TMPDIR=/private/tmp python3 -B -m unittest discover -s scripts/tests -p 'test_p8_*.py' -v` in that scratch copy. `TMPDIR` uses the canonical macOS path required by the existing anti-symlink checks; no assertion or safety check was relaxed.
- Published diagnostic logs are path-redacted copies. The receipts include original and published SHA-256 values; unredacted originals remain in the local execution checkpoint. This is not a new native scale-study archive.

## Original TODO accounting

The freshly read main ledger declares 192 original tasks: 176 done, 3 in_progress, 1 blocked and 12 todo; 16 remain not-done on that ledger. Its earlier twelve-task closure is the 2026-10-10 original-task-scope decision based in part on 45 cells / 85 records, not a newly established same-source N30 / 150-shard / 1500-observation acceptance in this execution.

The stricter carried handoff remains 164 accepted / 28 remaining, with only P8-005 credited against the earlier 163-task baseline (1/10). This execution does not independently recertify the twelve additional ledger-done IDs: P8-006, P8-007, P8-008, P8-009, P8-010, P8-011, P8-012, P8-013, P8-016, P8-017, P8-018, P8-019. No main task status is rewritten on the basis of this accounting discrepancy.

In each of this execution's three rounds (inventory/review, reproduction/repair/validation, publication/CI inspection), newly closed original TODO IDs are `[]` and the new completion count is 0. The patch, test methods, evidence documents, commit and PR are not original TODOs. Historical main-ledger growth of 13 must not be represented as 13 newly verified strict acceptances here.

## PR and execution constraints

The seven pre-existing PRs (#3, #4, #65, #127, #137, #188, #191) were open and Draft at intake. #188 retains concrete performance counterevidence; #191's 80 paired finite measurements do not establish a general full-build improvement. Neither is merged or retargeted. Existing branches/worktrees and original evidence are preserved. All 389 remote branch records were inventoried before this execution's new branch; no old branch was deleted.

Main CI run 38220672202 and the four other workflows at the exact base commit were observed successful. That is evidence about the base, not this repair's future PR head.

Two independent read-only local Claude Code review processes were attempted. Both exited 1 before producing a model response: `Failed to authenticate: OAuth session expired and could not be refreshed`. The Runner's registered coding-provider list was empty. Successful independent agent reviews: 0. The main assistant performed the repair and review; concurrent deterministic test processes are not represented as multiagent review. No credentials/settings were changed, no cloud coding session or paid benchmark/judge run was started, and no existing Actions experiment was cancelled or rerun.

Remaining acceptance blockers include the strict incremental-study evidence, G8 public DEV quality/clean holdout/current-candidate scope, authorized live-provider evidence for P7-018/P8-015, explicit optional judge scope for P8-014, and P9's original dependency on P8-020. Ordinary CI and independent review remain required for this patch.
