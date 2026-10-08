# P8-008 / P8-009: actual process lifecycle and resource observations

The driver is `scripts/p8_lifecycle.py`. It reuses the existing `Product` and
`StdioRPC` implementation to launch the real product, preserve all protocol
requests/responses, and close and reap owned subprocesses. The standalone
`p8-measurements` Rust binary reuses the existing normalizer, source verifier,
latency statistics, memory ledger, disk ledger, and cost ledger. It does not
implement a second scorer or change the existing gate thresholds.

This implementation does not itself close either original TODO. Closure needs
an independently reviewed fixed candidate and complete actual run evidence,
plus the original upstream dependencies. CI execution and the original
acceptance review remain separate from the existence of this driver.

## Observed strata

The bounded synthetic input contains uniformly shaped Python functions with
distinct names distributed over a fixed number of files. It is a lifecycle
workload, not retrieval-quality or real-corpus certification.

| Stratum | Direct evidence | Default sample count |
| --- | --- | ---: |
| Cold build | Newly created source/cache root; new product process; public indexed-file count zero before the full index request; complete indexed count afterward | 30 |
| Process reopen | New product process for each first query; preexisting database; no index RPC in that process; unchanged persisted generation and complete file count | 400 |
| Warm uncached | Prior explicit warmup; unique same-shape query; exactly one observed public graph/result-cache miss | 400 |
| Cache hit | Same live process and generation; exactly one observed graph/result-cache hit on the immediately repeated query | 400 |

The cache observer uses `status(aspect=index).diagnostics.search_cache`. It
checks all four monotonic counters and the persisted generation before and
after each sequential query. Repetition, query text, a requested profile, or
originating-work cache counters cannot substitute for the observed current
lookup. Missing/reset/multiple lookups and generation changes fail the control.
The product's automatic indexing and semantic configuration must be observed
disabled in status throughout the run.

Each timed call includes the synchronous client/transport/raw-journal work.
Before/after status and resource probes are outside that call's timing. All
attempts, errors, timeouts and missing samples remain visible. OS page cache is
not cleared; a reopened process is never called cold disk. Cold-build samples
use independent empty indexes, not a best-of selection from repeated builds.

Actual query responses are passed back through the original Rust normalizer
and source-byte verifier. A successful query requires the expected declaration
in a source-verified returned span. A supplied success label with no retained
raw response fails. Source verification also cannot erase a previously observed
cache-control failure.

The original per-stratum N/distributions/p95/p99 confidence intervals are
retained. Their IID assumption and any unbounded interval remain explicit.
The 30 cold-build observations do not imply stable tail latency. The default
400 query samples allow a finite order-statistic p99 interval when the input
permits it, but do not establish independence, a baseline ratio, or a performance
improvement. Release mode refuses fewer than 30 cold or 200 per query stratum.

## Native accounting and process attribution

`cc-index::current_process_usage` now owns the native SELF implementation;
the existing eval sampler re-exports that API. Product status adds
`diagnostics.process_resources`, without changing the tool count or input schema.

The public process record reports the actual calling PID, user/system CPU in
nanoseconds, current native RSS, lifetime `getrusage` RSS high-water, and optional
Linux `/proc/self/io` counters. Current RSS and lifetime high-water are separate.
Linux KiB/macOS byte conversion is preserved. Storage `read_bytes/write_bytes`
and logical `rchar/wchar` are separate cumulative counters. Missing/invalid
readings are null; no process-tree or external-service total is inferred.

The driver binds the product-reported PID to its owned stdio subprocess. This
provides native server observations even when the runner and mounted procfs use
incompatible numeric PID views. The Python runner has its own SELF record.
Status-bracket CPU/IO differences include observation work and are not presented
as pure backend service CPU/IO.

`p8_resources.OwnedProcessProbe` separately attempts a bounded process-tree
observation. It walks only the selected owner's kernel thread-child lists,
matches namespace PID, parent, namespace inode and executable, and checks
start-time/executable identity around reads using a proc directory descriptor.
Missing children, inaccessible topology, PID reuse, exec or ambiguous identity
leave the tree unavailable. This workspace has exposed a procfs view with
missing `task/<tid>/children`; no tree total is claimed from that view.

Resources are stage snapshots. Their observed maxima can miss transient peaks,
and peaks at different times are never summed. The shared memory ledger retains
its original separate runner, server, tree, and external-service roles.

## Storage, cost and immutable outputs

Closed fixture storage is enumerated by device/inode after the SQLite
observation connections close, so one SQLite file is not counted twice as
index and FTS. The final ledger takes a fresh inventory after every product
session has ended, including any remaining SHM/WAL sidecars; earlier per-cold
snapshots remain in the raw events. Logical FTS pages from SQLite `dbstat`, when
available, are a separate breakdown within the shared database. Missing logical
tables are unavailable, not measured zero. Each retained cold fixture is reported
separately; the sum across 30 fixture indexes is not one active-index footprint.
Artifact/source/storage file lengths are reported before the final receipt is
written, with that scope explicitly recorded.

The semantic provider is disabled. Reported and estimated billing amounts and
tokens remain null because this run has no live billing receipt. The optional
network-denial wrapper uses the existing explicit product launcher path;
without it the run does not assert an independently measured zero-network trace.

Output must be a new directory. It contains the frozen plan/product witness,
all per-process raw protocol logs, every measured attempt, actual lifecycle
controls, native resource records, closed database/FTS inventories, the Rust
replay input, report and a second byte-identical replay. Failed/incomplete runs
retain partial artifacts and nonzero receipts. No artifact directory is reused.
The CLI requires the product's complete crate/Cargo source witness to match the
current commit. It also compares every reused Python observer/transport module
with that commit and checks those module hashes again after execution. The
CLI consumes the separate replay build receipt, verifies the original Cargo
completion/artifact/profile/source and log hashes, and rechecks the witness
after measurement. The original Cargo manifest/source paths must identify the
same checkout, and the recorded Cargo executable/copy source must still exist
in its selected target directory with bytes matching the retained replay binary.
Every retained raw file, process/transport log, source,
SQLite file and replay artifact is sealed by the final receipt's full inventory.
`verify` rejects changed, deleted, added or symlinked files before later review.

```sh
cargo build --locked --release -p cc-eval --bin p8-measurements
python3 scripts/p8_lifecycle.py \
  --binary /absolute/product/codecortex \
  --build-receipt /absolute/product/build-receipt.json \
  --evaluator /absolute/release/p8-measurements \
  --evaluator-build-receipt /absolute/release/replay-build-receipt.json \
  --profile release --cold-samples 30 --query-samples 400 --files 32 \
  --output-dir /absolute/new-lifecycle-run
python3 scripts/p8_lifecycle.py verify --output-dir /absolute/new-lifecycle-run
```

The dedicated `p8-lifecycle.yml` workflow checks out the exact PR head, builds
the product with an actual fixed-source release receipt, binds the actual Rust
replay artifact, then runs the profile on its own runner after compilation has
finished. PR/main/manual executions preserve all original raw results and build
logs. Workflow success means complete selected observations, not G8 approval.

## Verification entry points

```sh
python3 -m unittest discover -s scripts/tests -p test_p8_resources.py -v
python3 -m unittest discover -s scripts/tests -p test_p8_lifecycle.py -v
cargo test --offline --locked -p cc-index --lib resource_accounting
cargo test --offline --locked -p cc-eval --test p8_lifecycle_replay
```

Synthetic control tests prove source/denominator/failure preservation and
accounting behavior. They do not count as measured product resource or latency
certification. Actual smoke/release receipts retain their own product and
observer identities and must be assessed separately.
