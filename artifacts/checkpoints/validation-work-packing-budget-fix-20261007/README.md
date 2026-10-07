# PR143 validation-work packing repair — 2026-10-07

The original failure was a production source-retention regression. The new
optional final-validation receipt bypassed the existing freshness-detail
projection at this fixture's size and displaced a complete source body. No
budget, fixture source, exact omission assertion or source-validation rule was
relaxed.

## Fixed sources and results

| Role | Fixed commit | Scope |
| --- | --- | --- |
| Failing A source | `781f0b790389134303d74ddc41689ad9793c9b52` | Original boundary target: 2 passed, 1 failed |
| A plus packing repair | `df43b7b1ddb63f2622d7336236001840cf28d2f5` | Original target: 3 passed; new controls: 2 passed |
| A+B plus packing repair | `ec2b3b4015de61d1e098ab05a99e76d0291f06ca` | 48 passed, 0 failed; workspace fmt passed |

The combined tree is `50359edd483001c03a80b90416c04179a974e01b`.
Its complete 768-input source manifest SHA-256 is
`0ade96519b0c9450c1757a33714dd65e41e6d3b5d6b2e72c26b27aadfbdb3e98`.
The input mapping was identical before and after the combined run, and its
worktree stayed clean. Combined test and fmt commands took 44.843 seconds in
all, including compilation. The existing explicitly configured stdio test was
the sole ignored test; it is not included in the 48 passed count.

`source-change.json` binds the original 765-input A tree and the 766-input
repair tree. The only source changes are `cc-search/src/selection/budget.rs`
and the new `cc-eval/tests/packing_validation_work.rs`. All Cargo inputs,
A's original five changed crate paths, and the original boundary test remain
byte-identical. A separate new contract document explains the packing order.
The combined tree additionally contains the separately reviewed B capture
implementation; the repair-only commit is not presented as that combined test
product.

## Original failure and cause

`remote-ci-original.log.gz` losslessly stores the parent-supplied log for run
`37623178087`, job `112798257508`. Its decompressed SHA-256 is
`beb79b4e8c2919b6fcf610902901acb2fadb0afd27ba0bbc18e138b696778155`.
`baseline-original-target.log` reproduces the same failure on the unmodified
A source: line 145 expected `omitted_hits == 2`, but observed 3.
`baseline.json` pins the command, source and old test hashes.

The independent, unchanged stage diagnostic recorded seven selected source
hits before packing. `validation_work` occupied 448 serialized bytes; its
coverage explanation was 224 UTF-8 bytes. The complete freshness object was
below the existing large-detail threshold. Both fix and refactor lost
`scope/impl.py` in the core packing stage, before final-handler metadata was
attached. This was more than an outdated numeric assertion.

| Intent | Baseline public used bytes | Baseline omissions | Baseline impl body | Repaired public used bytes | Repaired omissions |
| --- | ---: | ---: | --- | ---: | ---: |
| fix | 15519 | 3 | missing | 15859 | 2 |
| refactor | 15549 | 3 | missing | 15889 | 2 |
| trace | 15482 | 3 | present | 15833 | 2 |

These are separate real runs, with measured or self-accounted serialized bytes,
not an assertion that generation/timing bytes are stable across runs. The
unchanged effective cap is 16000, configured maximum 18000 and token budget
4000. Source proofs, original retained ordering and exact fixture facets are
checked by the existing test.

## Repair and diagnostic attempts

Before another body eviction, the packer tentatively removes the whole optional
work object and includes `source_freshness.details_omitted: true` in the measured
response. It keeps that omission only when it makes the current complete
bodies fit. Otherwise it restores the whole receipt and the original value or
absence of the marker, then follows the original eviction order. It does not
rewrite or zero individual SQL fields. Sufficient-space producer receipts stay
unchanged. Missing work never means zero.

The original test SHA remains
`552066132a2ab67ba1e9d260fa8a483085756749e57623513bac366c38382942`.
It still requires exactly two omissions, impl.py, the relevant interface/test
facets, actual source verification, Partial and monotone omission behavior,
repacking and the old numeric-width bound. Its real producer test still
compares every non-scope diagnostic field exactly.

The first candidate only shortened coverage to a known label. It did not fix
the three-omission failure and violated the producer preservation assertion;
`candidate-original-target.log` records both failures. That candidate was
fully removed; there is no new coverage label or producer modification in the
final patch. `candidate2-original-target.*` and `candidate2-boundary/` are the
passing final production patch on the original A tree.

The two new controls check successful omission and its complete byte receipt,
no diagnostic resurrection at a larger substage budget, and failed probes that
restore absent/false/true/null markers and the entire receipt. They also cover
an absent receipt, graph-induced Partial and actual source-proof preservation.
The initial new-control fixture used 8000 bytes and failed its preparation
precondition because optional work was already omitted there. The final control
uses the original public 16000-byte cap with exactly 64 synthetic excess bytes
and checks that the separate large-diagnostic projection is not triggered.
This new-control preparation change did not modify the original regression
fixture, cap or assertions. Its failed prototype/log is retained explicitly.

## Commands and evidence

All Rust commands used the installed official Rust 1.95.0 toolchain,
`--offline --locked`, two build jobs, debug info disabled and incremental builds
disabled. `combined-check/validation.json` records exact commands, per-command
exit codes, timings and log hashes. `combined-check/source-sha256.json` lists
every crate/Cargo input used by that run.

The combined run includes 21 adjacent packing tests (original boundary 3, new
controls 2, p5c budget 9, scope packing 3 and Partial packing 4), final-validation
SQL accounting 5, immutable Python capture 11 and capture revalidation 11.
The independent packing review is copied as
`packing-budget-fix-independent-review.json`; its read-only/source-hash audit
is separate from these author-executed behavioral tests.

The combination retained an intermediate v4 guard. The parent owns subsequent
source-version admission and CI integration. No source guard, historical
registry/review, task state or CI rule was changed in this repair branch. This
checkpoint does not claim a full workspace run, a passing remote CI after the
repair, broad lint, non-Linux behavior or a public-quality gate.

Local pass logs only had terminal blank lines removed; failure logs are kept
unchanged, and the original remote log is losslessly compressed.
`log-storage.json` records the storage transformations and digests.
