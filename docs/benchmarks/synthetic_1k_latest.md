# Synthetic Scale Benchmark: 1k

Generated: 2026-06-12T01:42:42.059094+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 144ms |
| cold full index wall | 1687ms |
| index db size | 25.5 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 158ms | 166ms | 166ms |
| write | 65ms | 69ms | 69ms |
| analysis | 34ms | 34ms | 34ms |
| postprocess | 28ms | 28ms | 28ms |
| resolve | 19ms | 26ms | 26ms |
| scan_diff | 8ms | 10ms | 10ms |
| parse | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 526ms | 549ms | 549ms |
| write | 378ms | 383ms | 383ms |
| postprocess | 42ms | 49ms | 49ms |
| resolve | 41ms | 46ms | 46ms |
| analysis | 37ms | 63ms | 63ms |
| parse | 10ms | 11ms | 11ms |
| scan_diff | 9ms | 9ms | 9ms |

## Per-Tool Latency

| Scenario | Tool | Iterations | p50 | p95 | Max | Avg Output |
|----------|------|------------|-----|-----|-----|------------|
| search_hybrid_needle_phrase | search | 7 | 0ms | 0ms | 0ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 7 | 0ms | 0ms | 0ms | 25.0 KB |
| find_symbol_exact_needle | search | 7 | 0ms | 0ms | 0ms | 307 B |
| find_symbol_fuzzy_prefix | search | 7 | 0ms | 0ms | 0ms | 307 B |
| impact_changes_hub_file | impact | 7 | 0ms | 0ms | 0ms | 7.9 KB |
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
