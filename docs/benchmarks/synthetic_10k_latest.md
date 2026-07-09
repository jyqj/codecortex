# Synthetic Scale Benchmark: 10k

Generated: 2026-07-09T19:36:39.449947+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 829ms |
| cold full index wall | 15835ms |
| index db size | 236.6 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 443ms | 605ms | 605ms |
| write | 154ms | 204ms | 204ms |
| analysis | 149ms | 241ms | 241ms |
| scan_diff | 132ms | 151ms | 151ms |
| parse | 4ms | 15ms | 15ms |
| resolve | 1ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 3144ms | 5568ms | 5568ms |
| write | 2691ms | 4527ms | 4527ms |
| analysis | 160ms | 349ms | 349ms |
| scan_diff | 110ms | 360ms | 360ms |
| parse | 42ms | 87ms | 87ms |
| resolve | 42ms | 54ms | 54ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 93.00ms | 96.30ms | 413µs | 514µs | 514µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 141.21ms | 539.35ms | 1.45ms | 1.97ms | 1.97ms | 28.3 KB |
| find_symbol_exact_needle | search | 3/7 | 1.65ms | 4.05ms | 423µs | 1.32ms | 1.32ms | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 3.26ms | 4.89ms | 426µs | 842µs | 842µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 6.52ms | 6.71ms | 1.14ms | 1.82ms | 1.82ms | 13.4 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 1.23ms | 1.70ms | 278µs | 355µs | 355µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.62ms | 9.17ms | 269µs | 641µs | 641µs | 2.7 KB |

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
