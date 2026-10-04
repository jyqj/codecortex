# TypeScript fixed public DEV paired evaluation — 20261004

Status: **measurement complete and replayable; quality FAIL / not certified**. Both arms: 396 scheduled / 396 executed / 0 missing / 0 error rows / 396 Partial / 0 Success / 0 NoMatch. All 20 run/replay exits 1 (quality failure), unchanged bytes, successful actual prepare/index/readiness. This is a quality evaluation result, not a product or CI pass.

Candidate source 37dd042eaa1209a86e0cafdcd92ae77e036e76f5, production source/Cargo/lock identical to 90858afae647a513537bf118932a7ba5020ee98b. Baseline 88f2cf099c8b81f3acef485fd5ac9b01c63ce790. Normal official Rust 1.95.0 locked build succeeded in separate checkout/target/binary/Cargo cache directories. Compiler-artifact and binary/source hashes retained per arm.

Original PR91 admission 5385f5a7a2a875c6d5cbd049bdde039bf71bbf32 SHA256 b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425. Exact TypeScript subset of JS schedule in 535ff1b13b841af8021346a660c83c57919525e2: ten suites, 73 native/59 compat, 219/177 scheduled rows per arm, repetitions 3/warmup 0/seed 20261003/top_k10/timeout30000ms. Same admitted source/query/gold/suite bytes both arms. No PYGO taxonomy. No extra pilot/repeated best-of; each scheduled case once per frozen repetition per arm. Baseline then candidate for each original suite. Product/scorer/normalizer/provider/config/thresholds unchanged.

This is exposed public DEV, 72 local correlated components, two projections of existing questions, not 600 or new samples. Clean holdout 0. Previous 1671 all Partial/qualityFAIL remains untouched. No six-repo, live-semantic or walltime/performance causal inference; no new CI claim or central TODO closure. Applicable AGENTS.md/.agents/skills absent; benchmark README and original protocol/runner read.

Plan and selftools frozen before this task's rankings. Prior results are known, disclosed; no gold/product tuning. All raw original outputs/source verification/cost/status/diagnostic evidence will be retained under this prefix with original license bytes, without full input source tree, binary or database publication. Run/replay byte equality is verified for each suite. Missing observations invalidate comparison and are not filled. Low score/Partial quality failures do not prevent remaining intact suites.

An initial preparation missed the original author Git object before any rankings. Normal origin fetch of exact 4c01f627bd6df73e70bfe1dbdc42b2a9234e797a restored it; empty first input directory remains, successful preparation uses fresh inputs02. No denied operation workaround.

## Fixed results and denominators

Pre-ranking plan commit `94e1e77e8433c056a970d443fa6d0707fe1e4a71` (use full SHA in run-order receipt), plan SHA256 `a26447e713e0539d993445dc69720f49b77f8a76a9310dffd6b9bac0f099f3ed`. Every original TypeScript suite, source domain, query, mode, repetition, schedule and seed was retained. 132 native/compat projected query keys over 72 admitted local correlated components; answerable means use 59 queries / 58 eligible components per profile. Repetitions do not multiply independent N.

Both arms native query micro Top1 **0.457627119**, nDCG10 **0.520051351**; family-balanced Top1 **0.456896552**, nDCG10 **0.520397064**. Both arms compat query micro Top1 **0.796610169**, nDCG10 **0.785564505**; family-balanced Top1 **0.793103448**, nDCG10 **0.783934690**. Native and compat use different frozen formulas and are never interpreted as an intervention against each other.

All ten same-profile original score vectors are byte-identical across arms, including every repetition, nullable unsupported score and no-answer score. Candidate-minus-baseline is zero for all seven supported native metrics and compat Top1/nDCG10, including every fixed category. All 10000 paired component bootstrap draws yield [0,0]; degenerate => **inconclusive**, never certified gain/non-inferiority. This is conditional exposed TypeScript public DEV only, no six-repository inference. See [all metrics and intervals](summary.json), [per-query deltas](paired-case-deltas.jsonl), and per-arm case means. Compat Recall5/10/MRR/span metrics remain unavailable; required-facet, graph correctness, Recall20, SymbolAccuracy and DuplicationRate remain not_implemented; freshness, semantic ablation and release performance not_run.

Each arm: 14 native no-answer queries / 42 scheduled and executed rows; strict correct 0, incorrect 42, no missing, scorer-strict discrepancy 0. Partial is never relabelled Complete. Native answerable metrics computed on Partial remain descriptive and do not overcome failed availability/no-answer/quality gates.

Baseline 1065 returned source hits, candidate 1068; invalid 0 and unverified 0 in each arm. These differences do not change the measured scores. Costs: each arm 396 originating-work receipts / 0 unavailable; complete per-row costs preserved. [Cost audit](retrieval-cost-audit.json) lists per-suite and per-arm numeric leaves with availability; overlapping counters never added into a fabricated total. Returned work is originating work, not current cache-hit work, token/money spend or a causal walltime comparison. Lane receipts/status/truncation counts and actual stage/engine/source identities are in summary and original manifests. No live/paid provider calls.

## Every suite on both arms

| Arm | Suite | Scheduled | Executed | Missing | Error | Partial | Top1 | nDCG10 | Gate |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| baseline | typescript-blocks-01-migration-native | 45 | 45 | 0 | 0 | 45 | 0.500000000 | 0.564229066 | fail (1) |
| baseline | typescript-blocks-01-migration-compat | 36 | 36 | 0 | 0 | 36 | 0.916666667 | 0.830186391 | fail (1) |
| baseline | typescript-blocks-02-native | 45 | 45 | 0 | 0 | 45 | 0.416666667 | 0.560195113 | fail (1) |
| baseline | typescript-blocks-02-compat | 36 | 36 | 0 | 0 | 36 | 0.750000000 | 0.791805998 | fail (1) |
| baseline | typescript-blocks-03-native | 48 | 48 | 0 | 0 | 48 | 0.538461538 | 0.521700261 | fail (1) |
| baseline | typescript-blocks-03-compat | 39 | 39 | 0 | 0 | 39 | 0.846153846 | 0.876239791 | fail (1) |
| baseline | typescript-blocks-04-native | 42 | 42 | 0 | 0 | 42 | 0.363636364 | 0.448959768 | fail (1) |
| baseline | typescript-blocks-04-compat | 33 | 33 | 0 | 0 | 33 | 0.636363636 | 0.602811796 | fail (1) |
| baseline | typescript-blocks-05-native | 39 | 39 | 0 | 0 | 39 | 0.454545455 | 0.497207157 | fail (1) |
| baseline | typescript-blocks-05-compat | 33 | 33 | 0 | 0 | 33 | 0.818181818 | 0.805668188 | fail (1) |
| candidate | typescript-blocks-01-migration-native | 45 | 45 | 0 | 0 | 45 | 0.500000000 | 0.564229066 | fail (1) |
| candidate | typescript-blocks-01-migration-compat | 36 | 36 | 0 | 0 | 36 | 0.916666667 | 0.830186391 | fail (1) |
| candidate | typescript-blocks-02-native | 45 | 45 | 0 | 0 | 45 | 0.416666667 | 0.560195113 | fail (1) |
| candidate | typescript-blocks-02-compat | 36 | 36 | 0 | 0 | 36 | 0.750000000 | 0.791805998 | fail (1) |
| candidate | typescript-blocks-03-native | 48 | 48 | 0 | 0 | 48 | 0.538461538 | 0.521700261 | fail (1) |
| candidate | typescript-blocks-03-compat | 39 | 39 | 0 | 0 | 39 | 0.846153846 | 0.876239791 | fail (1) |
| candidate | typescript-blocks-04-native | 42 | 42 | 0 | 0 | 42 | 0.363636364 | 0.448959768 | fail (1) |
| candidate | typescript-blocks-04-compat | 33 | 33 | 0 | 0 | 33 | 0.636363636 | 0.602811796 | fail (1) |
| candidate | typescript-blocks-05-native | 39 | 39 | 0 | 0 | 39 | 0.454545455 | 0.497207157 | fail (1) |
| candidate | typescript-blocks-05-compat | 33 | 33 | 0 | 0 | 33 | 0.818181818 | 0.805668188 | fail (1) |

All supported metric values/applicability counts, prepare/index/readiness, stage identities, source manifests, budgets/config, costs, status, and replay pins appear per suite in summary and [verification receipt](verification-receipt.json). Original per-row outputs are retained without transformation in the archive.

## Raw custody and validation

[Original output archive](typescript-paired-original-outputs.tar.gz): 1173 files, 27117070 uncompressed bytes; SHA256 `4b9c4b790658a169189d3d0026f8eaba644cc6090a3366379853bfc171d84edf`. [Manifest](raw-artifact-manifest.json) and [readback receipt](archive-readback-receipt.json) verify every original byte. Includes raw responses/source verification, normalized rows, query snapshots, original scores/metrics/gates, retrieval costs, resources/latency, prepare/readiness, engine/source identities, commands and run/replay stdout/stderr. No full admitted/upstream source tree, binary, database or private source published. Original TypeScript MIT/LICENSE/NOTICE/vscode-uri license bytes remain in retained-licenses.

Two selftool diagnostics are preserved: archive allowlist initially omitted original evaluator report.md (partial archive retained locally and hashed), and independent verifier initially compared evaluator BLAKE3 raw_digest to SHA256. Original evaluator replay had correctly verified BLAKE3 on all runs; corrected independent verification uses original replay plus original/archive SHA256 inventories. No raw/score/query/gold/product edits or extra searches occurred to recover these packaging/verification errors.

Validation: real Rust1.95.0 default-feature original-lock builds both arms; actual identical frozen input/preflight checks; 20 original evaluator replays; independent stage/schedule/score/hash consistency; 1173-file archive readback. No broad suite, excluded worker test, GC/WAL/kill/staging/EROFS/private export or 100k workload run. Product sources/Cargo/lock, old gold, old1671, central tasks and CI workflow untouched. No merge/deploy.

Reproduce with fresh isolated paths: normal origin fetch exact source/admission/original author SHAs, materialize only protocol metadata and specified admitted DEV files, official Rust1.95.0 original-lock builds; paired.py build for each arm, prepare, then execute once; analyze.py, cost_audit.py, package.py, verify.py consume new retained outputs. Existing immutable outputs must not be overwritten. Adapter/scorer/normalizer hashes and fixture/parser inventories are in preflight/build receipts. Baseline/candidate caches and targets were never mixed.

## Delivery

Normal push and a single draft attempt target `integration/packing-evidence-20261003`, whose observed base is the exact candidate 37dd042. Draft body is PR-BODY.md. Remote/publication receipts record actual terminal status; Forbidden stops that action with no alternative API/auth workaround. No CI success asserted.
