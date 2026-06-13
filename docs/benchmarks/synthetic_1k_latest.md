# Synthetic Scale Benchmark: 1k

Generated: 2026-06-13T04:33:18.694832+00:00
Dataset: synthetic 1k (seed 0xc0ffee)
Files: 1000 | Symbols: 5568

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 65ms |
| cold full index wall | 847ms |
| index db size | 24.7 MB |

## Incremental Latency: single_file

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 64ms | 73ms | 73ms |
| analysis | 29ms | 31ms | 31ms |
| write | 13ms | 24ms | 24ms |
| resolve | 11ms | 11ms | 11ms |
| scan_diff | 7ms | 7ms | 7ms |
| parse | 0ms | 0ms | 0ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 1000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 218ms | 221ms | 221ms |
| write | 157ms | 160ms | 160ms |
| analysis | 29ms | 30ms | 30ms |
| resolve | 10ms | 11ms | 11ms |
| parse | 8ms | 8ms | 8ms |
| scan_diff | 7ms | 7ms | 7ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 9.84ms | 10.11ms | 234µs | 334µs | 334µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 21.12ms | 24.31ms | 629µs | 730µs | 730µs | 25.3 KB |
| find_symbol_exact_needle | search | 3/7 | 164µs | 173µs | 71µs | 123µs | 123µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 467µs | 511µs | 107µs | 130µs | 130µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.10ms | 2.11ms | 514µs | 600µs | 600µs | 7.9 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 402µs | 693µs | 240µs | 299µs | 299µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 798µs | 840µs | 196µs | 295µs | 295µs | 2.7 KB |

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
