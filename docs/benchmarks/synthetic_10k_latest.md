# Synthetic Scale Benchmark: 10k

Generated: 2026-06-13T04:33:15.810893+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 686ms |
| cold full index wall | 10021ms |
| index db size | 244.0 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 345ms | 354ms | 354ms |
| resolve | 139ms | 140ms | 140ms |
| analysis | 78ms | 82ms | 82ms |
| scan_diff | 68ms | 70ms | 70ms |
| write | 58ms | 58ms | 58ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 2472ms | 2889ms | 2889ms |
| write | 2123ms | 2541ms | 2541ms |
| resolve | 155ms | 157ms | 157ms |
| analysis | 81ms | 81ms | 81ms |
| scan_diff | 73ms | 81ms | 81ms |
| parse | 22ms | 22ms | 22ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 62.05ms | 62.40ms | 239µs | 290µs | 290µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 97.50ms | 97.55ms | 666µs | 739µs | 739µs | 29.0 KB |
| find_symbol_exact_needle | search | 3/7 | 181µs | 195µs | 78µs | 124µs | 124µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 683µs | 695µs | 126µs | 149µs | 149µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.78ms | 2.85ms | 634µs | 703µs | 703µs | 13.6 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 623µs | 918µs | 216µs | 362µs | 362µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.24ms | 2.38ms | 203µs | 301µs | 301µs | 2.7 KB |

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

## Summary

- Incremental batch touched files: 480
- Ground-truth checks passed: 8/8
