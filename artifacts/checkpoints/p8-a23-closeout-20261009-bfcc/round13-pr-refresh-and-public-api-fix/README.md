# Round 13: PR source review and public dependency error-prefix repair

This checkpoint publishes 236 exact selected files in four verified archives. It changes evidence only. Original ledger: 192 tasks, 163 done, 29 remaining; no newly completed task is claimed.

## Contents

- `PR175-PR179-fixed-head-review.zip`: exact updated-head source identities, original review failures and corrections, and the original-contract boundary between a fixed study and later ledger updates.
- `PR180-PR181-source-review.zip`: independent full source-domain review and the coverage relationship between these drafts and fixed Gc8.
- `PR180-public-dependency-fix-source.zip`: the frozen three-file candidate patch, original files, author checks, and independent source review. Its freeze-time status is Rust not_run; subsequent actual native test evidence is recorded separately.
- `original-P7-logs-and-root-coordination.zip`: decoded original P7 job logs, external-main ledger reconciliation, and evidence-transport cleanup receipts.

The prior public API counterexample and exact SQL reproducer are in parent commit 802fbae9f984f921cdb5f78dad201dd80605d941 under `round13-pr180-public-api-prefix`.

## Source and completion boundaries

The original primary study remains source c8be5afaac568ffd40ef86d3795423c3b73c9f39, run 37902429727, attempt 1. PR175/179 external merges and PR180/181 sources are separate identities. None of their observations is pooled into the fixed Gc8 study.

Full original scale and one-hour soak acceptance remain pending at this checkpoint. Successful component reviews do not close individual dependent tasks early. The original contract permits eventual accepted-at-Gc8 evidence to be recorded in a fresh main ledger, with later external notes preserved and without inventing a latest-main N30 gate. This does not extend old measurements to later product sources.

P7 source and measurement limits, every expanded archive member path, byte count and SHA-256 are in `publication-manifest.json`. Logs are fetched decoded UTF-8 content; this packet does not attest original compressed log bytes. The development-profile one-row engineering baseline is not release or quality certification.
