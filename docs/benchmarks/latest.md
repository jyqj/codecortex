# Benchmark Results

Generated: 2026-07-10T13:36:13.433711+00:00
Dataset: fixture
Files: 18

## Per-Tool Latency

Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).

Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.

| Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
|------|-------|----------|----------|----------|----------|----------|------------|
| adr | 1 | 426µs | 426µs | 255µs | 255µs | 255µs | 11 B |
| architecture | 6 | 1.40ms | 6.59ms | 1.17ms | 5.98ms | 5.98ms | 5.4 KB |
| context | 8 | 17.16ms | 23.72ms | 5.69ms | 5.88ms | 5.88ms | 19.1 KB |
| explore | 5 | 2.16ms | 3.77ms | 1.60ms | 1.91ms | 1.91ms | 2.1 KB |
| files | 2 | 337µs | 1.01ms | 206µs | 1.05ms | 1.05ms | 1.2 KB |
| graph_query | 9 | 522µs | 2.87ms | 368µs | 2.42ms | 2.42ms | 148 B |
| impact | 7 | 1.82ms | 39.39ms | 1.40ms | 35.83ms | 35.83ms | 1.4 KB |
| index | 1 | 258.75ms | 258.75ms | 181.44ms | 181.44ms | 181.44ms | 620 B |
| ingest_traces | 1 | 746µs | 746µs | 372µs | 372µs | 372µs | 157 B |
| node | 6 | 1.84ms | 2.61ms | 758µs | 1.64ms | 1.64ms | 964 B |
| relations | 8 | 1.78ms | 6.41ms | 885µs | 3.54ms | 3.54ms | 979 B |
| search | 28 | 2.60ms | 54.53ms | 1.41ms | 10.59ms | 17.48ms | 4.5 KB |
| status | 1 | 6.50ms | 6.50ms | 7.42ms | 7.42ms | 7.42ms | 10.3 KB |
| trace | 11 | 3.39ms | 12.79ms | 851µs | 2.03ms | 2.03ms | 924 B |

## Summary

- Total cases: 94
- All tools under 500ms warm p95: YES
