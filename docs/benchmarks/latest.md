# Benchmark Results

Generated: 2026-05-28T06:44:20.453751+00:00
Dataset: fixture
Files: 15

## Per-Tool Latency

| Tool | Cases | p50 | p95 | Max | Avg Output |
|------|-------|-----|-----|-----|------------|
| adr | 1 | 0ms | 0ms | 0ms | 11 B |
| architecture | 6 | 0ms | 2ms | 2ms | 4.0 KB |
| context | 4 | 9ms | 11ms | 11ms | 22.9 KB |
| explore | 4 | 0ms | 0ms | 0ms | 1.2 KB |
| files | 2 | 0ms | 0ms | 0ms | 999 B |
| graph_query | 3 | 0ms | 2ms | 2ms | 80 B |
| impact | 4 | 0ms | 34ms | 34ms | 850 B |
| index | 1 | 173ms | 173ms | 173ms | 211 B |
| ingest_traces | 1 | 0ms | 0ms | 0ms | 122 B |
| node | 4 | 0ms | 1ms | 1ms | 711 B |
| relations | 4 | 0ms | 0ms | 0ms | 1.1 KB |
| search | 9 | 0ms | 14ms | 14ms | 1.7 KB |
| status | 1 | 2ms | 2ms | 2ms | 5.6 KB |
| trace | 5 | 1ms | 2ms | 2ms | 723 B |

## Summary

- Total cases: 49
- All tools under 500ms p95: YES
