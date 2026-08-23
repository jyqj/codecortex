# Synthetic Scale Benchmark: 1k

Generated: 2026-08-23T18:39:24.621470722+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 17ms |
| cold full index wall | 780ms |
| index db size | 24.0 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 11ms | 23ms | 23ms |
| scan_diff | 4ms | 5ms | 5ms |
| analysis | 2ms | 2ms | 2ms |
| write | 2ms | 15ms | 15ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 6ms | 16ms | 16ms |
| analysis | 2ms | 2ms | 2ms |
| write | 2ms | 11ms | 11ms |
| scan_diff | 1ms | 1ms | 1ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 81ms | 86ms | 86ms |
| write | 60ms | 65ms | 65ms |
| parse | 8ms | 9ms | 9ms |
| scan_diff | 4ms | 4ms | 4ms |
| analysis | 2ms | 2ms | 2ms |
| resolve | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 10.52ms | 10.94ms | 537µs | 1.19ms | 1.19ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 15.61ms | 17.81ms | 1.34ms | 1.58ms | 1.58ms | 25.0 KB |
| find_symbol_exact_needle | search | 3/7 | 501µs | 658µs | 72µs | 126µs | 126µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 699µs | 713µs | 128µs | 172µs | 172µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.10ms | 2.19ms | 766µs | 1.27ms | 1.27ms | 7.8 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 691µs | 726µs | 170µs | 264µs | 264µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 869µs | 963µs | 253µs | 466µs | 466µs | 2.7 KB |

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

## Summary

- Incremental batch touched files: 48
- Ground-truth checks passed: 8/8
