# Runtime review and author handoff

## Scope

Independent review started at commit `5610335b31f0f866cfa17712c8a55b9b22dac4cf`. Root authorized implementing confirmed defects in only `scripts/p8_runtime.py` and `scripts/tests/test_p8_runtime.py` in round 2. No Git commit was made by this agent. `author-source-binding.json` binds the source handed to cross-review; `author-source/` contains those exact two files.

These are driver correctness and retention controls. The child programs in these reproductions are explicitly synthetic stdio/oracle/statistics executables, and build admission is mocked. They are not real Cargo build receipts, measured product performance, an hour-long soak, or completion of original P8-007 through P8-010. Original thresholds, workload sizes, mutation cycle, parity oracle and formal TODO status are unchanged.

## Finding R1 — P2: completed unequal parity was classified as a driver error

Original source: `scripts/p8_runtime.py:441`, `:481-483`; requirement: `docs/P8_RUNTIME_EVIDENCE.md:24` reserves exit 1 for a failed actual gate and exit 2 for invalid/driver evidence. Original P8-010 (`docs/roadmap/code-index-v2/05-TODO.md:2403-2411`) requires endpoint full consistency and retaining explicit failure status.

`oracle-mismatch-before-v2/run/report.json` is the valid pre-fix control: all 60 offered stdio operations reached successful terminal rows; the actual controlled oracle process wrote `parity.json` with `equal=false` and exit 1. The driver reported exit 2 with no outcome/latency summary and no statistics output. `oracle-mismatch-before/` is an earlier invalid test-fixture attempt containing a Python syntax error and must not be cited as parity evidence.

`oracle-mismatch-after/run/report.json` retains failed/exit 1, 60 operation outcomes, 40 read and 20 build observations, original parity bytes, and two identical statistics replays. The source fix treats oracle exit 1 as a failed gate while maintaining exit 2 for oracle execution failure. Three driver regressions exercise oracle exits 0, 1 and 2 through actual subprocess/stdio/replay/seal paths.

## Finding R2 — P1: scheduler/journal failure sealed while writers remained active

Original source: `scripts/p8_runtime.py:410-411`, `:484-510`; requirement: `docs/P8_RUNTIME_EVIDENCE.md:24,34` requires canceled unstarted work, bounded cleanup, and sealing only after owned work has stopped. This directly affects P8-007 timeout/denominator visibility and P8-010 end-state integrity.

`cleanup_control.py` reproduces an exceptional offer-loop exit at the first queue rejection. It preserves actual stdio subprocesses, delays client processing of already received responses, and injects the existing raw-budget exception. One writer has performed its mutation and waits for client completion; another waits on the write lock.

`cleanup-before/control-result.json`: 132 submitted futures, 4 still running when `run()` returned, and `temporary.py` was created afterward by the waiting writer. The driver's own later mutation invalidated its completed seal. The original raw journal contained no operation terminal rows. The seal in this before directory is intentionally invalid now; do not reseal or reinterpret it.

`cleanup-after/control-result.json`: 132 submitted futures, 0 still running at return, no later `temporary.py`, 132 retained raw operation rows, and the original seal stays valid.

The fix sets cancellation before every executor exit, cancels and records unstarted futures, terminates the owned child and waits for running work within the same original 70-second bound. The original offer-drain deadline stays 180 seconds. Slot release now runs even if raw emission fails. If the raw sink remains exhausted, missing terminal metadata is separately retained as `terminal-retention-failures.json` and never substituted into scored raw. If owned work still cannot stop, the failed report explicitly withholds a completed archive seal.

The added budget regression exercises the actual `MAX_RAW_BYTES` guard, including subsequent canceled-row writes failing against the same exhausted sink. It verifies all 133 actually offered identities survive in the explicit fallback, work stops before sealing, and no statistics are invented. A separate controlled original-deadline regression verifies all 300 planned/terminal identities, cancellation records, and unchanged 180/70-second waits.

## Validation and review handoff

- `python3 -m unittest discover -s scripts/tests -p test_p8_runtime.py -v`: 13/13 passed (8 original plus 5 new controls).
- `python3 -m unittest discover -s scripts/tests -p 'test_p8_runtime*.py' -v`: 28/28 passed, 6.172 seconds as reported by unittest.
- `git diff --check`: passed at author handoff.
- The full Cargo product build and real long-duration runtime certification were not run by this agent.
- Root's backfill `--no-default-features` fix was independently inspected: it aligns Cargo invocation with the unchanged strict `features == ["semantic"]` check, and the new negative control still rejects `["default", "semantic"]`.

Root assigned independent cross-review to acceptance_review. Implementation is paused pending that feedback. Reverting the two runtime files together rolls back these driver repairs without changing product behavior, and the original red/green control directories should remain retained.
