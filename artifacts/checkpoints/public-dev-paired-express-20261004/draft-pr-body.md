This records a fixed same-input Express public DEV comparison between baseline 88f2cf0 and candidate 37dd042, using the original PR91 native/compat inputs and original schedules. Each arm executes all 387 cases with independent builds, binaries, caches and indices. No product, gold, scorer, normalizer, budget, provider or default knobs change.

Both arms remain 387 Partial and quality FAIL; zero missing/tool/protocol/timeout errors. Same-profile Top1/nDCG deltas are zero, strict no-answer is 0/33 in both arms, and native span precision decreases slightly. This does not certify quality, holdout, semantic gain, performance or CI.

Validation: actual official Rust 1.95 locked builds; original input/admission/source/license and binary hash checks; four actual seven-file prepare/index/Ready stages; all 774 scheduled rows verified; original replay changes zero bytes; derived analysis repeats byte-for-byte; 871 archived output members verified. Full raw/original output, costs, metrics, stage identities and diagnostics are retained under artifacts/checkpoints/public-dev-paired-express-20261004/. Central tasks remain open. No merge/deploy.

Fixed evidence payload: c13e21fa98b76bb31b3dca41f75f51db9efb00f0.
