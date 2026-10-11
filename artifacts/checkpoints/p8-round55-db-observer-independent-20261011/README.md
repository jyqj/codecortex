# R73 P8-006 / P8-007 independent acceptance record

Snapshot time: `2026-10-11T18:16:57Z`. Cutoff: `2026-10-11T16:52:28Z`.

This directory is a pure-additive independent acceptance record. The validation itself did not modify task statuses or source/run identities. No workflow was rerun or cancelled and no evidence was pooled across sources. Its later archival into the evidence branch does not alter these validation facts.

## Decision

- `P8-006`: **cannot receive a new original-TODO closure**. There is no same-source/binary/profile `N=30` cohort with `150/150` measurement shards, `1500` observations, successful `100k`, and successful aggregate. All six known fixed studies remain `completed/failure`.
- `P8-007`: its narrow four-concurrency DB-acquisition observation gap is now independently verified from the original ZIPs, but **the original TODO still cannot close** because it hard-depends on unaccepted `P8-006`. The observer reports themselves also say `task_complete=false`, `release_approval=false`, `release_certified=false`, and `new_performance_gate=false`.
- New original-TODO closures in this review: **none**.

Current main mechanically records these tasks as `done`, but Issue #158's latest coordination correction explicitly says those status changes are not creditable under the controlling acceptance contract. No Issue #158 comment was created or updated after the cutoff.

## Fresh state

- main: `c2b726538cacfdf6ae9f01977a7b93da48d2e2b2`
- commits since cutoff: `0`
- workflow runs created or updated since cutoff: `0`
- Issue #158 comments created or updated since cutoff: `0`
- latest Issue #158 comment: [6095956801](https://github.com/jyqj/codecortex/issues/158#issuecomment-6095956801), created and updated `2026-10-10T09:06:08Z`

## Fixed N=30 studies

| Run | Head | Terminal state |
|---|---|---|
| 37896198208 | `275e8799d4947d297329073eaa3ca675d3fd0777` | completed / failure |
| 37902429727 | `c8be5afaac568ffd40ef86d3795423c3b73c9f39` | completed / failure |
| 37910924354 | `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` | completed / failure |
| 37962416564 | `4fe927488d5cb7f26bd27c7624745fb1b6ca202b` | completed / failure |
| 37982220020 | `008f1ec0ae16b9b6aff9f20d033b17b2219168f8` | completed / failure |
| 38013753078 | `9cc6bf49f6dd81e4069a8004eed49addba0ab79b` | completed / failure |

## DB observer independent reception

Run [37930392164](https://github.com/jyqj/codecortex/actions/runs/37930392164), attempt 1, source `4d18dcdb34b5d7a277566b57801cc882d8b1eb61`, is `completed/success`. All four artifacts use profile `mixed`, diagnostic profile `mixed_db_lock_observation_v1`, product SHA-256 `5f42c2aceaf74d6abd6c96dd176f93f82f5bdcb898b80f9b2740cabe61745f7a`, runtime-statistics binary SHA-256 `c0b169f50d0530e64e9fb461504274d8ade82d4efe1d0ede632c14daa684027b`, and oracle SHA-256 `a48ab71e4342fa30b2e987b32cc172f0686c41d27e03cb2becf07c570873383a`.

| C | Artifact | Bytes | Official/local ZIP SHA-256 | ZIP CRC | Samples | Actual peak / overlap |
|---:|---:|---:|---|---:|---|---|
| 1 | 11623110311 | 16,608,748 | `6fa82bbe05ee1a35212cae00e0e861ad97d74f026dfd21a895b93020cbb895ac` | 0 | 900 expected / 900 recorded / 0 missing / 0 unexpected | 1 / false |
| 4 | 11620977433 | 16,993,296 | `67eba33ee740e923a303b6ec8c19018d6d40fd5b25f8ab7514ef56f1490e9cfc` | 0 | 900 / 900 / 0 / 0 | 4 / true |
| 8 | 11621010611 | 16,642,248 | `d12b7f72eee15a47691f3168342671f4d7426b76200e9ca3288fd844f33f0236` | 0 | 900 / 900 / 0 / 0 | 7 / true |
| 16 | 11623006495 | 16,407,395 | `3486cb9bfff9787be59db59ae7612d8b20b56846cb9858caf4709de5ade2586e` | 0 | 900 / 900 / 0 / 0 | 12 / true |

For each artifact:

- `unzip -t` exited `0`.
- The bundled original `p8-runtime-statistics` exited `0`. Its new output is byte-identical to both archived `statistics.json` and `statistics-replay.json`.
- The bundled original `p8-oracle` exited `0`, returned `comparison.equal=true`, and compared exactly 15 tables. After deleting only local left/right paths and volatile timing/timestamp fields, its output is byte-identical to archived `parity.json`.
- `offered=900`, `success=900`, `missing=0`, `unexpected=0`.
- Workload DB acquisition counters have `attempts=acquired`, `failed=0`, `would_block=0`, and boundary in-flight counts `[0,0]`.

### Workload DB acquisition counts

| C | Checkout attempts / elapsed ns / lifetime max ns | Pool attempts / elapsed ns / lifetime max ns | Writer attempts / elapsed ns / lifetime max ns |
|---:|---|---|---|
| 1 | 16,850 / 86,362,596 / 105,718 | 16,850 / 997,977 / 23,404 | 3,550 / 177,158 / 27,832 |
| 4 | 16,848 / 68,937,811 / 362,008 | 16,848 / 901,594 / 2,254 | 3,550 / 117,172 / 250 |
| 8 | 16,846 / 73,025,045 / 279,844 | 16,846 / 1,015,497 / 35,452 | 3,550 / 130,941 / 320 |
| 16 | 16,848 / 78,694,013 / 489,805 | 16,848 / 1,040,384 / 29,546 | 3,550 / 147,338 / 161 |

The last value is `process_lifetime_max_ns_at_end`; it is not an interval maximum. Each report explicitly has `interval_maximum_observed=false` and `sqlite_busy_wait_observed=false`.

## Local paths

- Download root: `/tmp/codecortex-db-observer.ns0KW5`
- ZIPs: `/tmp/codecortex-db-observer.ns0KW5/c{1,4,8,16}.zip`
- Extracted roots: `/tmp/codecortex-db-observer.ns0KW5/x{1,4,8,16}`
- Per-C archived inputs and reports: `<extract-root>/p8-lock-runtime/{plan.json,raw.jsonl,report.json,statistics.json,statistics-replay.json,parity.json,seal.json}`
- Per-C original validators: `<extract-root>/p8-lock-build/{p8-runtime-statistics,p8-oracle}`
- Per-C independent outputs: `<extract-root>/{independent-statistics.json,independent-parity.json}`
- Structured summary: `ACCEPTANCE.json`
- Exact commands and compact receipts: `VALIDATOR-COMMANDS.log`

## Interpretation limits

- Checkout timing may include connection validation or construction; it is not pure lock or SQLite busy-wait time.
- Writer and pool timing covers the actual acquisition call and uncontended overhead; it is not pure blocked-scheduler time.
- C8 and C16 observed peaks were 7 and 12. Configured values 8 and 16 must not be claimed as achieved concurrency.
- The instrumented diagnostic build must not be merged into default-build latency samples.
- The observer source/profile is retained as its own evidence identity and is not pooled or relabeled as an N=30 scale study.
