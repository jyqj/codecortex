# Synthetic Scale Benchmark: 50k

Generated: 2026-06-12T19:51:18.304864+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 3554ms |
| cold full index wall | 432434ms |
| index db size | 1247.1 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 2054ms | 2431ms | 2431ms |
| resolve | 770ms | 830ms | 830ms |
| scan_diff | 564ms | 977ms | 977ms |
| analysis | 339ms | 345ms | 345ms |
| write | 312ms | 329ms | 329ms |
| parse | 5ms | 6ms | 6ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 43935ms | 44810ms | 44810ms |
| write | 26363ms | 27317ms | 27317ms |
| resolve | 15423ms | 15711ms | 15711ms |
| scan_diff | 1381ms | 1420ms | 1420ms |
| analysis | 498ms | 802ms | 802ms |
| parse | 107ms | 113ms | 113ms |
| postprocess | 1ms | 2ms | 2ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 324.83ms | 326.35ms | 253µs | 343µs | 343µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 2.57s | 2.75s | 769µs | 837µs | 837µs | 31.8 KB |
| find_symbol_exact_needle | search | 3/7 | 603µs | 1.45ms | 109µs | 189µs | 189µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 840µs | 4.03ms | 176µs | 220µs | 220µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 3.03ms | 22.49ms | 664µs | 875µs | 875µs | 13.7 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 752µs | 3.32ms | 299µs | 417µs | 417µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 5.78ms | 22.35ms | 176µs | 209µs | 209µs | 2.7 KB |

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

## Summary

- Incremental batch touched files: 2400
- Ground-truth checks passed: 8/8
