# Fixed F fresh holdout: sealed single execution

## Outcome

Both repositories completed every planned query: 64 rows, one measured attempt per query, no warmup and no retries. The original F gate is `gate_failed`, exit 1, for both runs. There are 57 Partial responses and 7 NoMatch responses. All 8 no-answer variants fail the original no-answer rule. These are preserved negative quality results.

The execution integrity checks pass: all 83 fixed source files were materialized and indexed exactly, both public readiness receipts are Ready, every raw response was sealed before scores were inspected, and the unchanged F evaluator reproduced scores and gates byte for byte on copies.

## Locked candidate and measurement

Candidate: `fb772551cff6b4620a6fcdb94c57b78350cebb33`. Complete source input manifest: `ef3fcf15da0523391b9c25abbc589b3c14498896eff581df416228619ef22aa6` (798 inputs).

F evaluator SHA256: `554d1baefeadd367614a0755a94b2db5a6194b284a83c03b7d8fe172368dd33d`. F product SHA256: `b9656f63897c1d6df2aafec944e6bf8b71179c67a7c4d81d573f46bdc52207bb`.

MCP stdio, codecortex-native-v1, cc-eval-public-v7, smoke profile, N=1, warmup=0, seed=2026100804, top_k=10; `auto_index.enabled=false`. Two fresh isolated materializations. No external or live semantic provider.

| Repository | Fixed source files | Ready | Queries | Partial | NoMatch | Gate | Runtime |
|---|---:|---:|---:|---:|---:|---|---:|
| walkdir | 12 | 12 | 32 | 28 | 4 | exit 1, gate_failed | 2.730553s |
| click | 71 | 71 | 32 | 29 | 3 | exit 1, gate_failed | 4.813088s |

## Original positive-query scores

The denominator is 56 positive rows in 28 bilingual families. No-answer rows are reported separately. Metrics below are taken from the original F `scores.jsonl`, aligned with its normalized-row order and checked against the original reports.

| Stratum | Positive rows | Top1 | nDCG@10 | Recall@5 | Recall@10 | MRR@10 | Span precision | Span recall |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| all positives | 56 | 0.178571 | 0.190530 | 0.196429 | 0.196429 | 0.196429 | 0.062919 | 0.177491 |
| click | 28 | 0.107143 | 0.152209 | 0.178571 | 0.178571 | 0.142857 | 0.075319 | 0.123284 |
| walkdir | 28 | 0.250000 | 0.228851 | 0.214286 | 0.214286 | 0.250000 | 0.050520 | 0.231699 |
| query en | 28 | 0.250000 | 0.251384 | 0.250000 | 0.250000 | 0.267857 | 0.078849 | 0.241438 |
| query zh | 28 | 0.107143 | 0.129676 | 0.142857 | 0.142857 | 0.125000 | 0.046990 | 0.113545 |

Available/unavailable denominators for every optional metric, all nine original category strata, source-target languages and repository/language combinations are in the aggregate JSON. Missing optional metrics are not silently converted to zero.

| Macro rule | Top1 | nDCG@10 | Recall@5 | Recall@10 | MRR@10 | Span precision | Span recall |
|---|---:|---:|---:|---:|---:|---:|---:|
| equal repository | 0.178571 | 0.190530 | 0.196429 | 0.196429 | 0.196429 | 0.062919 | 0.177491 |
| equal positive category | 0.222222 | 0.249049 | 0.263889 | 0.263889 | 0.250000 | 0.089498 | 0.227413 |

## Paired language results and intervals

Chinese and English variants share one family and one split. The prespecified calculation resamples whole bilingual families within each repository; it keeps both variants together. A separate calculation resamples whole repositories with equal weights. Both use 2,000 resamples and seed 2026100804.

| Metric | Mean zh minus en | Available pairs | Family bootstrap 95% interval for delta | Family bootstrap 95% interval for positive mean |
|---|---:|---:|---|---|
| top1 | -0.142857 | 28 | [-0.321429, 0.000000] | [0.071429, 0.285714] |
| ndcg10 | -0.121708 | 28 | [-0.275240, 0.021149] | [0.085944, 0.309647] |
| recall5 | -0.107143 | 28 | [-0.250000, 0.035714] | [0.080357, 0.330357] |
| recall10 | -0.107143 | 28 | [-0.250000, 0.035714] | [0.080357, 0.330357] |
| mrr10 | -0.142857 | 28 | [-0.321429, 0.000000] | [0.089286, 0.321429] |
| span_precision | -0.031859 | 28 | [-0.081035, 0.007653] | [0.020199, 0.123954] |
| span_recall | -0.127892 | 28 | [-0.276576, 0.016819] | [0.080629, 0.295243] |

Only two repositories and seven source-file-linked components underlie these 32 labeled families. The intervals are conditional descriptions of this fixed packet. They do not establish 32 statistically independent source units, broad repository generalization or an improvement from semantic search. The original runner intervals are also retained without altering their labels.

## No-answer, returned evidence and hard negatives

All four no-answer base families were reviewed against actual source scope before retrieval. Both variants of each family were measured once. The original scorer marks all 8 incorrect; all 8 responses are Partial, so even an empty Partial response would not be accepted as an absence proof.

Returned hit source validation: 85 valid, 0 invalid and 0 unverified, out of 85 hits. Valid bytes establish source provenance, not relevance.

Hard-negative observations use exact path and nonempty byte-span intersection against annotations frozen before measurement. Hits intersecting both positive and negative spans are counted separately. This diagnostic does not change the native scorer or introduce a new gate.

| Observation over all 64 rows | Count |
|---|---:|
| hits_excluded_from_overlap_observation_nonverified | 0 |
| hits_without_byte_span | 0 |
| returned_hits | 85 |
| rows | 64 |
| rows_top1_negative_only | 0 |
| rows_with_negative_only_hit | 0 |
| rows_with_predeclared_negatives | 64 |
| verified_hits_both_positive_and_negative | 2 |
| verified_hits_negative_only | 0 |
| verified_hits_neither | 72 |
| verified_hits_positive_only | 11 |

## Partial response receipts

Public raw responses retain lane, output-packing and freshness receipts. Their co-occurrence is reported below; this experiment did not isolate or repair a cause after seeing holdout scores.

| Receipt | Count |
|---|---:|
| rows | 57 |
| with_any_incomplete_lane | 38 |
| with_freshness_partial | 0 |
| with_packing_partial | 38 |

Exact lane status/truncation counts and packing omissions are retained in the aggregate JSON. Missing capability is not changed into Ready, Partial is not changed into Success, and no query or gold was dropped.

## Custody, replay and limits

Execution lock SHA256: `25262754e2c084ae10a0a088afdc8a7643eb615c9bc48fd8e98efbd917cd4735`. Raw seal SHA256: `3bca67e793b1c9b3fde37273d54eeb6c12707d790a4086dee7edb554130c5d5d`. The seal covers 107 files / 1,887,925 bytes and was reverified before and after analysis.

Only copies were replayed; both retained exit 1 and every copied file was byte-identical after replay. Replay receipt SHA256: `82dfc276822b2e170b3cae0f09599fb38f8bea216a9f8495878a1f74c1810a6f`.

Source-only preflight made no search calls. A non-author independently reviewed every query/gold span and both variants before the packet was frozen. The root integrator saw only metadata until raw sealing. The author had contributed the earlier forwarding correctness repair before F was frozen; this role overlap is disclosed. Shared filesystem access logs provide role separation, not ACL-enforced blindness.

The one-time execution is now complete and the packet is used. No further measurement, relabeling, selection or tuning was performed from these results. The fixed corpus task can retain this negative result as evidence of a completed measurement; quality and broader G8 gates remain failed or unproven. N=1 and the measured runtimes do not certify tail latency, cold caches or performance goals.

## Artifacts

- `aggregate-results.json`: complete metadata and metrics without query/gold bodies.
- `replay-receipt.json`: original seal verification and identical score/gate replay on copies.
- `../once-20261008-F/raw-seal.json`: immutable manifest of original raw and derived run files.
- `../execution-freeze/analysis-plan.json`: analysis plan frozen before measurement.
