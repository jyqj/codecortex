# P8 round 37

Authoritative main is `23eab8a7ea1fdfd6333c2e5a001cfb41e2a10d69` (tree `e039a817291934b15cc018fd74d2c83982bb7f37`). The original ledger remains 192 total / 163 done / 16 in_progress / 12 todo / 1 blocked, so 29 remain. Newly completed original TODOs: none; cumulative progress from the 163 baseline: 0/10.

PR #180 was merged by another collaborator at 16:42:18Z. Its exact head `88d5f547...`, test merge, and main merge commit have the same tree. Current exact-head CI is 26/26 success; at merge time it was 25/26 and the soak later succeeded. The atomic ready-PID fixture repair is present in main. Main push CI was still 13 success / 3 in progress / 2 queued at the retained snapshot; no rerun was requested.

PR #188 is 26/26 green but remains Open/HOLD. Its two-file algorithm is semantically useful, yet new paired mechanism evidence shows wall regressions despite reduced dense rows/VM: dense 1.468x slower, sparse 93.55x slower, and 400-key multi-batch 11.19x slower. The PR remains based on an old integration branch and cannot be transplanted whole. It is not admitted as the G3 deadline fix.

The G3 raw now supports a much narrower causal decomposition. Within the 18,000s worker budget, 1,002 completed builds consumed 10,856.107s (60.31%) and six completed exact parity passes consumed 6,197.577s (34.43%). The last batch-10 full control ended at 17,065.613s; batch-10 parity did not finish before the deadline, leaving a derived final interval of at most 934.387s. This interval is inferred from timestamps, not a recorded parity timer. Batch-100 and batch-1000 never began.

The material domains are repeated 15-table exact parity, B=200 closure with 991 unscoped incremental scans across body/API/config, and eight full-control builds. PR #188 only addresses part of one SQL-work component and changes none of those dominant loop/protocol structures. A new 150-shard study is therefore not authorized until a concrete material product repair is established and bound to a fresh P/R/G.
