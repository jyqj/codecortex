# CodeCortex benchmark baseline

Suite: `P0 rust API mutation`
Adapter: `mcp-stdio`
Scoring: `Native`
Measurement profile: `smoke`

Status: **baseline_recorded_not_quality_certified** (exit 0)

Queries: 1; measured rows: 1.
Mean Top-1: 1.000000; mean nDCG@10: 1.000000.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 1, p50_us: Some(24190), p95_us: Some(24190), max_us: Some(24190), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| M01 | 1.000000 | 1.000000 | 1 |

## Measurement coverage

`latency-summary.json` retains its legacy success/no-match-only semantics. `latency-strata.json` retains every measured attempt, including deadline-censored timeouts and failures, and reports missing samples against the planned denominator. This runner records no per-request lifecycle/result-cache receipt, so these queries remain `unknown`; repetition and warmup do not prove cache hits. No OS cold-cache or performance-gate pass is inferred.

`resource-ledger.json` reports observed per-role RSS snapshots and missing observations. Native/ps runner alternatives, server PID/tree values and peaks from different stages are never summed. Disk layout, I/O and model billing remain unavailable until a profile supplies direct observations.

## Failures
