# Synthetic Scale Benchmark: 50k

Generated: 2026-08-23T18:17:26.321619798+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 1065ms |
| cold full index wall | 54889ms |
| index db size | 1183.1 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 384ms | 416ms | 416ms |
| scan_diff | 336ms | 362ms | 362ms |
| write | 16ms | 17ms | 17ms |
| analysis | 13ms | 15ms | 15ms |
| parse | 11ms | 13ms | 13ms |
| resolve | 3ms | 3ms | 3ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 95ms | 98ms | 98ms |
| scan_diff | 63ms | 69ms | 69ms |
| write | 10ms | 19ms | 19ms |
| parse | 6ms | 7ms | 7ms |
| analysis | 2ms | 2ms | 2ms |
| resolve | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 6105ms | 6129ms | 6129ms |
| write | 5260ms | 5309ms | 5309ms |
| scan_diff | 322ms | 332ms | 332ms |
| resolve | 162ms | 171ms | 171ms |
| parse | 145ms | 146ms | 146ms |
| analysis | 22ms | 26ms | 26ms |
| postprocess | 1ms | 1ms | 1ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 331.25ms | 332.57ms | 911µs | 1.30ms | 1.30ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 395.66ms | 571.39ms | 2.36ms | 2.64ms | 2.64ms | 31.7 KB |
| find_symbol_exact_needle | search | 3/7 | 1.06ms | 10.30ms | 104µs | 345µs | 345µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 1.47ms | 1.49ms | 276µs | 750µs | 750µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 3.85ms | 4.36ms | 1.81ms | 1.96ms | 1.96ms | 13.2 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 1.25ms | 1.28ms | 243µs | 629µs | 629µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 9.69ms | 11.26ms | 675µs | 1.10ms | 1.10ms | 2.7 KB |

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
