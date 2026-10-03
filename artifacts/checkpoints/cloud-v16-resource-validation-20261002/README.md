# V16 measured exact-scan resource submatrix

Target production source: PR45 `b973de5c542db2ba46ddb0798b03d67627e42275`. Only the new `crates/cc-eval/tests/v16_resource_bounds.rs` and this dedicated evidence directory change. No production, Cargo, configuration, existing test, tasks/TODO or ledger edits.

**Resource acceptance remains NOT_PASSED:** the real artifact-cache read path allocates the full corrupted object before validating it. Even with N=1, C=1, batch=1 and dimension=128 (512 expected payload bytes), a 64 MiB payload or metadata causes about 64 MiB of additional live Rust allocation. Both cases correctly degrade to an empty result, but corruption detection does not protect memory. Normal-source six-cell matrices stay within the preregistered measurement envelopes. Neither result closes full V16 or P7-015/V20.

## Authority and production paths

- `docs/roadmap/code-index-v2/06-VALIDATION.md:38`: V16 includes bounded memory alongside cosine/top-k/ties/filter/delete/space isolation. The owner handles the other functional oracle obligations.
- `artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md:534-554`: bounded candidate batch and size-k selection, with `O(batch_rows·dim + k)` peak-memory intent.
- `docs/roadmap/code-index-v2/tasks.json`, task P7-015: partial-backfill query/write/delete/model-switch competition and queue/CPU/DB observations; validation IDs V14/V17/V20. These workloads are **not_run** here.
- PR19's `crates/cc-eval/src/benchmark/sampler.rs` is byte-identical at the target (SHA256 `f9112d47c1efc3cc7b421459206106a38485ea8ebc45b060633595f831a5c06d`). The parent uses its real Linux `process_snapshot(child_pid)` API.
- Search uses production `cc_semantic::vector::exact::search`, `space_manifest_reads`, `SemanticManifestReads::scan_space`, and `ArtifactCache::get`; no copied search algorithm, in-memory candidate substitute, fake provider or network call. The new candidate observer only delegates and counts actual returned rows/batch size. This is the exact-backend production seam, not public MCP/server admission or an entire foreground/background lifecycle.

## Fixed matrix and measurement

Healthy cells: N={1000,8000,16000} × C={1,4}, dimension=128, candidate batch=64, k=10, two scans per worker. Seeded schema-22 disk SQLite database has the full foreign-key chain and N distinct addressed artifact objects (2N files). Every vector is all ones, and the query is all ones, giving deterministic tied top-k by doc-key; no O(N) reference corpus is retained in the query process. Each worker actually returns the first ten keys with cosine 1 and scans exactly 2N rows. Scope is unconstrained for this memory observation; functional scope acceptance belongs to the owner.

Each cell runs in a fresh child process. Seeding completes, its connection drops, and the child waits. The parent reads that PID's RSS baseline before releasing the query gate. C workers synchronize at a barrier; each opens one real DB connection with `cache_size=-2048` and mmap=0. Two iterations exercise first and repeated reads after seeding; **OS/cache coldness is not certified**. Seed writes warm filesystem pages. Setup time and seed allocation are excluded from the query live-allocation window, while retained process pages remain explicitly in the RSS baseline.

Two independently reported measurements:

1. A forwarding `System` global allocator measures actual requested live Rust bytes, including query-thread/harness overhead. It excludes C/SQLite allocations, allocator retained arenas and stack pages. The fixed incremental envelope is `4 MiB + C·(64·2048 + 10·128 + 128·8 + 1 MiB)`; the negative cases use the same conservative batch-64 envelope despite their batch=1.
2. PR19 sampler measures child PID RSS, including SQLite/allocator pages, nominally every 5 ms. Fixed incremental envelope is `32 MiB + C·4 MiB` (36/48 MiB). These envelopes were fixed before the initial run and were not adjusted to observations. They are harness budgets for these cells, not a product SLA or an asymptotic proof. Baseline, sampled maximum, delta, sample count, unavailable count and actual maximum sampling gap are retained per cell. RSS peaks between samples can be missed; `/proc/PID/stat` RSS is a kernel estimate, not a precise census or a process-tree/cgroup measurement. Missing snapshots remain counted, never replaced by zero.

Final runs additionally call SQLite's real `sqlite3_db_status` for cache-used (post-scan current bytes), cache hits and misses. These are observations, not an asserted constant. `cache_size` is a configuration/advisory setting: C4 post-scan cache use can exceed 2 MiB substantially. The receipt must not be read as enforcing a hard 2 MiB SQLite limit. Disk artifact file/byte counts are recorded before/after and remain identical; disk cache capacity/GC is not an in-memory vector LRU and is not certified here.

## Minimal defect reproduction and handoff

The first two matrix cells need only one published document and one normal 128-dimensional cache object. The fixture then extends either `.bin` or `.meta.json` to 64 MiB using `set_len` (no 64 MiB seed allocation), keeping the old sidecar/ref. Production exact search loads that one row with batch=1 and calls the production cache read. At `crates/cc-semantic/src/cache.rs:279`, both files are fully read by `std::fs::read` before metadata parsing, dimension/length/checksum checks. The two files are allocated together before validation; `batch_rows` cannot cap an individual corrupted file.

Handoff to owner: bound payload and metadata reads before allocating them, relative to the frozen dimension and a defined metadata limit; a size check alone also needs to account for mutation between check/read. Retain the existing corruption-degradation behavior. This branch deliberately contains evidence, not the production fix. Rerun the negative cells against the owner's fix; their envelope booleans should become true. No new corruption ignores or weakened bounds are used.

## Retained runs and limits

- `measurements-msrv-initial.json` / `run-msrv-initial.log`: initial Rust 1.95.0 matrix before SQLite counters were added. Its distinct harness SHA is recorded in `initial-harness-sha256.json`.
- `measurements-msrv.json` / `run-msrv.log`: final harness on Rust 1.95.0.
- `measurements-199.json` / `run-199.log`: same final harness on Rust 1.99.0.
- `summary.json` and `summary-table.txt`: verified counts/hashes and all retained readings; no best-of selection. An explicit measurement test can exit successfully while recording the two known out-of-envelope corruption cases: test success means healthy cells and capture assertions passed, **not** bounded-resource acceptance.

Platform: Linux 6.18.44, x86_64, glibc 2.41, 4096-byte pages, 100 Hz clock tick. Container memory limit 16 GiB; CPU quota 4 CPUs. Debug/test builds, not release latency/performance certification. C1/4 caller threads are measured; admission, backfill contention, C8/16, 100k scale, other dimensions/batches, memory-mapped caches, macOS, query-vector LRU, provider/network/cost, full CPU/DB-contention and stale publication matrices remain not_run. RSS/budget observations cannot be extrapolated as a guarantee for those workloads.

Largest actual sampling gap is 173 ms across all retained runs (120 ms on the final 1.99 run), despite the nominal 5 ms cadence. See `summary-table.txt` for every cell. Healthy final MSRV RSS deltas reach 42.754 MiB at 16k/C4; final 1.99 reaches 33.836 MiB there. Final 1.99 corruption cases show 64.309 MiB payload and 64.000 MiB metadata RSS deltas. These differences are retained and do not support selecting a favorable run. Final strict 1.99 clippy, rustfmt and whitespace checks pass; `verification.json` records the bounded validation status and confirms production/Cargo bytes match the target.

## Reproduce

From the repository root at the PR branch:

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=5
export V16_RESOURCE_OUTPUT=/tmp/v16-measurements.json
cargo +1.95.0 test -p cc-eval --features semantic --test v16_resource_bounds --locked --offline -- --ignored --exact measured_exact_resource_submatrix --nocapture
```

Use `+1.99.0` for the second toolchain. The resource test is explicitly ignored by default because it requires an output destination and real controlled measurements; default CI makes no resource-run claim. The worker entrypoint without its parent-supplied environment does no work. Evidence reproduction should use the explicit command above. Parent enforces a 180-second per-child limit and kills/reaps on timeout; each child gives its parent 20 seconds to start sampling. Temp DB/cache objects are removed after each child exits.

Verify the retained evidence without running a workload:

```sh
python3 artifacts/checkpoints/cloud-v16-resource-validation-20261002/summarize.py
```
