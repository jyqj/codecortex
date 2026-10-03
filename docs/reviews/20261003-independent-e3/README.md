# Independent bounded review of e3c04fed

Outcome: **PASS for the requested fresh synthetic normal-path review; no correctness counterexample found.** This is evidence for the fixed candidate, not authority to merge, deploy, or claim real-provider production performance. Original failed/unrun targets retain their status.

Candidate: `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`. PR123 comparator: `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`. Production reference: `11f5b76273a16b520e6b6e01ad32a0a6743173fb`. The string immediately following “PR123” is the 40-character comparator SHA. Production code change is only local width two → `min(4, max_batch)` when explicit per-project configuration is at least two. Zero/one remain serial. Gate 4/2, DB, cache durability and fsync are unchanged. Other source-file diffs are tests.

## Fresh executed evidence

`fixture.rs` was authored for this review and compiled only in new source copies. It builds real Rust files with `CodeIndex`, reads real indexed document inputs, assembles `SemanticSubsystem`, uses actual `ArtifactCache` and `Publisher`, and calls the actual `SemanticRuntime::from_config` HTTP assembly. No old local experiment report or diagnostic directory supplied evidence. No fake publisher is used.

| Case | Observed and asserted |
| --- | --- |
| New width four, actual production gate | Four claimed local tasks while precisely two real loopback HTTP requests have arrived and their responses remain withheld. No third request arrives during the 300ms hold. Releasing responses completes 24 tasks, each with a verified disk-cache vector/ref and done outbox row. |
| Claim budget / local width | Explicit width config two, budget 16: local peak four, claimed exactly 16. Budget two: peak two. Default zero/one: local peak one. |
| Default width + HTTP | Separate `from_config` runtimes for zero and one each hold exactly one HTTP request and one local claim before release; all five real inputs publish and read back. Default configs inherit the already configured shared gate. |
| Two projects | Distinct cache namespaces, four claims per project (eight total), four held HTTP requests (two per project), two runtime pins; no fifth request before release. Both projects finish 12 durable publications each. |
| Cache reuse | Remove only the fresh synthetic DB's publication/outbox rows, construct another runtime over its existing DB/cache, and reconstruct all 24 rows without any further HTTP request. This tests runtime/cache reuse, not physical DB close/reopen. |
| NeedsRetry | A fresh controlled provider returns Timeout once. The real queue fences it into backoff and continues ready work: budget 16 gives one retry and 15 successful real publications, leaving nine pending rows. |
| Close/cancel / physical join | Close while two HTTP responses are withheld and four local claims exist. The runtime pin stays one immediately and after another 100ms; publication remains zero. Release responses, wait for pin zero; still zero publication, and scheduling is refused. Cancellation does not claim to interrupt a blocking read before its response/deadline. |
| Lease token/state | Set claimed lease expiry in the fresh synthetic DB to zero without reclaim: real cache-first publish succeeds. Actually reclaim another expired claim: old token is refused, a newly claimed token succeeds. No hard-clock rule is invented. |
| Source / space fences | Directly perturb the fresh indexed document version after claim, or switch the actual DB active space after claim. Both real cache-first publications are refused with no manifest row. Source perturbation isolates the CAS fence; it is not an end-to-end concurrent source edit. |

Final candidate command selects only `independent_e3_review` with `--test-threads=1 --nocapture`: **4 passed, 0 failed, 0 ignored, 350 filtered out** (three correctness tests and one synthetic performance test). The baseline selects only the exact new performance test: **1 passed, 0 failed, 0 ignored, 352 filtered out**. Original queue/runtime suites are compiled but never selected. The final original 708 files were compared byte-for-byte to the candidate; only a test-module registration was appended to copied `lib.rs`. Production checkout, CI and central TODO remain unchanged.

## Resource cost and synthetic timing

Compared with width two, a project may now retain four whole-attempt workers and up to four owned input/vector sets; at the runtime's two-project capacity this is eight scoped workers/owned input sets plus the two blocking runtime jobs. With gate 4/2, each project may have two local workers waiting on provider admission. The admission budget remains 16 claims per runtime round; it does not allocate sixteen prefetched inputs. At the existing 1 MiB accepted-input ceiling the input-byte ownership bound alone is up to 8 MiB for two projects (versus 4 MiB at old width two); this is a source-derived bound, not a measured RSS bound. JSON, HTTP response, vector/cache and other process allocations are additional. No total RSS/CPU cost claim is made.

New synthetic performance job: 64 real indexed documents per repetition, an injected 20ms provider delay, actual cache/fsync/publication/readback, three repetitions. Both jobs were dispatched concurrently with separate build targets; candidate measurements overlapped the baseline build, and this is not a paired or uncontended performance study.

| Fixed source | Elapsed ms (three reps) | Local peak |
| --- | --- | --- |
| e3c04fed | 391.184 / 606.444 / 422.249 | 4 |
| 513a98c9 | 703.299 / 706.117 / 691.905 | 2 |

These numbers demonstrate the intended synthetic scheduling shape only. The timed injected provider bypasses HTTP admission; actual HTTP correctness is tested separately above. They cannot establish a speedup for gate-limited network work, true providers, heldout inputs, or production cache workloads.

## Identity, failure history and scope limits

`evidence.json` preserves full source SHAs, fixture/lock/binary SHA-256, official installed direct compiler identity, effective features (`semantic`, `semantic-http`), profile and exact filters. Each source has its own target directory. Original Cargo.lock was retained; normal `cargo fetch --locked` filled missing registry cache entries. No rustup operation, dependency replacement, compiler substitution or permission change occurred.

Own setup history is preserved: initial source-selector concatenation/archive errors, offline dependency miss, first fixture compile failure on private `InputDigest::new`, second compile failure on absent digest deserialization/private clock API, then an interim 3/3 run and the final 4/4 run. These were fixture/setup failures, not executed product correctness failures. Local logs remain local; only this compact synthetic summary and authored fixture/runner are delivered.

Not run: the prohibited `semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts`, any previously rejected retry target, broad runtime groups, GC/WAL/kill/crash, heldout, true providers and CI. The historical EROFS result, combined serial failure, prior synthetic close timeout and original ignored stronger-clock assertion are **not** converted into passes by this review. The requested independent normal cache/publication path now has passing real-cache evidence; global production acceptance beyond that path remains unverified.

Reproduce using `bash run.sh /path/to/repository /new/scratch candidate` or `base`. The runner archives only source/manifests and attaches this module in the copy. Its shell syntax was checked; the commands above were executed directly, rather than rerunning through the packaging script. Requires the fixed commits and preauthorized normal registry cache.
