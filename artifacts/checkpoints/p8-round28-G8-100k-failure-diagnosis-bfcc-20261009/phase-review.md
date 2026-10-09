# G8 100k repetition 0: original failed-run diagnosis

The original native supervisor explicitly ended with `deadline_exceeded` and exit 3. GitHub's measurement step returned 1; the worker's own exit is unknown (`null`). `stderr_complete=false`, so the two empty stderr files are not proof that a complete worker error stream was observed. No original validator, compiler, product, benchmark, or ZIP check was executed by this analysis.

Identity: source `4fe927488d5cb7f26bd27c7624745fb1b6ca202b`, run `37962416564`, attempt 1, job `113930880282`, failed artifact `11647876945`. Root retained the complete original failure at checkpoint `86d91967fc38d5a7973c4b9701c2f51c491cca46`. Original raw is 21,619,111 bytes, SHA256 `72ec93ae24739379a00d08d6f97219ce823f2a34762d7c47ecbe6b886da691a5`. The accompanying machine report binds the before/after original files and five exact G8 source files.

## What actually finished

All 1,004 emitted build starts have matching finish records: 9 full builds and 995 incremental builds. The last completed stage is `batch_10`. Both builds for `batch_100` finished, but its `stage_finished` record is absent; `batch_1000` was not entered. There is no worker summary or successful shard result.

| Completed work | Count | Recorded wall seconds |
| --- | ---: | ---: |
| Full builds | 9 | 5,050.290055 |
| Incremental builds | 995 | 6,338.003927 |
| Complete parity calls | 7 | 6,047.956396 |
| These nonoverlapping outer intervals | — | 17,436.250378 |

The seven complete comparisons are cold, no-op, body, API, config, batch 1 and batch 10. They account for 72,118,882,766 logical canonical bytes, not measured disk traffic. Their parity seconds are respectively 601.836351, 905.331465, 891.940155, 798.037058, 834.474460, 831.569257 and 1,184.767650.

Body/API/config used 490/171/330 incremental builds. Their outer incremental build durations are 3,040.730622 / 1,229.406373 / 2,039.155378 seconds. The stage-level incremental timer also includes evidence handling and is slightly larger; both definitions remain separate in the machine report. Reported phase and build-timing subfields overlap and must not be added to these outer totals.

The final full control completed at worker-clock 17,448.414449 seconds, taking 566.885081 seconds. Against the nominal 18,000-second deadline this leaves approximately 551.6 seconds, but that difference is not a measured parity duration: worker/supervisor clock origins differ, the supervisor also drains stderr, and its total 18,019.166 seconds includes 17.978 seconds of fixture cleanup.

## Exact stopping interval, with uncertainty preserved

G8 `p8_scale.rs:897-925` calls full control, determines completeness, calls `parity`, then performs `config_fact`, builds the verdict and emits `stage_finished`. `parity` at lines 581-603 first reads both counts and then invokes the streaming oracle for this scale. Thus the last unclosed interval is **after batch-100 full control and before its stage receipt**. There is no `parity_started` record or per-table timing. The original records do not identify the active function/table at termination and cannot exclude the later fact-check or emission portion of that interval. They do rule out an unfinished emitted build.

The seven completed comparisons were equal; this does not complete the eighth comparison, the final batch, or the registered shard. The failed shard contributes zero accepted samples. The study's prior accepted 4/150 shards and 41/1500 observations are not increased by these partial records.

## Source-backed implementation observations

G8 `streaming.rs:432-507` first spools A, attempts the raw-cell natural-order witness, and spools B on a mismatch. Cold and no-op report the witness true for all 15 tables. For every later completed stage, all 12 nonempty tables report false; only the three empty tables (`resolution_frontier`, `dispatch_sites`, `test_edges`) report true. Scratch high-water reports correspondingly change from 2,992,115,712 bytes to approximately 5,984,4xx,xxx bytes. This shows that the shortcut did not avoid the second spool on these ordinary update stages. It does not measure mismatch position, extra witness work, per-table IO, or the gain/loss against a dual-spool control. This run predates the separately reviewed b8ab dual-spool restoration and cannot supply that source's execution credit.

The actual G8 dependency method still probes `cap + excluded.len()` before Rust-side exclusion (`resolution_dependency_store.rs:119-174`). The positive-surface method has the same pattern over three tables and seed batches (`public_surface_store.rs:60-112`). The reconciliation code merges both methods' work into `dependency_sql` (`reconcile.rs:111-208`), so the counter is not a measurement of the dependency method alone. In the final body continuation it reports 322,140 returned rows, 1,032 statements and 14,785,091 VM steps; the final config continuation reports 258,130 rows, 1,030 statements and 11,270,518 VM steps. These are concrete retained-work clues, not a statement that SQL explains the entire wall time.

Existing narrow exclusion-query work can be evaluated against this exact-source gap without changing the 200-file budget, conservative invalidation, completed-set semantics, global sorted/distinct overflow witness, read-lease or epoch boundaries. Any claim about improvement still needs its own applicable source and original controls; the earlier scattered-key counterexample must not be hidden. This report creates no new candidate or experiment. The already reviewed immutable payload/window/RowProjection changes are present in G8 and must not be proposed as new fixes for this failure.

There is no original evidence here supporting an increase to the registered 2 MiB SQLite cache, a deadline increase, fewer stages, fewer comparisons, or relaxed validity. Endpoint disk snapshots and instantaneous process snapshots do not establish disk or memory peaks, and no OOM or host-loss cause is established by these records. Root's prior decision to retain the physical contract is unchanged.

## Reproduction of this diagnosis

`analyze-original-raw.py` reads the already extracted JSON/raw files, calculates summaries and verifies their hashes remain unchanged. `analysis-receipt.json` records its actual exit 0; stdout and stderr are retained. `diagnostic.json` is 64,850 bytes with SHA256 `e0c75292d0bbb77d069473b79dc440338107bd2d82a92c1a8bf0931d208f9d99`. No original transport or semantic acceptance was repeated. The source copies match the original 1,095-input source receipt, including exact Git blobs and SHA256 values.
