# P8 Round 46 evidence

Observed at 2026-10-10 00:40 UTC.

## Outcome

Current main remains `09d4454fa45455dc5a8bfdfec67e31bb7ed162c7`; the authoritative task blob remains `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`.

Ledger: **192 total / 164 done / 15 in progress / 12 todo / 1 blocked = 28 remaining**.

New original TODOs completed in this round: **none**. Since the 163-done baseline, only **P8-005** is accepted: **1/10**, with **9** still required.

## New terminal scale fact

The existing dirty4096 run `37982220020` / source `008f1ec0ae16b9b6aff9f20d033b17b2219168f8` / profile `scale_wide_dirty_v1` is now terminally failed:

- build and 1k/5k/10k/50k rep0 succeeded;
- 100k job `114000097090` completed/failure;
- no successful 100k shard exists;
- aggregate job `114077867196` completed/failure;
- measure job `114077867756` was skipped;
- only four of 150 shards exist; the aggregate reports 146 missing cells.

The original failure-matrix artifact `11651413055` was downloaded once. Its 580-byte ZIP SHA-256 is `1a442892700c45cebaaa4108fa04c5d84a533f34b8686530861672abb565a4e4`, matching the official digest. ZIP CRC passed. The payload is `status=failed`, `passed=false`, `release_certification=not_run`.

The original dirty200 run `37962416564` retains its separate failure identity: 100k failed, aggregate failed, measure skipped, 4/150 shards and 41/1500 composites. The two sources/profiles/budgets cannot be pooled or relabelled. P8-006 therefore remains open, as do all downstream hard dependencies.

## PR management

- New PR #199 is Ready and based on exact current main. Its head is `4786c1457b47c5baf4c09d32b96048795e863111`. At the frozen snapshot, 24/26 jobs succeeded while main check and one-hour soak remained in progress. Decision: keep open; do not merge until both finish successfully and head/base remain unchanged. Zero original-TODO credit.
- PR #191 head `1ebb6208f2f5f8fe8a5bf237c00a3d963f44bb08` has a complete 80-pair equal-lifetime observation, including 17 slower pairs and all ten single_plain retained-first pairs slower. It remains Draft/HOLD and grants zero TODO credit.
- PR #190 remains Draft/HOLD. Only after #199 is accepted and merged may #190 be considered safely superseded.
- PR #192 remains Draft/HOLD; its 1350-slot study remains untriggered and was not dispatched.
- PR #188 remains Draft/HOLD because its frozen performance counterevidence is unresolved.

No PR was merged or closed in this round.

## Independent reviews

- Ledger/dependency audit: blob `d3ec0c6913643093d3bdaab2e4f2864bbe02d9e2`
- Scale terminal audit: blob `bd6325b2a3468daeea8d80ff333307430d612f01`
- PR admission audit: blob `1cdbac3c920379d9588cc744c5d1d4e94563da15`

No workflow was dispatched, retried, or cancelled. No task definition, threshold, sample identity, or budget was changed.
