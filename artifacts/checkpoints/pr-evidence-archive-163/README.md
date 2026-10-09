# Preserve PR 126 evidence without occupying an excluded path

PR #163 archived 188 original files from PR #126 at a path that the existing historical integration gate explicitly requires to be absent. This change moves the complete, unchanged Git subtree into this archive container so that the existing gate can continue to enforce its original contract.

The original subtree is `347b848c78278c44622795e1a137987c2ca766aa`, containing 188 files and 26,428,588 bytes. Every file, Git blob, mode, nested tree, original manifest, and compressed log is preserved. No redirect is left at the excluded location.

| Location | Path |
|---|---|
| Original historical prefix | `artifacts/checkpoints/localwidth4-100k-paired-20261003` |
| Current archive prefix | `artifacts/checkpoints/pr-evidence-archive-163/relocated-originals/localwidth4-100k-paired-20261003` |

Use [the exact file mapping](proposed-relocation-mapping.json) to resolve paths recorded in the original evidence. The prior PR #163 README and manifest remain unchanged records of their original commit. [Relocation provenance](relocation-provenance.json) binds both locations, the original PR and head, the archive commit, and the retained subtree.

The original results remain unchanged: the baseline timed out; the candidate stopped early at its 12 GiB resource guard; post-ready work was `not_run`. This archive is not a completed paired 300-second throughput result or evidence of statistical significance.

[Independent design review](reviews/independent-design-review.json) checked all original mappings and the existing guard read paths. [The path control](reviews/root-existing-guard-path-control.json) executes the unchanged gate function against a private fixture, showing rejection at the old location, acceptance after relocation, and rejection of a redirect. That narrow control is not a full CI run. The actual pull request checks and subsequent independent tree review carry their own identities.

No product code, validation script, workflow, task definition, or task status is changed. The original task ledger remains 192 total, 163 done, and 29 remaining.
