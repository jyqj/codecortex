# Bounded current-source gold audit

Action for the quality-run owner: quarantine R09 from the next **current-source** quality run until an explicitly reviewed dataset revision resolves its intent and corpus coverage. Do not interpret its score as a retrieval regression. This audit does not edit or filter the dataset itself. No query, gold, scorer, production file, manifest or ledger changed; no retrieval results were examined.

Scope: all 14 rows of `crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl`, against source checkpoint `80a43e0ae9a2f640e51d101968d4fef87855c01c`. The manifest admits nine files. `audit.py` emits reproducible hashes, declaration checks and historical assertions; `receipt.json` records its output. Source references below are relative to `crates/` at that checkpoint.

## R09: confirmed obsolete literal entity

The actual query is `compute_fingerprint_for_unit`, with file-only gold `cc-index/src/indexer_phases/dirty.rs`. First **committed observable** mismatch is checkpoint `0a56a257f9a92c54d06ea5be0ce1d1763917a527`: its parent `4514630dcd26481cf6dbc2aff38824ed71ef06da` contains the helper at line 231; the checkpoint deletes it while preserving the old query. A checkpoint may aggregate earlier worktree edits; this does not establish a finer original refactoring commit. Reproduce with `git log --all -S compute_fingerprint_for_unit -- crates/cc-index/src/indexer_phases/dirty.rs` and compare those two revisions.

The parent file exactly matches the preserved P0 frozen file under `artifacts/checkpoints/20261001-paused-github-sync/evidence/artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/source/cc-index/src/indexer_phases/dirty.rs` (SHA256 `6a4f3d0598a2732f33ad56e99ce92f42fa633471d51e6097f2ba7489f0549e01`). The old literal query remains meaningful for that historical frozen corpus.

Current correct computation is `cc-model/src/public_surface.rs:201` (`PublicSurface::fingerprint`) and `:214` (`changed_from`), consumed by `cc-index/src/indexer_phases/dirty.rs:70-74`. It canonicalizes public surfaces with a format-version prefix, handles unknown/missing evidence conservatively, and compares stored surfaces. The old helper hashed sorted exported-symbol identity/signature/export-name records. This is responsibility migration with changed semantics, not simply a renamed function. `public_surface.rs` is outside the admitted corpus.

Owner decision: either keep the unchanged historical question on its frozen historical corpus, or explicitly revise the current question's intent, gold and corpus together. Mapping an obsolete literal query to whichever current file ranks well would be unsupported. The query's historical source-digest annotation is also different from both the preserved parent and current source; it is provenance debt, not current certification.

## Remaining rows: source evidence, independent of retrieval

| Row | Assessment | Current source evidence |
| --- | --- | --- |
| R01 | Entity exists | `cc-model/src/id.rs:20`, `StableId::symbol_uid` |
| R02 | Semantic identity excludes line numbers | `cc-model/src/id.rs:18-20`, path/qname/kind/normalized signature |
| R03 | Entity exists | `cc-parsers/src/chunker.rs:35`, `chunk_with_symbols` |
| R04 | Entry remains relevant; implementation coverage limited | `cc-parsers/src/chunker.rs:150` delegates to `chunker/split.rs:58`; budget test `chunker/budget.rs:16-22` still uses policy lines. Split/budget/policy implementation files are outside the nine-file corpus. No confirmed obsolete gold; full implementation evidence would require a reviewed corpus revision. |
| R05 | Entity exists | `cc-db/src/index_migrate.rs:35`, schema constant 22 |
| R06 | Mismatch branch still applies, with a behavior boundary | `cc-db/src/index_migrate.rs:48-54,64,91`: adjacent 21→22 migrates in place; other stored mismatches return `SchemaStatus::Mismatch`. Boundary introduced by `ff458bc591b4e7e444af4464d6eef2513cdb335c`. File gold remains relevant; do not assume every version inequality rebuilds. |
| R07 | Entity exists; wrapper delegates within same file | `cc-index/src/dirty_closure.rs:110,136` |
| R08 | Bounded fixpoint propagation remains | `cc-index/src/dirty_closure.rs:1-19,88,136`; budget/round limits may return partial closure, so unrestricted completion is not implied. |
| R10 | Cache concept remains; local name changed | `cc-index/src/indexer_phases/dirty.rs:204-232`, shared `targets_cache`, batch `reexport_targets_for_files`; question is conceptual, not a literal removed-symbol lookup. |
| R11 | Entity exists | `cc-search/src/preselect.rs:775`, `preselect_files` |
| R12 | Working-set/recent/pinned decay ranking remains | `cc-search/src/preselect.rs:12-13,48-49,168-174,222` |
| R13 | Both explicit gold alternatives exist | `cc-search/src/scope.rs:86`; `cc-search/src/plan.rs:261-262` delegates. Existing alternatives already represent source responsibility migration. |
| R14 | Normalization/materialization entry remains | `cc-search/src/scope.rs:5,86`; `cc-search/src/plan.rs:129` calls `normalize_request`. |

Result: one confirmed obsolete literal-entity row; no equivalent missing-entity defect established for the other 13 rows. R04, R06 and R08 have the explicit coverage/behavior boundaries above. This is author source-read review, not independent human gold certification or a measurement of retrieval success/failure.

## Why validation does not catch R09

All rows have file-only alternatives (`symbol:null`, `span:null`). `cc-eval/src/benchmark/metrics.rs:78` checks path and evidence validity; symbol/span constraints apply only when present. A hit in `dirty.rs` could therefore receive credit without containing the requested old entity. `cc-eval/src/benchmark/manifest.rs` verifies source/query locks and admitted paths, not the semantic truth of the query. PR40's repaired lock establishes reproducibility only; it does not resolve this pre-existing gold defect.

Reproduce: `python3 artifacts/checkpoints/cloud-p0-gold-bounded-audit-20261002/audit.py` and compare output with `receipt.json`. No benchmark scores, production tests or provider calls are required for this evidence-only change.
