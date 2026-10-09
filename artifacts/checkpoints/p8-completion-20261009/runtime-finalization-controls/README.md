# Runtime finalization Python control plan

This is a proposed auxiliary workflow based on immutable source `8e542e644b06a83fc82d18d9896cf82f5b2d796e`. It has not been published or executed. No test, Cargo command, product workload, release approval, benchmark result, or TODO completion is claimed by this package.

The only trigger is a push to `task/p8-runtime-finalization-controls-20261009`. One Ubuntu 24.04 job has a 20 minute deadline, read-only contents permission, and checks out the actual `github.sha`. The workflow requires attempt 1. It retains the original four complete test modules without changing any existing test or assertion:

| Module | Test methods | Explicit subTest callbacks |
| --- | ---: | ---: |
| test_p8_runtime | 14 | 0 |
| test_p8_runtime_evidence | 12 | 0 |
| test_p8_runtime_cache | 13 | 22 |
| test_p8_runtime_finalization | 8 | 2 |
| Total | 47 | 24 |

The two finalization subcases inject KeyboardInterrupt during sealing and verification, after the existing real-thread fixture has stopped its owned workers. The new module therefore contains eight methods and nine fixture executions. The protocol products are explicitly fake controls; they are not native product measurements. The original command-line exception-to-exit-2 mapping is not executed by these controls.

`test-expectations.json` lists every complete unittest method ID, source hash, and explicit subcase value. Callable subcase values use the pinned module, qualified name and source line, and retain their original lambda expressions. Ordinary assertion loops remain inside the unchanged method denominator. The controller loads each complete module and rejects missing or additional tests, zero matches, skipped tests, expected failures, unexpected successes, missing terminal events, or a different explicit subcase population. Passing a child process alone is insufficient.

The controller is embedded verbatim in the workflow. It is copied to a new runner-temp file, then invokes four separate Python subprocesses. `controller.py` in this evidence package is an exact review copy of that embedded body, not a second implementation. The retained plan and progress record the actual argv, Python executable/version, timestamps, exit codes, stdout and stderr. Each test module records start/stop/outcome and subcase events. Existing retained real-thread fixtures remain under the artifact output directory.

Before and after controls, the original source snapshot checks all 1,087 Cargo/crates inputs against the fixed 8e production manifest `7a561d39191708023052b22ef5d626bf26ecf4ca6042eb39dc2f50a82ceb34bf`. The original observer snapshot and fixed-input check bind the seven runtime observer files, all four test modules, manifest and actual workflow to their committed bytes. The runtime observer contains the proposed finalization repair; the other six observer files and all three original test modules remain byte-for-byte identical to 8e. The new test and workflow increase the prospective validation path count from 137 to 139 if this branch is later adopted; this package does not update source approval pins or claim that admission has occurred.

The final receipt inventories all retained control files after source and observer rechecks. A stopped child and complete matching test population are required for a successful result. An interruption with unconfirmed child termination is explicitly unsealed. The Actions artifact step uses `always()` to retain available output; a runner shutdown or job hard termination can prevent final receipt or upload, and missing evidence never passes. The finalization helper does not promise writable failure metadata after SIGKILL or an initial report-write failure. Actual CLI exit and original seal verification remain mandatory for product evidence.

Local preparation performed only Git byte reads, AST parsing, YAML parsing, exact payload comparison and static inventory checks. Neither the 47-method candidate suite nor the proposed separate old-8e negative comparison was executed locally. The workflow below schedules only the 47-method candidate population; an old-source negative run would require a separately identified execution and is not implied by this plan.

The production snapshot, old workflow files, guard logic, budgets and task ledger are unchanged. No scale, mixed, soak or recovery result is replaced. There are zero newly completed TODOs and 29 remaining.
