# Synthetic Scale Benchmark: 10k

Generated: 2026-06-12T01:45:25.179225+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 939ms |
| cold full index wall | 86126ms |
| index db size | 256.9 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 2221ms | 2271ms | 2271ms |
| postprocess | 890ms | 981ms | 981ms |
| write | 819ms | 847ms | 847ms |
| resolve | 366ms | 372ms | 372ms |
| analysis | 84ms | 89ms | 89ms |
| scan_diff | 72ms | 74ms | 74ms |
| parse | 1ms | 1ms | 1ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 9428ms | 19576ms | 19576ms |
| write | 6075ms | 13271ms | 13271ms |
| postprocess | 1653ms | 4066ms | 4066ms |
| resolve | 1341ms | 1718ms | 1718ms |
| analysis | 133ms | 138ms | 138ms |
| scan_diff | 127ms | 232ms | 232ms |
| parse | 47ms | 83ms | 83ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 0ms | 1ms | 1ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 0ms | 1ms | 1ms | 28.9 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 0ms | 0ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 1ms | 1ms | 307 B |
| impact_changes_hub_file | impact | 7 | 0ms | 0ms | 0ms | 13.4 KB |
| graph_query_calls_varlen | graph_query | 7 | 0ms | 0ms | 0ms | 306 B |
| trace_chain_4_hops | trace | 7 | 0ms | 1ms | 1ms | 2.7 KB |

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
