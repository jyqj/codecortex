# Synthetic Scale Benchmark: 10k

Generated: 2026-06-12T14:54:56.886484+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 681ms |
| cold full index wall | 20186ms |
| index db size | 256.8 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 684ms | 713ms | 713ms |
| postprocess | 251ms | 255ms | 255ms |
| resolve | 229ms | 260ms | 260ms |
| analysis | 75ms | 78ms | 78ms |
| scan_diff | 67ms | 68ms | 68ms |
| write | 58ms | 59ms | 59ms |
| parse | 1ms | 1ms | 1ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 3597ms | 4223ms | 4223ms |
| write | 2309ms | 2716ms | 2716ms |
| resolve | 783ms | 795ms | 795ms |
| postprocess | 275ms | 544ms | 544ms |
| scan_diff | 82ms | 106ms | 106ms |
| analysis | 80ms | 81ms | 81ms |
| parse | 25ms | 30ms | 30ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 0ms | 0ms | 0ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 0ms | 0ms | 0ms | 27.7 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 0ms | 0ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 0ms | 0ms | 307 B |
| impact_changes_hub_file | impact | 7 | 0ms | 0ms | 0ms | 13.6 KB |
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
| relations_py_hub_callers | YES | expect caller fn_00421_1 |
| graph_query_rs_intra_edge | YES | expect fn_00017_0 -> fn_00017_2 |

## Summary

- Incremental batch touched files: 480
- Ground-truth checks passed: 8/8
