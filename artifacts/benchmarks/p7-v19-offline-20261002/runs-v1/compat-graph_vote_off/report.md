# CodeCortex benchmark baseline

Suite: `P7 V19 retrospective compat/graph_vote_off`
Adapter: `mcp-stdio`
Scoring: `OceCompat`
Measurement profile: `smoke`

Status: **gate_failed** (exit 1)

Queries: 14; measured rows: 42.
Mean Top-1: 0.857143; mean nDCG@10: 0.847123.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 3, p50_us: Some(12197), p95_us: Some(40238), max_us: Some(40238), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| R01 | 1.000000 | 1.000000 | 3 |
| R02 | 1.000000 | 1.000000 | 3 |
| R03 | 1.000000 | 1.000000 | 3 |
| R04 | 1.000000 | 1.000000 | 3 |
| R05 | 1.000000 | 1.000000 | 3 |
| R06 | 1.000000 | 1.000000 | 3 |
| R07 | 1.000000 | 1.000000 | 3 |
| R08 | 1.000000 | 1.000000 | 3 |
| R09 | 0.000000 | -0.000000 | 3 |
| R10 | 1.000000 | 1.000000 | 3 |
| R11 | 1.000000 | 1.000000 | 3 |
| R12 | 1.000000 | 1.000000 | 3 |
| R13 | 1.000000 | 0.859719 | 3 |
| R14 | 0.000000 | 0.000000 | 3 |

## Failures

- R10 Partial
- R11 Partial
- R02 Partial
- R14 Partial
- R13 Partial
- R05 Partial
- R06 Partial
- R01 Partial
- R08 Partial
- R03 Partial
- R04 Partial
- R07 Partial
- R12 Partial
- R07 Partial
- R08 Partial
- R03 Partial
- R11 Partial
- R06 Partial
- R14 Partial
- R04 Partial
- R13 Partial
- R12 Partial
- R10 Partial
- R01 Partial
- R02 Partial
- R05 Partial
- R13 Partial
- R14 Partial
- R07 Partial
- R01 Partial
- R05 Partial
- R11 Partial
- R03 Partial
- R12 Partial
- R08 Partial
- R10 Partial
- R06 Partial
- R04 Partial
- R02 Partial