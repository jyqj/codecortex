# Round 6 execution acceptance records

This directory records work against execution commit
`29682890c89511dd6f477a6bf48bd969aa1537af`. It is an in-progress evidence
collection, not a completed task or release certificate. At the preparation
checkpoint the original ledger remains **163 done / 192 total / 29 remaining**.

## Execution and review identities

| Purpose | Exact Git identity |
| --- | --- |
| Existing main baseline | `7354db236c9d9850a75f31672697ae9eab44565e` |
| Original round 5 execution | `599a7050e7d52b5b7b93975c419138e175b3f754` |
| Round 6 product and local regression checkpoint | `3e3981163efd8def60b89dde150d5bd9e09fccf8` |
| Round 6 independent review checkpoint | `f25d0b994edf9c08c5514864701e3b050b317c88` |
| Round 6 pinned execution | `29682890c89511dd6f477a6bf48bd969aa1537af` |
| GitHub PR merge checkout for current CI and P7 engineering | `452620b3aed27b7b61b56cae19d25ba315437b93` |

The merge checkout has parents `7354db2` and `2968289`; both the merge checkout
and the execution head have complete tree
`c75038d358e6c8b881a246bfd3c3111cda86dde0`. Original CI logs retain the actual
merge checkout identity. The independently reviewed full product input
manifest contains 1,087 files; its canonical SHA-256 is
`ac7421155638a64b3690cb63d5434e562f1b0645f0145d298e9594ef62175a59`.

## Applicability of original observations

The round 6 change affects the runtime observer's exceptional cleanup and
terminal evidence handling, plus its tests. All 1,087 product inputs are
unchanged from the round 5 execution. Of the 136 validation inputs, exactly
`scripts/p8_runtime.py` and `scripts/tests/test_p8_runtime.py` changed.

The exact product inputs, observer modules and local import closures for
scale, lifecycle/resources, platform cold builds, fault/rollback, gate
controls and P7 evidence were independently compared. Their round 5 original
observations remain at their original commit and are associated with this
candidate through
[the applicability proof](preparation/platform-review/round6-execution-applicability-proof.json).
This does not relabel an original artifact as a round 6 execution.
The separate [historical observer addendum](preparation/platform-review/round6-historical-observer-applicability-addendum.json)
explicitly checks the original P7 lifecycle driver under `artifacts/`, outside
the 136 validation input inventory. P7 closeout, mechanism and offline jobs
explicitly check out head `2968289`; their original logs must retain that
identity.

The mixed runtime, one-hour soak and fake-provider backfill receipts all
include the changed runtime observer. Each requires a fresh actual round 6
execution and independent original-artifact verification. The active replay
helpers require the exact frozen checkout, complete source inventory and
exact observer key set. They check all owned writers are stopped before the
sealed inventory is accepted, then replay the original statistics and
full-table oracle where applicable.

The original full scale run is
[37835810882](https://github.com/jyqj/codecortex/actions/runs/37835810882).
The new runtime run is
[37844310853](https://github.com/jyqj/codecortex/actions/runs/37844310853).
Neither a queued job nor a successful partial matrix completes an original
TODO. The active checklist is
[round6-final-ten-status-review-checklist.json](preparation/platform-review/round6-final-ten-status-review-checklist.json).
It preserves the original 192 task definitions, acceptance criteria,
dependencies and status-update rules.

## Preparation and completed regression records

[preparation/preservation-manifest.json](preparation/preservation-manifest.json)
records the exact original path, repository copy, byte count and SHA-256 of
the helper reviews, actual manifest controls, frozen-checkout record, source
applicability proof and concurrent PR triage. Those preparation records do
not assert successful native runtime observations.

[p7-engineering/p7-engineering-independent-review.json](p7-engineering/p7-engineering-independent-review.json)
records current P7 engineering run
[37844310755](https://github.com/jyqj/codecortex/actions/runs/37844310755),
job `113541228282`. All 14 job steps succeeded, with 24 Rust result groups
and 15 Python evidence-lock tests. Result groups are not claimed as unique
tests. The original ignored stdio mechanism test remains explicitly ignored;
the separate source-isolated mechanism workflow is reviewed independently.
The full original official job log is preserved in
[p7-engineering/original-job-log.tar.gz](p7-engineering/original-job-log.tar.gz),
with original and archive hashes in
[p7-engineering/preservation.json](p7-engineering/preservation.json).

The separate round 6 cleanup checkpoint preserves the failed old controls,
the corrected controls, a second reviewer's actual controls, and all 361
passing P8 regression tests. These protocol controls are not native load or
performance observations. Their original execution and source identities
remain in the adjacent `round6-runtime-cleanup` records.

## Remaining acceptance at this checkpoint

The complete scale matrix, every new mixed concurrency level, new one-hour
soak, new backfill, and current final CI gates must finish and be independently
accepted before proposing the ten original task transitions. Accepted scope,
source admission, live-provider quality and release approval are recorded
separately according to the original repository contracts. The existing
real-provider authorization block remains unchanged.
