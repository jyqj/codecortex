# CodeCortex original-task acceptance audit — 2026-10-12

Reviewer: `/root/acceptance_audit`. This is a read-only independent audit of prior
acceptance, with new local audit outputs only. No remote branch, PR, task state,
product, original source, original artifact or acceptance threshold was changed.

## Result and counting boundary

- Immutable observed main: `c2b726538cacfdf6ae9f01977a7b93da48d2e2b2`.
- Current ledger: **192 total, 176 done, 3 in_progress, 12 todo, 1 blocked; 16 remaining**.
- This audit closed **0** original TODOs.
- Historical original-task progress from the prior 163-done baseline is **13**:
  P8-005 from #197, then P8-006 through P8-013 and P8-016 through P8-019 from #202.
  These are historical closures, not newly earned by this audit or PR management.
- Direct raw replay in this audit supports the P8-006 descriptive evidence.
  P8-005 and the other eleven #202 task decisions were reviewed at the pinned
  document/source/declared-scope level; their full original experiments were not
  all rerun. This audit does not certify G8 or approve a release.

## P8-006: actual originals independently checked

The raw custody index was freshly retrieved through GitHub at immutable commit
`e21a9ae93b0a9d017909ab02d2c4a38245e35b42`, Git blob
`062cd1b36269c2a4f8c7b5fe0e03b79b9eaf8e1b`. The retained local index matches that
exact Git blob. Every one of its 45 original ZIPs was located and checked:

- 45 complete ZIP SHA-256 and computed Git blob hashes matched.
- All 118 ordered published byte-leaf ranges matched length, offset, SHA-256 and
  Git blob hashes; total original ZIP bytes: **68,960,381**.
- Full ZIP CRC inspection passed for every archive.
- Original task receipts all identify source
  `f97c5068d056969705e8387ba1f37d83847f5bee`, run `38026411200`, attempt `1`.
- All raw lines were parsed: 45 run headers, 40 cold comparisons, 40 mutations,
  40 terminal mutation records, 5 fanout starts and 5 fanout finishes, plus 1,794
  matching native build start/finish records.
- The unchanged original `p8_task_profile_matrix.inspect_raw` was invoked for all
  45 cells after checking its two source blobs against current GitHub. Every
  original raw predicate passed; all **85 measurement objects** exactly match the
  published original aggregate matrix SHA-256
  `307900563e166677db09721100179f8d885ea5e57acb643876a2abbbcac51fca`.

This executes only a raw parser and Python custody checks. It does not run the
native binaries, collect new measurements, or rerun the original aggregate.
`verify_45_originals.py` and `raw45-command-execution.json` record the reproducible
command and its actual outcome. `raw45-audit-result.json` retains all 45 rows.
The original aggregate's 42-input Actions failure remains a failure. The later
45-input local recovery is a separate historical action, not a new Actions pass.

## Scope and quantitative findings

The independent study was preregistered as N=1 per cell: eight fresh mutation
profiles across five scales, plus five fanouts. Its five cold points are a subset
of its 40 setup records, so the count is 85, not 90. It does not replace the old
150-shard or 1,350-cell N30 studies. It claims no stable tail, confidence interval,
cross-host causal speedup, full V20, or G8 result.

The original P8-006 clauses require the named mutation/fanout coverage, phase
counts, explicit incomplete status, and full parity after closure. The pinned
09-BENCHMARK document calls N>=30 a starting query-profile recommendation and
N>=200 a formal tail requirement; 06-VALIDATION states Vxx are validation packages
and reserves full release scope for G8. N=1 alone is therefore not a substantiated
reason to reverse this explicitly descriptive task decision. No raw-evidence
defect invalidating P8-006 was found in this audit.

The retained data do expose material performance work:

| 100k profile | Incremental engine time | Full control | Incremental builds |
| --- | ---: | ---: | ---: |
| body | 3,065.083 s | 685.832 s | 490 |
| API | 1,259.137 s | 558.806 s | 171 |
| config | 1,915.250 s | 599.643 s | 330 |

These are individual measured observations, not statistical speedup estimates.
The large repeated closure work is a practical optimization lead; no causal
product defect is asserted without further analysis and fresh validation.

## P8-005 / #197 historical closure

GitHub confirms #197 merged as
`379099a4a7b09ebe9928c190017bae53d8d1edf6` on 2026-10-09 at 22:26:44 UTC, with head
`746181eb0efcecd87e9e174bbefedc96acaca7cb` and base
`51138c236cf2ed47858e2306f9c7bfd3600aaef9`.
Its five-file documentation change marks only P8-005 done, yielding 164 done.

The pinned custody ledger at commit
`a2daea38bc72cb3cf9c95535b852fff214debf2b`, blob
`41cc5217cffe5167a47c5fbd36817bc695165959`, was freshly retrieved. It has exactly
150 distinct (scale, repetition) slots, 30 at each of 1k/5k/10k/50k/100k. All 150
declared receipt digests equal their exact-original-aggregate links. It binds
source `044c008c9459cfa61e7701db2eb868342c508a39`, run `37954851017`, attempt 1,
and original matrix SHA-256
`d608a541af36e0ac5fbe3e9534bd1649e01727c5408f025f6da0250725926bae`.

The independent docs review at commit
`a258fc6607a754b708f9d9abd54537fdf2edcf43`, blob
`77db45eb277c0ec2c5de4daaab67a1a8e5ab0576`, accepts the original fixed-source cold
scope, preserving all other 191 task records and immutable definitions.
This audit checked these documents and slot/receipt metadata; it did not restore
or rerun all 150 raw cold measurement archives. Vectors remain disabled/null,
environment strata remain N=1, and no complete V20 or release claim is inferred.

## Source binding and remaining limitations

Fresh Git tree reads show current main and `f97c5068...` have identical:

- crates tree `7cb70a9b0843fa9c222996b626b78fd97a5ad63b`;
- scripts tree `f6bd02b2fb83c34d7b9182604a5d34325b63c0ef`;
- workflow root tree `ede4a22fef1d3b8c776e225d847ed9c80e4e3f56`;
- Cargo.lock blob `0c0bdc1601013b195c3580be4b0a9238557251cd`;
- Cargo.toml blob `c376fdc70465a237022de8a05b7b27cc28f72cc8`.

The older G275/Gc8/4d18 component measurements keep their own source/run identities.
Gc8 source `c8be5afaac568ffd40ef86d3795423c3b73c9f39` has a different crates tree
`67707c0ff283b0393047b57b82fbcc708fbc29f9`; current regression coverage is not a
claim that its historical platform/mixed/fault measurements ran on current main.
This limits any current G8 assertion. The PR's scoped historical task acceptance
must not be promoted to full current-source release certification.

The remaining P7-018, P8-014, P8-015, P8-020 and P9-001..012 remain open.
New live-model or judge evidence remains subject to the original authorization,
budget and clean-custody conditions. Performance sampling and release evidence
gaps are substantive; a progress-doc PR or passing archive integrity check does
not close them.

## Primary evidence

- [PR #202](https://github.com/jyqj/codecortex/pull/202)
- [PR #197](https://github.com/jyqj/codecortex/pull/197)
- [Current task source](https://github.com/jyqj/codecortex/blob/c2b726538cacfdf6ae9f01977a7b93da48d2e2b2/docs/roadmap/code-index-v2/tasks.json)
- [Original 47-clause acceptance](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/formal-twelve/47-clause-acceptance.final.json)
- [Complete 45-custody index](https://github.com/jyqj/codecortex/blob/e21a9ae93b0a9d017909ab02d2c4a38245e35b42/artifacts/checkpoints/p8-round48-acceptance-bfcc-20261010/complete-45-raw-custody.published.json)
- [Original full45 recovery review](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/aggregate-recovery/complete45-recovery-execution-review.json)
- [Original cold150 custody](https://github.com/jyqj/codecortex/blob/a2daea38bc72cb3cf9c95535b852fff214debf2b/artifacts/checkpoints/p8-round28-cold150-complete-custody-ledger-bfcc-20261009/complete-cold150-custody-ledger.json)
- [Original P8-005 independent docs review](https://github.com/jyqj/codecortex/blob/a258fc6607a754b708f9d9abd54537fdf2edcf43/artifacts/checkpoints/p8-round28-005-independent-docs-review-bfcc-20261009/independent-final-review.json)
