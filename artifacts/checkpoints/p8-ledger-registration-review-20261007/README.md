# Final local P8 ledger and registration review

`review.json` records an independent read-only accounting review by `/root/pr_audit` of the immutable root snapshot `85c4b58e8e6cc54f75f3ada90b8c5386c9a63cf0`. The verdict is `accepted_scoped`. No root files were edited and no Cargo, product runtime or remote action was executed for this review.

## Original TODO accounting

| Round | Original IDs advanced | Cumulative | Batch remaining | Done | In progress | Todo | Unfinished |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline | — | 0 | 10 | 150 | 1 | 41 | 42 |
| 1 | P8-001, P8-008, P8-009, P8-019 | 4 | 6 | 150 | 5 | 37 | 42 |
| 2 | P8-005, P8-006, P8-013 | 7 | 3 | 150 | 8 | 34 | 42 |
| 3 | P8-007, P8-010, P8-018 | 10 | 0 | 150 | 11 | 31 | 42 |

Exactly these ten original task objects changed from the fixed `6d02d77` baseline. Every transition is `todo` to `in_progress`; only status, evidence and implementation notes changed. Original evidence is retained as a prefix. Dependencies, acceptance criteria and validations remain unchanged. All three PLAN-CHECK receipts agree with the round counts; the final receipt hashes the actual final task file. This is ten advances, with zero new formal completions.

## PR accounting

The recorded closures of PRs [#10](https://github.com/jyqj/codecortex/pull/10), [#11](https://github.com/jyqj/codecortex/pull/11) and [#111](https://github.com/jyqj/codecortex/pull/111) match the exact audited heads and semantic-adoption recommendations. The operation receipt references the exact SHA256 of the original audit. The historical set changes from 46 to 43 retained PRs. That count excludes any later delivery PR; retained differences are not blanket proof that their semantic scope remains unadopted. This final check makes no new live GitHub assertion.

## Local source registration

The accepted local registration is commit `9e03ed67a143dae03fa7fb2d862de29d7a254513`, product source `853385b7ccb2780818f9f8e8a83791f1c197efcf`, independent semantic review commit `d07fceea16e16a7b487a0a8887125bc3e3aa1df3`, and delta base `6d02d77f018a5965a6f289b0b43558ed4b9f8322`. This review reconstructed all Git object input hashes without calling the repository admission implementation: exactly 17 changed paths extend the previous 768 inputs to 777. Before and after hashes, the separately committed review blob, current input bytes and hardcoded registry pins agree. Both prior delta records and nine historical helpers remain byte-identical. The CI change in the registration commit is only the v7 to v8 selector. The complete parity oracle is unchanged.

The explicit per-delta base extension is constrained to already accepted before-bytes and the declared path inventory. It does not import unrelated bytes from another base. This metadata and boundary review is separate from the release agent's independent semantic review of the Rust changes. The reviewer authored the scale implementation and does not self-certify its semantic independence.

Later GitData source/review publication uses different commit identities. Any R1/R2 identity rebinding requires its own fixed-source review; it is outside this receipt.

## Archived execution evidence reconciliation

All 68 load SHA256SUMS entries match the committed raw files. The four 12-file concurrency runs (1, 4, 8, 16) each contain 36 unique successful terminal operation IDs: 24 actual query operations and 12 actual modifications plus incremental builds. In total this is 144 terminal successes and 48 modifications. Every run has 15-table complete full/incremental parity and three equal query probes, without repairing the incremental index before comparison. Actual backend call peaks are 1, 3, 6, 8; observed read/build overlap is false, true, true, true. Producer logs contain 9 load integration passes, 1 route regression pass and 2 stderr unit passes.

The facts artifact records 13 positive/negative CLI cases with matching expected exits. Its committed source hashes agree with this snapshot. The observed declarations are 8 crates, schema 25, 30 base tables, 5 FTS tables and 14 MCP tools; runtime certification remains false. These producer executions were read, not rerun or reattributed to this reviewer.

The accepted scope remains local engineering evidence. Release-scale 100k results, quality gates, live providers, long soak stability, cross-platform installation and remaining P7 dependencies are not completed by these receipts. Exact evidence hashes and immutable references are in `review.json`.
