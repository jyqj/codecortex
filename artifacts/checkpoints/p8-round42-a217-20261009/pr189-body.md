## Current status — 2026-10-09

The current source is `10ec828beb94200ec679302c45115760ebd55ee9`, a formatter-only successor of the registered cold-study source. This remains a Draft against the fixed review base `fffd950d5b34b0180f308db1428a57d5bf358bf1`.

The original cold study is [run 37954851017, attempt 1](https://github.com/jyqj/codecortex/actions/runs/37954851017), bound to **`044c008c9459cfa61e7701db2eb868342c508a39`**. Its source, executable, driver, run and attempt are unchanged. The run was triggered once; the later formatter repair did not trigger another study.

As of the original intake completed at 18:17:43 UTC, **16/150 complete shards and 16/150 samples** passed the unchanged original build/shard validators:

| Scale | Accepted repetitions |
| --- | --- |
| 1k | 0 |
| 5k | 0 |
| 10k | 0 |
| 50k | 0 |
| 100k | 0–11 |

The registered population remains five scales × N30. The first five preflights passed, allowing the workflow's original remaining 145 shards to expand automatically. Unfinished slots and capacity receipts do not count as samples. There is no partial aggregate or formal TODO completion.

## Protocol and measurement meaning

Each complete sample performs two fresh A/B full builds and all fifteen canonical-table comparisons, retaining the original configuration witnesses, exact counts, raw reports and resource observations. The five-hour worker deadline, 512 MiB captured-output budget, 350-minute job limit and max-parallel 10 remain unchanged.

The OS page cache is not reset. Resource observations are snapshots, not measured continuous peaks. Separate sources, attempts and studies are not pooled; no speed or stable-tail claim is derived from this partial population. The earlier full-stage failed studies remain failed/incomplete.

## Actual verification

[Current normal CI check 113916979105](https://github.com/jyqj/codecortex/actions/runs/37959075859/job/113916979105) passed formatting, Clippy, all-target compilation, the authorized default/semantic regressions and the original source/task guards. Both new cold controls actually passed in the default and semantic executions: explicit cold-stage identity with unchanged old-plan wire, and both fresh builds with all fifteen tables.

The formatter successor's original local v15 and task-plan checks also passed once, with inputs and HEAD/index preserved. Passing normal CI on `10ec` does not change the original cold study's `044c` source identity.

## Original evidence and remaining work

The [original ZIP custody branch](https://github.com/jyqj/codecortex/tree/evidence/p8-originals-cold044c-a217-20261009) preserves complete originals, official metadata, restoration manifests and original intake receipts. Its current published commit `9aba8f01e5f5766ac55f3ebed1ff21991dcb6cdb` contains the first 26 original ZIPs and receipts through 10 accepted samples. Another 15 original ZIPs were received and byte-verified locally before the workspace disconnected; their unchanged original intake receipts bring the accepted total to 16. The append-only custody publication for these originals is pending.

The workspace service now returns `409 environment_offline / Environment is not connected`. GitHub Actions continues to execute the original study, but newly appearing artifacts have not yet passed local original intake and are not added to the accepted count. No measurement, aggregate, original validator or source guard has been retried to work around this interruption.

P8-005 can close independently once this original cold population reaches 150/150 with the original aggregate and its declared regression scope satisfied; it does not depend on finishing the separate #193 full population or starting the untriggered #192 profile population. It remains incomplete at the current 16/150.

The full original protocol on PR #193 is a separate source/run and is reviewed separately. This PR remains reviewable while the registered matrices finish.

Original task ledger: **192 total / 163 done / 29 remaining / 0 newly completed**. Task definitions, dependencies, N and budgets are unchanged.
