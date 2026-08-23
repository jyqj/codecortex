# Synthetic Scale Benchmark: 10k

Generated: 2026-08-23T18:23:15.965268219+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 246ms |
| cold full index wall | 8212ms |
| index db size | 236.4 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 60ms | 68ms | 68ms |
| scan_diff | 48ms | 55ms | 55ms |
| analysis | 4ms | 4ms | 4ms |
| write | 4ms | 5ms | 5ms |
| parse | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 17ms | 18ms | 18ms |
| scan_diff | 9ms | 9ms | 9ms |
| write | 3ms | 3ms | 3ms |
| analysis | 2ms | 2ms | 2ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 867ms | 895ms | 895ms |
| write | 722ms | 731ms | 731ms |
| scan_diff | 59ms | 73ms | 73ms |
| parse | 32ms | 33ms | 33ms |
| resolve | 23ms | 24ms | 24ms |
| analysis | 5ms | 5ms | 5ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 75.28ms | 97.10ms | 751µs | 1.09ms | 1.09ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 73.52ms | 110.98ms | 1.65ms | 1.91ms | 1.91ms | 27.8 KB |
| find_symbol_exact_needle | search | 3/7 | 335µs | 848µs | 248µs | 510µs | 510µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 778µs | 838µs | 199µs | 416µs | 416µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.27ms | 2.43ms | 1.44ms | 1.67ms | 1.67ms | 13.4 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 812µs | 929µs | 279µs | 479µs | 479µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.08ms | 2.38ms | 237µs | 524µs | 524µs | 2.7 KB |

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
