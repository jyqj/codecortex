# P8 a23 original acceptance checkpoint

This checkpoint tracks ten **original** tasks: P8-005 through P8-013, plus P8-016. At this progress snapshot the authoritative ledger remains **192 total, 163 done, 29 remaining; zero newly completed tasks in this session**. All original task definitions, dependencies, workload registrations, failure records and release boundaries remain in force.

## Fixed identities

| Role | Exact identity |
| --- | --- |
| Main at the original integration inspection | `4775bf44dc09c11cea88211e06ccec97c4de44aa` |
| Product composition | `254009277688d64677361a0ca33e5dea73f295ef` |
| Independent source review | `3a11f30f9f00a89fe3cd481b3b7609066728baad` |
| P8 actual measurement source | `a23bb72d3c954f385b99fe81ce9189885c208557` |
| Actual CI / security / MSRV / P7-engineering checkout | `75649f8cb08dcd5427e7e58b1ca2b8fa67c9d037` |
| Identical full Git tree for that CI checkout and a23 | `58147c952505c44da1f41eb4b9c31643f2303b96` |

The CI merge checkout is recorded as actually observed. Official Git commit metadata proves every tracked path, mode and blob is identical to a23. P7 closeout and the P8 measurements keep their own actual checkout identities. See the [CI identity proof](round4-reviewed-progress/review-ci/actual-ci-p7-checkout-tree-equivalence.json) and [original CI audit v2](round4-reviewed-progress/review-ci/ci-original-check-audit-v2.json). V1 is retained and superseded only for its incomplete checkout labeling.

## Actual progress; not task completion

| Original acceptance area | Verified evidence at this snapshot | Still needed for original task closure |
| --- | --- | --- |
| P8-005 / 006 scale and fanout | Original 1k, 5k, 10k and 50k index-0 shards: 4/150 shards, 41/1500 samples independently accepted, including exact original Linux validate_build/validate_shard replay; five original capacity receipts | Original 100k preflight, remaining 145 shards, strict complete aggregate and hard dependency closure |
| P8-007 mixed load / backfill | Four original C1/4/8/16 jobs, 900/900 operations each; actual peaks 1/4/7/12; 768 fake-provider backfill requests; complete raw/stdio/statistics/parity review | P8-005/006 dependency closure |
| P8-008 / 009 lifecycle / resources | 1230 original samples, 1261 resource snapshots, physical/logical storage attribution and null/unknown cost boundaries independently checked | Original prior-task dependency closure |
| P8-010 soak | Original job succeeded at 03:49:15 UTC on 2026-10-09; 3,601/3,601 operations over 3,600,053,467,347 ns, 3,594 resource samples, four actual cache windows and complete unrepaired 15-table terminal parity independently accepted; configured concurrency 4, actual peak 1 | Original prior-task dependency closure |
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

PR #167 is the frozen measured candidate tracked by this checkpoint. PR #168 (G1) and its separate successor #169 (G2) use their own frozen sources; none of their results are credited here. PR #161 later advanced to 34aa and includes new product changes; the earlier e95 adoption inventory is not reused for that new head. Exact scope inventories for #160 and #161 are retained under `round4-reviewed-progress/review-pr/`; matching engineering files do not imply matching historical evidence or new a23 measurement credit. Their old branches and original records are retained pending final disposition.

The two already-invalid historical scale studies G (`37830173594`) and D0 (`37854240827`), plus the old e95 runtime (`37859755918`), were administratively stopped to release queue capacity. This was a resource-management decision outside their measurement protocols. Original failures and final aggregate rejections remain failures; active interrupted work is cancelled, queued work was not run, and no primary was replaced or retried. Before/after API captures and original aggregate logs are retained in this checkpoint.

## Round 4 completion and Round 5 start

Round 4 newly completed **zero original TODOs**: the canonical ledger remains **163 done / 29 remaining**. The original 50k index-zero shard succeeded with native wall time 4,214,047 ms. Its nine original samples and all four completed shards passed unchanged original Linux validation. The final original 100k preflight, remaining 145 shards and complete aggregate are still required; the original one-hour soak also awaits its terminal evidence.

The [Round 4 completion manifest](round4-completion-reviewed/copied-audit-manifest.json) preserves the exact new inputs. The [Linux original-function replay summary](round4-completion-reviewed/review-scale/linux-native-replay-summary.json) includes fixed official Python image manifest/config provenance, network-disabled execution, read-only source/original mounts, original native BLAKE3 calls, before/after byte checks, and both earlier environment-observation errors. These replays create no new primary measurements. The [original 50k ZIP](round4-completion-reviewed/raw/11593548201/original.zip) is 2,964,759 bytes with SHA-256 d0163f3421ea9abf3d544edad56a9386aec2db55fef0aa11155e30e10a27b06d.

The [current PR management record](round4-completion-reviewed/review-pr/round4-pr-management-observation.json) captures 35 open PRs and the exact heads observed in this round. The [30 historical PR inventory](round4-completion-reviewed/review-pr/historical-pr-main-adoption-inventory-v1.json) compares original PR deltas against main by exact mode/blob. None has all non-archive paths identical at main; this inventory is not automatic semantic closure approval, and their branches remain retained. The [new PR161 delta](round4-completion-reviewed/review-runtime/pr161-e95-to34aa-delta-review.json) separates its 859 archival, eight crates and two scripts changes from the earlier e95 source.

The [PR168/169 coordination review](round4-completion-reviewed/review-pr/pr168-readonly-coordination-review.json) preserves G1's actual E0282 failure, G1 Python50's bounded success, P2's one-line type fix and G2's exact source identity. It identifies three a23 validation protections absent from G1/P2/G2 and links their exact blobs and original controls. Follow-up comments are recorded in [#168](https://github.com/jyqj/codecortex/pull/168#issuecomment-6073667411) and [#169](https://github.com/jyqj/codecortex/pull/169#issuecomment-6073711634); the existing frozen heads and original runs were not changed.

Round 5 continues the original complete scale study while preparing a minimal, separately reviewed candidate that retains a23 validation protections and incorporates bounded snapshot batching product changes. Any new candidate keeps an independent source chain and measurement identity. The [P8-007 through P8-010 evidence append draft](round4-completion-reviewed/review-runtime/p8-007-010-evidence-append-draft.json) remains the unchanged Round 4 draft with then-missing terminal fields; it is not a task state change.

## Round 5 original soak terminal evidence

The original a23 soak job [113631481157](https://github.com/jyqj/codecortex/actions/runs/37871838957/job/113631481157) succeeded. Its exact artifact `11594089439` is 51,069,087 bytes, SHA-256 `d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086`, with 2,129 members and 450,984,795 extracted bytes. All 3,601 offered operations succeeded: 1,201 builds and 2,400 compound reads. The observed work duration is 3,600,053,467,347 ns; the workflow step also includes preparation and sealing, so its UTC boundaries are not presented as exact measurement boundaries.

Independent review verified 200 branch switches, 25 catalog compactions, 15-table terminal equality without prior repair, and complete owned-process/sampler termination. All 8,395 status RPCs are bound to the 4,800 read probes and 3,594 resource snapshots plus endpoint status. Actual cache hit/miss/invalidation counts are 1,400/1,000/999, and all three event classes occur in each of four actual completion-time quarters. Configured concurrency was 4; observed peak was 1 with no read/build overlap. No saturated concurrent-tail conclusion is inferred.

The warmed RSS was 115,081,216 bytes; tail median 144,728,064 stayed below the original 177,405,952-byte bound. Original native statistics replayed identically twice. The [terminal review](round5-soak-terminal/review-runtime/round5-p8-007-010-terminal-review.md), [full evidence mapping](round5-soak-terminal/review-runtime/round5-p8-007-010-terminal-evidence.json), [whole-artifact review](round5-soak-terminal/review-runtime/runtime-11594089439-review-v2.json), and [stdio binding review](round5-soak-terminal/review-runtime/soak-11594089439-stdio-binding-review.json) preserve these scoped conclusions and their limits.

The [copied manifest](round5-soak-terminal/copied-audit-manifest.json) contains 50 exact selected files totaling 7,629,003 bytes, including the original selection manifest. The [raw-stream archive explanation](round5-soak-terminal/review-runtime/soak-11594089439-archive-README.md) identifies the complete original streams and lossless per-line offset/length/hash indices. The 70,117,809-byte raw JSONL and 322,493,166-byte product stdio remain complete in the official original artifact and the retained native originals; indices are locators, not replacement raw data or a self-contained replay claim.

All seven a23 runtime/lifecycle components now have scoped independent acceptance. The complete original scale aggregate and hard dependencies remain pending. This terminal evidence does not change the canonical **163 done / 29 remaining**, and the session still has **zero newly completed original TODOs**.

## Round 6 source admission and engineering follow-up

PR [#167](https://github.com/jyqj/codecortex/pull/167) was merged by `jyqj` at 2026-10-09 04:16:47 UTC into actual main `b21cce4c8661589267ad5719f850accbec088d2f`. This session did not perform that merge. The [independent merge equivalence review](round6-reviewed-engineering/review-pr/main-M-a23-equivalence/main-M-a23-equivalence-review.json) verifies all 1,087 product inputs, all 138 validation inputs and the canonical task blob are exactly a23. Its 188 other changed paths are artifacts. Original measurements remain attributed to their actual a23 checkout. PR #168 was independently closed without merging at 04:16:02 UTC; its G1 failure is retained. PR #169 remains a draft at frozen G2 and is now conflicting; the [integration comment](https://github.com/jyqj/codecortex/pull/169#issuecomment-6074381694) and needs-rebase label record that follow-up.

The literal `cargo test --workspace` command was not present in the original split CI logs, so it was run separately against unchanged a23 on the connected Mac. The [original command audit](round6-reviewed-engineering/review-pr/a23-engineering-premerge-command-audit.json) preserves the distinction. Default SDK 27 failed at the linker before tests. A separate Rust 1.95.0 / SDK 15.4 attempt compiled and then failed `subprocess_descendant_cannot_hold_stderr_past_worker_deadline`; its p8_scale binary reported 11 passed and 1 failed. The same original binary failed once in isolation, and a persistent external observer calling the original supervisor reproduced deadline_exceeded with empty stderr and no worker summary. The exact workspace gate has therefore **not passed**, and doctests were not reached.

The [failure assessment](round6-reviewed-engineering/review-ci/premerge-workspace/failure-assessment.json) retains all original failures, command/source/ABI identities, full captured streams and process-group cleanup evidence. Original Linux CI ran the same test successfully under observed stable Rust 1.99.0; that different platform/toolchain result does not erase the Mac failures. At this archive snapshot the cause is still under investigation; no deadline, test assertion or formal scale parameter was weakened. The [public follow-up](https://github.com/jyqj/codecortex/pull/167#issuecomment-6074384941) also records that the external merge preceded the newly observed failure.

A separate snapshot-batching candidate now has the fixed chain P `305bf145e2474000d7c7d806507117d65a4863ad` → R `252e28744aa55a4b14414787f74e00783f87712f` → G `5cae24fbdf456dcf06932d9ed83a4c600d538136`. Its [independent source review](https://github.com/jyqj/codecortex/blob/252e28744aa55a4b14414787f74e00783f87712f/artifacts/checkpoints/p8-a23-snapshot-bfcc-20261009/independent-source-review.json) retains all 138 a23 validation inputs, including the three protections absent from G1/G2. The original v15 source verifier completed with actual integer exit 0 and complete stdout/stderr. Its actual run was R plus two uncommitted binding files. The [post-G binding review](round6-reviewed-engineering/review-next-candidate/post-g-binding/post-g-source-binding-review.json) proves those exact two verified files became G, every other tree input stayed at R, and the official remote ref matches. It does not relabel that earlier execution as a direct G checkout.

The [first local wrapper timeout](round6-reviewed-engineering/review-next-candidate/original-v15-cli/outer-timeout-record.json) remains retained with unknown original child integer exit; the successful [second execution receipt](round6-reviewed-engineering/review-next-candidate/original-v15-cli-attempt02/result.json) is separate. The larger local source-verification wrapper window did not change any measurement protocol deadline. No G workload or full-scale acceptance is claimed.

The [Round 6 copied manifest](round6-reviewed-engineering/copied-audit-manifest.json) binds 81 exact selected files totaling 2,730,014 bytes. The [ten-original-task closeout map](round6-reviewed-engineering/review-next-candidate/original-task-closeout-map.json) retains the original definitions, dependencies and still-pending closure conditions. The original a23 full scale study continues unchanged. Counts remain **192 total, 163 done, 29 remaining; zero newly completed original TODOs**.

## Completion and rollback

The canonical tasks file will change only after original acceptance and hard dependencies have closed and the resulting PR is reviewable. Generated views will be regenerated with the existing plan command. Earlier source measurements, failed studies, unknown memory/causal/tail metrics and unapproved live-provider or LLM work are never promoted by this checkpoint. No live-provider, G8 or release certification is claimed.

Rollback preserves the last verified binary/configuration and every original artifact. If the candidate must be reverted, revert its product changes and matching source-review installation together; this evidence archive remains historical evidence.
