# P8 round 38 progress checkpoint

Snapshot: 2026-10-09T17:53:07Z

This checkpoint is append-only evidence on the existing evidence branch. It does not change the product tree, the benchmark protocol, the task ledger, or any workflow. The evidence branch contains historical product trees and must not be merged wholesale into main.

## Verified repository state

- main commit: `23eab8a7ea1fdfd6333c2e5a001cfb41e2a10d69`
- main tree: `e039a817291934b15cc018fd74d2c83982bb7f37`
- tasks blob: `959a25ba851ff88f286bab0fa14167878129b1dd`
- original ledger: 192 total / 163 done / 16 in_progress / 12 todo / 1 blocked
- original TODOs remaining: **29**
- original TODOs newly completed in this round: **none**
- cumulative original TODOs completed after the 163 baseline: **0 / 10 required**

Main push CI was read from the official check-runs endpoint at the snapshot time: 18 total, 16 successful and two queued, with no failure. The only nonterminal checks were the existing stable Linux cold-build jobs. They were not cancelled or rerun.

## Multi-agent work completed

Three independent workstreams were used.

1. PR and ledger audit: verified main, PRs 178/180/181/184/188, issue 158, the task blob and current CI; identified new PRs 189–193 and separated studies, partial mechanisms and current-main candidates.
2. Scope-less resume-scan review: traced all 991 scans through the public MCP build boundary and established why an old manifest, empty scope, root mtime or the current watcher cannot safely certify no intervening filesystem change. The review defines a future watermark-backed MutationJournal design and negative-control matrix.
3. Exact-oracle/PR193 review: found no static false-equal path in the strict typed-cell witness, but found two admission-contract deltas and insufficient representative performance evidence.

The full subreviews are stored under `review/` in this checkpoint.

## PR decisions

- PR 188 stays open and ready but HOLD: its exact head is green, while paired mechanism diagnostics remain slower (dense 1.468x, sparse 93.55x, multi-batch 11.19x). No merge or close.
- PR 193 stays Draft/HOLD at `4fe927488d5cb7f26bd27c7624745fb1b6ca202b`. Its witness is strict when it succeeds, but:
  - not inserting scratch side B changes the A-fits/A+B-`SQLITE_FULL` outcome relative to the frozen two-spool resource-failure behavior;
  - it adds `equal_input_order_witness` to each public table result;
  - diagnostics are mixed (same-order 0.90763, reverse-order 1.36832, tail mismatch 1.22545);
  - the fixed scale workload's real witness hit rate is unproven;
  - it does not remove parity calls or the 991 scope-less scans.
- PRs 189–192 are not original-TODO completions. PR192 is a proposed new 1,350-cell study and was not dispatched.
- An independent HOLD review was posted to PR193: https://github.com/jyqj/codecortex/pull/193#issuecomment-6086310660

## Product decision

No safe small patch exists under the current scope-less manual-build contract. Each resume call is a new filesystem observation boundary, while durable freshness debt proves only previously observed resolution work. Skipping the walk without a complete event watermark would miss undeclared source edits, same-size/restored-mtime changes, create/delete/rename, nested ignore, dot-path, config and infra changes.

The minimum credible future product direction is a per-project MutationJournal independent of auto-indexing, with a platform event watermark, identity-bound token, conservative fallback, and acknowledgement only after stage-3 publication. That is a multi-module feature, not a round-local cache patch.

No benchmark budget, stage, B=200 window, resume=1024 bound, oracle table, evidence history or task definition was changed. No new 150-shard study was started.

## Next admissible work

- Receive the two existing main cold jobs and PR193 ordinary CI without rerun.
- Require PR193 either to preserve the old resource/output contract or explicitly version and obtain acceptance for each delta.
- Require representative fixed-stage hit-rate and wall-time evidence before treating the witness as a root-cause improvement.
- Design and independently review the MutationJournal/watermark contract before implementation.
- Keep all ten priority original TODOs in progress until the fixed 150-shard acceptance and hard dependencies genuinely pass.
