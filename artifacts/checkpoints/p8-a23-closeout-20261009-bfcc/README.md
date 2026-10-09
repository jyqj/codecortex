# P8 a23 original acceptance checkpoint

This checkpoint tracks ten **original** tasks: P8-005 through P8-013, plus P8-016. At this progress snapshot the authoritative ledger remains **192 total, 163 done, 29 remaining; zero newly completed tasks in this session**. All original task definitions, dependencies, workload registrations, failure records and release boundaries remain in force.

## Fixed identities

| Role | Exact identity |
| --- | --- |
| Main inspected for integration | `4775bf44dc09c11cea88211e06ccec97c4de44aa` |
| Product composition | `254009277688d64677361a0ca33e5dea73f295ef` |
| Independent source review | `3a11f30f9f00a89fe3cd481b3b7609066728baad` |
| P8 actual measurement source | `a23bb72d3c954f385b99fe81ce9189885c208557` |
| Actual CI / security / MSRV / P7-engineering checkout | `75649f8cb08dcd5427e7e58b1ca2b8fa67c9d037` |
| Identical full Git tree for that CI checkout and a23 | `58147c952505c44da1f41eb4b9c31643f2303b96` |

The CI merge checkout is recorded as actually observed. Official Git commit metadata proves every tracked path, mode and blob is identical to a23. P7 closeout and the P8 measurements keep their own actual checkout identities. See the [CI identity proof](round4-reviewed-progress/review-ci/actual-ci-p7-checkout-tree-equivalence.json) and [original CI audit v2](round4-reviewed-progress/review-ci/ci-original-check-audit-v2.json). V1 is retained and superseded only for its incomplete checkout labeling.

## Actual progress; not task completion

| Original acceptance area | Verified evidence at this snapshot | Still needed for original task closure |
| --- | --- | --- |
| P8-005 / 006 scale and fanout | Original 1k, 5k and 10k index-0 shards: 3/150 shards, 32/1500 samples independently reviewed; five original capacity receipts | Original 50k/100k preflight, remaining 145 shards, strict complete aggregate and hard dependency closure |
| P8-007 mixed load / backfill | Four original C1/4/8/16 jobs, 900/900 operations each; actual peaks 1/4/7/12; 768 fake-provider backfill requests; complete raw/stdio/statistics/parity review | P8-005/006 dependency closure |
| P8-008 / 009 lifecycle / resources | 1230 original samples, 1261 resource snapshots, physical/logical storage attribution and null/unknown cost boundaries independently checked | Original prior-task dependency closure |
| P8-010 soak | Original one-hour workload began at 02:48:58 UTC on 2026-10-09 | Actual terminal run, all offered operations, original resource trends, four completion-time cache windows and unrepaired final parity |
| P8-011 / 016 recovery / rollback | Actual local/active fault records, 14 database-copy integrity/FK checks, source/config preservation and historical 25→24→25→backup rollback reviewed | Original hard dependencies |
| P8-012 platforms | Eight original fresh release builds and eight-cell collector passed; actual Linux/macOS × Rust 1.95.0 / observed stable 1.99.0 × default/semantic | Original hard dependencies |
| P8-013 failure gates | Six non-ignored original Rust controls and seven exact retained-CLI scenario replays with expected nonzero exits; original bytes unchanged | Original hard dependencies |
| Common CI / P7 regression | Original CI and P7 workflows succeeded; all seven new inline Rust tests appear as `ok` in the original CI log | No replacement of scale/soak acceptance is inferred |

The [copied audit manifest](round4-reviewed-progress/copied-audit-manifest.json) records exact bytes and SHA-256 for this progress snapshot. The [runtime evidence map](round4-reviewed-progress/review-runtime/round4-p8-007-009-evidence-map.json) and [scale acceptance checklist](round4-reviewed-progress/review-scale/scoped-acceptance-checklist.json) distinguish actual required gates from allowed unknown/not-established results. Original source registrations and all sampling budgets remain unchanged.

The [platform/PR addendum manifest](round4-reviewed-progress/platform-pr-addendum-manifest.json) preserves the completed [platform, recovery, rollback and failure-gate acceptance mapping](round4-reviewed-progress/review-platform/independent-acceptance.json), the [exact seven-scenario CLI replay](round4-reviewed-progress/review-platform/gates-retained-cli/replay-report-v2.json), and the conditional PR #160 adoption review. These additions do not change task counts.

## Original artifacts and deterministic replay

`originals/gates-11592180029.zip` is the exact original GitHub Actions artifact: **32,985,345 bytes**, SHA-256 `792a4fa9cfdcc3d9f1bf077a20e3d951958c144142fac1e81183aa132c4dbd25`. Its original API metadata, transport receipt and ZIP member inventory are adjacent. The retained CLI is unchanged; the successful independent replay used a fixed official Ubuntu amd64 image, no network during execution, read-only originals and a separate writable fixture copy for derived reports. Earlier loader/read-only-environment probes are retained separately and are not product measurements.

Other original ZIPs remain identified by their official artifact IDs, names, sizes, hashes and observed source in [the artifact locator manifest](artifact-locators.json). They are also retained in the connected native project's isolated `artifacts/benchmarks/p8-bfccb8494ba0/raw/` directories; small scale shard ZIPs and capacity originals are included in this checkpoint. This progress checkpoint does not claim to contain every large original artifact or a complete scale study.

## PR and queue management

PR #167 is the frozen measured candidate tracked by this checkpoint. PR #168 is a separate later source under its own review and execution; none of its results are credited here. Exact scope inventories for #160 and #161 are retained under `round4-reviewed-progress/review-pr/`; matching engineering files do not imply matching historical evidence or new a23 measurement credit. Their old branches and original records are retained pending final disposition.

The two already-invalid historical scale studies G (`37830173594`) and D0 (`37854240827`), plus the old e95 runtime (`37859755918`), were administratively stopped to release queue capacity. This was a resource-management decision outside their measurement protocols. Original failures and final aggregate rejections remain failures; active interrupted work is cancelled, queued work was not run, and no primary was replaced or retried. Before/after API captures and original aggregate logs are retained in this checkpoint.

## Completion and rollback

The canonical tasks file will change only after original acceptance and hard dependencies have closed and the resulting PR is reviewable. Generated views will be regenerated with the existing plan command. Earlier source measurements, failed studies, unknown memory/causal/tail metrics and unapproved live-provider or LLM work are never promoted by this checkpoint. No live-provider, G8 or release certification is claimed.

Rollback preserves the last verified binary/configuration and every original artifact. If the candidate must be reverted, revert its product changes and matching source-review installation together; this evidence archive remains historical evidence.
