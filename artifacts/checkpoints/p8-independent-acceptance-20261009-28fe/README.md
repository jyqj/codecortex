# P8 independent acceptance repairs — workspace 28fe

## Outcome and task accounting

This supplement targets the active P8 completion candidate in PR #159. Product commit
`f2366d9139049d301f4f1d6695d307f52d8855b3` changes exactly six Python controller and
regression-test inputs on parent `f8ca8070592a1bc03cedc2df4e27e1382a3077a7`.
All 1,087 Rust/Cargo inputs remain byte-identical to the previous accepted product
`94364da0cf771ff8f9b4dc81779cfc35e58f11a3`. The complete validation inventory remains
135 files, with only those six reviewed contents changed.

The original roadmap remains **192 total / 163 done / 29 remaining**. Newly completed
original TODOs in these repair rounds: **0**. `acceptance-review/acceptance-matrix.md`
records the full requirements for P8-005 through P8-013 and P8-016. These repairs
advance those checks; they do not satisfy missing scale/runtime evidence or bypass
hard dependencies. No task status, acceptance criterion, sample count, timeout,
performance threshold, original source-proof predicate or CI workflow is relaxed.

## Repairs and independent ownership

| Component | Repair | Author | Independent reviewer |
| --- | --- | --- | --- |
| Portable platform collection | Revalidate the same Cargo/build/log/feature/target contract as local validation; bind original eight-cell selection; retain invalid collection receipts | `/root/acceptance_review` | `/root` |
| Rollback manifest | Accept both original list and historical map formats under their recorded digest/count; reject ambiguous entries; hash and parse the same schema bytes | `/root` | `/root/pr_audit` |
| Runtime termination | Preserve completed unequal-parity results and statistics; cancel and converge owned work before sealing; retain raw-budget failures; account for submit failures and KeyboardInterrupt | `/root/runtime_review` | `/root/acceptance_review` |

All names in this table refer to workspace `28fef0db5e01`. Earlier PR #159 reviews
retain their original workspace and authorship; they are inherited only for unchanged
inputs. The prior main review is preserved verbatim under `history/` and remains
available at its original immutable review commit.

The previously reproduced semantic-only backfill Cargo feature defect was independently
fixed by the owner of PR #159. This supplement inherits that change from its parent
and does not duplicate it. The minimal real Rust 1.95 compiler reproduction is retained
under `cargo-feature-repro/`; it is not a full-product runtime certification.

## Validation and original failures

- Platform: 42 independently executed Python controls passed.
- Rollback/recovery: 29 independently executed Python controls passed.
- Runtime: 30 independently executed Python controls passed after the third repair round.
- Complete `test_p8_*.py` integration: **350 passed**, exit 0. The final log SHA-256 is
  `9bc8857b42ca5f6cfe2e319c64d2977c8b1c361bd4c2af6fa5b4e3768c5fcfde`.
- The first two integration attempts failed because this sparse checkout omitted tracked
  historical fixtures. Their original logs and receipts remain under `integration-validation/`.
  Exact HEAD blobs were restored; test bodies and thresholds were unchanged.
- Original synthetic red/green reproduction directories are preserved in
  `runtime-review/raw-reproductions.tar.gz`, with every member bound by
  `runtime-review/raw-reproductions-index.json`. The initial `oracle-mismatch-before/`
  harness contained a syntax error and is not a valid product-defect reproduction;
  `oracle-mismatch-before-v2/` is the valid original counterexample. The old
  `cleanup-before/` seal remains invalid exactly as observed; it was not rewritten.

`evidence-index.json` binds every original copied audit file by bytes and SHA-256.
The standalone product-binding audit rechecks complete manifests, all six independent
review hashes, and the original integration logs. The subsequent v15 pin/old-proof
checks must be recorded separately after their exact REVIEW commit exists.

## Remote observations and limits

Remote CI logs retain their original run/job/commit identity. The scale run
`37824742267` belongs to `6e3eb4fd97edcf238774e74b88d397e059337bac`; it is not relabeled
as a later product. Earlier `5610335` main CI failed its original 500 ms index gate
with 528.02 ms. That failure is preserved and has no asserted root-cause fix here.
The later `f8ca807` backfill success is bound to job `113491126945`; raw artifact
acceptance remains separate from observing its success and seal-check log.

PR #160 is a separately forked active candidate, not a child of PR #159. No changes
from that candidate or its executions are silently incorporated into these approvals.
Historical draft PR heads are retained pending explicit evidence-based adoption review.

## Rollback and replay

The product change is isolated in `f2366d9139049d301f4f1d6695d307f52d8855b3` and can be
reverted together with its later source-admission metadata if needed. Controller
fixtures are isolated from user projects and databases. Preserve both successful and
failed observation archives across any rollback. `runtime-review/integration-guidance.json`
records the exact unchanged v14/v15 proof commands and their original fixture requirements.
