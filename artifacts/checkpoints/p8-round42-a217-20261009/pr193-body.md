## Current status — 2026-10-09, 18:40 UTC

The published head is now **[`52bef997`](https://github.com/jyqj/codecortex/commit/52bef997f0825451e32fc6291f90cd8df5c8bbd7)** following a concurrent branch update. Its chain is original `4fe` → product `ce9891e1` → review `dfee4317` → guard `52bef997`. The product commit's entire tree is **`5fc7edc2dbf33bcf1a57e96ecf7c09b0cfaf6bba`**, exactly the same as the independently reviewed four-file formatter candidate `8b29dd83`. The duplicate local publication plan is paused; it must not overwrite the new branch.

[Current normal CI, run 37974288651](https://github.com/jyqj/codecortex/actions/runs/37974288651), is attached to `52bef997`. At the latest job-level readback, check `113968501720`, MSRV `113968501411` and security `113968501748` were all queued with empty steps. New native formatting, tests and original Python guard results have not yet been established. The published static/V8 review is distinct from those pending executions.

The separately running original full study continues on **source `4fe` / run 37962416564 / attempt 1**. GitHub Actions continues while the local workspace reports `409 environment_offline / Environment is not connected`.

## Problem and resulting behavior

Incremental publication serializes already captured state more than once, while the exact oracle always writes two complete canonical row sets. This PR reuses immutable work and avoids the second scratch row set when a complete, conservative equality witness succeeds.

- **Captured project inputs:** reuse the digest already computed by the private immutable capture and borrow the same inputs during acknowledgement. Metadata comparison/publication and configuration verification remain at their existing points.
- **Durable frontier:** prepare its validated payload once at the original transaction preflight and consume that bound payload at the original publication point. Clearing, error ordering, generation checks and rollback behavior remain covered by controls.
- **Oracle:** after fully spooling A, validate all of B and compare exact typed SQLite cells under the same projected column layout. Only ordinary stored tables can take this path; both EOFs and counts must match. Signed-zero bits, mixed SQL types, different layouts, views and virtual tables retain the full original comparison. All 15 tables, duplicate multiplicity, original digests and logical/resource limits remain in scope.

A failed witness falls back to the original complete second spool. A late mismatch can therefore add a full B projection pass. The held frontier string also lives through the fact write; this PR does not claim every input has lower time or memory.

## Fixed source and independent review

This is a follow-up to merged [#180](https://github.com/jyqj/codecortex/pull/180). Main merge `23eab8a7ea1fdfd6333c2e5a001cfb41e2a10d69` has exactly G6B's tree; the net product delta is six files.

| Role | Fixed commit |
| --- | --- |
| Product P8B | [44e62481](https://github.com/jyqj/codecortex/commit/44e6248106335b5f0625e53de38f76d26aeeadcf) |
| Independent R8B | [cb57d6e3](https://github.com/jyqj/codecortex/commit/cb57d6e39f446e68ed5bd5bce26a7a76d1a26e5c) |
| Original study G8B | [4fe92748](https://github.com/jyqj/codecortex/commit/4fe927488d5cb7f26bd27c7624745fb1b6ca202b) |

[The complete review and originals](https://github.com/jyqj/codecortex/tree/cb57d6e39f446e68ed5bd5bce26a7a76d1a26e5c/artifacts/checkpoints/p8-immutable-payload-cell-witness-bfcc-20261009) bind 1,095 product inputs, 75 original-BASE differences and 142 unchanged validation inputs. R8 adds exactly six artifact files, including the retained P7/v1 review and both complete diagnostic archives. G8 changes only the original five registry fields and four guard identity constants. The original source-verifier algorithms, historical proofs, workflows and task definitions are unchanged.

The current formatter follow-up changes exactly four existing product files through the original formatter hunks, adds [two new review artifacts](https://github.com/jyqj/codecortex/tree/dfee431744820caab8436e8d201933d995e0f576/artifacts/checkpoints/p8-G8-format-admission-bfcc-20261009), and updates the registry/guard bindings. Its actual P/R/G commits are `ce9891e15a0a4fc085e5fa8a57a172d3df458c36`, `dfee431744820caab8436e8d201933d995e0f576`, and `52bef997f0825451e32fc6291f90cd8df5c8bbd7`. The original failed check and original study remain bound to `4fe`.

## Verification

Independent source and actual Git-object reviews passed. Added controls compare borrowed versus owned acknowledgement bytes, frontier validation/rollback, original oracle digests and full byte accounting, signed zero, types/layouts, complete EOF, row/canonical limits, and view/virtual-table fallback.

Two separately frozen Python/SQLite diagnostics each completed 18 calls with all nine full-output byte pairs equal. They are limited design diagnostics, not Rust performance measurements. In v2, paired median candidate/old wall ratios were 0.90763 for same-order equality, 1.36832 for reverse-order equality and 1.22545 for a last-row mismatch. All trials and adverse cases are retained; no cross-v1/v2 speedup is inferred.

[Historical 4fe CI check 113927770107](https://github.com/jyqj/codecortex/actions/runs/37962259107/job/113927770107) failed `cargo fmt --all -- --check` at 18:07:39 UTC. Clippy, compilation, regressions and original guards in that job were skipped. The four affected files are the freshness test, streaming oracle, streaming oracle tests, and project model. The complete original 44,391-byte failure log is retained; the repair follows all nine original hunks, including the formatter's single-expression match-arm block and optional trailing commas.

The separate [MSRV job 113927770517](https://github.com/jyqj/codecortex/actions/runs/37962259107/job/113927770517) subsequently completed both Rust 1.95.0 checks successfully. This compilation result does not establish the skipped test controls. Security remains pending in the latest job snapshot.

The independently prepared local candidate `8b29dd83` has sole parent `4fe` and changes exactly the same four files now present in product `ce9891e1`; their complete product trees are identical. The local candidate's complete mapping remains 1,095 product inputs, 75 original-BASE differences and 142 unchanged validation inputs, with only four product/delta after hashes changed. Its frozen core and second independent report were saved as immutable GitHub blobs. Its first formatter peer was written before the disconnection and still needs local readback for archival custody.

The local `8b29` candidate did not create its own R/G and did not execute v15 or task-plan. That historical preparation limit does not mean the current `ce9891`/`dfee431`/`52bef997` objects are absent: they have been read from GitHub and the current chain is published. The actual new native/guard outcomes must come from this current source's CI; no result is transferred from static generation or from the old source.

G6B's successful main/P7 checks retain their own source identity.

## Original studies and TODO accounting

The existing `p8-scale-run` label launched [the original complete study](https://github.com/jyqj/codecortex/actions/runs/37962416564), bound to **source `4fe927488d5cb7f26bd27c7624745fb1b6ca202b` / run 37962416564 / attempt 1**. Local unchanged original intake has accepted **3/150 complete shards and 32/1500 samples**: repetition 0 at 1k, 5k and 10k. These are finite partial observations, not complete scale or tail acceptance.

The 50k first shard subsequently finished successfully at 18:29:36 UTC and produced original artifact `11637747431`; it has not yet been downloaded or locally validated, so it is not counted above. The 100k first shard remains in progress. The original remaining 145 shards expand only after all five preflights pass. The five-hour worker deadline, 512 MiB captured-output limit, 350-minute job limit, fixed five scales × N30 and original 1,500-sample protocol remain intact. No partial aggregate has been substituted.

The existing #189 cold study remains bound to its own original `044c008c…` source. #192's separate 1,350-sample fresh-history profile study is untriggered. Samples are kept with their original producers and are not pooled across these studies. A later formatter-only branch update does not re-trigger this labeled-only measurement workflow; the running study remains bound to its original event source.

Original C3ff/Gc8 deadline failures and partial sequential cohorts remain preserved. The task ledger stays **163 done / 192 total, 29 remaining, 0 newly completed**. Engineering merge and original task/release acceptance remain separate decisions; incomplete or failed studies are not marked done.