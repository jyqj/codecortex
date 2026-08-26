# Synthetic Scale Benchmark: 10k

Generated: 2026-07-11T18:36:27.888282+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 698ms |
| cold full index wall | 10213ms |
| index db size | 236.5 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 202ms | 202ms | 202ms |
| analysis | 75ms | 76ms | 76ms |
| scan_diff | 66ms | 66ms | 66ms |
| write | 56ms | 57ms | 57ms |
| parse | 1ms | 1ms | 1ms |
| resolve | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 1613ms | 1691ms | 1691ms |
| write | 1367ms | 1439ms | 1439ms |
| analysis | 79ms | 85ms | 85ms |
| scan_diff | 73ms | 79ms | 79ms |
| parse | 26ms | 26ms | 26ms |
| resolve | 26ms | 28ms | 28ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: targeted_single_file (watcher parity)

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 200ms | 238ms | 238ms |
| analysis | 91ms | 92ms | 92ms |
| write | 75ms | 97ms | 97ms |
| scan_diff | 29ms | 36ms | 36ms |
| parse | 1ms | 7ms | 7ms |
| resolve | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |

Targeted = watcher-parity `BuildScope::Targeted` scan (event-reported paths only), driven directly through `Indexer::prepare_build`/`commit_build`; total elapsed is harness wall time across both halves.

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 60.44ms | 62.70ms | 354µs | 452µs | 452µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 107.08ms | 119.57ms | 961µs | 1.07ms | 1.07ms | 28.8 KB |
| find_symbol_exact_needle | search | 3/7 | 197µs | 279µs | 115µs | 403µs | 403µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 878µs | 926µs | 141µs | 166µs | 166µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 4.94ms | 5.30ms | 1.03ms | 1.10ms | 1.10ms | 13.5 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 760µs | 769µs | 263µs | 324µs | 324µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.11ms | 2.20ms | 318µs | 983µs | 983µs | 2.7 KB |

## Ground-Truth Correctness

| Check | Passed | Detail |
|-------|--------|--------|
| search_symbol_needle_top5 | YES | rank = Some(1) |
| search_hybrid_needle_phrase | YES | phrase 'verdant obsidian palisade kestrel' surfaced needle: true |
| impact_hub_known_callers | YES | 10/10 probed hub callers in blast radius |
| graph_query_varlen_chain | YES | 8 rows; expect fn_00003_0 reachable from fn_00002_0 |
| graph_query_cycle_closes | YES | cycle legs reachable: ["cyc_00005_a", "cyc_00005_b", "cyc_00005_c"] |
| trace_chain_path | YES | paths found: true, via fn_00003_0 |
| relations_py_hub_callers | YES | expect caller fn_00421_1 |
| graph_query_rs_intra_edge | YES | expect fn_00017_0 -> fn_00017_2 |

## Process RSS

Single-process harness (generator + in-process MCP server + bench driver): an upper bound on the serving footprint, tracked for regression trends.

| Milestone | RSS |
|-----------|-----|
| after repo generation | 6.3 MB |
| after cold full index | 494.8 MB |
| after tool scenarios | 411.4 MB |
| after incremental scenarios | 390.0 MB |
| after targeted scenario | 326.5 MB |

## Summary

- Incremental batch touched files: 480
- Ground-truth checks passed: 8/8
