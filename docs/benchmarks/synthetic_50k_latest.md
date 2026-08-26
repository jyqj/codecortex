# Synthetic Scale Benchmark: 50k

Generated: 2026-07-11T18:39:06.159798+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 3519ms |
| cold full index wall | 74860ms |
| index db size | 1183.3 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 1033ms | 1316ms | 1316ms |
| scan_diff | 371ms | 411ms | 411ms |
| analysis | 338ms | 351ms | 351ms |
| write | 299ms | 549ms | 549ms |
| parse | 6ms | 6ms | 6ms |
| resolve | 4ms | 4ms | 4ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 12125ms | 12995ms | 12995ms |
| write | 10705ms | 11581ms | 11581ms |
| analysis | 459ms | 469ms | 469ms |
| scan_diff | 410ms | 412ms | 412ms |
| resolve | 159ms | 169ms | 169ms |
| parse | 128ms | 131ms | 131ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: targeted_single_file (watcher parity)

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 974ms | 1007ms | 1007ms |
| analysis | 407ms | 431ms | 431ms |
| write | 375ms | 381ms | 381ms |
| scan_diff | 172ms | 174ms | 174ms |
| parse | 5ms | 5ms | 5ms |
| resolve | 4ms | 4ms | 4ms |
| postprocess | 0ms | 0ms | 0ms |

Targeted = watcher-parity `BuildScope::Targeted` scan (event-reported paths only), driven directly through `Indexer::prepare_build`/`commit_build`; total elapsed is harness wall time across both halves.

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 294.06ms | 297.93ms | 358µs | 443µs | 443µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 528.41ms | 1.24s | 1.03ms | 1.15ms | 1.15ms | 31.2 KB |
| find_symbol_exact_needle | search | 3/7 | 238µs | 1.56ms | 662µs | 3.38ms | 3.38ms | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 1.09ms | 2.63ms | 167µs | 211µs | 211µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 4.49ms | 5.49ms | 1.03ms | 1.12ms | 1.12ms | 13.3 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 792µs | 1.33ms | 272µs | 391µs | 391µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 7.53ms | 9.36ms | 249µs | 297µs | 297µs | 2.7 KB |

## Ground-Truth Correctness

| Check | Passed | Detail |
|-------|--------|--------|
| search_symbol_needle_top5 | YES | rank = Some(1) |
| search_hybrid_needle_phrase | YES | phrase 'verdant obsidian palisade kestrel' surfaced needle: true |
| impact_hub_known_callers | YES | 10/10 probed hub callers in blast radius |
| graph_query_varlen_chain | YES | 8 rows; expect fn_00003_0 reachable from fn_00002_0 |
| graph_query_cycle_closes | YES | cycle legs reachable: ["cyc_00005_a", "cyc_00005_b", "cyc_00005_c"] |
| trace_chain_path | YES | paths found: true, via fn_00003_0 |
| relations_py_hub_callers | YES | expect caller fn_02077_1 |
| graph_query_rs_intra_edge | YES | expect fn_00017_0 -> fn_00017_2 |

## Process RSS

Single-process harness (generator + in-process MCP server + bench driver): an upper bound on the serving footprint, tracked for regression trends.

| Milestone | RSS |
|-----------|-----|
| after repo generation | 14.6 MB |
| after cold full index | 796.2 MB |
| after tool scenarios | 473.4 MB |
| after incremental scenarios | 910.7 MB |
| after targeted scenario | 602.8 MB |

## Summary

- Incremental batch touched files: 2400
- Ground-truth checks passed: 8/8
