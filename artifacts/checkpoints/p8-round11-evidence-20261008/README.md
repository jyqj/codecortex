# P8 round 11: original failures and scoped reviews

Original task ledger: **192 total / 163 done / 16 in progress / 12 todo / 1 blocked**.
New original TODOs completed in this round: **0**. Remaining original TODOs: **29**.

The active goal remains ten original tasks: P8-005 through P8-013, plus P8-016.
This checkpoint preserves evidence gathered while those original acceptance gates remain open.
It does not change task definitions, task status, product source, observers, CI gates, or any running study ref.

## Contents and restoration

`round11-evidence.tar.gz` contains 78 original metadata, log, code and review files, totaling 3,902,656 uncompressed bytes. Its SHA256 is `afdf46abc3a40066b48ebaee1d76ddb8242a839ef55a2d1b2a43964587410e72`. The complete path, length and SHA256 inventory is in `round11-manifest.json`. Every member was read back and compared byte-for-byte with its input; all original input files were rechecked unchanged after packaging.

The complete original failed peer ZIP, artifact **11586146906**, is preserved separately as two ordered byte parts. It is 5,204,573 bytes, contains all 12 original members, and has SHA256 `ebcd2b6f8961c44298862be6a5c03fa364f9e7a7a1cdb887258c1b172ee1470e`. Each part and the reassembled byte stream were verified against the original ZIP. The split changes transport packaging only; it does not rewrite any ZIP member, receipt, source identity or failed outcome.

After downloading this directory, restore that exact ZIP with:

```sh
python restore_peer_zip.py --output peer6e3-100k-failed.original.zip
```

The restoration command verifies both parts before creating a new output file, refuses to overwrite an existing output, and verifies the whole original ZIP digest. The original GitHub artifact remains separately identified by its run and source in the manifest.

## Two different scale failures

### Original peer 6e3: real native deadline

[Run 37824742267](https://github.com/jyqj/codecortex/actions/runs/37824742267), measured source `6e3eb4fd97edcf238774e74b88d397e059337bac`, retained a complete failure ZIP after its 100k trial reached the original native deadline. The original report records `deadline_exceeded`, exit 3, wall time 18,021.885 seconds, including 20.740 seconds of fixture cleanup.

Only five of the nine required scale stages completed their full 15-table parity checks: cold, no-op, body, API and config. The batch-1 full control completed, but its next parity was not sealed; batch-10, batch-100 and batch-1000 did not start. The unchanged original raw validator rejects the result for missing registered mutation stages. This is preserved as a failed original study, not accepted scale evidence.

Its completed phases identify material work: seven full builds spent 4,150.691 seconds in staging; 993 incremental builds spent 3,391.198 seconds in preparation and 2,911.050 seconds in commit; five completed oracle comparisons used 5,622.544 seconds. The source and host differ from D0, so these observations do not establish D0's outcome, causal speedup or expected finish time.

The D0 engineering 10k observation independently still spent 157.186 seconds in full staging and 176.719 seconds in oracle comparison out of 489.810 seconds. `D0-deadline-risk.md` explains the remaining risk with the ten specifically checked staging-path identities. It does not assert that all production source or every possible behavior is unchanged.

### D0 first primary failure: runner shutdown and missing raw upload

[Run 37854240827](https://github.com/jyqj/codecortex/actions/runs/37854240827) retains controller `ca72a2dde363e35203f7b3cb30ed00729d460fd7`, measured source `d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`, attempt 1 and all 150 originally registered primary trials.

The original 100k/index-8 job **113580044384** records exit 143 and a runner shutdown signal. Its measurement step ran from 23:05:20 to 23:40:49 UTC, 2,129 seconds, well below the original 18,000-second measurement budget. Capacity had passed beforehand with approximately 90.176 GB available. The raw shard upload and post-checkout steps were explicitly skipped. The available evidence does not identify what caused the shutdown; it is not proof of a product rejection, OOM or manual cancellation.

The complete decoded job log, terminal job API, run API and independent review are preserved under `D0-first-failure-100k-08/`. The original capacity artifact remains recorded separately. No terminal measurement raw file was available for this mandatory primary sample. Consequently the fixed full matrix is not acceptable as complete.

At the separate observation of 23:45:50 UTC, the full 151-job list contained one successful admission, one failed measurement, five measurements in progress and 144 queued measurements. The eight artifacts consisted of admission, build and six capacity artifacts; no completed measurement shard was available. This observation is a progress snapshot, not a final study report.

The earlier G-study runner-shutdown failure is retained in the preceding [round 10 checkpoint](https://github.com/jyqj/codecortex/tree/64230c8a628813d4f4f7c733e9779f17e85606b8/artifacts/checkpoints/p8-round10-evidence-20261008). No original G, peer or D0 study has been cancelled, retried, overwritten or silently substituted by this work.

## CI, PR management and source scope

- **G4 CI:** security and MSRV completed successfully. The check job passed all 372 P8 Python controls and 181 source-integrity tests, then failed the unchanged historical-integration path guard; later checks did not run. G4 remains a failed CI run.
- **G3 CI:** its original three jobs completed successfully, including its own 372 Python controls, 181 source-integrity tests and historical guard. Those results retain their older G3 source and merge identity; they do not replace G4 CI.
- **PR #165:** independent review of fixed head `f88a41e6af46ffe67b76f420f1d6b4ef21155946` against main `341db995e5f092fd95aed35af84cbc8b69e1af40` confirms preservation of the complete original 188-file Git subtree and all modes/blob/tree identities, with five added mapping/review files. The original timeout, 12-GiB guard stop and not-run post-ready states stay explicit. The original excluded path and its CI contract are restored without adding the archived failures to the accepted historical integration. The review accepts this narrow preservation scope; it does not grant CI or merge approval. At 23:45:50 UTC, MSRV was successful and seven checks were still queued.
- **P7 engineering:** the original 26-member ZIP from job **113576447291** was fully received and verified. It retains actual PR-merge source `5b7858cf584d1bfdbfaa89027b05bb4d7cb1cd49`; all 1,087 recorded production input bytes match G4. Its 95 passing Rust tests, one original ignored test, 15 input-lock Python controls and 768 functional requests are engineering regression evidence. This workflow does not provide a complete release-binary/Cargo-JSON chain or formal tail/concurrency acceptance.

The immutable round-10 publication was independently rechecked: four added artifact files, G4 as its sole parent, all other root entries unchanged, 428 members verified and no private signed download URL included. This round retains the actual publication objects and that independent audit.

## Soak receiver preparation

The G4 soak receiver and its separate wire-binding helper were independently reviewed at fixed SHA256 identities. An actual offline replay of the retained older 14-read engineering probe bound all 56 original RPCs, and a deliberately changed hybrid response was rejected. The older probe remains an older source and a short engineering control.

The formal receiver requires the actual G4 run/job/artifact and full original data. It keeps 3,601 offered operations as 2,400 compound reads and 1,201 builds. Each read contains two status calls, one symbol search and one local hybrid search; its four RPCs stay inside one latency sample. All 9,600 read RPCs must match original arguments, returned tool JSON, separately verified wire bytes, and a common monotonic clock origin. Sampler status calls may interleave.

The unchanged original cache summary must verify actual-time quarter coverage, hits, misses, invalidations, native process identity, complete generations, current source evidence and bounded shared query-pool counters. The original retained Rust statistics executable may only replay original plan/raw files offline. No G4 soak artifact had been received and the formal receiver had not been run when this preparation was archived.

## Round 12 work

Round 12 investigates the measured full-staging bottleneck and reviews a legitimate continuation path after the infrastructure failure, while the original studies and PR checks remain under observation. The target of ten completed original TODOs remains open. Existing failures, original denominators, task dependencies and all acceptance gates remain part of the record.
