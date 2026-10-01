# CodeCortex benchmark baseline

Suite: `P1-B authored exact and component-scope contracts`
Adapter: `mcp-stdio`
Scoring: `Native`
Measurement profile: `ablation-diagnostic`

Status: **baseline_recorded_not_quality_certified** (exit 0)

Queries: 8; measured rows: 24.
Mean Top-1: 0.625000; mean nDCG@10: 0.625000.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 24, p50_us: Some(955), p95_us: Some(2318), max_us: Some(2733), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| E01 | 1.000000 | 1.000000 | 3 |
| E02 | 1.000000 | 1.000000 | 3 |
| E03 | 0.000000 | -0.000000 | 3 |
| E04 | 0.000000 | -0.000000 | 3 |
| E05 | 1.000000 | 1.000000 | 3 |
| E06 | 1.000000 | 1.000000 | 3 |
| E07 | 0.000000 | -0.000000 | 3 |
| E08 | 1.000000 | 1.000000 | 3 |

## Failures
