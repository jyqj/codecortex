# CodeCortex benchmark baseline

Suite: `P0 authored smoke`
Adapter: `mcp-stdio`
Scoring: `Native`
Measurement profile: `smoke`

Status: **gate_failed** (exit 1)

Queries: 11; measured rows: 33.
Mean Top-1: 0.700000; mean nDCG@10: 0.700000.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 33, p50_us: Some(9324), p95_us: Some(14974), max_us: Some(17013), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| S01 | 1.000000 | 1.000000 | 3 |
| S02 | 0.000000 | -0.000000 | 3 |
| S03 | 1.000000 | 1.000000 | 3 |
| S04 | 0.000000 | -0.000000 | 3 |
| S05 | 1.000000 | 1.000000 | 3 |
| S06 | 0.000000 | -0.000000 | 3 |
| S07 | 1.000000 | 1.000000 | 3 |
| S08 | 1.000000 | 1.000000 | 3 |
| S09 | 1.000000 | 1.000000 | 3 |
| S10 | 1.000000 | 1.000000 | 3 |

## Failures

- S11 no-answer failure
- S11 no-answer failure
- S11 no-answer failure