# Round 3 runtime exit-path corrections

Independent reviewer acceptance_review supplied the exact pre-fix cases in `/workspace/scratch/28fef0db5e01/acceptance-review/reproduce_runtime_exit_gaps.py` and `runtime-exit-negative-results.json`.

## R3: failed submit loses an already offered terminal identity

A pool failure on the fifth submit before enqueue left terminal IDs 0 through 3 but omitted the already offered ID 4. The failed report had 4 terminal rows. The correction records the fifth request as the existing `status="error"` with explicit `reason="submission_error"`, its original error and offer/finish times, without claiming execution or a response. The acquired slot is released even if journaling fails. The run remains failed/exit 2 and does not enter statistics replay. Unoffered planned requests are not synthesized.

A small per-submission admission Event prevents a worker from entering the operation until submit returned and the future was recorded. If submit throws after internal enqueue, the gate is still released in finally with accepted=false; that wrapper cannot write a second terminal row, release the slot twice, or mutate source. Rejected submissions already recorded by the scheduler are skipped when later canceling a registered future. The new regression covers both before-enqueue and after-enqueue exceptions and directly verifies restored semaphore capacity.

## R4: KeyboardInterrupt loses its cause and can seal a running report

The independent `keyboard_interrupt_after_drain` case waited for all offered work to finish, then raised KeyboardInterrupt from wait(180). The old except(Exception) did not catch it, so report.json retained status=running, error=null while its seal passed. With pending work, the original interrupt cause was similarly absent and only a secondary cleanup error might turn the report failed.

The correction catches KeyboardInterrupt alongside ordinary runtime exceptions and records failed/exit 2, interrupted=true, the original KeyboardInterrupt message and the actual terminal count before final cleanup/sealing. It does not emit successful statistics or expand the denominator beyond actual offers. The new regression exercises interruption both before and after drain, checks all real offered IDs, all returned futures finished, a retained failed report and a valid final seal.

## Validation

`round3-runtime-tests.json`, `.stdout`, and `.stderr` retain the actual execution receipt and complete output. The command ran 15 tests successfully, including the prior 13 and the two new tests (four boundary subcases). Unittest reported 3.633 seconds. All created fixtures were restricted to one command's private /dev/shm TMPDIR because the shared workspace disk was full; no test inputs or gates were relaxed. Source and final small receipts remain in the workspace.

`round3-author-source-binding.json` and `round3-author-source/` retain exact handoff bytes. The prior author snapshot remains unchanged. Root and acceptance_review have been notified that the source is frozen for independent recheck; no Git commit was made by this agent.
