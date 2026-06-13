# Synthetic Scale Benchmark: 10k

Generated: 2026-06-12T19:40:19.578925+00:00
Dataset: synthetic 10k (seed 0xc0ffee)
Files: 10000 | Symbols: 55617

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 929ms |
| cold full index wall | 17877ms |
| index db size | 249.3 MB |

## Incremental Latency: single_file

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 345ms | 346ms | 346ms |
| resolve | 135ms | 137ms | 137ms |
| analysis | 79ms | 79ms | 79ms |
| scan_diff | 70ms | 85ms | 85ms |
| write | 58ms | 58ms | 58ms |
| parse | 1ms | 1ms | 1ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 10000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 2979ms | 3029ms | 3029ms |
| write | 2220ms | 2289ms | 2289ms |
| resolve | 553ms | 581ms | 581ms |
| scan_diff | 82ms | 94ms | 94ms |
| analysis | 78ms | 80ms | 80ms |
| parse | 22ms | 23ms | 23ms |
| postprocess | 0ms | 0ms | 0ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 62.88ms | 65.06ms | 233µs | 297µs | 297µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 99.63ms | 104.55ms | 663µs | 707µs | 707µs | 27.8 KB |
| find_symbol_exact_needle | search | 3/7 | 158µs | 177µs | 85µs | 109µs | 109µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 643µs | 771µs | 126µs | 153µs | 153µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 2.92ms | 3.66ms | 622µs | 704µs | 704µs | 13.5 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 774µs | 2.10ms | 178µs | 401µs | 401µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 2.33ms | 3.19ms | 174µs | 459µs | 459µs | 2.7 KB |

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
