# Synthetic Scale Benchmark: 50k

Generated: 2026-06-13T13:55:50.900032+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 3573ms |
| cold full index wall | 82684ms |
| index db size | 1220.6 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 1820ms | 1829ms | 1829ms |
| resolve | 808ms | 823ms | 823ms |
| scan_diff | 362ms | 363ms | 363ms |
| analysis | 318ms | 331ms | 331ms |
| write | 293ms | 333ms | 333ms |
| parse | 6ms | 6ms | 6ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 17115ms | 17791ms | 17791ms |
| write | 14328ms | 14497ms | 14497ms |
| scan_diff | 1303ms | 1510ms | 1510ms |
| resolve | 1014ms | 1086ms | 1086ms |
| analysis | 475ms | 486ms | 486ms |
| parse | 108ms | 120ms | 120ms |
| postprocess | 1ms | 1ms | 1ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 303.03ms | 305.42ms | 247µs | 313µs | 313µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 631.39ms | 1.72s | 745µs | 784µs | 784µs | 31.7 KB |
| find_symbol_exact_needle | search | 3/7 | 177µs | 190µs | 84µs | 376µs | 376µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 823µs | 2.82ms | 152µs | 192µs | 192µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.45ms | 4.71ms | 679µs | 771µs | 771µs | 13.3 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 748µs | 882µs | 274µs | 391µs | 391µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 6.09ms | 6.29ms | 161µs | 208µs | 208µs | 2.7 KB |

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
