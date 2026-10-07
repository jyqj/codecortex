# P7-016: truthful GC unlink accounting and replayable retry

This checkpoint closes one proven GC error-reporting defect. It is a scoped
engineering result within P7-016, whose original brief still requires the
representative combined lifecycle fault matrix, owned-child SIGKILL evidence,
and opportunistic-reclaim responsibility. It makes no fake-vector quality claim.

## Frozen source and result

| Phase | Commit | Result |
| --- | --- | --- |
| Original production + frozen fixture | `c27bfd0e315187be96c267914d0b74de8044918c` | 0 passed, 5 failed, 0 ignored; exit 101 |
| Production fix, same fixture bytes | `84b33cf6a8751b316e5b2102a11f329bc1deab09` | 5 new + 8 original GC tests passed; 0 failed/ignored |
| Same repaired source checks | `84b33cf6a8751b316e5b2102a11f329bc1deab09` | Scoped strict Clippy and workspace format check exit 0 |

The base is `65eb87d70bd7bfd10d251b5d1cbb25850958196b`. Exactly two source
paths differ: `crates/cc-semantic/src/gc.rs` and the new frozen
`crates/cc-semantic/tests/gc_unlink_accounting.rs`. Cargo inputs and the original
`semantic_gc.rs` oracle are unchanged. All 769 `crates/**` plus root Cargo inputs
are hashed in each phase map; no source drift occurred during a command.
`source-comparison.json` provides exact hashes and the source-difference boundary.

## Failure and behavior

The old `remove_quiet` discarded every unlink error. Object, half and temporary
entry counters therefore advanced even when a directory occupied the collected
file path. Already absent object halves also produced a false deletion count.
The baseline logs and pre-assertion observations retain these actual failures.

The replacement returns `true` only after an actual unlink, `false` for
`NotFound`, and propagates every other IO error. An object counts once after
both unlink attempts succeed and at least one removed a path. Failure of the
first unlink stops before the counterpart; failure of the second can leave
the first file deleted. An error carries no counters and must not be read as
zero work or rollback. Existing conservative DB mark and freshness decisions
are unchanged.

The deterministic fault is an owned-file-to-directory replacement after the
public collection call and before the public sweep call. Repair restores the
exact original bytes, then uses a fresh collection and DB mark. The second-half
failure recovers as a half-object and is counted accordingly. Tests verify
final cache miss, bounded exhaustion, unchanged DB generation and idempotent
subsequent sweep. There are five Rust test functions and eight fixed seed
scenarios; they are not counted as thirteen new cases.

`run_gc_pass` propagates the sweep error before constructing its next cursor.
This is a source-level contract check: the injected failure uses the public
split collect/sweep API, while recovery exercises `run_gc_pass` itself. There
is no fault hook inside production code. Full mark-to-unlink replacement or
publication races and filesystem transactionality are explicitly unproven.

## Replay and provenance

Run from the respective commit, using Rust 1.95.0 and the unchanged lockfile:

```sh
cargo +1.95.0 test --offline --locked -j2 -p cc-semantic --test gc_unlink_accounting -- --nocapture
cargo +1.95.0 test --offline --locked -j2 -p cc-semantic --test gc_unlink_accounting --test semantic_gc -- --nocapture
cargo +1.95.0 clippy --offline --locked -j2 -p cc-semantic --lib --test gc_unlink_accounting --test semantic_gc -- -D warnings
cargo +1.95.0 fmt --all -- --check
```

The first command is expected to fail on the fixture-only commit. The receipts
record the actually executed command arrays (including Cargo JSON compiler
messages), toolchain versions, low-debug/non-incremental build environment,
durations, unnormalized log hashes, source maps, and each real test executable's
source path and SHA-256. Optional `CODECORTEX_GC_UNLINK_EVIDENCE_DIR` captures the
small per-seed JSON observations. No stdio binary, provider network call,
process kill, permission change, historical GC/WAL staging or full workspace
test was used here. `capture-validation.py` is the exact local capture script;
its original absolute execution paths are evidence, not a portable installer.

`fault-matrix.json` records the seed, fault script, operation order and recovery
assertions. Baseline execution of the already-absent loop stopped at seed 61;
67/71/73 are explicitly marked unreached in the baseline. The four error cases
also stopped before recovery in the baseline. Green recovery observations are
associated only with the repaired source.

## Storage

Bulky raw Cargo logs are stored losslessly in `raw-observations.tar.gz` with
their original relative names. Small observations, receipts and maps remain
readable. `checkpoint-files.json` binds every original by SHA-256 and size;
`verify_storage.py` validates both the archive and any matching disk mirrors
without extracting or executing payloads. See `STORAGE.md` for the exact count
and size. Storage verification is independent of the behavioral test result.
