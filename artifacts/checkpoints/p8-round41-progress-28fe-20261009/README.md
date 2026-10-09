# P8 Round 41 progress checkpoint

Frozen at 2026-10-09T20:52:35Z.

## Outcome

- Authoritative main: `23eab8a7ea1fdfd6333c2e5a001cfb41e2a10d69`
- Authoritative tasks blob: `959a25ba851ff88f286bab0fa14167878129b1dd`
- Ledger: 192 total / 163 done / 16 in_progress / 12 todo / 1 blocked
- Original TODOs newly completed this round: none
- Cumulative after the 163-done baseline: 0/10
- Remaining original TODOs: 29

## Material changes and decisions

1. PR #195 was moved from Draft to Ready by another collaborator after the Round 40 HOLD. Fresh review found the same exact source/test blobs and the same three blockers: skipped B spooling, no exact A-fits/A+B-SQLITE_FULL control, and public `equal_input_order_witness`. Root restored the PR to Draft at 2026-10-09T20:50:34Z. It remains HOLD/DO NOT MERGE even if its final two checks turn green.
2. New PR #196 at `065f5ab5da1bc5de74cbd3e4c360b51e09200687` contributes four scoped fixes: exact Claude hook ownership, generic JSON uninstall preservation, create-if-absent first archive pointer publication, and sparse unchanged action materialization. It remains Draft; its exact-head matrix is not terminal. The work is a scoped contribution to P8-018/P8-019, not full acceptance, and those tasks remain hard-dependency blocked.
3. Original G8/dirty200 run 37962416564 remains at 4/150 shards and 41/1500 composite observations. The 100k preflight is still in progress and the later 145 jobs have not materialized.
4. PR #194's dirty4096 population remains separate. It has build plus 1k/5k/10k successes while 50k/100k are in progress; it supplies zero original-TODO credit.

No workflow was dispatched, retried, cancelled, or relabeled. No task status or acceptance threshold changed. The only PR state mutation was the reversible restoration of PR #195 to Draft to enforce the already-published admission boundary.
