# 2026-10-08 original task acceptance

## Result

Seven original tasks are accepted in dependency order: P7-014, P7-015, P7-016, P7-017, P7-019, P7-020 and P8-001. The task inventory remains 192. This advances 153 to 160 done and reduces 39 to 32 unfinished. Prior P7-011, P7-012 and P7-013 completion is not counted again.

Original acceptance, steps, dependencies, rollback, validations and conditional authorization clauses are preserved. The exact transition is checked by the unchanged repository plan checker and recorded in task-transition-receipt.json.

| Original task | Completed work | Current fixed evidence |
|---|---|---|
| P7-014 | Config/status/MCP lifecycle and bounded scheduled maintenance | Original13 checkpoints,107 actual RPCs; runtime maintenance and old compatibility tests |
| P7-015 | Slow background model does not starve local work |384 requests/12 cells/3 seeds; original2s/5s bounds; old-space publication0; real paging and reopen reuse |
| P7-016 | Fake fault and recovery composition, including original wiring9 |3 actual SIGKILL seeds, original30s backoff, GC deletion and warm-cache recovery;9 new plus29 old reclaim tests |
| P7-017 | Default and disabled offline contracts |4 full14-tool matrices total plus4 separate reopens;8 roots/257 trace PIDs; external-network attempts0, anonymous IPC8 |
| P7-019 | Original fake engineering ablation |4 distinct builds,795 complete source inputs each;36 actual requests/24 measured;0/5/1/6 lanes; scores and2000 paired draws replayed |
| P7-020 | Separate G7 engineering/fake and live decisions |Original13 wiring rows reconciled; explicit conditional dispositions for7/13; independent manual G7 review |
| P8-001 | Actual fixed candidate and evidence input freeze |Default DEV binary/config/corpus/scorer/model/environment; actual original one-question run/replay;7 drift controls rejected |

## Source and execution

- PR input source: ffdc6f0f97db78cc25a6c026904e7c2adde05d14, tree9e59b41540eb3769e9ca0a787c9ed60441259d20.
- Product commit09291fdf4d968b0929d598cd5df6a3d1fbd3d6cc and independent source-review commit7b5d6f1fa436f4c655b0c0fafc66b521faeb480e.
- All795 source inputs and99 reviewed validation inputs are preserved. Main/engineering actually executed synthetic merge683d8882c108e4e9be68ebac127f3c709e329711 with the exact same complete tree.
- [Main CI37729686657](https://github.com/jyqj/codecortex/actions/runs/37729686657): default regression2635 passed/0failed/67existing ignored. These are execution counts, not unique function totals.
- [Engineering CI37729686671](https://github.com/jyqj/codecortex/actions/runs/37729686671): passed.
- [Closeout/offline/mechanism CI37729686665](https://github.com/jyqj/codecortex/actions/runs/37729686665): all required jobs passed.
- [Independent actual artifact audit37731869613](https://github.com/jyqj/codecortex/actions/runs/37731869613): all required steps passed, five ZIPs actually streamed and validated, source unchanged before/after. The root second closeout replay is byte-identical.

## Independent acceptance and exact bytes

[acceptance.json](acceptance.json) records the integrator decision and [g7-non-author-review.json](g7-non-author-review.json) records the separate final review. Author roles are explicit: contributors do not approve their own implementations as non-authors. Runtime/P19, reclamation, isolation and candidate execution each have cross-review evidence.

[evidence-index.json](evidence-index.json) binds exact current files and [generated-final/report.json](generated-final/report.json) is produced by the unchanged original G7 dossier tool. Its engineering status deliberately remains not_accepted because that command only checks provenance; the separate manual decision supplies scoped G7 acceptance. It is not patched to output approval.

The complete successful main, closeout, engineering, mechanism and offline raw job logs are retained, including the actual complete independent audit log. All426 exported audit files were individually SHA256/UTF8 verified before integration. [raw-preservation-receipt.json](raw-preservation-receipt.json) additionally binds265 current original text members:55 closeout,26 engineering and184 mechanism. Existing offline/P8 full selected raw is already retained.

Large executable/cache/SQLite artifacts are not represented as permanent Git files. Their complete member inventories, exact hashes and actual build identities remain explicit in the receipts. The original GitHub ZIP retention expires2027-01-06. The archived source and raw evidence do not make a claim of hardware attestation.

## Exact remaining boundaries

P7-018 remains blocked under the original D1+D2 decision. There are no live-provider benefit or currency claims. Crash response consumption/billing remains unknown/null. The actual fault paths and explicit counters are tested; exactly-once billing is not claimed.

Rows7 and13 retain the original on-demand conditions. A persistent historical GC counter table and a bound on the historical space-switch log have not been implemented and are not claimed. Current consumers, actual counters and explicit switch triggers satisfy the accepted present scope. Hot reload remains not_run.

Full V19 corpus/custody/facet/report/quality statistics and G8 release certification remain open. Four-arm fake input differences are mechanism observations, not clean-heldout semantic benefit. The P8 candidate runs the original single-question DEV baseline; its actual gate is baseline_recorded_not_quality_certified, release_certified=false and latest_updated=false.

The request for at least10 complete original TODOs is not satisfied by these7. Public DEV corpus expansion and external suite recovery are useful subsequent work, but do not count as completed tasks while their original corpus/clean-holdout prerequisites remain unmet.

## Preserved failed history

Earlier compiler/test/isolation/source-pin failures and the rejected stale review declaration remain separately recorded. The initial independent-audit workflow configuration failure37731707939 executed zero jobs and is retained as a configuration failure. The corrected workflow ran the same semantic assertions. A prior proposal overstated the number of per-package offline matrices; the correction and original proposal identity are preserved in acceptance-proposal.json.

Final repository publication additionally requires successful original plan/dossier generation, exact output integration and latest PR CI. The utility audit workflow is excluded from the product branch. Finalization receipts and the PR merge record distinguish these steps from the earlier fixed-source execution.
