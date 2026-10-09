# P8 Round 40 progress checkpoint

Frozen at 2026-10-09T20:17:03Z.

## Outcome

- Authoritative main: `23eab8a7ea1fdfd6333c2e5a001cfb41e2a10d69`
- Authoritative tasks blob: `959a25ba851ff88f286bab0fa14167878129b1dd`
- Ledger: 192 total / 163 done / 16 in_progress / 12 todo / 1 blocked
- Original TODOs newly completed this round: none
- Cumulative after the 163-done baseline: 0/10
- Remaining original TODOs: 29

## Independent reviews

Three independent work lines fresh-read GitHub and converged on the same decision:

1. Ledger/dependency review: none of P8-005..P8-013 or P8-016 may move to done. P8-005 still has open full-scale, paired-causal, and whole-tree memory gates; its hard dependency chain blocks the remaining nine.
2. PR admission review: PR #195 must remain Draft/HOLD. It inherits the PR #193 oracle behavior that skips B spooling under the equal-input witness, exposes `equal_input_order_witness` in public JSON, and still lacks the exact A-fits/A+B-hits-SQLITE_FULL control. PR #193 remains 25 green / 1 failed due the warm-index p95 gate.
3. Scale acceptance review: original G8/dirty200 run 37962416564 remains at 4/150 shards and 41/1500 composite observations. Its 100k preflight is still in progress; there is no 100k shard, no remaining 145 shards, and no aggregate.

## Separate studies and ownership

PR #194 advanced to `008f1ec0ae16b9b6aff9f20d033b17b2219168f8`. Its run 37982220020 is a dirty4096 population and must remain separate from the original dirty200 population; it supplies zero original-TODO credit. PR #188 was moved to Draft by its owner. Issue #158 records another workspace owning P8-018 Claude hooks; this round did not overlap that work.

No workflow was dispatched, retried, cancelled, or relabeled. No task definition or status was changed, and no acceptance threshold was weakened.
