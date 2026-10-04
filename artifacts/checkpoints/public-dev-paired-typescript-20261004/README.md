# TypeScript fixed public DEV paired evaluation — 20261004

Status: build and immutable input preparation complete; rankings not yet issued at plan commit.

Candidate source 37dd042eaa1209a86e0cafdcd92ae77e036e76f5, production source/Cargo/lock identical to 90858afae647a513537bf118932a7ba5020ee98b. Baseline 88f2cf099c8b81f3acef485fd5ac9b01c63ce790. Normal official Rust 1.95.0 locked build succeeded in separate checkout/target/binary/Cargo cache directories. Compiler-artifact and binary/source hashes retained per arm.

Original PR91 admission 5385f5a7a2a875c6d5cbd049bdde039bf71bbf32 SHA256 b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425. Exact TypeScript subset of JS schedule in 535ff1b13b841af8021346a660c83c57919525e2: ten suites, 73 native/59 compat, 219/177 scheduled rows per arm, repetitions 3/warmup 0/seed 20261003/top_k10/timeout30000ms. Same admitted source/query/gold/suite bytes both arms. No PYGO taxonomy. No extra pilot/repeated best-of; each scheduled case once per frozen repetition per arm. Baseline then candidate for each original suite. Product/scorer/normalizer/provider/config/thresholds unchanged.

This is exposed public DEV, 72 local correlated components, two projections of existing questions, not 600 or new samples. Clean holdout 0. Previous 1671 all Partial/qualityFAIL remains untouched. No six-repo, live-semantic or walltime/performance causal inference; no new CI claim or central TODO closure. Applicable AGENTS.md/.agents/skills absent; benchmark README and original protocol/runner read.

Plan and selftools frozen before this task's rankings. Prior results are known, disclosed; no gold/product tuning. All raw original outputs/source verification/cost/status/diagnostic evidence will be retained under this prefix with original license bytes, without full input source tree, binary or database publication. Run/replay byte equality is verified for each suite. Missing observations invalidate comparison and are not filled. Low score/Partial quality failures do not prevent remaining intact suites.

An initial preparation missed the original author Git object before any rankings. Normal origin fetch of exact 4c01f627bd6df73e70bfe1dbdc42b2a9234e797a restored it; empty first input directory remains, successful preparation uses fresh inputs02. No denied operation workaround.
