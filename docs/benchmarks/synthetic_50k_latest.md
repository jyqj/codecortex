# Synthetic Scale Benchmark: 50k

Generated: 2026-07-09T19:41:55.193200+00:00
Dataset: synthetic 50k (seed 0xc0ffee)
Files: 50000 | Symbols: 278074

## Cold Full Index

| Metric | Value |
|--------|-------|
| generate wall | 4287ms |
| cold full index wall | 135543ms |
| index db size | 1183.3 MB |

## Incremental Latency: single_file

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 1269ms | 2717ms | 2717ms |
| scan_diff | 500ms | 1875ms | 1875ms |
| analysis | 404ms | 439ms | 439ms |
| write | 347ms | 378ms | 378ms |
| parse | 7ms | 14ms | 14ms |
| resolve | 5ms | 6ms | 6ms |
| postprocess | 0ms | 0ms | 0ms |

## Incremental Latency: five_percent_batch

Files: 50000 | Measured iterations: 3

| Phase | p50 | p95 | Max |
|-------|-----|-----|-----|
| total elapsed | 25263ms | 28069ms | 28069ms |
| write | 22576ms | 24914ms | 24914ms |
| scan_diff | 1534ms | 1702ms | 1702ms |
| analysis | 555ms | 602ms | 602ms |
| resolve | 236ms | 255ms | 255ms |
| parse | 207ms | 243ms | 243ms |
| postprocess | 2ms | 2ms | 2ms |

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

| Scenario | Tool | Iters (cold/warm) | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|----------|------|-------------------|----------|----------|----------|----------|----------|------------|
| search_hybrid_needle_phrase | search | 3/7 | 325.24ms | 357.40ms | 324µs | 416µs | 416µs | 8.4 KB |
| search_hybrid_mixed_terms | search | 3/7 | 743.05ms | 1.68s | 1.07ms | 1.18ms | 1.18ms | 31.2 KB |
| find_symbol_exact_needle | search | 3/7 | 218µs | 363µs | 90µs | 368µs | 368µs | 307 B |
| find_symbol_fuzzy_prefix | search | 3/7 | 3.04ms | 5.03ms | 189µs | 247µs | 247µs | 307 B |
| impact_changes_hub_file | impact | 3/7 | 4.33ms | 38.35ms | 940µs | 1.18ms | 1.18ms | 13.3 KB |
| graph_query_calls_varlen | graph_query | 3/7 | 955µs | 1.74ms | 275µs | 482µs | 482µs | 306 B |
| trace_chain_4_hops | trace | 3/7 | 9.38ms | 17.08ms | 224µs | 280µs | 280µs | 2.7 KB |

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
