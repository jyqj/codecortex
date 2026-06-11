# Synthetic Scale Benchmark: 1k

Generated: 2026-06-11T16:48:45.761504+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 99ms |
| cold full index wall | 1296ms |
| index db size | 25.5 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 252ms | 9743ms | 9743ms |
| write | 84ms | 95ms | 95ms |
| postprocess | 83ms | 90ms | 90ms |
| analysis | 66ms | 223ms | 223ms |
| resolve | 23ms | 29ms | 29ms |
| scan_diff | 13ms | 9300ms | 9300ms |
| parse | 0ms | 2ms | 2ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 586ms | 625ms | 625ms |
| write | 459ms | 494ms | 494ms |
| resolve | 40ms | 42ms | 42ms |
| postprocess | 36ms | 38ms | 38ms |
| analysis | 31ms | 31ms | 31ms |
| parse | 8ms | 8ms | 8ms |
| scan_diff | 7ms | 9ms | 9ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 2ms | 6ms | 6ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 4ms | 5ms | 5ms | 25.0 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 1ms | 1ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 1ms | 1ms | 307 B |
| impact_changes_hub_file | impact | 7 | 2ms | 8ms | 8ms | 7.9 KB |
| graph_query_calls_varlen | graph_query | 7 | 0ms | 3ms | 3ms | 306 B |
| trace_chain_4_hops | trace | 7 | 1ms | 2ms | 2ms | 2.7 KB |

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
