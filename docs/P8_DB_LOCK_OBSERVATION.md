# Opt-in database acquisition observations

`p8-db-lock-observation` is a compile-time diagnostic feature, disabled by
default. It records actual acquisition calls in the owned product process.
The feature build has a separate `mixed_db_lock_observation_v1` receipt and
must not be presented as the default binary or combined with default-build
latency samples. It does not approve a release or complete a roadmap task.

## Reproduce the registered mixed workload

Use a clean, reviewed checkout with its exact source pins and locked
dependencies. The dedicated `p8-db-lock-observation.yml` workflow runs the
original source-admission command, formatting, default-off and feature-on Rust
controls, and Python receipt/failure controls before starting measurements.
It supports manual dispatch, or creation of the single explicitly named
diagnostic branch. It does not change the original scale or runtime workflows.

```sh
cargo fetch --locked
python3 scripts/p8_runtime_build.py --lock-observation --output /tmp/p8-lock-build
python3 scripts/p8_runtime.py \
  --binary /tmp/p8-lock-build/codecortex \
  --oracle /tmp/p8-lock-build/p8-oracle \
  --statistics /tmp/p8-lock-build/p8-runtime-statistics \
  --build-receipt /tmp/p8-lock-build/build-receipt.json \
  --lock-observation --profile mixed --concurrency 4 \
  --operations 900 --files 1000 --interval-ms 500 \
  --output /tmp/p8-lock-runtime
python3 scripts/p8_runtime.py verify --output /tmp/p8-lock-runtime \
  --build-output /tmp/p8-lock-build
```

Every output and Cargo target must be a new directory. The workflow runs the
same commands separately for C=1/4/8/16. The original mixed schedule offers
concurrency-sized bursts at the fixed 500 ms interval, with 900 operations and
1000 fixture files. One operation in three is a build; the existing six-action
write-admission mutation cycle and read/build overlap check remain in effect.
The two diagnostic snapshot RPCs are outside the offered-work interval and
are not added to its latency denominator. This option rejects the soak profile;
it does not introduce another one-hour experiment.

## What the counters mean

The snapshot is cumulative over every `IndexDb` handle in one product process,
including retired handles after branch changes. No counter is reset. The raw
baseline follows the initial cold build; the raw endpoint follows completion
of offered work and drain of the resource sampler, before the independent
full-build comparison. Responses bind the actual owned child PID.

| Metric | Actual boundary | Interpretation |
| --- | --- | --- |
| `writer_mutex_acquire` | The original writer mutex acquisition call | Acquisition wall time, including uncontended call overhead; not pure scheduler-blocked time |
| `read_pool_lock_acquire` | The original read-pool `RwLock` acquisition call | Acquisition wall time, separately from connection checkout |
| `read_connection_checkout` | Lookup and checkout of a read connection | Wall time can include connection validation or construction; not pure mutex wait |

`workload` and `observer` are separate counter groups. The complete synchronous
status handler uses a thread-local observer scope. Thus the existing resource
status RPCs account for their own database acquisitions without relabeling
concurrent workload threads. `status(aspect="lock_observation")` is available
only with the feature and reads counters without acquiring a DB connection or
issuing SQL. In this mixed profile, offered operations are search and build,
not status calls. The observer role does not purport to classify arbitrary
future workloads that themselves offer status operations.

Each metric retains attempts, successful acquisitions, poisoned/failed results,
would-block results where applicable, in-flight attempts, total nanoseconds,
and its process-lifetime maximum. Exact integer differences of drained,
coherent boundaries provide interval totals and outcome counts. A lifetime
maximum cannot be subtracted: the report labels its endpoint lifetime maximum
and leaves the interval maximum `null`. SQLite busy/transaction waits are not
measured. The observer has overhead, including atomic accounting while an
acquired guard remains owned; these data belong to the diagnostic build.

## Identity, failures, and retained evidence

The original build receipt verifies committed crate/Cargo and observer bytes,
compiler identity, a fresh private target, the actual feature lists in all
three Cargo binary events, copied binary hashes, and the retained build seal.
Default and diagnostic receipts cannot be used in each other's execution path.
Original raw, wire, stderr, all offered outcomes, Rust statistics, full-table
endpoint parity, owned-process drain, and artifact seals remain required.

Raw snapshot responses are retained before validation. Incoherence, overflow,
unbalanced outcomes, an undrained boundary, a changed PID, or counter reset
fails the observation instead of becoming zero or a reconstructed sample.
An endpoint observation failure still permits the original oracle and
terminal-outcome statistics to be retained. Failed executions are uploaded and
keep their failure status; a valid archive seal does not make them successful.
The `verify` command replays the retained counter boundaries and their timing
envelope without running another native workload.
