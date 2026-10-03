# Independent ArtifactCache bounded-allocation review

Fixed candidate PR50: `db9841ec13f50e5932f469200019d04d7f3b5610`.
Frozen negative source PR48: `671352d175a552b2448a8c9618f94bef94d9cad6`.
Only a new independent integration test, replay runner and this evidence directory change. Production cache, frozen resource worker, existing author tests, Cargo and ledger are untouched. All inputs are synthetic temporary files; no service/provider/network call.

## Frozen negatives independently reproduced before and after

The exact unchanged PR48 `resource_worker` source was built at each SHA, with each executable snapshotted before changing checkout. The new parent runner invokes only the two original negative cells: N=1, C=1, dim128, batch1, k10, two exact scans, one sparse 64 MiB payload or sidecar. It leaves the original worker assertions/allocator/SQL/cache/scan logic unchanged. Each scans exactly two rows, returns no corrupt candidate, and preserves cache disk bytes/file count.

| Original negative cell | Before Rust live peak delta | Fixed Rust live peak delta | Before sampled RSS delta | Fixed sampled RSS delta |
| --- | ---: | ---: | ---: | ---: |
| payload .bin | 67,114,592 B | 9,364 B | 67,444,736 B | 0 B |
| metadata .meta.json | 67,114,325 B | 8,575 B | 66,035,712 B | 0 B |

These independently measured small deltas differ slightly from the author's 9,180/8,391 bytes; fixture path/process overhead participates in the unchanged whole-worker allocation window. No assertion requires an identical incidental byte count. The fixed results meet the replay's 64 KiB regression envelope; before results exceed 64 MiB. This envelope is a test alarm, **not** a new production file admission rule.

Original worker allocation instrumentation measures live requested Rust bytes in its entire scan window. It excludes C/SQLite allocation, allocator arenas and unrelated setup before the baseline. RSS is an independent parent sample of `/proc/<child>/statm` resident pages at nominal 5 ms. Before had 21/12 samples; fixed had 2 samples each, only one after baseline. Fixed observed zero is **not a continuous peak bound** or proof of zero RSS growth. Gaps, baselines, PIDs, exact binary hashes, all four cell results and sampler method are retained in frozen-negative-replay.json. The large allocation defect is established/removed by allocator evidence, not by assuming sparse files consume no RAM or relying on RSS samples.

## Seven independently authored public-cache regressions

These call real public ArtifactCache::put/get; no private production helper, author fixture or cache priming shortcut is imported. A per-thread global allocator measures every Rust alloc/alloc_zeroed/realloc request within the complete get call (plus a separate invalid-constructor window), reporting largest request and sum of requested bytes. These are **not live peaks**; other fixture threads, C allocations and setup/put are excluded. The recorded sums can exceed live memory because sequential/reallocated buffers are counted repeatedly.

1. Legal extremes: dimensions1 and65536, exact 512-byte model with worst six-byte JSON escapes, i64::MIN and bit-preserving negative-zero/1.25 vectors round-trip. The maximum payload is262144 B; largest actual request is262145 B and request sum542253 B. Metadata with canonical maximum field lengths is3477 B (below4096). Valid JSON whitespace padding through4096 B remains a Hit;4097 is Corrupt. Over-cap noncanonical/padded sidecars are intentionally discardable; this is not arbitrary rejection of canonical legal writer output.
2. Metadata dimension0/65537/u32::MAX/u64::MAX cannot become an allocation size. Invalid requested dimensions are rejected at the only public VectorSpace constructor (VectorSpace is sealed and not Deserialize). Eight-GiB sparse payload/sidecar corruption has maximum observed request4097 B. For every payload length0..17, a freshly recomputed checksum cannot bypass exact dimension length: only16 B hits; all other lengths corrupt.
3. Missing halves remain Miss even if the other half is64 MiB; a directory payload produces an ordinary I/O error, while unreadable/unknown sidecar content produces Corrupt. No cache get writes or quarantines files.
4. A metadata FIFO pauses public get after its payload handle is actually observed open via `/proc/self/fd`. In-place extension of that regular payload to8 GiB then releases valid metadata: get returns Corrupt without a large allocation. Atomic pathname replacement with an8 GiB inode instead yields a valid Hit from the already-open original inode, proving handle pinning. Next-read/path snapshot coherence is not claimed.
5. A growing sidecar FIFO supplies4097 B and keeps its writer open: public get returns Corrupt without awaiting EOF, maximum allocation4097 B.
6. A growing payload FIFO first supplies4 B; FIONREAD confirms public get consumed the initial short read. Fourteen further bytes arrive while get is in its payload loop, with the writer kept open. Get rejects for exceeding the payload byte cap, leaves exactly1 of18 bytes unread, and thus consumes exactly dimension-bytes16 + probe1 without waiting for EOF. This is actual public read-loop growth; regular-file growth after a partially completed syscall is not separately intercepted.
7. Parser/diagnostic allocations are separately bounded by the already capped metadata input. A4095-byte unknown JSON field name is Corrupt but produces largest request8516 B / total37396 B; a3500-byte invalid model string produces4097 B / total11672 B. The test uses a conservative four-times-format-cap allocation alarm for diagnostic overhead. **budget+1 describes the read buffer, not every allocation in get.** No arbitrary-file-length-dependent allocation reappears; no claim that the author negative-cell numbers upper-bound all corrupt JSON shapes.

All seven pass (7/0/0), targeted clippy `-D warnings` passes, and the new file's rustfmt check passes. Exact allocation observations are in independent-tests.log and allocation-observations.json. Linux FIFO tests are explicitly platform-scoped and use two-second fail bounds; ordinary functional tests remain portable.

## Static correctness boundary

Payload admission comes from validated VectorSpace.dimension() in1..=65536, not mutable sidecar dimension or filesystem length. The derived maximum262144 B +1 is representable on supported32/64-bit usize; no attacker-supplied addition/multiplication participates. Metadata admission4096 B derives from six times the512-byte model limit plus1024 fixed-format overhead, covering four sealed64-hex digest/checksum strings, fixed keys and bounded numbers. The independent legal-extreme test exercises that real public writer contract. Raw digest constructors are crate-internal.

The bounded helper allocates its fixed buffer once and reads only into the remaining slice, limiting bytes consumed even under growth and short reads. No stat length/read_to_end drives allocation. Both files are opened before consumption; original missing/error handling and checksum/exact-length/finite-float validation remain. Path replacement cannot enlarge an open inode's buffer. This guarantees bounded memory/read volume, not an immutable filesystem snapshot: mutation after an accepted EOF is not promised to be noticed, and reads must satisfy existing validation rather than a new snapshot protocol.

No current production counterexample found in these boundaries. Downstream JSON diagnostics disprove only an overbroad interpretation of “every get allocation is budget+1”, not the P1 file-length allocation fix.

## Reproduction and provenance

Rust1.95.0, x86_64 Linux. PATH=/workspace/.cargo/bin:$PATH, RUSTUP_HOME=/workspace/.rustup, CARGO_HOME=/workspace/.cargo, CARGO_TARGET_DIR=/tmp/p7-017-build, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0.

```sh
cargo test -p cc-semantic --locked --offline --test p7_cache_allocation_independent_review -- --nocapture
cargo clippy -p cc-semantic --locked --offline --test p7_cache_allocation_independent_review -- -D warnings
rustfmt --edition 2021 --check crates/cc-semantic/tests/p7_cache_allocation_independent_review.rs
```

For frozen negatives, build `cargo test -p cc-eval --features semantic --locked --offline --test v16_resource_bounds --no-run --message-format=json-render-diagnostics` at each exact source; copy its compiler-artifact executable to a distinct path before changing checkout, then:

```sh
python3 scripts/review/p7_cache_frozen_negative_replay.py --before /tmp/p7-cache-review-binaries/before --fixed /tmp/p7-cache-review-binaries/fixed --output /tmp/cache-negative-replay.json
```

build-receipts.json binds both compiler artifacts, sources, cache/worker fingerprints and binary hashes. The full eight-cell N/C matrix, read-soak, other toolchain/platform, resource contention, whole-suite/CI and exhaustive filesystem schedules were not rerun. No P7-015/V20 or full V16 acceptance claim.

Ledger suggestion for integrator: add bounded independent confirmation of the specific P1 oversized-read fix, frozen before/after measurements and legal/TOCTOU regressions; distinguish buffer/request/live-peak/RSS measures and preserve the diagnostic-allocation observation. Keep unrelated/full resource gates open. This review does not edit shared acceptance state.
