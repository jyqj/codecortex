# Current-source DEV corpus migration — 20261003

Base/PR107 remote head verified: `350aaacf2f24f533321c495cd4a72ea38029f7df`.
Parent explicitly approved the source-reviewed R09 semantic migration after the obsolete
symbol was reported. New revision: `p0-current-source-dev-20261003-public-surface-v2`.
Only R09 changes query (`PublicSurface::fingerprint`) and gold (`cc-model/src/public_surface.rs`);
that defining file is added to the corpus. Other 13 query/answers stay byte-equivalent as JSON
values. All 14 annotations rebind current source SHA256. R04 documents facade/delegated
multi-dimensional budget boundaries; R07 explicitly documents its cfg(test) scope.
Scoring, timeout, seed, thresholds, repetitions, config and all other suites remain unchanged.
This is a new dataset version, not evidence of retrieval quality improvement, and cannot be
compared with the prior revision as a same-denominator improvement. No retrieval was run.

## Source/Git evidence and preserved historical failure

`source-hash-audit.json` records all original nine admitted file identities, old/new SHA256,
matching committed versions and change commits. Six annotation files drifted; id.rs and
preselect.rs did not; retrieval.rs had no annotation and matched manifest authoring source.
Several annotations were already stale in authoring checkpoint 0a56a257; unmatched historic
hashes are explicitly marked with no matching committed endpoint, without invented mapping.
`lock-version-transition.json` records old/new BLAKE3 locks and the added source identity.

R09 deletion commit: `0a56a257f9a92c54d06ea5be0ce1d1763917a527`.
Old definition: parent commit's dirty.rs lines 231–257. Current dirty.rs line 74 calls
PublicSurface::changed_from; definitions are public_surface.rs lines 214 and 201.
`R09-migration.patch` preserves deletion evidence. `historical-inputs/manifest.json` and
`queries.jsonl` preserve every original manifest/query/answer/annotation; reviewed-head-source
preserves all nine original admitted source files. pre-removal-dirty.rs preserves the old
implementation, separately from the already-failing old query at reviewed head.
`six-suite-validation.log` preserves the original five-valid/one-drift failure, never overwritten.
`source-evolution.patch` maps schema/plan evolution after manifest authoring.

Independent original gold review: R01/R02 id.rs semantic identity; R03 chunker.rs existing
symbol; R04 facade/delegation remains responsible; R05/R06 version constant/mismatch reset;
R07 existing test-only wrapper; R08 resuming importer fixpoint; R09 obsolete symbol migrated
only after parent approval; R10 batched reexport cache; R11 preselect_files wrapper;
R12 working-set/recent/pinned rank decay; R13 both chunk_scope definitions;
R14 normalize/intersect/materialize request constraints. No additional gold migration found.
No applicable AGENTS.md or checkout .agents/skills/SKILL.md exists at this head/ancestors.
No heldout, provider, GC/WAL, prior denied writes, production code or global ledger changed.

## Task TODO and limited validation

- [x] Fetch exact base and verify PR107 remote head.
- [x] Review nine-file drift, Git evolution and 14 original gold independently of retrieval.
- [x] Report R09 deletion before modification; record explicit parent migration decision.
- [x] Preserve original inputs/source identities and original failure receipts.
- [x] Update only authorized DEV manifest/query annotations and R09 query/answer.
- [x] Formal CI six-suite validate with --locked: all six valid (six-suite-reconciled.log).
- [x] Semantic/source/unchanged-13/config checks and formal validator negative controls
      pass (migration-and-negative-controls.log; rerunnable verify_reconcile.py).
- [x] Existing benchmark_lock tests: 10 passed, including source/query/materialization drift.
- [x] Plan check: 192 tasks / 150 done / 41 todo / 1 in_progress passed.
- [x] Commit/push/draft PR and verify remote head: PR110, draft, base 350aaacf.
      Implementation commit 23fa92eaed623defb9b9eaf1eb95742eb09547aa verified against remote.
      Final evidence-receipt commit verified in delivery response.

These limited checks do not represent full CI. Historical baseline and failure remain intact.

Delivery: https://github.com/jyqj/codecortex/pull/110 . Existing GitHub connector created
the draft successfully; local gh auth status reported invalid GH_TOKEN. No concrete 403,
Username error, merge, force push or deploy occurred.
