# Benchmark Results

Generated: 2026-06-12T19:51:43.090266+00:00
Dataset: codecortex-rust workspace copy
Files: 330

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| architecture | 1 | 209.61ms | 209.61ms | 227.78ms | 227.78ms | 227.78ms | 9.9 KB |
| context | 1 | 7.56ms | 7.56ms | 795µs | 795µs | 795µs | 1.2 KB |
| files | 1 | 320µs | 320µs | 192µs | 192µs | 192µs | 0 B |
| graph_query | 1 | 940µs | 940µs | 693µs | 693µs | 693µs | 1.7 KB |
| impact | 1 | 15.99ms | 15.99ms | 4.76ms | 4.76ms | 4.76ms | 3.1 KB |
| node | 1 | 1.31ms | 1.31ms | 742µs | 742µs | 742µs | 351 B |
| relations | 1 | 680µs | 680µs | 419µs | 419µs | 419µs | 413 B |
| search | 2 | 1.02ms | 97.77ms | 565µs | 27.71ms | 27.71ms | 102.7 KB |
| status | 1 | 31.23ms | 31.23ms | 36.50ms | 36.50ms | 36.50ms | 10.2 KB |

## Summary

- Total cases: 10
- All tools under 500ms warm p95: YES
