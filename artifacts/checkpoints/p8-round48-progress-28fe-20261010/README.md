# P8 Round 48 — PR #200 merge and live 100k diagnostic prefix

Frozen observation: 2026-10-10T02:57:30Z

## Decision

PR #200 was merged externally as main `b9b089bb4eae072affe9326681d4980eae15fd84` (tree `6461938788665701cfa4fa095a064a3a404ec492`, parents `30a6dff…` and head `9cc6bf49…`) at 02:24:46 UTC. This round did not merge it. Fixed-head ordinary admission was 7 workflows / 26 successful jobs, but merge and CI receive zero original-TODO credit.

The authoritative tasks blob remains `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`: 192 total / 164 done / 15 in_progress / 12 todo / 1 blocked, 28 unfinished. Round 48 newly completed original TODOs: none. Active-request cumulative completion remains 1/10: `P8-005` only.

## Diagnostic custody and new narrow fact

Run [38012409915](https://github.com/jyqj/codecortex/actions/runs/38012409915), source `9cc6bf49…`, attempt 1, remains in progress. Checkpoints 00–05 were independently downloaded, SHA-256 checked and CRC-tested; they form continuous prefix `[0,13992488)`. Artifact 05 is `11656794128`, SHA-256 `2f6830c…599a`. Its manifest has no capture fault, but `supervisor_terminal_observed=false` and `native_EOF_claimed=false`; the unacknowledged tail is unknown, not zero.

The exact diagnostic is `scale_wide_dirty_v1`, dirty4096/resume1024, seed12648430, five-hour native deadline, single 100k rep0, `skip_fanout=true`, and `whole_cohort_completion_credit=false`.

The prefix supplies one useful but narrow diagnosis:

- 100k no-op passed exact 15-table parity. Incremental was 2.631434s, full control 615.685136s, parity 1623.147962s: about 2241.464532s (37m21s) combined.
- 100k body required 24 incremental builds. Indices 0–22 remained budget-exceeded; index23 consumed the remaining3791 dependents and returned freshness to ready. Incremental/resume wall totaled517.857701s (8m38s).
- Body full-control started at native monotonic4867.461321s and remained unfinished at checkpoint05.
- Largest observed single-process RSS snapshot was10463698944 bytes; this is not a peak/process-tree/OOM proof.

This establishes substantial cumulative stage pressure from full-control/parity and resume chains for G9cc6/dirty4096. It does not establish the unique cause of historical dirty200 failure or this live run's terminal outcome, and cannot be pooled/relabelled across sources/profiles.

## Acceptance and PR management

`P8-006` remains in_progress: two prior studies remain terminal 100k/aggregate failures with measure skipped, while this diagnostic has no EOF/exit/aggregate, explicitly skips fanout, and is not a cohort. Downstream `P8-007`–`P8-013` and `P8-016` remain dependency-open.

Separate full-wide run [38013753078](https://github.com/jyqj/codecortex/actions/runs/38013753078) has successful build and 1k/5k/10k/50k preflights; 100k preflight remains in progress, with no aggregate or TODO credit. This round did not dispatch/rerun/cancel it.

No open PR is presently safe to merge or close: #188, #191 and #192 remain fixed-history Draft/HOLD; #3/#4/#65/#127/#137 retain unadopted intent.

This round did not change tasks, thresholds, budgets, identities, historical failures or subgates, and did not run provider/holdout/local native GC-WAL work. The archival evidence branch retains an older product tree and must never be merged wholesale into main.
