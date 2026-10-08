# P8 corpus and compatibility evidence review — 2026-10-08

## Result and scope

The compatibility checker now binds retained measurements to the locked query
snapshot and the complete query-ID × repetition matrix. The previous checker
checked the number of queries and measured rows without binding the emitted
query text, gold, annotations or query identities to the input it had pinned.
An equal-size replacement could therefore pass those structural checks.

`scripts/p8_compat.py` now retains parsed public DEV queries during input
inspection, compares every emitted query value against that snapshot, and
requires each requested ID and repetition exactly once. It also rejects shared
raw-response paths, a different backend adapter or measurement profile, and a
gate whose status or integer exit code conflicts with the process result. The
normalization only fills optional fields that cc-eval's existing serde schema
serializes as null or an empty annotations map. It preserves order and all
other values, including native answer groups, symbols, spans and primary flags.

No retrieval, ranking, importer, scorer, gold or custody policy was changed.
`tasks.json` and generated TODO files were not edited. This is a completed
checker repair; **P8-002, P8-003 and P8-004 are not completed by this checkpoint**.

## Executed verification

| Evidence | Observed result | Meaning |
|---|---|---|
| `before-fix-negative-tests.log` | The initial 13-case identity suite fails on the original checker | Reproduces the weak identity checks; assertion failures are not a count of separate TODOs |
| `after-fix-tests.log` | 63 tests pass, including 16 identity-matrix controls | Covers equal-denominator substitution, query/gold drift, unknown/duplicate/missing IDs, invalid repetitions, raw-response reuse, adapter/profile and gate drift; valid shuffled runs and optional serde defaults remain accepted |
| `retained-controls-check.json` | Four original public Express DEV runs pass the repaired structural checker: 774 retained requests | Two native/compat pairs, independently rebound to historical pinned query blobs; zero fresh retrieval requests and no rescoring |
| `corpus-audit.json` | `passed_local_audit`, process exit 0 | Rechecks fixed public DEV bytes, spans and coverage only |
| `leakage-audit.json` | `boundary_review_required`, process exit 1 | Both existing source-signature findings remain visible; no clean holdout or absence-of-overfitting claim |
| `external-input-probe.json` | GitHub repository, fixed tree and two metadata files read successfully | No external question body was read, counted or copied |

The retained compatibility evidence is the existing archive with SHA-256
`31366e8130b97d5ee092e17b48dda60505a17dde4f5401d490daee71cb9214a4`.
Its original evaluator was built from
`78ae91eeae6edae6bea29c27f24b251773341c00`, with binary SHA-256
`1217b6249fc5527e05d9d6ae2abd1c7db37a7ea60d8e0024a33f4dce7b2865e2`.
The original comparison remains `comparison_not_passed`, exit 1. These are
historical rg controls with zero reported mean Top-1/nDCG, not evidence of
current CodeCortex retrieval quality. Revalidation leaves the archive untouched.

The command receipts record the base Git HEAD and exact script/test hashes.
At execution the repair was present as a worktree change on
`9f6684ac81fa82f2bc99903ff42fb532478c2aaf`; that base HEAD alone is not the
identity of the repaired script. The recorded file hashes bind the actual code.

## Original task acceptance gaps

### P8-002 — real multi-repository native corpus

The admitted historical snapshot contains 4 repositories, 301 native DEV rows,
256 compatibility rows, 100 source files and 476 verified source spans. Its 301
family labels reduce to 287 local components and 280 conservative global
correlation components. The historical review explicitly does not prove their
full semantic independence. This audit adds zero independent gold reviews and
cannot credit any family toward the formal 600-family acceptance target.

Rust/serde and the mixed JS/TS Vite monorepo are still absent from the admitted
corpus. The `file_exact_match` and `cross_language` categories are also absent.
The user previously authorized public source repositories at fixed commits;
that source direction is already decided. Remaining work is actual source
admission, independent source/gold review, hard-negative and family-equivalence
review, and the full six-repository quality evidence. Source locks for the two
missing candidates remain:

- `serde-rs/serde@6693a89cca77e0151437da1c7f890090b9ebf04c`
- `vitejs/vite@10033218d239c927cdc375970b5741cce408e81b`

### P8-003 — cc-switch and Flask external compatibility suites

The original target is `oce-ai/oce-benchmark@d4f10554a18e31599d1e46d5d56da6588d4aa86c`,
with `farion1231/cc-switch@40cac1a68edf8c9e7b3a89125cf40bb93a348404` and
`pallets/flask@22d924701a6ae2e4cd01e9a15bbaf3946094af65`. Both metadata files still
declare 100 questions. This is a declaration, not a verified body count.

The GitHub repository response reports `license: null`; its complete fixed
20-entry tree has no named LICENSE/COPYING file. This observation makes no legal
claim. The project's existing `09-BENCHMARK.md` section 1 requires externally
supplied benchmark input and a record of its permitted use, and forbids copying
the full third-party corpus into this repository by default. No such admitted
external input is present in the evidence reviewed here. The probe therefore
reads only metadata. Complete acceptance still needs the supplied input and use
record, fixed clean project checkouts and common source inventory, actual
question counting and review, and cc-eval validation/run/replay evidence. The
774 historical Express control requests satisfy none of those target counts.

### P8-004 — sealed holdout and overfitting checks

The pinned custody decision remains `custody_blocked`: of 117 candidate IDs,
at least 72 are known to have been exposed, and **zero clean holdout IDs are
certified**. The other 45 IDs are not established clean: reader history, first
visibility and principal/ACL verification are unknown. A fresh developer copy
or a changed label cannot repair that provenance.

Completion requires an independent custodian to supply a new, access-controlled
evaluation packet and auditable reader/visibility history, a frozen candidate
and configuration, and a once-only evaluation receipt plus the prescribed
family/translation/paraphrase review. This work read no protected bodies,
changed no permissions, ran no heldout evaluation and relabelled no gold.

## Reproduction

From the repository root:

```bash
python -m unittest scripts.tests.test_p8_compat scripts.tests.test_p8_corpus_audit scripts.tests.test_p8_leakage_audit -v
python artifacts/checkpoints/p8-corpus-lock-review-20261008/verify_retained_controls.py --output /tmp/p8-retained-controls-new.json
python scripts/p8_corpus_audit.py --output /tmp/p8-corpus-audit-new.json
python scripts/p8_leakage_audit.py --output /tmp/p8-leakage-audit-new.json
```

Use fresh output paths so existing receipts remain available. The final leakage
command is expected to retain exit 1 while those source signatures require
review; it must not be reported as a clean boundary scan.
