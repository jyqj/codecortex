# Benchmark Results

Generated: 2026-07-10T07:29:41.479234+00:00
Dataset: codecortex-rust workspace copy
Files: 672

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| architecture | 1 | 356.68ms | 356.68ms | 229.53ms | 229.53ms | 229.53ms | 10.5 KB |
| context | 1 | 7.88ms | 7.88ms | 1.42ms | 1.42ms | 1.42ms | 1.2 KB |
| files | 1 | 295µs | 295µs | 177µs | 177µs | 177µs | 0 B |
| graph_query | 1 | 1.12ms | 1.12ms | 810µs | 810µs | 810µs | 1.6 KB |
| impact | 1 | 19.65ms | 19.65ms | 5.02ms | 5.02ms | 5.02ms | 3.1 KB |
| node | 1 | 3.31ms | 3.31ms | 933µs | 933µs | 933µs | 352 B |
| relations | 1 | 797µs | 797µs | 447µs | 447µs | 447µs | 413 B |
| search | 2 | 1.21ms | 110.58ms | 576µs | 64.79ms | 64.79ms | 103.9 KB |
| status | 1 | 36.54ms | 36.54ms | 34.78ms | 34.78ms | 34.78ms | 10.3 KB |

## Summary

- Total cases: 10
- All tools under 500ms warm p95: YES
