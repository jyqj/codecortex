# P8 platform and recovery execution

## Cold platform matrix (P8-012)

`.github/workflows/p8-platform.yml` executes eight independent jobs: Linux and
macOS, Rust 1.95.0 and the installed stable release, and default or `semantic`
packages. Each job fetches the locked dependencies before measurement, then
builds the release binary offline in a previously absent target directory.
No target cache is restored. Dependency downloads are not cold compilation
time; the fetch log is retained separately.

The existing cold-build receipt verifies the complete committed Cargo/crate
input inventory, compiler bytes and versions, Cargo configuration, exact target
path, requested features, non-fresh Cargo artifact, and output binary hash.
The actual new executable then performs a real stdio smoke sequence:
initialize, list all fourteen tools, full index, capabilities, local symbol
search, and a clean stdin-EOF exit. Both package variants use explicitly disabled
semantic configuration for this smoke. This is a platform/local-startup gate;
HTTP-provider recovery belongs to the separate recovery suite.

The single-cell driver intentionally exits 2 with one passed and seven
`not_run` cells. The workflow does not interpret exit 2 as success by itself.
`--export-selected` verifies the one selected cell, the seven explicit
non-selections, the live source/compiler/binary receipt, and every required
smoke RPC response before exporting the binary and raw evidence. A failed
compile or smoke cannot produce a passing cell archive.

`--collect-cells` requires all eight distinct cells from one exact source commit,
rechecks the complete artifact inventories and byte hashes, compares the source
manifests against that checkout, and replays the Cargo and stdio assertions.
Missing cells, duplicate cells, changed binaries, incorrect features, warm
artifacts, missing responses, and different source commits fail collection.
The resulting matrix receipt is build evidence, not an automatic task-state or
release approval. The task's recovery dependency and independent acceptance
still apply.

Example for an installed Linux Rust 1.95.0 toolchain:

```sh
cargo +1.95.0 fetch --locked
python3 scripts/p8_cold_build.py --execute --stdio-smoke \
  --only-toolchain 1.95 --only-package default \
  --output-dir /absolute/new/cold-output
# The preceding selected matrix returns 2; strictly verify its actual cell:
python3 scripts/p8_cold_build.py \
  --export-selected /absolute/new/cold-output/matrix.json \
  --only-platform linux --only-toolchain 1.95 --only-package default \
  --output-dir /absolute/new/cell-archive
```

The workflow retains preparation logs and failed receipts even when a job
fails. Successful cell archives contain the actual binary, complete source
manifest, Cargo logs, fixture, raw RPC stream, status/search observations, and
process cleanup evidence. Compiler binaries and the disposable Cargo target
tree are not uploaded; compiler identity is recorded and checked on the runner
before export.

## Source approval

The repository's versioned source guard includes all scripts and workflows.
New platform/recovery tooling must be admitted through a new independently
reviewed source-and-validation delta with full fixed commit and byte inventories.
The preceding guard, registry, historical review evidence, and existing workflow
requirements remain frozen. A passing platform job does not grant that approval
or replace the original CI source-integrity gate.

## Full recovery execution (P8-011 / P8-016)

The same workflow has a separate Linux recovery job. `p8_recovery.py
--full-matrix` first builds default, semantic, and semantic-HTTP products from
one exact committed Cargo/crate inventory in a newly owned shared target. These
builds are explicitly recorded as engineering builds, not cold-build cells.
Original Cargo JSON, source manifests, binaries, runner source, and commands are
retained. A nonzero command, skipped test, timeout, changed source, or changed
executable makes the matrix fail.

The existing kill/restart, actual SQLite busy-window, and persisted deletion
stdio tests run unchanged. Three further generated seeds execute the real HTTP
product against an owned loopback provider:

- Receive an actual document HTTP request and disconnect before response bytes;
  prove pending work and partial readiness, local fallback without another call,
  and recovery after an actual new source version.
- Corrupt a payload actually published by the worker; prove incomplete query
  coverage using the cached query vector, then restore the exact payload and
  observe complete coverage again.
- Change that same artifact's format version; prove the enabled reader rejects
  it, then restores correct reads with the original metadata.
- Hold an old-incarnation document HTTP request, change source, delete another
  document, and execute a real full rebuild while the old request is still held.
  After releasing the request, prove only current manifest versions are
  published and deleted source remains absent after a fresh process reopens.

The global readiness fields describe durable manifest coverage. The cache
corruption checks separately verify the actual query's incomplete artifact
coverage; they do not reinterpret a global manifest count as a per-query check.

For persistence boundaries, the runner compiles and invokes the original exact
nonignored tests for uncommitted/claimed/response/artifact/published SIGKILL,
database sidecar/rename/reopen crashes, and concurrent writer serialization.
Original artifact-cache tests also exercise unsupported format, vector-space
isolation, and namespace isolation. The test executable's hash, Cargo artifact,
complete source, and actual one-test result are bound; an empty selection cannot
pass. Each executed test binary is copied and hash-checked outside the disposable
target before execution, so the artifact upload retains its exact bytes. The HTTP
product's binary hash is checked before and after every active fault seed.
These tests link the production crates and use actual owned subprocesses
where specified. They do not claim arbitrary power-loss/device-failure coverage.

The rollback drill retains the source, configuration, and original database
backup, exercises controlled unsupported-schema rebuild, switches from semantic
to default package with semantic disabled, and restores the old SQLite backup.
The actual active cache-format tests complement its unchanged limited report.
The schema increase remains a documented injected future format, not an invented
released new-schema/old-release package pair. Original task dependencies and
independent release acceptance remain separate from the execution receipt.

## Actual historical schema rollback

The recovery job also checks out the unmodified public source revision
`277f2490fad3fa30f2812b5547bad033867c9ea5`, whose real production schema is 24.
The requested current source uses schema 25. Both default products are compiled
from their own complete committed inventories with their own Cargo receipts.
The historical build uses a separate target under the owned disposable target.
The version-pair driver rejects equal source revisions, equal schemas, and any
schema-source bytes that differ from the build manifest.

Four actual stdio processes create a current database, open it using the older
binary, restore the current binary, and restore the original current SQLite
backup. The old binary must emit its existing schema-mismatch diagnostic and
perform its real rebuild to its own schema; the driver does not alter a version
number or write a sentinel. Each process must index successfully, return the
exact fixture symbol with a valid source file/span, expose disabled semantic
fallback, and exit cleanly after stdin EOF. SQL integrity, source/configuration
bytes, package hashes, and the untouched original backup are checked throughout.
The derived database is expected to change during controlled rebuild; the
preserved original backup must not change.

This is compatibility evidence between two actual public source revisions.
Neither revision is relabeled as a release tag, and the driver does not publish
packages or claim released-package certification.

```sh
python3 scripts/p8_recovery.py --full-matrix --expected-commit "$CURRENT_COMMIT" \
  --previous-source-root /absolute/checkout/of/previous/source \
  --previous-commit 277f2490fad3fa30f2812b5547bad033867c9ea5 \
  --output-dir /absolute/new/full-recovery
```

To rerun only the actual database version pair with retained build receipts:

```sh
python3 scripts/p8_rollback.py --version-pair \
  --default-binary /absolute/current/codecortex \
  --default-receipt /absolute/current/build-receipt.json \
  --previous-binary /absolute/previous/codecortex \
  --previous-receipt /absolute/previous/build-receipt.json \
  --output-dir /absolute/new/version-pair
```
