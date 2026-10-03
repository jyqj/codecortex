# PR127 independent bounded facade review

Reviewed 2026-10-03 in a fresh cloud workspace. Fixed head:
`9f47a21a75a7f997c83a58020919a4eedfb85b3b`; comparison base PR123:
`513a98c9a94b15ec77153df41af26fa3c8c0b5e8`; actual production reference:
`11f5b76273a16b520e6b6e01ad32a0a6743173fb`.

## Conclusion

No newly introduced CAS, epoch, retry-idempotence, or lifecycle ordering defect
was reproduced under the tested recoverable failures. The facade delegates to
the frozen CAS on one receiver connection, validates the group before locking,
uses one transaction and lifecycle permit, and returns outcomes in request order.
This is bounded evidence for the opt-in DB facade, not a production integration
approval. Queue, wiring, cache verification, providers, runtime suites, 100k runs,
performance, crash durability and real storage failure behavior were not tested.

One **conditional recovery finding** remains: a failed rollback can leave the
shared connection in an open transaction holding the SQLite write lock. This is
also the existing single API's pattern; it is not presented as a regression in
that frozen API, nor as a demonstrated naturally occurring disk failure.

## Finding R1 — rollback failure does not invalidate the shared connection

Location at fixed head: `crates/cc-db/src/semantic_publish.rs:448`, also 454/464;
the closed-lifecycle branch at 440 reports rollback failure but likewise leaves
connection state unresolved.

The facade discards rollback errors in its CAS, epoch and COMMIT error paths,
then returns the original error and releases the Rust mutex/permit. Releasing
those guards does not end the SQLite transaction. There is no autocommit check,
connection quarantine or recovery before the next `BEGIN IMMEDIATE`.

Independent reproduction: install SQLite's authorizer on a **temporary fixture**,
deny only `SQLITE_TRANSACTION / ROLLBACK`, then invoke the normal public group
API. Separately exercise closed lifecycle, second-item manifest failure,
second-item epoch failure and deferred-FK COMMIT failure. Remove the authorizer
before the next call. In all four cases:

- `Connection::is_autocommit()` is false after the returned error;
- the next normal public group fails with `cannot start a transaction within a transaction`;
- a separate connection cannot obtain `BEGIN IMMEDIATE`;
- explicit test-only rollback restores normal writes and lock acquisition.

This is an approved controlled SQLite test seam, not product fault injection or
a real filesystem/provider experiment. It proves what happens **if rollback is
refused and the transaction remains open**. It does not establish the frequency
or specific SQLite I/O codes that could produce that condition in deployment.
The public documentation correctly promises an *attempt* to roll back, so this
finding is a recovery limitation rather than a claim that its documented promise
of guaranteed rollback was broken.

Suggested follow-up: preserve cleanup failure context, inspect autocommit state
and quarantine/recover the connection when cleanup cannot end the transaction.
Do not infer that an ambiguous COMMIT failed to take effect, and do not blindly
replay prepared requests. This review does not modify production handling.

## Independent evidence

New source: `crates/cc-db/src/semantic_publish_independent_review.rs`.
Eight independent test functions, with case loops, passed. Author fixtures were
not imported. All databases are newly created temporary files; fixture SQL seeds
source rows, and public outbox enqueue/claim and publication APIs drive behavior.
The existing private after-item seam is used only for the lifecycle barrier;
private connection access is used for health inspection and controlled authorizer
or busy-timeout setup. The added module registration is `#[cfg(test)]` only.

| Independent oracle | Result |
| --- | --- |
| Oversize, repeated task/token/document and mixed incarnation | InvalidParams before lifecycle access; full affected-table snapshots unchanged; next normal metadata write and separate writer lock healthy |
| Empty group while lifecycle permit is already held | `Some([])` without trying to enter lifecycle |
| Four-item accepted/rejected/changed/identical-content mix | Ordered outcomes; exact document/artifact/incarnation content; duplicate timestamp unchanged; epoch +2 with other clocks unchanged |
| Repeat the same completed/rejected requests three times | LeaseLost outcomes; all outbox/manifest/metadata columns unchanged; no second attempt charge or epoch bump |
| Live authority changed after constructing requests | Source version/digest, active space, incarnation and token rejected from current DB state |
| Expired token without reclaim | Both valid items publish; expiry alone is not a lease fence |
| Second item CAS JSON read/retry/manifest/ack/epoch errors | First item's manifest, ack and epoch rolled back; all-column snapshots preserved; normal write, separate lock and fresh publication succeed after removing fault |
| Second item invalid timestamp | InvalidParams after transaction entry; first item rolled back; next publication healthy |
| Deferred-FK COMMIT failure | Error explicitly says uncertain/reconcile; observed rollback for this particular injected failure; next publication healthy after removing fault |
| BEGIN busy, closed lifecycle and poisoned lifecycle | No leaked transaction in these cases; next ordinary DB operation healthy |
| Close while facade is blocked in SQLite BEGIN | Close returns with independent SQLite writer still held; group subsequently returns None with zero writes |
| Barrier after final item, success and failing COMMIT variants | Reader sees pre-group snapshot; permit still held; close waits; success exposes both rows and epoch +2; controlled failure is cleaned up before close returns |
| Rollback refusal in four branches | Conditional R1 reproduced; next normal operation remains blocked until explicit test cleanup |

The snapshot oracle serializes **all columns** of semantic_outbox,
semantic_manifest and metadata, not just counts. Health checks verify autocommit,
a public metadata write/read and separate SQLite write-lock acquisition. Error
cases also retry fresh valid publication after removing their fault. Normal
rejections are committed token-fenced retry state, not SQL errors.

Original CAS and single implementation were compared byte-for-byte to PR123.
The production-reference versus PR123 delta consists of checkpoint evidence and
roadmap metadata; this review does not treat PR123 as the actual deployed head.
The review branch changes only new tests, evidence/report and test-only module
registration. No product implementation, lockfile or dependency feature changes.

## Reproduction and toolchain

Official direct toolchain binary: `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc`:
`rustc 1.95.0 (59807616e 2026-04-14)`.
Direct toolchain `cargo`, original `Cargo.lock`, `--locked`, normal dependency
fetch. Original lock SHA256:
`ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`.

```sh
export PATH=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$PATH
export CARGO_HOME=/workspace/.cargo
cargo test --locked -p cc-db --lib semantic_publish::independent_review -- --test-threads=1
cargo test --locked -p cc-db --test semantic_publish
```

Recorded logs: `pr127-independent-evidence/independent.log` and
`pr127-independent-evidence/single-regression.log`.
The single-publication integration regression has 11 tests; no broad runtime
suite was run. New review source passed rustfmt and the review diff passed
`git diff --check`.

Initial local test construction needed a Rust scoped-closure capture correction
and a semantic-space fixture state correction (`revoked`, not `retired`). These
were review-test mistakes corrected before the final successful run, not product
findings.

CI is **not claimed green**. Parent supplied the fixed-head CI result: security
and MSRV succeeded, Clippy failed on the author's snapshot return tuple
`type_complexity`; author is preparing a test-only refactor. This review remains
pinned to 9f47a21 and does not review that later head.

## Delivery limitation

`gh pr view 127 --json headRefName,headRefOid,baseRefName,url` returned
`Post https://api.github.com/graphql: Forbidden`. That API operation was stopped;
no escalation, credential workaround or repeated forbidden API call was attempted.
A ready draft PR body is saved alongside this report. Git fetch/read operations
succeeded. A successful branch push, if separately confirmed in the final
receipt, is not a successful draft PR creation. No merge or deployment requested
or performed.
