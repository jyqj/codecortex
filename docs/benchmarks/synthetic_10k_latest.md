# Synthetic Scale Benchmark: 10k

Generated: 2026-08-23T18:15:42.201217262+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 300ms |
| cold full index wall | 8272ms |
| index db size | 236.4 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 62ms | 70ms | 70ms |
| scan_diff | 50ms | 56ms | 56ms |
| analysis | 4ms | 4ms | 4ms |
| write | 4ms | 5ms | 5ms |
| parse | 1ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 17ms | 17ms | 17ms |
| scan_diff | 8ms | 8ms | 8ms |
| write | 3ms | 3ms | 3ms |
| analysis | 2ms | 2ms | 2ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 875ms | 877ms | 877ms |
| write | 725ms | 728ms | 728ms |
| scan_diff | 59ms | 60ms | 60ms |
| parse | 30ms | 31ms | 31ms |
| resolve | 26ms | 27ms | 27ms |
| analysis | 5ms | 5ms | 5ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 59.50ms | 61.36ms | 692µs | 1.11ms | 1.11ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 73.00ms | 111.48ms | 1.67ms | 1.87ms | 1.87ms | 28.3 KB |
| find_symbol_exact_needle | search | 3/7 | 402µs | 454µs | 182µs | 490µs | 490µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 1.15ms | 1.26ms | 144µs | 223µs | 223µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 3.60ms | 4.34ms | 1.56ms | 1.77ms | 1.77ms | 13.5 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 885µs | 977µs | 259µs | 768µs | 768µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.37ms | 2.44ms | 386µs | 777µs | 777µs | 2.7 KB |

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
