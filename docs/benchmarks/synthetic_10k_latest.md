# Synthetic Scale Benchmark: 10k

Generated: 2026-06-11T16:51:09.060584+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 1054ms |
| cold full index wall | 65157ms |
| index db size | 256.9 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 1755ms | 1879ms | 1879ms |
| postprocess | 674ms | 833ms | 833ms |
| write | 628ms | 638ms | 638ms |
| resolve | 254ms | 287ms | 287ms |
| analysis | 79ms | 81ms | 81ms |
| scan_diff | 70ms | 77ms | 77ms |
| parse | 1ms | 1ms | 1ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 22232ms | 22387ms | 22387ms |
| write | 17779ms | 17979ms | 17979ms |
| postprocess | 3028ms | 3415ms | 3415ms |
| resolve | 903ms | 905ms | 905ms |
| analysis | 118ms | 121ms | 121ms |
| scan_diff | 85ms | 137ms | 137ms |
| parse | 35ms | 38ms | 38ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 0ms | 1ms | 1ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 0ms | 0ms | 0ms | 28.9 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 0ms | 0ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 0ms | 0ms | 307 B |
| impact_changes_hub_file | impact | 7 | 0ms | 0ms | 0ms | 13.4 KB |
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
