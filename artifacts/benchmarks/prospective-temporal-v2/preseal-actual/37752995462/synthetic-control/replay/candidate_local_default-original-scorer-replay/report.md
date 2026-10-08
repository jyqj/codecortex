# CodeCortex benchmark baseline

Suite: `temporal-v2-invented-control`
Adapter: `mcp-stdio`
Scoring: `Native`
Measurement profile: `smoke`

Status: **baseline_recorded_not_quality_certified** (exit 0)

Queries: 48; measured rows: 144.
Mean Top-1: 0.800000; mean nDCG@10: 0.800000.

These are baseline observations, not a release quality certificate.
Latency: Distribution { samples: 144, p50_us: Some(1862), p95_us: Some(3782), max_us: Some(5569), tail_claim: "insufficient_for_tail_claim" }; samples are retained, not best-of.
Source evidence: 0 invalid, 0 unverified hits.

| Case | Top-1 | nDCG@10 | Repetitions |
|---|---:|---:|---:|
| ptv2.invented-control.f0001.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0001.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0001.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0001.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0002.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0002.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0002.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0002.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0003.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0003.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0003.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0003.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0004.en_original | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0004.zh_translation | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0004.en_paraphrase | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0004.zh_paraphrase | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0005.en_original | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0005.zh_translation | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0005.en_paraphrase | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0005.zh_paraphrase | 0.000000 | 0.000000 | 3 |
| ptv2.invented-control.f0006.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0006.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0006.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0006.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0007.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0007.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0007.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0007.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0008.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0008.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0008.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0008.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0009.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0009.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0009.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0009.zh_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0010.en_original | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0010.zh_translation | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0010.en_paraphrase | 1.000000 | 1.000000 | 3 |
| ptv2.invented-control.f0010.zh_paraphrase | 1.000000 | 1.000000 | 3 |

## Measurement coverage

`latency-summary.json` retains its legacy success/no-match-only semantics. `latency-strata.json` retains every measured attempt, including deadline-censored timeouts and failures, and reports missing samples against the planned denominator. This runner records no per-request lifecycle/result-cache receipt, so these queries remain `unknown`; repetition and warmup do not prove cache hits. No OS cold-cache or performance-gate pass is inferred.

`resource-ledger.json` reports observed per-role RSS snapshots and missing observations. Native/ps runner alternatives, server PID/tree values and peaks from different stages are never summed. Disk layout, I/O and model billing remain unavailable until a profile supplies direct observations.

## Failures
