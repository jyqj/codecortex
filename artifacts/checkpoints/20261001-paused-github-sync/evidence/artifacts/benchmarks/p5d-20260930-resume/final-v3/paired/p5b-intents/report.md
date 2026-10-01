# CodeCortex benchmark baseline

Suite: `P1-C paired EN/ZH intentions (authored dev)`
Adapter: `mcp-stdio`
Scoring: `Native`
Measurement profile: `smoke`

Status: **gate_failed** (exit 1)

Queries: 18; measured rows: 54.
Mean Top-1: 0.625000; mean nDCG@10: 0.656250.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 24, p50_us: Some(4622), p95_us: Some(14934), max_us: Some(15135), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| I01 | 1.000000 | 1.000000 | 3 |
| I02 | 1.000000 | 1.000000 | 3 |
| I03 | 1.000000 | 1.000000 | 3 |
| I04 | 0.000000 | -0.000000 | 3 |
| I05 | 1.000000 | 1.000000 | 3 |
| I06 | 1.000000 | 1.000000 | 3 |
| I07 | 0.000000 | -0.000000 | 3 |
| I08 | 0.000000 | -0.000000 | 3 |
| I09 | 1.000000 | 1.000000 | 3 |
| I10 | 1.000000 | 1.000000 | 3 |
| I11 | 1.000000 | 1.000000 | 3 |
| I12 | 0.000000 | -0.000000 | 3 |
| I13 | 1.000000 | 1.000000 | 3 |
| I14 | 1.000000 | 1.000000 | 3 |
| I15 | 0.000000 | 0.500000 | 3 |
| I16 | 0.000000 | -0.000000 | 3 |

## Failures

- I14 Partial
- I10 Partial
- I07 Partial
- I05 Partial
- I15 Partial
- I11 Partial
- I03 Partial
- I06 Partial
- I13 Partial
- I09 Partial
- I03 Partial
- I15 Partial
- I10 Partial
- I11 Partial
- I06 Partial
- I14 Partial
- I09 Partial
- I05 Partial
- I13 Partial
- I07 Partial
- I10 Partial
- I09 Partial
- I13 Partial
- I05 Partial
- I14 Partial
- I15 Partial
- I11 Partial
- I03 Partial
- I06 Partial
- I07 Partial