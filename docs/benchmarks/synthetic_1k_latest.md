# Synthetic Scale Benchmark: 1k

Generated: 2026-06-12T14:54:17.086942+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 64ms |
| cold full index wall | 886ms |
| index db size | 25.5 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 84ms | 86ms | 86ms |
| analysis | 31ms | 36ms | 36ms |
| resolve | 17ms | 19ms | 19ms |
| postprocess | 12ms | 14ms | 14ms |
| write | 11ms | 12ms | 12ms |
| scan_diff | 6ms | 7ms | 7ms |
| parse | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 252ms | 255ms | 255ms |
| write | 146ms | 149ms | 149ms |
| resolve | 34ms | 35ms | 35ms |
| analysis | 27ms | 28ms | 28ms |
| postprocess | 24ms | 25ms | 25ms |
| parse | 7ms | 7ms | 7ms |
| scan_diff | 7ms | 7ms | 7ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 0ms | 0ms | 0ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 0ms | 0ms | 0ms | 25.0 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 0ms | 0ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 0ms | 0ms | 307 B |
| impact_changes_hub_file | impact | 7 | 0ms | 0ms | 0ms | 7.8 KB |
| graph_query_calls_varlen | graph_query | 7 | 0ms | 0ms | 0ms | 306 B |
| trace_chain_4_hops | trace | 7 | 0ms | 0ms | 0ms | 2.7 KB |

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
