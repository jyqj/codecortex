# Benchmark Results

Generated: 2026-07-11T18:43:23.726210+00:00
Dataset: codecortex-rust workspace copy
Files: 680

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| architecture | 1 | 102.44ms | 102.44ms | 104.44ms | 104.44ms | 104.44ms | 10.8 KB |
| context | 1 | 1.36ms | 1.36ms | 299µs | 299µs | 299µs | 1.2 KB |
| files | 1 | 74µs | 74µs | 41µs | 41µs | 41µs | 0 B |
| graph_query | 1 | 278µs | 278µs | 151µs | 151µs | 151µs | 1.7 KB |
| impact | 1 | 10.10ms | 10.10ms | 1.30ms | 1.30ms | 1.30ms | 3.3 KB |
| node | 1 | 2.01ms | 2.01ms | 265µs | 265µs | 265µs | 352 B |
| relations | 1 | 1.36ms | 1.36ms | 271µs | 271µs | 271µs | 413 B |
| search | 2 | 1.36ms | 43.76ms | 177µs | 1.90ms | 1.90ms | 103.4 KB |
| status | 1 | 19.77ms | 19.77ms | 20.71ms | 20.71ms | 20.71ms | 10.3 KB |

## Summary

- Total cases: 10
- All tools under 500ms warm p95: YES
