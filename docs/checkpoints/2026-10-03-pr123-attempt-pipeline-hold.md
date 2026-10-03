# PR123 attempt pipeline candidate — contract verified; delivery status below

The initial HOLD diagnosis below is retained as history. The parent subsequently ruled that strict clock expiry is not the existing contract; the new oracle verifies token fencing without changing DB policy.

Source: `11f5b76273a16b520e6b6e01ad32a0a6743173fb`.
Fixed starting head: `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`.
Remote: `https://github.com/jyqj/codecortex.git`.
Branch: `candidate/pr123-attempt-pipeline`.
Both supplied objects were fetched directly from origin. The starting head descends from source. PR123 metadata could not be independently verified: GitHub GraphQL returned Forbidden. No push, draft PR, merge or deployment was performed. Remote CI: not run.

## Minimal implementation

Only queue production behavior changes: the existing explicit per-project setting >= 2 opts into at most four joined local attempt workers, capped by max_batch. Settings 0/1 keep width one. Runtime claim budget remains 16 per drain, shared by its workers. Claims, tokens, pre-work renewal, space/lifecycle fences, retry backoff, cache layout, fsync/readback, publish CAS, provider configuration and shared gate are unchanged. Other code edits are narrowly scoped tests.

HTTP global 4 / per-project 2 remains separate from whole-attempt width. Two held HTTP requests can coexist with two gate waiters. Cancellation charges all four entered handlers under the existing started-attempt semantics, even when only two HTTP requests entered; this does not imply four billed requests. Retrying providers can make multiple HTTP calls per attempt under their existing cost/retry budgets.

Four simultaneous projects can have sixteen local worker threads plus their coordinators, but still at most four admitted HTTP requests globally. There is no new global project-count cap. Every worker owns at most one accepted input (frozen 1 MiB maximum) and one vector (maximum 65,536 f32 values, 256 KiB). Thus accepted payload bounds are 4 MiB text + 1 MiB vectors per project, or 16 MiB + 4 MiB across four projects. These exclude thread stacks, JSON/transport/cache copies, DB buffers and allocator overhead. The DB read deserializes record_json before validating text size, so these are not hard bounds on malformed persisted-row allocation or total process memory. No batch-sized input prefetch is introduced.

## Validation and blockers

Direct installed Rust 1.95 compiler/toolchain; original Cargo.lock, locked dependency fetch. No rustup, credentials or permissions changes. Repository/ancestor AGENTS.md files were not present.

- cc-semantic bounded_parallel: 11 passed, 1 failed before the final input-bound test was added. The failing new controlled test holds four provider calls beyond a 0.1-second lease and observes four publications where zero are required. No reclaim or fault injection is used. All budget, distinct-token, ordinary retry, space switch, document edit, cancellation and physical-join checks in this suite passed. The final added frozen-input boundary test is not run.
- cc-server semantic-http bounded_parallel tests: 3 passed, 1 intentionally ignored frozen counterexample. Actual admitted provider cap stays global 4 / project 2; runtime pins/factory survive close until physical join; default-disabled factory calls remain zero.
- Isolated production-gate harness: its four existing gate/config/disabled cases passed; combined serial queue failed publication counts. The fixture uses a default cache at CodeIndex assembly rather than the helper's explicit synthetic cache. It was not rerun. Later width-one coverage added to the harness is not run.
- Existing runtime recoverable-retry test failed because successful provider outcomes hit a read-only default cache (EROFS). This restricted case was mistakenly invoked once and was stopped without retry or relocation.
- A broader runtime command was interrupted during compilation; no broad-suite result is claimed.
- git diff whitespace check passed before final formatting; final check recorded by local commit workflow. Full workspace suite, release build, benchmark, 100k workload, independent review and remote CI: not run.

## Original HOLD diagnosis (superseded by parent contract ruling)

HOLD: publish CAS checks claimed state and lease token, but does not compare lease_expires_at with current time. Expiry alone therefore does not reject publication; expiry followed by reclaim invalidates the token. Renewal also accepts an unreclaimed token after expiry. A strict elapsed-lease fence requires the shared DB publish contract to change atomically. A queue-side time check cannot cover cache-write/publication races and is not a substitute. DB production changes are outside this delegated ownership; no workaround or weakened assertion was introduced. The deliberately failing regression remains reviewable locally. This candidate is not production-ready and claims no performance improvement.

No previous diagnostic/experiment artifacts, raw logs, source copies or restricted file sets were copied or staged.


## Parent ruling and new contract verification

Parent explicitly withdrew the stricter elapsed-lease requirement: semantic_outbox.rs lines 17–22 define task_id + token + claimed fencing; expiry enables reclaim, and token invalidation after reclaim/re-claim prevents old writes. No production DB, publisher or lease policy was changed.

Original local candidate commit: `c13e7fa9e1e485b5b9243243483dfd92d2ecdad2`. The original strict-clock test and its observed four publications remain intact. That test is now explicitly ignored as an unimplemented, inapplicable stronger policy; the prior failure is not converted into a passing result.

New independent `attempt_lease_contract.rs` oracle was authored in this task and run identically on:

- Fixed original head `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`, original width 2 (`CC_CONTRACT_EXPECT_WIDTH=2`).
- Current candidate on that same head, new width 4 (`CC_CONTRACT_EXPECT_WIDTH=4`).

Each run passes three controlled normal-API cases. Beyond the measured current lease expiry: unreclaimed tokens publish successfully (2/4); actual reclaim API reclaims exactly 2/4 and old publications return LeaseLost; after fresh claim with different tokens, old publications still return LeaseLost and replacement publications succeed (2/4). Stale renew/retry writes also return false. No clock/row mutation, DB corruption, kill, WAL or GC test is used. Only this newly written oracle file was copied to the detached original-head worktree; no previous source copies or experiment artifacts were reused.

Candidate bounded_parallel after this ruling: 12 passed, 1 ignored (the original stronger-policy assertion). Frozen 1 MiB accepted-input bound now tested. Existing HTTP/runtime results above are historical and unchanged. The EROFS target and combined serial failure were not rerun or relocated. Added width-one production combination remains not run; it cannot replace those earlier failures. Thus full production-cache acceptance remains unverified; no production-readiness or performance claim is made.

Denied GitHub action was read-only metadata lookup: `gh pr view 123 --repo jyqj/codecortex --json baseRefName,headRefName,headRefOid,state,url`; endpoint `POST https://api.github.com/graphql`, response `Forbidden`. This was not a git push or PR-create attempt, nor an automatic-approval-review rejection. The denied API was not retried and no alternate identity or API route was used. At the earlier HOLD checkpoint, push was unattempted; the authorized new-code git delivery attempt is recorded separately after execution. Draft PR creation has not been attempted because its metadata preflight is denied. Remote CI remains not run.
