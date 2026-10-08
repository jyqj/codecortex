# Round 10: original evidence, reception reviews, and remaining gates

This checkpoint closes **zero original TODOs**. The authoritative ledger remains
**192 total / 163 done / 29 remaining**. Implementation, prerequisite controls,
individual platform cells, receiver controls, and evidence archival are not
counted as completed original tasks.

## Fixed identities

The active implementation PR is [#160](https://github.com/jyqj/codecortex/pull/160),
with P5 production source `f5319d6a04f83aabe9dd635137382f4782d8d189`, independent
review R4 `9c24adfa6bb8b8114b4dec8c8a344072957956ce`, and G4 verification head
`260f596582f2d82b8d7c707b61a6b8b6a43b069f` (tree
`645404431ca15770d6e729785a3ada68b096c593`). The existing source guards and task
definitions are unchanged. This packet includes the independently checked actual
publication bridge and the full D0-to-P5 production-byte identity boundary.

The new 150-cell scale study has controller
`ca72a2dde363e35203f7b3cb30ed00729d460fd7` and actually measures source
`d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`. Its run is
[37854240827](https://github.com/jyqj/codecortex/actions/runs/37854240827), attempt 1,
with fixed registration SHA-256
`4a56e31e9838f0db1d83a3efd4a10d0a51f8b443dbce4413e68253881740eebc`.
The controller, measured source, and later production-byte bridge are distinct
identities; no original measurement receipt is relabeled as P5 or G4.

## Completed scoped evidence

- **G4 original failure gates:** run
  [37854847767](https://github.com/jyqj/codecortex/actions/runs/37854847767), artifact
  `11584996050`, ZIP SHA-256
  `b5d6fd945c468822700aafe0f66fcf482697c2333e2197fd2e0039969671a529`.
  All 337 original members, the complete 336-file receipt, all 1,087 source
  inputs, actual Cargo artifacts, and six executed nonignored Rust tests were
  checked. Five original comparison outputs preserve the expected failures;
  the tests assert six CLI process invocations, including the repeated
  bad-policy output-overwrite refusal. The other two Rust controls exercise
  library gate behavior. These synthetic failure fixtures are not product
  performance or retrieval-quality observations.
- **G4 lifecycle:** run
  [37854847926](https://github.com/jyqj/codecortex/actions/runs/37854847926), artifact
  `11585181176`, ZIP SHA-256
  `81756f7d1990147961c87f905625c33da69b57d703f8eb4afa7e0f690ca52c31`.
  All 2,391 original members were checked. The original retained evaluator
  replayed all 1,230 samples and 1,200 source-verified queries with byte-identical
  output. The observation has 431 closed sessions, 400 distinct reopened native
  PIDs, and 1,261 resource snapshots. All 30 closed fixtures and all 90 physical
  cache objects correspond to the archive in both directions: 128,901,120 bytes.
  OS page-cache state remains unknown; physical FTS bytes are not double counted,
  disabled-provider costs remain explicitly not applicable, and no whole-process-
  tree peak or causal performance improvement is asserted.
- **D0 actual first admission:** the original admission and re-uploaded build
  were received as 78 members with full CRC/SHA verification. All six build
  members are byte-identical to original upstream artifact `11582571291`; ZIP
  container SHA changes do not imply changed build bytes. The original build,
  both diagnostic shard validators, twelve prerequisite commands, and all 67
  upstream members were independently checked. The two diagnostics remain
  prerequisites and are not samples in the new 150-cell primary study.
- **D0 complete receiver preparation:** original transport controls remain,
  with fifteen executed controls covering the three identities, fixed
  registration, all 67 upstream members including both old capacity receipts,
  and the required 302 artifacts plus one optional original aggregate artifact.
  Full reception retains the original 150 shard/capacity pairs and original
  combine rules. It has not been executed on an incomplete primary matrix.

## Current CI and unfinished work

The G4 CI check ran the complete 372 P8 Python tests and all 181 source-integrity
tests successfully, including the actual v15 P5/R4 bindings. The subsequent
unchanged historical integration gate failed because main `341db995...` had
acquired the excluded historical paired directory through archival PR #163.
The original check job log and failure review are included. This CI check is
**failed**, and later skipped steps are not reported as passed. The source-only
main-invariance proof remains a source-only proof: it did not establish that
artifact-path constraints would pass. Existing
[PR #165](https://github.com/jyqj/codecortex/pull/165) is being independently
reviewed for the archive boundary correction; it is not accepted by this packet.

The full scale study, all eight G4 platform cells, mixed workload, semantic
backfill, one-hour cache/worker soak, and recovery/rollback still require their
complete original observations and applicable acceptance checks. At the retained
23:25 UTC D0 observation, admission succeeded, three primary measurements were
running, 147 were queued, and zero primary measurements had a terminal result.

The separate original G study is preserved. Its 100k/index1 job received a runner
shutdown signal and exited 143 before either stated deadline, with raw upload
skipped. Its original metadata and log are included. The log does not establish
who caused the shutdown, a product parity failure, or a budget timeout. No
replacement sample or rerun is substituted for that missing original evidence.

## Packet and original artifacts

`round10-evidence.tar.gz` contains 428 unchanged selected evidence/audit members.
`round10-manifest.json` records every byte count, SHA-256, original location,
archive member origin, and full original ZIP reference. Packaging checked every
member again after a complete archive round trip and confirmed that original
local input files remained unchanged.

All original gate nonbinary members are included. Selected original lifecycle
metadata and all local replay receipts are included. Large original binaries,
databases, and lifecycle raw streams remain in the complete retained GitHub
artifact ZIPs identified above; their full member inventories are included.
The review packet is not presented as a replacement for either complete raw ZIP.
All original absolute path strings, historical failures, limitations, and
unexecuted-work statements are preserved. Packaging performs no new product,
Cargo, oracle, or workload execution.
