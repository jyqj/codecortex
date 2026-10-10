# P8 Round 53 — terminal task-profile reception and artifact-cap fix

Frozen at 2026-10-10T07:45:36.396Z UTC.

## Authoritative state

- main: `0da9a2800d2b3112d584a8bad4f79785e01162a9`
- main tree: `39a053ae49b486764751a570d164e7cd41d7ff29`
- tasks blob: `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`
- original ledger: 192 total / 164 done / 15 in_progress / 12 todo / 1 blocked
- remaining original TODOs: 28
- new original TODOs completed this round: 0
- cumulative since the 163 baseline: 1/10 (`P8-005` only)

## Task-profile terminal result

Run [38026411200](https://github.com/jyqj/codecortex/actions/runs/38026411200) is now `completed/failure`.

- source: `f97c5068d056969705e8387ba1f37d83847f5bee`
- jobs: 47 total; build 1 success, measure 45/45 success, aggregate failure
- official artifacts: 1,037 unique = 1 build + 45 capacity + 945 progress + 45 shard + 1 matrix
- aggregate enumeration stopped at 1,000 artifacts and downloaded 42/45 shards
- official matrix artifact: `11662173135`
- official and independently downloaded matrix SHA-256: `846446d1267f876f0b03ac514cfe78c14748a53b38043bbf357e77689cc01a42`
- matrix ZIP CRC: OK
- matrix result: `status=failed`, `passed=false`, accepted 42/45, `release_certification=not_run`
- missing slots reported by the official aggregate: `batch_100/100000/0`, `body/100000/0`, `config/100000/0`

The three reported-missing shard artifacts exist after the failed aggregate and were independently received:

| slot | artifact | SHA-256 | ZIP / shard result |
|---|---:|---|---|
| batch_100 / 100000 / 0 | 11663266493 | `785e8886ed4ef1321d58bf04cee579d9bfbf994f8bb21eafc60b1db96bbc401d` | CRC OK / passed |
| body / 100000 / 0 | 11662607565 | `2b24b90309acd058b85fc820b43c50d4060aa299577bd6d85bf023ecc91ebae4` | CRC OK / passed |
| config / 100000 / 0 | 11663330163 | `d0f78047920e1a6a942d511bd631ac5b64d97a0fcf8eee5413928505d4803153` | CRC OK / passed |

These later receipts do not rewrite the official aggregate failure and were not used to manufacture a replacement matrix.

## P8-006 acceptance

`P8-006` remains `in_progress`. The task-profile study is explicitly N=1 / 45 cells / 85 retained records when complete. It is not the original N30 / 150-shard / 1,500-composite scale study. It cannot be pooled with any failed scale cohort and earns no original TODO completion credit.

The original acceptance still lacks a successful same-source, same-binary, same-profile 100k shard and a successful 150/150 aggregate with all 1,500 required observations and downstream scope checks.

## Draft PR #203

Draft PR [#203](https://github.com/jyqj/codecortex/pull/203) was opened from commit `797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f` against the fresh main above.

- changed file: `.github/workflows/p8-task-profile.yml`
- diff: +2 / -10
- product/task definitions changed: none
- patch: retain local checkpoint 19, remove its redundant remote upload, and give `finish` no previous artifact identity so checkpoint-19 pending chunks are included in the final bundle
- expected complete-run artifact count: 992
- raw/shard/matrix/timeout/cell/capacity/sampling requirements: unchanged
- known interruption boundary: a catastrophic runner loss after local checkpoint 19 and before final upload leaves the last remote proof at checkpoint 18; the tail remains unknown and is never recorded as zero

An earlier candidate that reused upload-18 identity at `finish` was independently rejected because it would falsely acknowledge checkpoint 19 and corrupt custody. That unsafe form was not published.

At this freeze, #203 is OPEN/Draft with exact head `797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f`; normal PR CI has started and is not yet complete. No merge or rerun was performed.

## PR management

Fresh review found seven pre-existing open Draft PRs (#3, #4, #65, #127, #137, #188, #191), all diverged from main. Safe immediate merge candidates: 0. Safe immediate close candidates: 0. #203 is an additional isolated Draft fix and is not counted as an original TODO.

## Scope

No paid provider, holdout, secret, local blocked native GC/WAL action, workflow rerun, replacement study, cancellation, task-ledger mutation, or cross-source/profile pooling occurred. This evidence branch preserves an old product tree and must never be merged wholesale into main.
