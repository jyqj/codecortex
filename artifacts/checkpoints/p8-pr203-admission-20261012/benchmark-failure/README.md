# Round 4: original PR #203 benchmark failure

Reviewer: `/root/acceptance_audit`. Read-only source/log investigation; zero Rust
executions, workload reruns, source edits or original TODO closures.

## Finding

The old required `check` genuinely failed its unchanged latency assertion. The
available evidence establishes three tools over the limit, but does not establish
the cause of the slowdown. There is no demonstrated product bug or CI configuration
fix that can currently be claimed to restore this gate. Treat the failure as open.

The [original job](https://github.com/jyqj/codecortex/actions/runs/38035469451/job/114164870834)
ran on 2026-10-10. Check metadata names PR head
`797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f`, while the checkout log identifies actual
synthetic merge `c5bfa6150eb57a22adfbd34be1cd04d763d7d853` against base
`0da9a2800d2b3112d584a8bad4f79785e01162a9`. Fresh GitHub tree retrieval confirms
that actual merge has crates tree `7cb70a9b0843fa9c222996b626b78fd97a5ad63b`, the
same product tree previously audited for f97/current main. Its `ci.yml` blob is
`8c7325eb88b791f909e72c4b6c79b417185e4386`.

Seventeen relevant source files, including benchmark, runner, statistics, handlers
and CI, were checked byte-for-byte against 797e, c2b, P 0692 and G f631. All match.
`benchmark-fixture-review.json` records every path and hash.

## What the log actually says

| Tool | Cases | Cold p50 | Warm p50 | Warm p95 | Warm max |
| --- | ---: | ---: | ---: | ---: | ---: |
| context | 8 | 1.72 s | 1.19 s | 1.72 s | 1.72 s |
| search | 28 | 19.79 ms | 10.38 ms | 659.53 ms | 1.04 s |
| index | 1 | 5.01 s | 4.23 s | 4.23 s | 4.23 s |

The test sorts tool names and asserts each tool's warm p95 is strictly below
500,000 microseconds. It panicked first on `context`; the already printed table
also proves `search` and `index` exceed the same gate. A context-only improvement
would not establish that the whole fixture passes.

The job used Rust 1.99.0, `test [unoptimized + debuginfo]`, Ubuntu 24.04 image
20261004.327.1, and the original command:

```text
cargo test --workspace --all-targets --exclude cc-semantic --locked
```

The cc-eval library summary was 70 passed / 1 failed / 7 ignored, 118.85 seconds;
the command returned 101. Subsequent ordinary check steps were skipped. The build
cache action reports a successful cache restore; this is not query-cache telemetry.

## Exact timing and percentile semantics

[`measure_case`](https://github.com/jyqj/codecortex/blob/797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f/crates/cc-eval/src/bench.rs)
does one cold call on a fresh backend, then one discarded warmup and two timed
warm calls on the shared backend. It keeps the smaller of the two warm durations.
The timer covers `CodeIndexBackend::call_tool`, including in-process MCP routing
and response handling. Opening and dropping the cold session are outside its
timer. The extra output-size JSON serialization is also outside the timer.

`run_benchmark_named` groups one minimum per case by tool, sorts them, then calls
the canonical nearest-rank implementation. Thus:

```text
case value = min(warm1, warm2)
tool p95 = sorted case values at one-based rank ceil(0.95 × case count)
```

For eight context cases, rank 8 is the maximum. The slowest case's two measured
calls therefore both took at least approximately the displayed 1.72 seconds.
The context p50 is rank 4, so at least five case minima were at least approximately
1.19 seconds. The report rounds display values, so exact original microseconds
cannot be reconstructed. Search p95 is rank 27 of 28; index p95 is its only case.
This is a legacy smoke gate over case minima, not a request-latency distribution
or a complete P8/V20/G8 tail certificate. Its existing gate must remain intact.

## Why “warm” does not identify the cause

The context path is:

1. `mcp.rs::tool_context` invokes `handlers/context.rs::context_async_with_strategy`.
2. A query handle and freshness are captured through the bounded execution pool.
3. `engine.rs::task_symbols_direct` may return direct symbols, otherwise
   `query_handle.rs::search_async` enters context assembly.
4. `engine.rs::assemble_context_once` can use graph candidate cache entries, but
   it still creates a fresh `EvidenceHydrator`, validates source records and disk
   evidence, selects context, and constructs the response.
5. Context finalization performs freshness and output-budget work before the MCP
   response returns.

The graph cache contains candidate/enrichment data, not the final verified response.
Removing source verification or other correctness work to obtain a timing pass
would not be a valid fix. The index corpus case has `full = true`, so every cold,
warmup and measured call performs the requested full index operation.

The CI command does not set `--test-threads` or `RUST_TEST_THREADS`. The timed test
runs in the same library test invocation as `integration_fixtures_and_corpus` and
other tests. `service_factory.rs::query_pool` is a process-wide pool with limits
4 CPU / 32 queued / 8 asynchronous; each eval backend also owns its two-worker
Tokio runtime. These are real scheduling variables, but neither queue occupancy,
per-phase timing, resource pressure nor cache hit/miss data was retained for this
failed benchmark. The log does not justify attributing the result to CPU, cache,
shared-runner noise, or a specific algorithm.

## Concrete evidence gap and defensible next step

`bench.rs::CaseMeasurement` stores no case name, retains only the warm minimum,
and discards each call's success/error result. The report further retains only
per-tool aggregates. Its `output_bytes` is from the final measured call, which is
not necessarily the selected minimum. No individual trace identifies which case
produced 1.72 seconds or which phase was slow.

If fresh exact-head CI fails again, preserve that full failure and prepare a
separately reviewed diagnostic change that records case ID, source/run identity,
cold/warmup/warm1/warm2 durations, and success/error status in bounded records.
Emit diagnostics after the timed calls and before the existing assertion, so the
failure preserves useful evidence. Keep the same 94 cases and parameters, their
order, all call counts, min-of-two rule, nearest-rank rule and strict 500,000-us
predicate. This addresses an observability deficiency; it is not a claimed latency
fix and must undergo source admission if it enters a guarded domain.

Keep the original mandatory CI command. An isolated, exact-source companion run
can help investigate scheduling, with all results preserved, but its success
cannot replace the failed required check or establish a causal environment flake.
A release-profile run changes the condition and likewise cannot substitute for
the original debug gate. Do not raise thresholds, add best-of attempts, remove
slow cases/tools, skip this test, or merge past a failing required check.

## Evidence files

- `benchmark-fixture-review.json`: source inventory, all 14 original report rows,
  original corpus definitions, exact semantics, unknowns and next steps.
- `old-actual-source.json`: fresh immutable GitHub tree and workflow binding.
- `original-failure-extract.txt`: line-numbered original failure segment.

Full saved original log SHA-256:
`00892e671fda03bf45e7dcfcfaceaa8f61d84a4b83e0bb0160f08cae7099c74b`.

This review supplies no new original TODO completion credit and no CI pass claim.
