# Synthetic Scale Benchmark: 1k

Generated: 2026-07-09T19:25:53.908249+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 264ms |
| cold full index wall | 1367ms |
| index db size | 24.0 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 59ms | 106ms | 106ms |
| analysis | 35ms | 65ms | 65ms |
| write | 13ms | 28ms | 28ms |
| scan_diff | 10ms | 11ms | 11ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 220ms | 225ms | 225ms |
| write | 136ms | 138ms | 138ms |
| analysis | 34ms | 39ms | 39ms |
| scan_diff | 20ms | 22ms | 22ms |
| parse | 13ms | 17ms | 17ms |
| resolve | 2ms | 3ms | 3ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 12.47ms | 13.69ms | 335µs | 561µs | 561µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 30.45ms | 42.38ms | 1.29ms | 3.54ms | 3.54ms | 25.0 KB |
| find_symbol_exact_needle | search | 3/7 | 206µs | 1.80ms | 102µs | 266µs | 266µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 666µs | 711µs | 145µs | 570µs | 570µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.52ms | 4.11ms | 698µs | 752µs | 752µs | 7.8 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 675µs | 982µs | 285µs | 599µs | 599µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 1.44ms | 3.02ms | 239µs | 816µs | 816µs | 2.7 KB |

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
