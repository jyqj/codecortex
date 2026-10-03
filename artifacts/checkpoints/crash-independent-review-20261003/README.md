# Independent bounded review of PR81

Review target: `0807567a4faa2ccb3bd5412499d75dafaef9c51c`; original execution source: `ee46fa564714ef75b2d839e5e37364cd35fd7406`. Requests corpus remains frozen. No production code, original tests, Cargo, shared ledger, or corpus is changed. New harness scaffold is explicitly derived from the original test, with independently added observations and negative controls; it is not a second implementation of the production pipeline.

Verdict: the original twelve process kills are real owned-child SIGKILLs at the documented boundaries, useful as **prepared** evidence. Three boundaries signal only after successful claim, cache put, or publisher return. These verify recovery from completed operations, not crashes inside them. The fourth holds a manually seeded SQL transaction open before commit; it tests SQLite rollback of those writes, not an interrupted production publish/source-update transaction. The `semantic_manifest` table is untouched in that rollback setup, so its zero-row assertion is not a manifest rollback proof.

Independent extension uses three new synthetic inputs and five points. A separate parent reader checks durable outbox/manifest state before killing its own PID-matched child. The uncommitted point also proves writer-lock contention and invisibility while the transaction is still alive. Cache reads before kill verify presence only for artifact/published points. Reopening before natural lease expiry must do no reclamation, no replay, no handback and no claim-count/epoch increment. After natural expiry, recovery and worker settlement check integrity, foreign keys, one manifest, one epoch increment, and three further noop scans.

The added response-before-cache point records a successful FakeProvider return and kills before cache persistence. Recovery sees a cache miss and the worker calls FakeProvider again: **two total fake calls**. Therefore the original nine completed task scenarios' one-call property is bounded to their sampled points, not an exactly-once cost guarantee across arbitrary crashes. This is an uncovered cost window, not a contradiction of PR81's explicit limited claim. The inserted pause is a test-level API boundary corresponding to the production handler's provider-return/publisher-call gap; no instruction-level preemption or production hook was added. No real provider or monetary ledger was exercised.

Source inspection: `queue.rs::EmbedHandler::handle` obtains vectors before calling `Publisher::publish_embedding`; `publish.rs::publish_embedding` puts and verifies artifact before DB publication; `semantic_publish.rs::publish_semantic_with_lifecycle` calls publish-and-ack and bumps epoch inside BEGIN IMMEDIATE/COMMIT. This supports the atomicity design but process kills after publisher return do not independently test every internal SQL instruction. Original artifact hashing verifier passed all 56 files and the twelve recorded cases; new execution results and hashes are separate.

P7-016 remains **prepared, not done**. P7-015 is todo and depends on incomplete P7-014, so hard dependencies remain unmet. This review does not edit their authoritative statuses. Staging rename/rebuild, staging GC or GC/publish races, cache corruption/deletion, internal cache/CAS fault injection, power loss/fsync behavior, production stdio automatic reopen, and the full retry/close/rebuild/delete/lease/GC matrix were **not tested**. No full coverage or V14/V17/live quality conclusion is claimed.

Replay in a fresh output directory (existing scenarios are refused):

```sh
CRASH_INDEPENDENT_EVIDENCE_DIR=/tmp/crash-independent-review-new CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-semantic --test crash_independent_review -- --nocapture
python3 artifacts/checkpoints/crash-independent-review-20261003/verify.py
```

The verifier validates the checked-in review evidence; to review a fresh execution compare its results with the asserted matrix. Local SQLite/cache originals are retained only in the receipt's isolated directory; no binaries or database bundles are committed.
