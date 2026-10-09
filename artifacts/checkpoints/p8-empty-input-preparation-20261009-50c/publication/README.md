# Empty-input preparation: engineering and independent acceptance evidence

This checkpoint records two `cc-index` preparation fixes, their original failures and corrected engineering checks, and independent read-only reviews of existing G275 acceptance artifacts. The reviews preserve the original task definitions, dependency order, sample denominators, source identities, and unavailable measurements.

The task ledger remains **192 original tasks: 163 done, 16 in progress, 12 todo, 1 blocked; 29 remaining**. This session has closed **0 original tasks**. The request to complete at least 10 original tasks is not satisfied by these engineering fixes or by the downstream evidence reviews. In particular, P8-005 and P8-006 still require the complete registered scale study before their dependent tasks can close.

The final ledger SHA-256 is `a0d24b33c2f68dd33697b2b0a5c38cb5f633f3f8f8a199b8e578f372b3b99ec7`. Independent reviews verify that only nine original tasks receive appended evidence/notes, the other 183 task objects remain identical, and all original definitions, dependencies, statuses and navigation remain unchanged. `rounds.json` records the original-task count for each of the six rounds. The original no-argument plan check and historical-integrations verifier both passed on the prospective publication working tree; `final-publication-checks/` retains their exact argv, outputs and source boundary. These checks validate the ledger and frozen definitions, not full runtime or release acceptance.

## Source identities

| Role | Exact source |
| --- | --- |
| Main baseline for this PR | `bd4693351149a443bd0054a37508c53f74ed7b52` |
| Corrected product P2 | `b4fef72211e5967f4fba729d25d0ca2958094fd5` |
| Evidence and independent product review R | `98fe910f22eb92f8d9f9c8a8043492ba45dc997c` |
| Original v15 binding G | `50d9b3bc7d1ecca1b6b1c3822a36c12b546b0529` |
| Existing acceptance source G275 | `275e8799d4947d297329073eaa3ca675d3fd0777` |
| Separately owned scale candidate Gc8 | `c8be5afaac568ffd40ef86d3795423c3b73c9f39` |

G275 acceptance results certify only the reviewed G275 observations. They do not certify P2, PR #181, Gc8, a released package, or a combined candidate assembled from different sources. The final publication commit adds documentation and evidence after G; the original P2, R, and G objects remain unchanged.

## Engineering changes and original outcomes

The full-snapshot config path skips symbol target preparation and deep map clones when the already-scanned heuristic token set is empty. Typed config handling, empty cache serialization, clearing old links, and incremental behavior retain their original semantics. The interface-dispatch path delays unused UID/container maps until interface and implements inputs require them. It retains the original CALL, symbol, and implements SQL read order and typed error propagation; it does not reduce those SQL reads.

The final P2 checks include formatting and strict full-workspace Clippy success, 23 `cc-index` test targets with 590 passed and 3 pre-existing ignored tests, and the selected evaluation corpus test. The full-workspace test command failed in the existing Linux child-process sampler test, and its isolated retry also failed. The sampler file is byte-identical to main. A local PID/proc namespace mismatch was observed; it is not a proved root cause or a baseline pass. Full-workspace tests therefore remain failed, not waived.

The first new config test used the wrong existing SQLite column name and failed before it was corrected. The original remote product failed Clippy in three new test assertions before correction in P2. These original failures remain in the parent checkpoint. No shortened rerun replaces an original failed command.

The subsequent original [GitHub CI run 37903250638](https://github.com/jyqj/codecortex/actions/runs/37903250638) completed successfully. Its actual checkout `0805905f48b963b60864f40e74fef6aed7c4f934` and G have the identical complete Git tree `87e746f47d4ea8c1a6b06cb29e180fc7bbd78137`. `g181-ci-review/` preserves the full original check log, official API inputs, and an independent review. Both the default and eval-http executions of the child-sampler test passed there. The complete original check log contains 419 Rust summaries, 3,291 passed executions, 0 failed and 132 original ignored executions; Python shows 650 distinct verbose method results and nine additional nonverbose tests. CI used Rust 1.99.0 and its original selected default/semantic test commands, so it is not a rerun of the local Rust 1.95.0 unfiltered workspace command. Both outcomes remain visible without assigning a proved cause to the local failure. The new read-only log parser's initial over-strict count assertion and its correction are retained; the original CI log was unchanged.

`g-binding-independent-review.json` checks all 1,091 product inputs and 139 validation inputs, the reviewed delta, evidence files, and the four permitted v15 binding constant changes. The original guard algorithm, frozen files, exclusions, version, and historical base remain unchanged.

## Native source checks and exact partial-clone repair

`native-source-checks-initial/` retains both original failed commands. The native suite ran 124 tests and reported four setup/import errors; the v15 CLI also failed because historical Git input was unavailable.

The diagnosis identified exactly two absent historical `.github/workflows/ci.yml` blobs in the partial clone. `native-source-history-diagnosis/` records their original OIDs, the normal explicit fetch, and the original catalogue/protected-byte verification after hydration. No source guard, test, execution bound, branch, or worktree changed. `v13-inventory-cascade-diagnostic.json` explains why the failed v14 adapter caused the earlier v13 inventory cascade; no later validation paths were removed.

The unchanged native suite and original v15 entry both completed with exit 0 at exact G under the same per-command 3,600-second bound. The original suite reported **181 tests in 1,304.289 seconds, OK**, with 181 method results and no errors, failures, or skips. Its supervisor wall time was 1,314.941 seconds; the separate v15 CLI took 212.522 seconds. Both post-command HEAD checks remained G. `native-source-checks/` preserves the final bytes collected under the local `native-source-checks-hydrated` directory, including receipt SHA-256 `a1fa063e889d04e2e14eb58015ee8b651df31a92af7a566fe29124d17764baeb`. The original failed invocation remains in `native-source-checks-initial/`. This source-integrity success does not override the separate full-workspace Rust failure.

## Independent review of original G275 artifacts

| Original tasks | Evidence package | Accepted observations and limits |
| --- | --- | --- |
| P8-007, P8-010 | `g275-runtime-review/` | Four 900-operation mixed workloads and the actual 3,600-second soak; original statistics and all 15 oracle tables independently reconciled. Configured concurrency is 1/4/8/16; observed maximum active client calls are 1/4/7/12. Queue status is sampled, and raw operation rows do not provide unique RPC IDs. |
| P8-008, P8-009 | `g275-lifecycle-review/` | All 1,230 measurements and 1,261 resource snapshots checked against the original seals, raw transcripts, databases, and byte-identical statistics replay. OS page cache was not cleared. N=30 cold upper confidence bounds remain null. Process roles and physical storage retain their original scope; unavailable totals and provider costs remain null. |
| P8-011, P8-012, P8-013, P8-016 | `g275-platform-gates-rollback-review/` | Eight original fresh platform cells; source-bound failure tests and offline failure controls; actual schema 25 → historical source schema 24 → 25 rollback; scoped local recovery and active fake-provider faults. Local recovery remains 3 passed / 4 not_run. Historical source builds are not released-package attestation. |

Runtime and soak originals are from [run 37890757030](https://github.com/jyqj/codecortex/actions/runs/37890757030). Lifecycle and resource originals are from [run 37890757049](https://github.com/jyqj/codecortex/actions/runs/37890757049). Platform originals are from [run 37890757129](https://github.com/jyqj/codecortex/actions/runs/37890757129). Original failure gates are from [run 37890756917](https://github.com/jyqj/codecortex/actions/runs/37890756917). Each package identifies its further raw artifacts, source manifests, file hashes, command outcomes, and review limits.

`round6-g275-ci-recheck.json` independently rechecks the complete original CI at actual checkout `d53ea0be17a53d1354ee1d6f0a111c5e588e91f5`: 636 unique Python methods passed; 418 Rust summaries contain 3,281 passed executions, 0 failed, and 132 original ignored executions. All 1,089 product and 139 validation inputs match G275 by path, mode, and blob. The separate `round6-g275-source-artifact-bridge.json` checks the 187 consumed artifact paths and 236 historical protected paths, addressing the different artifact roots. This establishes the stated input equivalence while preserving the original CI checkout and command identities; it is not a new G275 execution or certification of P2.

The failure-gate review distinguishes six original exact tests from seven offline command invocations. Three retained comparison reports are byte-identical; two differ only in copied input paths; the zero-latency comparison is an additional control without an original comparison report; overwrite refusal preserves the existing output bytes and does not produce another comparison report. The original zero-plan case is supported by its original in-memory test rather than a fabricated replay report.

The rollback reader's first diagnostic attempt failed because it expected a generic status label. Its corrected read-only attempt recognized the original `passed_actual_source_version_pair` label. Both diagnostic attempts are retained; this did not change or reexecute the original product workload.

`g275-fault-scope-addendum.json` records the retained evidence granularity: the internal crash controls include their original exact test binaries, build/execution logs, and receipts, but their temporary per-seed SQLite files were not uploaded. The independent review does not claim to replay those inner databases. Original local and active end-to-end fault RPC/database artifacts remain separate. The recovery drill used the original development profile, while the eight cold platform cells used release builds; these identities are not interchangeable. Neither a released tag nor per-seed database upload is added as a new task acceptance condition.

## Open acceptance and PR management

The complete P8-005/P8-006 protocol requires all 150 registered scale shards and 1,500 stage/fanout samples with full oracle acceptance. [G275 run 37896198208](https://github.com/jyqj/codecortex/actions/runs/37896198208) and [Gc8 run 37902429727](https://github.com/jyqj/codecortex/actions/runs/37902429727) are separate studies. Timestamped status snapshots in this directory are metadata observations, not accepted raw sample counts. This session made no manual scale or soak dispatch, added no scale label, and did not cancel or rerun either owner's study.

The detailed review also found a remaining P8-007 attribution requirement, shared with the P8-010 performance scope, in the original benchmark section 9. The original client phase durations, build stage timings, and sampled pool counters support their declared observations, but they do not separately measure backend queue/service durations, database lock wait, and worker contention for the mixed workload. The required timing boundaries are not recorded in the original raw data and cannot be reconstructed from successful outcomes. Main and inspected PR #175 at `8da1f956f6b0dd3413a37c639a79e1e0d939de81` do not expose all of them. `downstream-acceptance-gap-review.json` and `p8-runtime-attribution-source-audit/` map the original requirements to evidence and the minimum missing diagnostic boundaries. Completing that attribution requires product diagnostic wiring, an independently reviewed new source binding, and new observations; this publication does not modify the benchmark requirements or relabel the existing G275 evidence as complete.

[PR #181](https://github.com/jyqj/codecortex/pull/181) carries the two engineering fixes and this evidence publication. The local unfiltered workspace failure, the later successful original GitHub CI, and the incomplete full scale/attribution acceptance retain separate records. Current head-specific automatic check states are kept in the PR and timestamped snapshots. Its normal automatic workflows were triggered by opening/updating that PR; no manual scale dispatch was requested.

[PR #99](https://github.com/jyqj/codecortex/pull/99) was closed only after independently verifying that all 19 original files and modes were retained in the mainline diagnosis archive through its successors. Its branch and head were retained. [The PR #179 review](https://github.com/jyqj/codecortex/pull/179#pullrequestreview-5467271098) reports the original formatting failure at its reviewed head; the author's later correction is a separate head and is not conflated with that result. Other active owners' branches and runs were preserved.

No live provider, LLM, or billing work was authorized or performed. Missing live evidence and the remaining release dependencies retain their original open/blocked states.
