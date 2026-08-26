# Benchmark Results

Generated: 2026-07-11T18:43:17.377981+00:00
Dataset: fixture
Files: 18

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| adr | 1 | 172µs | 172µs | 61µs | 61µs | 61µs | 11 B |
| architecture | 6 | 303µs | 1.59ms | 238µs | 1.05ms | 1.05ms | 5.4 KB |
| context | 8 | 5.74ms | 7.03ms | 600µs | 662µs | 662µs | 19.1 KB |
| explore | 5 | 1.33ms | 1.53ms | 401µs | 471µs | 471µs | 2.1 KB |
| files | 2 | 78µs | 209µs | 35µs | 149µs | 149µs | 1.2 KB |
| graph_query | 9 | 190µs | 329µs | 133µs | 282µs | 282µs | 148 B |
| impact | 7 | 912µs | 33.54ms | 370µs | 33.27ms | 33.27ms | 1.4 KB |
| index | 1 | 77.54ms | 77.54ms | 74.97ms | 74.97ms | 74.97ms | 616 B |
| ingest_traces | 1 | 329µs | 329µs | 159µs | 159µs | 159µs | 157 B |
| node | 6 | 783µs | 1.22ms | 240µs | 291µs | 291µs | 964 B |
| relations | 8 | 492µs | 656µs | 214µs | 224µs | 224µs | 979 B |
| search | 28 | 507µs | 6.61ms | 136µs | 587µs | 998µs | 4.5 KB |
| status | 1 | 958µs | 958µs | 786µs | 786µs | 786µs | 10.3 KB |
| trace | 11 | 635µs | 838µs | 145µs | 269µs | 269µs | 924 B |

## Summary

- Total cases: 94
- All tools under 500ms warm p95: YES
