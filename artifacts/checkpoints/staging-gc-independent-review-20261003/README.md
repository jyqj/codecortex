# Independent staging / GC bounded review

Target PR84 `2b64c7c8f7f2d3736d621546706ccedc5b4406dc`; original execution source `a83aa1bd4d0c7bbc6a9ce9d5d7c19a226c607bd3`. Only the exclusive new `staging_gc_independent_review` test and this evidence directory change. Harness scaffold derives from the original test, adding separate parent observations, new seeds, explicit GC ready PID verification and foreign-key checks; it does not claim a second implementation of production recovery. Corpus, original tests, production, Cargo and authoritative task ledgers stay unchanged.

## Boundary findings

- **staging-writing:** a genuine pause inside `IndexDb::build_temp_db_staging`'s callback, after projection writes and before the transaction commit. The callback asserts `!is_autocommit()`. A separate parent connection observes zero staged document rows and SQLITE_BUSY on BEGIN IMMEDIATE while the owned child remains alive, proving the writer transaction remains open. Kill/reopen verifies the staged writes were rolled back and the main published object remains intact. This is a real staging transaction boundary; the projection content is fixture SQL, not a full indexing frontend.
- **staging-built:** marker is emitted after build returns, commit, index reconstruction and pragma restoration. Parent sees the committed staged document and unchanged main manifest. This covers completed build residue, not a crash during commit or index reconstruction.
- **staging-swapped:** marker follows `swap_rebuild_staging` return. Production source removes old WAL/SHM, renames staging over main, reopens write connection and read pool, then checkpoints before returning. None of those internal windows is paused by the test. Parent sees the new document projection and no semantic manifest. Reopening then explicitly registers a semantic space and reconciles from cache. This is caller-driven recovery, not automatic product startup/ready.
- **GC:** `collected` follows `collect_candidates`; publisher claim or commit happens before `sweep-release`, and `ready` follows `sweep_batch` return. Production sweep first computes filesystem freshness, then obtains one DB mark snapshot, then unlinks. Both scenes therefore exercise collect → claim/publish → freshness/mark → complete unlink, not mark → concurrent publish → unlink. Both GC and publisher are genuinely SIGKILLed, but GC itself is killed only after the sweep has completed. Held publisher dies with an unfinished claimed task; committed publisher dies after publication.

The independent parent observes three cached candidates and no target task after collection, then observes the actual claimed/done target state before releasing sweep. After sweep, before kill, it checks the target remains a verified hit, the orphan is a miss and exactly two candidates remain. This validates actual deletion rather than relying solely on GC counters (`remove_quiet` ignores unlink errors). Final reopen checks integrity and foreign keys, preserved manifest/cache reference parity, explicit reuse, final task states and no additional GC deletion.

## Limits and cost

GC candidates use synthetic `created_at=1000`, caller clock `now_unix=5000`, retention 3600. This is deterministic aging, not a real hour-long wait. Both published cached vectors pre-exist; direct SQL fixture enqueue is not a production write/config gate. Live-task and manifest protection are tested; there is no generic pin API/contract exercised, so **generic pin protection is not claimed**.

The zero-new-provider reuse claim is bounded to direct cache/reconcile/recovery APIs, which do not receive a provider; the fixture's setup counter observes one call for staging or two for GC. It is not an instrumented production worker or real monetary ledger. PR85 remains byte-identical: its response-before-cache cases each call FakeProvider twice, and original real-provider billing after such a crash is **unknown**, never zero. No arbitrary-crash exactly-once billing conclusion is made.

Without production hook/API changes, this review does **not** cover: finalization/WAL-delete → rename, rename → connection reopening, GC freshness/mark → unlink with new concurrent publication, between bin/meta pair unlinks, generic pin, power loss, full retry/close/rebuild/delete combinations or production stdio auto-recovery. No unsupported internal-hook coverage is inferred from marker names. Source inspection supports the described order, but is not a dynamic internal-fault proof or proof that every race is safe.

P7-016 remains **prepared, not done**. P7-015 depends on incomplete P7-014; hard dependencies remain unmet. No authoritative ledger status changes or full V17 acceptance are made.

## Replay

Use a fresh output directory; existing scene directories are refused:

```sh
SG_INDEPENDENT_EVIDENCE_DIR=/tmp/staging-gc-independent-review-new CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-semantic --test staging_gc_independent_review -- --nocapture
python3 artifacts/checkpoints/staging-gc-independent-review-20261003/verify.py
```

The verifier checks recorded evidence and frozen original tests. Local synthetic DB/cache originals are retained with hashes; no database bundles, binaries, private material or provider traffic are published. Only this harness's own PID-matched child handles are signaled.
