## Current status — 2026-10-09, 18:32 UTC

The published head remains **`4fe927488d5cb7f26bd27c7624745fb1b6ca202b`**. Normal CI found nine formatter hunks in four files; the repair is prepared as product commit [`8b29dd83`](https://github.com/jyqj/codecortex/commit/8b29dd836d5e53f098ae0071c5b661f2b4475493), but has not moved this branch. Independent full-byte formatter reconstruction and actual P/tree/maps/core reviews passed. R/G creation, final guard pins, and the original v15/task-plan executions for this repair remain pending because the workspace reports `409 environment_offline / Environment is not connected`.

The separately running original full study continues on `4fe`, run **37962416564, attempt 1**. GitHub Actions is unaffected by the workspace interruption; its original source and protocol are unchanged.

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
| Current G8B | [4fe92748](https://github.com/jyqj/codecortex/commit/4fe927488d5cb7f26bd27c7624745fb1b6ca202b) |

[The complete review and originals](https://github.com/jyqj/codecortex/tree/cb57d6e39f446e68ed5bd5bce26a7a76d1a26e5c/artifacts/checkpoints/p8-immutable-payload-cell-witness-bfcc-20261009) bind 1,095 product inputs, 75 original-BASE differences and 142 unchanged validation inputs. R8 adds exactly six artifact files, including the retained P7/v1 review and both complete diagnostic archives. G8 changes only the original five registry fields and four guard identity constants. The original source-verifier algorithms, historical proofs, workflows and task definitions are unchanged.

## Verification

Independent source and actual Git-object reviews passed. Added controls compare borrowed versus owned acknowledgement bytes, frontier validation/rollback, original oracle digests and full byte accounting, signed zero, types/layouts, complete EOF, row/canonical limits, and view/virtual-table fallback.

Two separately frozen Python/SQLite diagnostics each completed 18 calls with all nine full-output byte pairs equal. They are limited design diagnostics, not Rust performance measurements. In v2, paired median candidate/old wall ratios were 0.90763 for same-order equality, 1.36832 for reverse-order equality and 1.22545 for a last-row mismatch. All trials and adverse cases are retained; no cross-v1/v2 speedup is inferred.

[Normal CI check 113927770107](https://github.com/jyqj/codecortex/actions/runs/37962259107/job/113927770107) failed `cargo fmt --all -- --check` at 18:07:39 UTC. Clippy, compilation, regressions and original guards in that job were skipped. The four affected files are the freshness test, streaming oracle, streaming oracle tests, and project model. The complete original 44,391-byte failure log is retained; the repair follows all nine original hunks, including the formatter's single-expression match-arm block and optional trailing commas.

The separate [MSRV job 113927770517](https://github.com/jyqj/codecortex/actions/runs/37962259107/job/113927770517) subsequently completed both Rust 1.95.0 checks successfully. This compilation result does not establish the skipped test controls. Security remains pending in the latest job snapshot.

For the unpublished repair, the actual P has sole parent `4fe` and changes exactly the four expected files. The complete mapping remains 1,095 product inputs, 75 original-BASE differences and 142 unchanged validation inputs; only four product/delta after hashes change. The frozen core and second independent report were saved as actual immutable GitHub blobs. The first independent formatter report was written before the workspace disconnected and still needs to be read back for the planned review archive. No original v15 or task-plan execution for the repair has started, and no R/G or ref publication is claimed.

G6B's successful main/P7 checks retain their own source identity.

## Original studies and TODO accounting

The existing `p8-scale-run` label launched [the original complete study](https://github.com/jyqj/codecortex/actions/runs/37962416564), bound to **source `4fe927488d5cb7f26bd27c7624745fb1b6ca202b` / run 37962416564 / attempt 1**. Local unchanged original intake has accepted **3/150 complete shards and 32/1500 samples**: repetition 0 at 1k, 5k and 10k. These are finite partial observations, not complete scale or tail acceptance.

The 50k first shard subsequently finished successfully at 18:29:36 UTC and produced original artifact `11637747431`; it has not yet been downloaded or locally validated, so it is not counted above. The 100k first shard remains in progress. The original remaining 145 shards expand only after all five preflights pass. The five-hour worker deadline, 512 MiB captured-output limit, 350-minute job limit, fixed five scales × N30 and original 1,500-sample protocol remain intact. No partial aggregate has been substituted.

The existing #189 cold study remains bound to its own original `044c008c…` source. #192's separate 1,350-sample fresh-history profile study is untriggered. Samples are kept with their original producers and are not pooled across these studies. A later formatter-only branch update does not re-trigger this labeled-only measurement workflow; the running study remains bound to its original event source.

Original C3ff/Gc8 deadline failures and partial sequential cohorts remain preserved. The task ledger stays **163 done / 192 total, 29 remaining, 0 newly completed**. Engineering merge and original task/release acceptance remain separate decisions; incomplete or failed studies are not marked done.