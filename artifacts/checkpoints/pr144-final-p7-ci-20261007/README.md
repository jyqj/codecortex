# PR #144 final PR and main P7 engineering evidence

Both recorded P7 engineering jobs completed successfully. Each job actually ran
the original 58-function deadline/cache matrix exactly once per identity and
passed all 58. Each complete selected job had **95 Rust + 15 Python passes,
zero failures and one ignored Rust smoke**. These are two executions of the
same selected cases, not 220 distinct tests.

| Execution | Run | Job | API conclusion | Original 58 | Full selected result |
|---|---|---:|---|---|---|
| Final PR #144 | [37657882000](https://github.com/jyqj/codecortex/actions/runs/37657882000) | 112917552850 | Success; all 14 steps | 58 / 0 / 0 | 110 passed; 1 ignored |
| Main after merge | [37661087236](https://github.com/jyqj/codecortex/actions/runs/37661087236) | 112928487184 | Success; all 14 steps | 58 / 0 / 0 | 110 passed; 1 ignored |

Both are attempt 1 of their own workflow run. The reviewer fetched their terminal
run/job metadata, decoded complete job logs and original uploaded ZIPs, then
independently parsed each run. No test, product or GitHub workflow was rerun by
this review. Execution belongs to GitHub Actions; these two reviews and their
storage verification belong to `/root/ci_evidence`.

## Source identity

| Property | Final PR job | Main job |
|---|---|---|
| Audited head | `eb7cdc55aa94c8d6865bed14fa37fff08080af33` | `b951f27d3ed50b7755bc2456c6425355f753ec17` |
| Actual checkout | `389bcf601b6c25dd3a299a94a87f1c71c2481351` | `b951f27d3ed50b7755bc2456c6425355f753ec17` |
| Checkout kind | PR test merge | Main commit |
| Original ZIP artifact | `11499237680`, 217,779 bytes | `11502030128`, 217,543 bytes |
| Uploaded ZIP members | 26 | 26 |

All three heads/checkouts have full tree
`25558c46010e085d13f134f6ff623e7ce285cbd3`. Each job's source-before and source-after
files are byte-identical within that job. Direct `git ls-tree` and
`git cat-file --batch` hashing independently matched every uploaded crate/Cargo
path against its immutable audited head: **776 inputs**, canonical map SHA256
`baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`.
The snapshots differ between the two jobs because they record different checkout
commits; the complete input maps agree.

All 13 files carrying the original 58 tests are byte-identical to the earlier
executed source `5af7ac0089ee7522e78ff2ce2468f881c8cf70f2`. Actual command logs retain
the original libtest scheduling: no `--test-threads` override or
`RUST_TEST_THREADS` override. The numeric scheduler width was not separately
measured. The expected identities come from the preserved original execution
receipt; observed identities, individual result lines and per-target summaries
were independently reconciled for each new run.

## Observed scope, per job

| Selected group | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Coverage, configuration, hard scope and exact-oracle controls | 14 | 0 | 0 |
| Original integration deadline/cache/public matrix, 11 targets | 40 | 0 | 0 |
| Original query-encoding unit controls | 13 | 0 | 0 |
| Original execution-deadline unit controls | 5 | 0 | 0 |
| Python input-lock controls | 15 | 0 | 0 |
| GC unlink and existing GC controls | 13 | 0 | 0 |
| Worker, lifecycle and strategy protocol controls | 10 | 0 | 1 |

Each job preserves six actual public deadline/cancellation scenarios, 384 worker
request rows and six lifecycle scenario receipts. They are observations inside
the test functions and are not added to test counts. Old held-input publications
were zero for each of seeds 7, 19 and 43 in both runs. Resource attribution remains
unknown and these observations do not accept the full worker performance gate.

The previously failing public deadline recovery function passed in both jobs.
The historical normal-schedule 57/1 result and separate serial diagnostic 2/0
remain unchanged. These passing combined-source runs do not establish the root
cause of that earlier failure. The 58 cases include unit, service and actual
product stdio with owned loopback HTTP; they are not all live-provider cases.

The ignored `actual_stdio_local_and_auto_have_equal_effective_budget_and_verified_source`
was not executed. The separate actual three-policy stdio target was not selected.
Neither job produced a standalone product-binary SHA256 receipt or archived its
binaries. No such receipt is inferred from source snapshots or command logs.
Python sources belong to the fixed full tree, outside the 776 crate/Cargo map.

## Evidence and acceptance boundaries

`pr-independent-review.json` and `main-independent-review.json` preserve the two
actual review results. `source-bindings.json` connects their exact runs, heads,
snapshots and ZIPs. `audit-method.py` is a parameterized version of the earlier
read-only method by `/root/pr_audit`; its original script SHA and the changes are
recorded in each new review. The earlier reviewer is not credited with reviewing
these later runs.

`raw-observations.tar.gz` stores 46 originals under their run-ID directories:
complete connector API responses, full decoded job logs, unchanged GitHub ZIPs,
per-member indexes, expected/actual test identities, source comparisons, command
indexes and actual audit execution receipts. ZIP members are retained once inside
their original ZIP, without repeated extracted copies. API JSON preserves the
connector response, not an assertion about the original HTTP wire bytes. The
download connector's temporary signed URL is kept outside the repository; the
sanitized download receipt, original GitHub artifact API metadata and exact ZIP
bytes remain reviewable here.

The corresponding legacy CI runs have separate evidence and conclusions. This
checkpoint does not substitute the P7 workflow for any legacy stdio gate. It does
not change task statuses or decide P7-013 acceptance. At packaging, the coordinator
reported **40 unfinished original TODOs**. No full G7, live-provider quality/cost,
dense-only counterfactual, P7-017 isolated-gate or P8 release acceptance is claimed.

## Storage verification

```sh
python3 -B artifacts/checkpoints/pr144-final-p7-ci-20261007/verify_storage.py
```

The verifier pins the original 50-entry manifest and the archive bytes before
parsing, checks all 46 archive members and four direct original files, then checks
both unchanged GitHub ZIPs against API digests and all 52 member hashes. It also
reconciles every indexed Rust/Python result with its preserved raw line, verifies
the exact original 58 identities and checks each complete before/after map against
its independently reconstructed fixed-head map. It extracts nothing to disk and
executes no payload. Git source comparisons were performed by `audit-method.py`
and remain recorded separately from storage checks. The actual verification
result and command receipt are retained alongside this README.
