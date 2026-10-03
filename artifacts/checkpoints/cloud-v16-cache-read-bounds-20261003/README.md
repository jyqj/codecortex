# ArtifactCache bounded-read fix

Parent/frozen negative evidence: PR48 `671352d175a552b2448a8c9618f94bef94d9cad6`, whose production base is PR45 `b973de5c542db2ba46ddb0798b03d67627e42275`. Only `cache.rs`'s ArtifactCache read path/helper, two dedicated new regression files and this evidence directory change. The PR48 matrix and all its evidence remain byte-identical; no other production, Cargo/configuration, existing tests or ledger edits.

## Fix and compatibility

`ArtifactCache::get` no longer uses file-length-dependent `fs::read`. It opens both halves first to preserve incomplete-object `Miss`, then reads each through a private fixed-buffer helper. Payload admission is the requested, validated `VectorSpace.dimension() * 4` bytes (dimension 1..=65536); it never uses metadata's claimed dimension to allocate. Sidecar admission is 4096 bytes: 1024 bytes for v1's fixed field names, four 64-byte hex strings and bounded integers, plus six times the frozen 512-byte model-ID bound for worst-case JSON escaping. Existing canonical `put` output, including maximum dimension, all 512 model bytes requiring six-character escapes and `i64::MIN`, round-trips unchanged. v1 writing/addressing/spec and vector bit encoding are unchanged.

The helper allocates exactly `budget + 1` bytes, repeatedly reads into the remaining slice and rejects when the sentinel fills. No stat length, metadata size hint, `read_to_end`, or unbounded EOF scan participates. This bounds even an indefinitely growing stream. It preserves interrupted-read retry and ordinary I/O errors. Oversized objects become `Corrupt`, never a truncated successful hit. Metadata is parsed only inside its byte cap; requested dimension/model and the addressing tuple are verified before payload decoding. Existing checksum, exact length and finite-float gates remain.

TOCTOU: opened handles pin files across atomic replacement, while growth of an already-open file cannot increase the allocation or consumed-byte cap. A deterministic regression starts with a real 16-byte file, extends it to 64 MiB after its first four-byte read and proves rejection after exactly 17 consumed bytes. This is a memory and read-volume guarantee, not a new mutable-filesystem snapshot protocol: bytes actually read must still pass the existing coherence/checksum gates; writes after EOF are not promised to be observed.

Compatibility boundary: canonical legal v1 writer output fits the sidecar budget. Whitespace-padded valid metadata is accepted through 4096 bytes and rejected at 4097; unbounded padding or noncanonical oversized metadata is intentionally discardable. Maximum payload (65536 f32s / 262144 bytes) stays valid. No limits or versions in frozen spec/config are changed.

## Actual before/after observations

The unchanged PR48 real exact/SQL/cache matrix was run on Rust 1.95.0 and 1.99.0 after the fix, retaining every cell. Parameters/envelopes are unchanged: N={1000,8000,16000} × C={1,4}, dim=128, batch=64, k=10, two scans, SQLite cache_size=-2048 and mmap=0. The two original negative cells still use N=1/C=1/batch=1 with a sparse 64 MiB payload or sidecar. All post-fix heap and sampled-RSS envelope booleans pass, every worker completes 2N candidate rows, normal top-k remains correct, corrupt results remain empty, and disk cache bytes/files stay unchanged. `comparison-table.txt` includes all paired cells, not selected runs.

| Original 64 MiB case | Before Rust live peak delta (both final PR48 runs) | After Rust live peak delta (both toolchains) |
| --- | ---: | ---: |
| payload | 67,114,385 bytes | 9,180 bytes |
| metadata | 67,114,118 bytes | 8,391 bytes |

The two post-fix negative scans finish in about 2 ms, leaving only one RSS baseline sample at the 5 ms nominal cadence. Their recorded RSS delta 0 is **not** a measured continuous query peak. The allocator records the complete call window independently. To obtain actual post-fix RSS observations without holding artificial memory, the separate read-soak harness performs 10,000 real public `ArtifactCache::get` calls in each fresh child for payload/metadata corruption and for a valid hit:

| Post-fix soak | Rust live peak delta | Actual workload duration range | Live RSS samples | Observed RSS delta range |
| --- | ---: | ---: | ---: | ---: |
| oversized payload | 6,211 bytes | 194–199 ms | 37–39 | 0 bytes |
| oversized metadata | 5,417 bytes | 133–141 ms | 26–28 | 0–139,264 bytes |
| valid hit | 7,091 bytes | 249–272 ms | 48–52 | 0–139,264 bytes |

All six soaks pass their fixed 256 KiB incremental Rust-allocation and 8 MiB sampled-RSS envelopes. These real read loops, the unchanged full scan matrix, and the fixed-buffer helper tests show the large allocation was removed rather than moved to a later stage. Sampling does not certify a continuous RSS upper bound. PR19's sampler is unchanged; platform remains Linux 6.18.44/x86_64, glibc 2.41, page size 4096, ticks 100, container limit 16 GiB/CPU quota 4. Largest post-fix matrix sampling gap is 154 ms, soak gaps up to 8 ms. Baselines, unavailable sample counts and gaps are retained. Rust allocation counters exclude SQLite/C allocations and retained arenas; PID RSS includes them. SQLite's 2 MiB setting remains advisory, not a hard bound.

## Regression results and provenance

- Both toolchains: semantic library **227/0**, artifact-cache integration **17/0**, real manifest/exact integration **4/0**.
- Eight new unit regressions cover maximum legal writer output, metadata exactly at/above budget, all payload truncations/one extra byte, 8 GiB sparse files, untrusted u32::MAX metadata dimension, incomplete-object Miss, infinite/short/interrupted/error streams, and deterministic file growth.
- Both toolchains: unchanged eight-cell resource matrix and new three-case actual-read soak pass their capture/envelope assertions.
- Strict 1.99 clippy for semantic library/tests and the new soak target, per-file rustfmt and whitespace checks pass.
- Initial diagnostic failure is retained: an existing test requires `payload byte length` in the overlong-payload diagnostic; only the production read diagnostic was corrected. Existing tests were not changed.
- Two initial soak fixture failures are retained: a missing spec directory in the new test's synthetic object path caused `NotFound` before measurement. The new fixture path was corrected; these attempts contribute no resource data or passing results.

`comparison.json` records before/final file hashes and verifies every frozen PR48 evidence byte. The raw unchanged matrix's literal `target_source_sha=b973de5...` is its **origin marker**, not the post-fix production identity; the post-fix cache source is identified by its hash in the comparison and this PR's exact commit SHA. This prevents relabeling frozen evidence or mistaking the marker for an attestation.

Acceptance: this specific oversized-cache allocation defect and its bounded read regression block pass. Full V16, P7-015/V20, server admission/backfill competition, C8/16/100k, other platforms, provider/network/cost and arbitrary filesystem scheduling remain outside this block. No task/ledger acceptance is changed here.

## Reproduce

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=5
cargo +1.95.0 test -p cc-semantic --lib --test artifact_cache --test manifest_exact_integration --locked --offline
export V16_RESOURCE_OUTPUT=/tmp/v16-fixed-matrix.json
cargo +1.95.0 test -p cc-eval --features semantic --test v16_resource_bounds --locked --offline -- --ignored --exact measured_exact_resource_submatrix --nocapture
export V16_SOAK_OUTPUT=/tmp/v16-fixed-soak.json
cargo +1.95.0 test -p cc-eval --features semantic --test v16_cache_read_soak --locked --offline -- --ignored --exact measured_bounded_cache_read_soak --nocapture
```

Repeat with `+1.99.0`. The explicitly invoked measurements use bounded child lifetimes and cleanup, only synthetic files owned by the fixture, and no provider calls. Verify all retained comparisons with `python3 artifacts/checkpoints/cloud-v16-cache-read-bounds-20261003/compare.py`; do not regenerate the frozen PR48 summary under changed production source.
