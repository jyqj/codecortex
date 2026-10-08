# P7-016 opportunistic reclaim correction

The independent G7 review found that both embed drains and the explicit revocation drain still ran the unbounded global reclaim before each claim. Original wiring item 9 belongs to P7-016; the earlier green CI did not satisfy that missing implementation.

The new target-space primitive preserves the old global APIs and their byte content. Each of the three production drains reclaims one page, capped at min(max_batch, 64), before claiming. Older foreign-space tasks cannot consume that page. Existing ready-task claim budgets and provider/lifecycle fences remain. This bounds expired row mutations; it does not claim a universal bound on SQL scan time.

Nine new real-SQLite tests passed. They distinguish explicitly aged synthetic rows from one real-clock expiry control, observe actual row changes using a trigger, and verify 64/64/9/0 convergence, foreign/lease protection, attempt and epoch preservation, rollback, invalid limits, cancellation and small budgets. The four unchanged queue/parallel/recovery/switch targets passed 29 tests with one pre-existing ignore. The first compiler failure and exact original test source are retained. Independent source review and fresh full CI are still required.

TODO ledger unchanged: 153 done, 39 unfinished; no new task is closed by this author record.
