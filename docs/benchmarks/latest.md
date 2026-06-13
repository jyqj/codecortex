# Benchmark Results

Generated: 2026-06-12T19:51:25.016074+00:00
Dataset: fixture
Files: 18

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| adr | 1 | 428µs | 428µs | 219µs | 219µs | 219µs | 11 B |
| architecture | 6 | 1.31ms | 6.28ms | 1.16ms | 5.90ms | 5.90ms | 5.4 KB |
| context | 8 | 15.91ms | 23.54ms | 5.40ms | 5.67ms | 5.67ms | 19.1 KB |
| explore | 5 | 1.85ms | 2.52ms | 1.39ms | 1.60ms | 1.60ms | 2.0 KB |
| files | 2 | 311µs | 957µs | 166µs | 802µs | 802µs | 1.2 KB |
| graph_query | 9 | 518µs | 3.36ms | 378µs | 3.29ms | 3.29ms | 148 B |
| impact | 7 | 1.56ms | 35.75ms | 1.10ms | 33.78ms | 33.78ms | 1.4 KB |
| index | 1 | 197.08ms | 197.08ms | 143.71ms | 143.71ms | 143.71ms | 324 B |
| ingest_traces | 1 | 647µs | 647µs | 421µs | 421µs | 421µs | 139 B |
| node | 6 | 728µs | 1.65ms | 521µs | 1.24ms | 1.24ms | 964 B |
| relations | 8 | 896µs | 2.25ms | 655µs | 740µs | 740µs | 979 B |
| search | 28 | 792µs | 20.07ms | 438µs | 3.94ms | 7.78ms | 4.5 KB |
| status | 1 | 3.73ms | 3.73ms | 3.27ms | 3.27ms | 3.27ms | 10.1 KB |
| trace | 11 | 1.25ms | 1.70ms | 594µs | 912µs | 912µs | 924 B |

## Summary

- Total cases: 94
- All tools under 500ms warm p95: YES
