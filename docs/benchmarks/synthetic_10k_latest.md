# Synthetic Scale Benchmark: 10k

Generated: 2026-08-23T18:34:06.464372317+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 201ms |
| cold full index wall | 8345ms |
| index db size | 236.5 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 31ms | 35ms | 35ms |
| scan_diff | 20ms | 22ms | 22ms |
| analysis | 4ms | 4ms | 4ms |
| write | 4ms | 5ms | 5ms |
| parse | 1ms | 2ms | 2ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: single_file_scoped

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 18ms | 22ms | 22ms |
| scan_diff | 9ms | 10ms | 10ms |
| write | 3ms | 3ms | 3ms |
| analysis | 2ms | 6ms | 6ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |
| resolve | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 824ms | 849ms | 849ms |
| write | 706ms | 736ms | 736ms |
| parse | 32ms | 34ms | 34ms |
| scan_diff | 25ms | 26ms | 26ms |
| resolve | 22ms | 23ms | 23ms |
| analysis | 5ms | 6ms | 6ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 60.95ms | 62.12ms | 747µs | 1.17ms | 1.17ms | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 75.08ms | 108.86ms | 1.65ms | 2.12ms | 2.12ms | 28.3 KB |
| find_symbol_exact_needle | search | 3/7 | 378µs | 990µs | 81µs | 351µs | 351µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 1.06ms | 1.89ms | 141µs | 303µs | 303µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.97ms | 3.13ms | 1.43ms | 1.60ms | 1.60ms | 13.6 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 918µs | 953µs | 219µs | 392µs | 392µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.22ms | 2.35ms | 302µs | 440µs | 440µs | 2.7 KB |

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
