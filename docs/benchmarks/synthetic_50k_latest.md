# Synthetic Scale Benchmark: 50k

Generated: 2026-06-13T04:39:04.961738+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 3782ms |
| cold full index wall | 121820ms |
| index db size | 1220.6 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 2469ms | 2741ms | 2741ms |
| resolve | 1110ms | 1153ms | 1153ms |
| scan_diff | 792ms | 827ms | 827ms |
| write | 367ms | 380ms | 380ms |
| analysis | 349ms | 368ms | 368ms |
| parse | 7ms | 9ms | 9ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 45803ms | 47687ms | 47687ms |
| write | 42338ms | 43634ms | 43634ms |
| scan_diff | 1805ms | 1877ms | 1877ms |
| resolve | 1176ms | 1303ms | 1303ms |
| analysis | 551ms | 617ms | 617ms |
| parse | 217ms | 227ms | 227ms |
| postprocess | 1ms | 2ms | 2ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 351.00ms | 411.70ms | 661µs | 1.22ms | 1.22ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 2.57s | 2.98s | 793µs | 903µs | 903µs | 31.8 KB |
| find_symbol_exact_needle | search | 3/7 | 213µs | 591µs | 70µs | 183µs | 183µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 894µs | 4.99ms | 155µs | 193µs | 193µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 3.49ms | 23.03ms | 643µs | 677µs | 677µs | 13.2 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 748µs | 3.88ms | 253µs | 316µs | 316µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 6.42ms | 30.05ms | 173µs | 211µs | 211µs | 2.7 KB |

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
