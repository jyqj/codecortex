# Synthetic Scale Benchmark: 50k

Generated: 2026-08-23T18:35:30.033366279+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 1109ms |
| cold full index wall | 49626ms |
| index db size | 1182.6 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 145ms | 148ms | 148ms |
| scan_diff | 104ms | 106ms | 106ms |
| analysis | 13ms | 16ms | 16ms |
| write | 12ms | 12ms | 12ms |
| parse | 8ms | 10ms | 10ms |
| resolve | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 77ms | 84ms | 84ms |
| scan_diff | 54ms | 54ms | 54ms |
| write | 8ms | 16ms | 16ms |
| parse | 7ms | 7ms | 7ms |
| analysis | 2ms | 2ms | 2ms |
| resolve | 2ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 6005ms | 6214ms | 6214ms |
| write | 5403ms | 5604ms | 5604ms |
| resolve | 152ms | 158ms | 158ms |
| parse | 145ms | 145ms | 145ms |
| scan_diff | 115ms | 123ms | 123ms |
| analysis | 17ms | 17ms | 17ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 276.38ms | 296.56ms | 615µs | 1.14ms | 1.14ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 326.03ms | 521.20ms | 1.62ms | 1.94ms | 1.94ms | 31.8 KB |
| find_symbol_exact_needle | search | 3/7 | 594µs | 794µs | 79µs | 596µs | 596µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 1.11ms | 1.65ms | 213µs | 910µs | 910µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 4.06ms | 196.34ms | 1.49ms | 1.76ms | 1.76ms | 13.7 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 928µs | 1.06ms | 176µs | 255µs | 255µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 6.03ms | 6.86ms | 234µs | 501µs | 501µs | 2.7 KB |

## Ground-Truth Correctness

| Check | Passed | Detail |
|-------|--------|--------|
| search_symbol_needle_top5 | YES | rank = Some(1) |
| search_hybrid_needle_phrase | YES | phrase 'verdant obsidian palisade kestrel' surfaced needle: true |
| impact_hub_known_callers | YES | 10/10 probed hub callers in blast radius |
| graph_query_varlen_chain | YES | 8 rows; expect fn_00003_0 reachable from fn_00002_0 |
| graph_query_cycle_closes | YES | cycle legs reachable: ["cyc_00005_a", "cyc_00005_b", "cyc_00005_c"] |
| trace_chain_path | YES | paths found: true, via fn_00003_0 |
| relations_py_hub_callers | YES | expect caller fn_02077_1 |
| graph_query_rs_intra_edge | YES | expect fn_00017_0 -> fn_00017_2 |

## Summary

- Incremental batch touched files: 2400
- Ground-truth checks passed: 8/8
