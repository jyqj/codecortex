# P8 Round 49 progress checkpoint

Frozen at: 2026-10-10T03:34:14.158Z

This is an archival evidence checkpoint only. Its branch retains historical product trees and **must not be merged wholesale into main**.

## Authoritative ledger

- main: `b9b089bb4eae072affe9326681d4980eae15fd84`
- main tree: `6461938788665701cfa4fa095a064a3a404ec492`
- tasks.json blob: `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`
- ledger: 192 total / 164 done / 15 in_progress / 12 todo / 1 blocked
- remaining: 28
- newly completed original TODOs in this round: none
- cumulative completion relative to the 163-done baseline: 1/10 (`P8-005`)
- next task: `P8-006`

PR merges, CI results, tests, fixes, documentation, artifacts, and acceptance subgates are not counted as original TODO completion.

## New diagnostic evidence

Run [38012409915](https://github.com/jyqj/codecortex/actions/runs/38012409915), attempt 1, source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b`, remains in progress. Build job `114095124901` succeeded; diagnose job `114096992369` remained in progress at interval 9/20.

Newly received checkpoints:

| checkpoint | artifact | ZIP SHA-256 | CRC | raw range |
|---|---:|---|---|---|
| 06 | 11657318324 | e87aa1a25aeb8d02f818bd1c450be2c0fde3008c9088b0643f40037a4f6e08ae | pass | [13,992,488, 14,674,260) |
| 07 | 11658151472 | 428368d66bc8850fed73486e80a1c61d3c097564877ff4e542762330424e02f3 | pass | [14,674,260, 14,714,549) |

Together with checkpoints 00–05, the retained native raw prefix is continuous over `[0, 14,714,549)`; `capture_faults=[]`. The manifest still says `native_EOF_claimed=false` and `supervisor_terminal_observed=false`, so the tail after the latest successful upload is unknown, not zero.

New phase facts:

- body/full_control: 922.132074 s
- body incremental: 24 builds, 517.867898 s cumulative
- body parity: 1104.713606 s; all 15 tables equal
- body stage: `complete=true`, `full_complete=true`
- body scratch peak: 5,984,497,664 bytes
- independent config fact: passed
- API incremental: 9 builds, 299.005102 s cumulative
- API builds 1–8: budget_exceeded/incomplete; completed_files advanced 4,097 → 32,769
- API build 9: normal/ready
- maximum discrete single-PID RSS snapshot: 9,110,126,592 bytes (not a continuous/process-tree peak)
- API/full_control started at native monotonic 7193.331205 s, but completion was not observed

Missing: supervisor terminal, native/driver exit code, native EOF, terminal `shard.json`, `measurement_complete`, and observer-after identity closure. This one 100k rep0 diagnostic cannot replace the complete 150-shard/1500-composite cohort or successful aggregate and cannot close `P8-006`.

## Full-wide acceptance

Run [38013753078](https://github.com/jyqj/codecortex/actions/runs/38013753078), source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b`, profile `scale_wide_dirty_v1`, remains in progress.

- build succeeded
- 1k/5k/10k/50k preflight shards succeeded
- 100k preflight job `114100438875` remains in step 7; no successful 100k shard artifact
- only 4/150 successful shards
- no 145 measurement jobs
- no aggregate job or matrix artifact
- 1500 composite observations remain incomplete

The current run, old dirty4096 run `37982220020`, and original dirty200 run `37962416564` retain distinct source/profile identities. Nothing was pooled or relabeled. `P8-006` remains `in_progress`.

## PR management

Eight PRs remain open: #192, #191, #188, #137, #127, #65, #4, #3. There is no safe immediate merge or close candidate. Main, every open PR head/state, tasks blob, and Issue #158 coordination state are unchanged since Round 48. No merge, close, rerun, dispatch, or cancellation was performed.

## Independent reviews

- diagnostic terminal review blob: `d0384e8db260ca2a019e817f1ae6b63b7703ba6b`
- scale acceptance review blob: `14088d80c495411e40e6ad1d1b23c9d557ebec5c`
- PR/ledger review blob: `3e03e8633233bdacb4efa5d00c23474a8519b4f5`
