# Synthetic Scale Benchmark: 1k

Generated: 2026-07-11T18:35:28.729839+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 76ms |
| cold full index wall | 923ms |
| index db size | 24.0 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 47ms | 62ms | 62ms |
| analysis | 27ms | 32ms | 32ms |
| write | 10ms | 21ms | 21ms |
| scan_diff | 7ms | 9ms | 9ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 181ms | 251ms | 251ms |
| write | 122ms | 175ms | 175ms |
| analysis | 27ms | 41ms | 41ms |
| parse | 11ms | 13ms | 13ms |
| scan_diff | 7ms | 7ms | 7ms |
| resolve | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: targeted_single_file (watcher parity)

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 45ms | 45ms | 45ms |
| analysis | 28ms | 28ms | 28ms |
| write | 12ms | 13ms | 13ms |
| scan_diff | 3ms | 3ms | 3ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

Targeted = watcher-parity `BuildScope::Targeted` scan (event-reported paths only), driven directly through `Indexer::prepare_build`/`commit_build`; total elapsed is harness wall time across both halves.

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 10.42ms | 10.78ms | 382µs | 451µs | 451µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 20.31ms | 20.50ms | 860µs | 938µs | 938µs | 25.0 KB |
| find_symbol_exact_needle | search | 3/7 | 199µs | 252µs | 123µs | 394µs | 394µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 695µs | 1.04ms | 119µs | 621µs | 621µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 4.00ms | 4.19ms | 892µs | 928µs | 928µs | 7.9 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 762µs | 2.44ms | 198µs | 328µs | 328µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 1.56ms | 2.15ms | 254µs | 835µs | 835µs | 2.7 KB |

## Ground-Truth Correctness

| Check | Passed | Detail |
|-------|--------|--------|
| search_symbol_needle_top5 | YES | rank = Some(1) |
| search_hybrid_needle_phrase | YES | phrase 'verdant obsidian palisade kestrel' surfaced needle: true |
| impact_hub_known_callers | YES | 10/10 probed hub callers in blast radius |
| graph_query_varlen_chain | YES | 8 rows; expect fn_00003_0 reachable from fn_00002_0 |
| graph_query_cycle_closes | YES | cycle legs reachable: ["cyc_00005_a", "cyc_00005_b", "cyc_00005_c"] |
| trace_chain_path | YES | paths found: true, via fn_00003_0 |
| relations_py_hub_callers | YES | expect caller fn_00037_1 |
| graph_query_rs_intra_edge | YES | expect fn_00017_0 -> fn_00017_2 |

## Process RSS

Single-process harness (generator + in-process MCP server + bench driver): an upper bound on the serving footprint, tracked for regression trends.

| Milestone | RSS |
|-----------|-----|
| after repo generation | 4.3 MB |
| after cold full index | 144.4 MB |
| after tool scenarios | 146.1 MB |
| after incremental scenarios | 160.2 MB |
| after targeted scenario | 139.3 MB |

## Summary

- Incremental batch touched files: 48
- Ground-truth checks passed: 8/8
