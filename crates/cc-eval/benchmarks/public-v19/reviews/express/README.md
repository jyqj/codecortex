# Independent Express public-dev source review

Author freeze SHA: `a739431b6595bb44e79130843e27e2cde2d34eb1` (PR68).
Protocol SHA: `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6` (PR65).
Source SHA: `7ef98448f8b38099ab1ded55e458538ad47a51e7`.
Delta author SHA: `0850acc4fdacde2609c56cf733b7776e5027ceff` (PR69).
Delta allowlist SHA256: `b8b7fe5a22222fc91cd8e09cdccbe24e3d2fadf8e1a60df02e27a9d2ac4f6b15`.
Reviewer: `independent-source-review/cloud-express-dev-review`; author: `B/express`.

Public reports contain counts, hashes, decisions and error codes only. They contain
no query, rationale, answer text, source excerpt or question-bearing comment.
Per-row exact-byte SHA256 and opaque family/component SHA256 identify every
decision. This reviewer did not author this corpus and does not sign as its author.
Only this review namespace changes; no author gold, source, protocol, registry,
production, scorer, task or gate ledger is edited.

## Frozen counts

| Review block | Dev rows | accept | reject | needschange | Visible local components | Local accept | Local needschange |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| First complete public-dev block | 20 | 20 | 0 | 0 | 20 | 20 | 0 |
| Full fixed PR68 | 68 | 61 | 0 | 7 | 66 | 59 | 7 |
| PR69 new public-dev delta | 2 | 1 | 0 | 1 | 2 | 1 | 1 |
| Combined current public dev | 70 | 62 | 0 | 8 | 68 | 60 | 8 |

The first block is a public-dev review block, not a protocol formal complete20
block containing separately reviewed/custodied holdout. Full and first-block
counts overlap; never add20+68. Native/compat are one family, not two samples.
Current projections contain70 native/59 compat and11 bounded no-answer rows.

Holdout body reads0; historical candidate body reads0; rank/provider/scorer runs0;
other-repository body reads0; source-read refusals0. Confirmatory accepted holdout0;
formal complete20 blocks0. The32 blocked holdout components were not reviewed or
resurrected from public Git history. No claim is made that the total100 candidate
components are globally independent or accepted.

## Evidence status

All8 fixed official upstream source/license files were independently downloaded
from the exact raw GitHub source SHA, and match the admitted snapshot byte for
byte. Per-file SHA256, Git blob SHA1, byte lengths and UTF8 checks pass. The MIT
license SHA256 is `95a5762890e5c1c9808921cef095661fc482c5e1f0bba31446ac85595df6237c`.
All6 copied protocol files match their exact protocol Git objects and SHA256.

Every visible row's cited source bytes, line range, SHA256, native alternative
path/symbol/span mapping, required-facet/group references, grades, fixed dev split,
native/compat stable path projection and chain endpoint indices were checked.
These structural checks do not sign semantic correctness. This independent review
separately read the public-dev tasks and evidence against the pinned source, checked
source-task meaning, declarations/bindings and edge direction/target identities,
and inspected the admitted definition/import/delegation surface. Bounded absence
claims were restricted to the seven admitted files, verified against their actual
bytes and external boundaries; literal absence alone was not treated as proof.
No unsupported external implementation or alternative metric was invented.

All11 bounded no-answer dev rows pass this source review. Native empty arrays and
compat exclusion are verified; this is not retrieval rejection accuracy. Each
answer group's supplied alternatives was checked; no unseen alternative answer
or performance-dependent rewrite is introduced. The actual evaluator's BLAKE3
freeze/validate is not independently rerun here, and no ranking metric is claimed.

The7 PR68 needschange rows comprise one citation mismatch and six chain metadata
target problems. Their source byte/span hashes still pass; six rows' underlying
answer facts and all compat path projections pass. The delta's one needschange
has valid facts/spans but a primary-group designation problem. Accepting path-only
compat or input-schema validation must not silently accept those native obligations.

Current local relation pairs are reviewed as visible positive/negative counterparts.
One additional pair requires source-task-equivalence adjudication; its two row and
component hashes are in `component-dispute.json`. No relation, canonical ID, split,
gold or accepted count in the author shard was changed. Shared words or merely
sharing a file were not used as equivalence. The dispute does not silently merge
or rebalance the split. No would-be holdout member was opened to resolve it.
Cross-repository associations remain outside this supplied review scope, so local
component accept does not establish the global sampling denominator.

The PR69 allowlist is verified before its dev file bodies are read. Only the two
new appended dev rows are semantically reviewed in the delta; exact old native/
compat prefixes, relations and source bytes remain unchanged. The two obligations
are locally distinct from the previously visible dev tasks; claims involving
unread blocked members or other repositories remain unreviewed.

## Error codes and bounded remediation

| Error code | Count | Meaning / author or designated-reviewer action |
| --- | ---: | --- |
| R_CITATION_SYMBOL_WRONG | 1 | Cited symbol identity does not match the source statement inside its byte-valid span. Author must version the binding citation; valid hashes alone do not prove this fact. |
| R_CHAIN_EXTERNAL_TARGET_INTERNAL | 3 | An external target is represented as the internal caller endpoint. Represent the excluded boundary explicitly; do not score a false self edge. |
| R_CHAIN_DYNAMIC_TARGET_INTERNAL | 1 | A dynamically obtained callback is represented as the caller endpoint. Declare the actual dynamic target/boundary and edge kind. |
| R_CHAIN_TARGET_WRONG | 2 | The labelled target resolves to a different internal declaration than the recorded endpoint. Correct source-grounded target identity/direction. |
| R_PRIMARY_GROUP_NOT_ANSWER | 1 | The primary group cites setup/binding evidence while the requested behavioral branch is a secondary group. Independently version/review primary and required-secondary designation. |
| C_TASK_EQUIVALENCE_REVIEW_NEEDED | 2 rows / 1 pair | Source-task overlap warrants designated global component adjudication before counting those two IDs as independent. Never adjust IDs/split to obtain a quota. |
| G_CROSS_REPOSITORY_DEV_NOT_SUPPLIED | scope blocker | No cross-repository global independence certification from this single supplied shard. |
| H_CUSTODY_BLOCKED_UNREVIEWED_32 | 32 | No restricted custody or holdout review established; no body/history access attempted. |

These are source-derived findings before ranking. This reviewer leaves all8 rows
in needschange and does not patch author gold to fit product behavior. Versioned
author fixes require a new independent delta review; global adjudication and
custody remain separate responsibilities.

## Reproduce the counts/hash-only checks

Fetch the three explicit PR refs if their exact objects are absent. Do not open
old candidate query bodies or execute the author's historical migration script.

```sh
git fetch origin refs/pull/65/head:refs/remotes/origin/pr65
git fetch origin refs/pull/68/head:refs/remotes/origin/pr68
git fetch origin refs/pull/69/head:refs/remotes/origin/pr69
python3 crates/cc-eval/benchmarks/public-v19/reviews/express/review_dev.py --limit 20 --check
python3 crates/cc-eval/benchmarks/public-v19/reviews/express/review_dev.py --limit 68 --check
python3 crates/cc-eval/benchmarks/public-v19/reviews/express/review_delta.py --check
```

The scripts verify and replay the frozen independent decisions; they do not
automatically discover or certify source semantics. Their immutable allowlist
restricts query reads to the explicitly authorized current dev projections.
An initial local check lacked the protocol commit object; fetching the authorized
PR65 ref resolved it. There was no upstream source refusal or alternate-channel
retry. No compilation or provider was required.
