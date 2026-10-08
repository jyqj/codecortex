# Publication identity

The first four local commits and their original review/test records are retained
verbatim in `../history/local-reviewed-commits.bundle`. The bundle requires public
base `f8ca8070592a1bc03cedc2df4e27e1382a3077a7` and contains local product `f2366d9`,
review `8aebe297`, pins `d5ec0c4f`, and progress `ffa88631`.

The authenticated GitHub connector published product
`b592d57756adf20361e18a2b54045aedfb056cd8`, whose entire tree is exactly
`57d41b7184ffc959f2be19b3349a0c16ed588b02`, identical to the independently reviewed
local product. The publication review checks all 21,383 recursive Git tree entries
and the six exact file hashes; no implementation content changed.

Original logs remain bound to their original commits. The updated main review
records this identity mapping, and final published pins must pass the complete
source verifier separately. Task completion remains 163 done / 29 remaining.

## Published review and final verification

The published review is `33a36ae5514fe4c5f1a74da315d373a7084787f1` (tree `4eaed587fa8659b99b47efb828472a36bfea8acf`). The admission metadata commit is `88f864b59320dc24c0db549551ac695667cbe247` (tree `3af2485f2d79d5dca2ff4b3863dd4f6c258aed9a`). The published product remains `b592d57756adf20361e18a2b54045aedfb056cd8`, with the exact same whole source tree as the originally tested local product.

`publication-v15-final-independent-review.json` independently verifies the two remote commits, their parents and trees, all 95 review paths, the unchanged original records and the two admission files. `verification/receipt.json` records a new actual full v15 source verification on the published identities, plus task, facts and whitespace checks. All four commands exited zero. The earlier 350 P8 controls and 28 v14/v15 controls retain their original local execution identities; they were not relabeled or represented as a second run.

The original `head_before_pin_commit` field in the earlier receipt captured the local HEAD observed when that receipt was written, not the start of a pre-pin run. Its original bytes are preserved; `integration/index.json` already records this interpretation. The new receipt uses `head_at_verification_start`.

Original task accounting remains 192 total, 163 done, 29 unfinished; this supplement completes zero original tasks and does not change their prerequisites or acceptance criteria.
