# Synthetic Scale Benchmark: 10k

Generated: 2026-08-23T17:57:42.631481046+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 224ms |
| cold full index wall | 8400ms |
| index db size | 236.5 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 59ms | 64ms | 64ms |
| scan_diff | 46ms | 51ms | 51ms |
| analysis | 4ms | 4ms | 4ms |
| write | 4ms | 5ms | 5ms |
| parse | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 64ms | 65ms | 65ms |
| write | 26ms | 27ms | 27ms |
| analysis | 25ms | 26ms | 26ms |
| scan_diff | 8ms | 8ms | 8ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 816ms | 823ms | 823ms |
| write | 677ms | 687ms | 687ms |
| scan_diff | 54ms | 56ms | 56ms |
| parse | 30ms | 31ms | 31ms |
| resolve | 21ms | 21ms | 21ms |
| analysis | 5ms | 5ms | 5ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 61.37ms | 61.38ms | 841µs | 1.34ms | 1.34ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 72.42ms | 74.23ms | 1.64ms | 2.00ms | 2.00ms | 27.8 KB |
| find_symbol_exact_needle | search | 3/7 | 366µs | 648µs | 98µs | 226µs | 226µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 991µs | 1.16ms | 272µs | 643µs | 643µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.20ms | 2.76ms | 1.31ms | 1.44ms | 1.44ms | 13.2 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 663µs | 950µs | 240µs | 559µs | 559µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.03ms | 2.06ms | 280µs | 464µs | 464µs | 2.7 KB |

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
