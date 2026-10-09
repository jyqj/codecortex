# A23 snapshot leaf preparation — round 17

This checkpoint preserves an optional six-file product candidate and its independent scope review. It is not an executed or source-approved replacement for the active A23 study.

## Fixed identities

| Object | Commit or tree |
| --- | --- |
| Measured A23 source | `a23bb72d3c954f385b99fe81ce9189885c208557` |
| Selected donor | `0272a1fb152fd76a7cfb22386a580629d4038a64` |
| Prepared product | `62484abaae02640d5a922a017b78e6e7f966fe21` |
| Prepared product tree | `92a15629f4189ff6e8e4a1c3947fb6caf27e54c1` |

The prepared product has exactly five production-file changes and one integration-test change, with four modified files and two additions. It contains 1089 native inputs. All other A23 entries, including runtime/cold-build/rollback observers, workflows, Cargo inputs, `p8_scale.rs`, task definitions and previous artifacts, are unchanged at that product commit.

The six selected file bodies are the exact fixed donor blobs. The four pre-existing files equal the common base in A23, so these replacements do not discard A23-specific edits. The independent reviewer rebuilt the expected Git trees before creation and then read back the actual commit, sole parent, branch, trees and complete selected file contents.

## Acceptance boundary

- [non-author-scope-review.json](non-author-scope-review.json) accepts exact scope and content preparation only. Its original 6283 UTF-8 bytes have SHA-256 `4d1386a8fd840fc20f70f14475efd47adf3dd67710981a49ba40599eb1cab047` and Git blob `f13b1b6196f784daa39761097f0419175fd27f45`.
- [preparation-custody.json](preparation-custody.json) preserves the root's creation and official readback responses, including the zero-run observation and unchanged PR167 head.
- The product still carries A23's source pins. They are **not** source approval for this new combination. Exact-candidate tests and measurements are `not_run`.
- No A23 or donor observation is relabeled as a sample of this candidate. The active A23 150-shard workflow, its budgets and all older D0 failures retain their original identities.
- No original TODO was closed by this preparation: **192 total / 163 done / 29 remaining**. Full P8-005 acceptance and all original dependencies remain open.

The branch is preparation-only and has no PR. All eight inherited workflows were inspected before branch creation: push triggers are restricted to main; the complete scale workflow uses explicit dispatch or its existing PR label. The initial official head-run collection was empty. This is a dated observation, not a promise about future branch activity.
