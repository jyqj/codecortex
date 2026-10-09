# P8 Round 45 evidence

Observed at 2026-10-09 23:50 UTC.

## Outcome

Current main is `09d4454fa45455dc5a8bfdfec67e31bb7ed162c7` (PR #198 merge). The authoritative `tasks.json` blob remains `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`.

Ledger: **192 total / 164 done / 15 in progress / 12 todo / 1 blocked = 28 remaining**.

New original TODOs completed in this round: **none**. Since the 163-done baseline, only **P8-005** is accepted: **1/10**, with **9** still required.

## PR management

- PR #195 merged as `a406a4968c46ab005225666e60e81acddaa77de8` after PR #197. Its seven exact-head workflows and 26 jobs passed; its engineering admission is accepted. It adds no original TODO completion credit.
- PR #198 merged as current main and modifies only three documentation paths. Its exact-tree workflows passed; it adds no original TODO credit.
- PR #193 is an ancestor absorbed by #195, whose corrections removed the shortcut/public-witness performance intent. No separate performance claim or TODO credit is accepted.
- PR #194 is closed unmerged. Its still-running wide-dirty study retains its original source/profile identity.
- PR #188 remains Draft/HOLD because performance counterevidence is unresolved.
- PR #190 remains Draft/HOLD: fresh head `f6750cad0df45af4ec2dddc456e76cc188f3a35a`, closeout failure and runtime nonterminal.
- PR #191 and #192 remain Draft/HOLD; no extra study was dispatched.

## Scale acceptance boundary

Original dirty200 run `37962416564` / source `4fe927488d5cb7f26bd27c7624745fb1b6ca202b` / profile `scale_capacity_v1` remains a terminal failure: 100k failed, aggregate failed, measure skipped, only 4/150 shards and 41/1500 composites accepted.

Separate dirty4096 run `37982220020` / source `008f1ec0ae16b9b6aff9f20d033b17b2219168f8` / profile `scale_wide_dirty_v1` remains in its 100k step. It cannot be pooled with or relabeled as the original dirty200 population.

Therefore P8-006 remains in progress. P8-007 and the downstream P8 dependency chain cannot be closed.

## Independent reviews

- Ledger and PR audit: blob `eaad70b58e11606c2c71924ee1a0748c91567561`
- PR #195 admission audit: blob `fd90ab839efa733991ed5a8b4dd12cc1a3f51458`
- Scale terminal audit: blob `b9832beaa5c04adb712d065a70d6298b3f3e0137`

No workflow was dispatched, retried, or cancelled. No sample was pooled, relabeled, or deleted. No task definition or ledger field was changed.
