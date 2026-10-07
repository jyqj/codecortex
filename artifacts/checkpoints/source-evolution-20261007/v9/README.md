# Reviewed source v9: version the historical integration entry

The first PR #144 legacy CI run failed because its historical verifier compared a frozen task status snapshot with the current task table. The preserved original scripts remain unchanged. This version explicitly selects `verify_historical_integrations_v2.py`, whose historical checks bind task snapshots to fixed integration commits and whose current-task check preserves the original definitions against a fixed baseline.

The registry changes only its source-version label. All 776 crate/Cargo inputs, six reviewed source deltas and product commit remain identical to v8. The legacy workflow changes only the selected version and the single historical verification entry relative to the preceding PR head; earlier default product references remain intact. CI rejection controls also reject reverting or skipping the selected historical entry.

`entry-migration.json` records the fixed entry migration before the new verifier is integrated. Implementation, independent review, actual validation and subsequent CI are recorded separately. Prior v4-v8 records and the original failed run remain historical evidence. No task, G7, live or release acceptance follows from this source-only migration.

## Actual local verification and independent review

The new historical CLI passed at fixed integration source `ab691b0370e6b28f5356c08e1647436b8e251a0c`: 731 historical packing inputs and 246 evidence files, 708 E3 inputs and 58 imports, all 192 current task definitions and four generated views matched. The ten new controls passed at their fixed author source, with their three source files unchanged before/after. Exact commands, elapsed times, logs and source hashes are retained here.

`independent-review.json` accepts the combined source and explicit CI entry migration only. It preserves the original failed CI and does not count the still-running old local source suite or pending updated GitHub workflows as passed. Current task state remains 40 unfinished and P7-013 remains in progress pending the required CI.
