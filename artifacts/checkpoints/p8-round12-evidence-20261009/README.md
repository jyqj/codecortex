# Round 12: verified controls and retained failures

**Original TODO ledger: 192 total, 163 done, 29 remaining; 0 newly closed.** This checkpoint records engineering progress and the exact limits of the evidence. It does not complete the scale gate, the ten-task dependency chain, or release approval.

## Verified results

The fixed engineering source is [`8e542e644b06a83fc82d18d9896cf82f5b2d796e`](https://github.com/jyqj/codecortex/commit/8e542e644b06a83fc82d18d9896cf82f5b2d796e), with an unchanged branch after publication. [Run 37863581678](https://github.com/jyqj/codecortex/actions/runs/37863581678) actually completed all 13 selected commands and all **41 selected test methods**, with no failures or selected ignored tests. Both independent receivers exited zero. All 32 control-artifact members and all six build-artifact members match the original ZIP bytes, GitHub artifact digests, and complete 1,087-input source snapshots. The original build validator and original binary `--hash-file` verified the release build. The binary SHA-256 is `968d139f51b00c3bf5a130eb4959e16b67bf0c627a6d3ef35156de371193f017`.

These are scoped engineering results. The fixed 1k/10k diagnostics were still queued at the last observation in this packet, and the 100k diagnostic had not been admitted. No new primary N30 study was started, and no old measurement was replaced.

The G4 [recovery and rollback run](https://github.com/jyqj/codecortex/actions/runs/37854847808) produced original artifact `11588128186`. The original receiver and supplemental raw checks exited zero. All 499 archive members, source and observer bindings, preserved database integrity checks, actual process failures, and the real 25 → 24 → 25 → 25 rollback sequence were retained and checked. This remains G4 functional recovery evidence; it is not the eight-cell release-platform result or a scale result.

The G4 [mixed-runtime run](https://github.com/jyqj/codecortex/actions/runs/37854847820) completed C16 with 900 offered and successful operations: 600 reads and 300 builds, including six mutation strata of 50. All 2,125 original archive members and both inventories were independently checked, and the original 15-table parity report is bound to the sealed raw records. **C1/C4/C8, the independent Rust statistics replay, and the complete original four-cell collector remain pending.** The C16 records in this packet are archive-integrity evidence only.

## Failures remain failures

Original G run `37830173594` has eight received, independently checked native 300-minute deadline failures: samples 0, 2, 3, 4, 5, 6, 7 and 9. Their different incomplete stage counts remain visible; the original validators reject the missing registered stages. Sample 1 separately failed with runner shutdown and exit 143. No timeout was relabeled as an external interruption, and no interrupted or failed sample was rerun or replaced. D0 still has its missing mandatory shard following its separate runner shutdown; its study cannot be completed by borrowing another source or a later passing attempt.

The mixed C16 intake adapter's first attempt expected the wrong success label, `passed`. Original G4 runtime code defines `passed_observation`. The second adapter changes that one constant only. The first failed receiver execution, the correction, and the second zero exit all remain in the packet; the product, original ZIP and original report were unchanged.

The owned lifecycle expansion reclaim also has two distinct records. The pinned original directory was removed successfully after exact byte and inode verification. It subsequently appeared again with a different inode, while the original ZIP and 25 protected files remained unchanged. The cause was not established. No further deletion occurred, and this checkpoint does not claim durable free-space recovery.

## Source and dependency limits

The original ten tasks are P8-005 through P8-013 plus P8-016. Their original hard dependency chain begins at P8-005. The task ledger, acceptance criteria, sample populations, budgets, fixed studies and CI gates were not weakened or changed.

Source-admission preparation is still a draft. The source-identity review distinguishes historical results from evidence applicable to later code: a new SHA does not mechanically invalidate every unrelated check, but changed writes affect cold-build, mixed/soak and transaction-recovery observations. Static similarity and 41 functional controls do not turn G4 observations into measurements of 8e. The original final release collector still requires its own exact candidate binding.

The next optimization is only an independently reviewed design: bounded same-file, same-table insertion batches. No speedup or completed implementation is claimed here. PR #159/#161 were also inspected for reuse; their production tree contains no performance change after source 599. The reusable runtime finalization defect and subsequent implementation draft are tracked separately in round 13.

## Packet contents

`round12-review-records.tar.gz` preserves **213 original review, metadata, control and log files** (6,150,884 uncompressed bytes), including the complete small controls ZIP. `manifest.json` gives every original local path, archive path, byte count and SHA-256. The archive SHA-256 is `831a1e8351dbd1794c263e3eeeb354f6558fe5e301aad3d1d961d2c7947abbe0`.

The manifest separately lists the unchanged bulk ZIPs for the release build, G4 recovery, mixed C16 and all eight received G failures. They remain in GitHub Actions and in the existing local receptions. This review-metadata checkpoint does **not** duplicate those bulk payloads or claim that the metadata packet alone can replay them. Each reference includes its original artifact ID, run, byte count, digest and Actions URL. No signed download URL is included.

This is an artifact-only checkpoint following round 11 commit `b2aede645d76cf44f58d70de1df94de00c32031b`. It does not move PR #160, any fixed study branch, the production code, or the original TODO ledger.
