# Round 28 evidence checkpoint

Observation cutoff: **2026-10-09T13:43:43Z**. This checkpoint preserves **0 newly closed TODOs; 163 done / 29 remaining**. It does not certify all ten pending P8 tasks.

The proposed sole parent is Round 27 `3a20dbeb0f84b43c630e84769b7bd30e1471a651` (tree `f92e9873072aabb02ac8bc2580a25cca2762d599`). Every proposed path is a new file below `artifacts/checkpoints/p8-round28-evidence-20261009/`. All older paths, source files, task definitions, statuses, dependencies, guards and measured sources remain untouched.

The manifest lists **78 payload paths / 73 distinct payload blobs / 7,653,183 payload bytes**. README and manifest are the two additional metadata files. Empty original streams remain separate paths even when Git stores their common empty blob once.

## Preserved evidence and its scope

| Evidence | Actual identity and conclusion |
|---|---|
| Original G workspace | `b356043c2c0c970d000e5f5e32f1dff69f88454f`; exit 101, 102 groups, 722 passed / 1 failed / 58 original ignored. The descendant-stderr test failed. The fixture command in that stop-on-failure runner was not run. Full stdout, stderr, manifest, runner and independent reception are retained. |
| Original G isolated method and stage probe | The same method failed when isolated. The one external probe compiled, then returned exit 3 with a 254 ms supervisor report, null worker exit/summary and an empty worker stderr before the first stage tag. Complete supplied probe sources, command receipt, plan/report and streams remain. No particular OS startup mechanism is inferred. |
| Original G separate fixture/corpus | A later, separate command on b356 passed its one selected fixture/corpus method. Its complete three result files and runner remain separate; this does not change the failed workspace result. |
| P4 fixture correction and actual contribution checks | `cfc77b68828228cea1466e0161f9dc745fe5aef3`, tree `35cd8de386bd5b4d3755929743efe7bc8a9544fd`. Five original checks passed: fmt, strict workspace/all-target Clippy, the exact descendant test, full workspace with `--no-fail-fast`, and fixture/corpus. Workspace: 220 groups, **2,731 passed / 0 failed / 73 original ignored**. All 13 supplied original files, root receipt, independent receipt and canonical source review are preserved. |
| Final #184 binding | P4 → R `d7512ed031f4027437dc756012cb7f21582b50f7` → G `883230f2b44ebcf09495870db8c0e31d61270331`, tree `e738e0d6bd25f18775379dc6e3849ac9373b92d6`. R adds one independent review; G changes only the registry and four guard identity assignments, with both trust files mode 100644. Product/validation inventory is 1,092 / 139, with 60 BASE-delta inputs. Root published this G and kept #184 Draft. Its native guard job was launched but **no terminal result had been received at cutoff**. P4 results are not relabelled as G results or remote CI results. |
| #180 overlap and two-assertion composition | The fixed 832f overlap review and the 31b340→969ba composition retain their original scope. The composition only adds measured worker exit-zero and summary-passed assertions while preserving the existing supervised readiness and all 12 methods. It has no new execution result. Later #180 head `0e6b07c75183da79c5a6679d4c2b66c65f7db663` was an external update and was still Draft at cutoff; its new source exceeds the old review domain. Original 3ff measurements and results keep their original identities. |
| Original c8 failed 100k | Source `c8be5afaac568ffd40ef86d3795423c3b73c9f39`, run 37902429727, attempt 1, artifact 11617588539. Original report remains `deadline_exceeded`, exit 3, wall 18,019,626 ms, null worker exit/summary and `stderr_complete=false`. Full original ZIP, complete job log, API observations, metadata and root transport receipts are preserved. No failed sample is replaced or declared accepted. |
| #187 documentation | Before/after `docs/BENCHMARK.md`, author handoff and independent documentation review are preserved. Root's exact tool record contains the actual merge `d22d36dddf8b38f1a864c933deefe0b6d3b5baf3` and one-leaf documentation tree change. This does not alter product inputs or certify a live model execution or close P8-014. |

## Original evidence rather than replacement summaries

P4 original files are under `P4/originals/`; the full raw workspace logs are retained. The old failed executions remain under `old-G/`. All ten supplied stage-probe files are retained; the compiled probe binary itself is not copied into this archive, and its original identity stays in the probe manifest. This is an evidence checkpoint, not a standalone compiler/source/environment replay bundle.

The complete c8 ZIP is `formalc8/original.zip`, Git blob `ecabdc3c5503dfbdfb4e041812c6588d01052d6a`, **5,212,266 bytes**, SHA256 `f758ca519a6b5ed90bfe1040944ad32b1daad8248e44b8ea4c7d4840d4a43236`. The archive compiler did not download it again. Root's original reception verifies the whole ZIP and all 12 member CRC/SHA values; the complete Git upload/transport record is retained. Full raw data remains inside that ZIP. Small extracted report/shard files are additional entry points, not substitutes for the archive.

All 77 selected UTF-8 payload paths have complete byte-count, SHA256 and Git blob SHA1 verification. Repeated blobs are checked once by their complete immutable body. A text scan found no signed download URI or credential token matches. The archive compiler did not run Cargo, tests, native products, a new scale study or a receipt consumer.

## Authorship and execution boundaries

The original P4 fixture and its two added assertions were authored by pr_audit and independently reviewed by acceptance_audit in `12aec6cc8c37b20e4a233aedf32c2e485cd218c6`. The later PR180 composition was authored by scale_engineering; pr_audit's `f877ff…` review explicitly limits itself to exact composition and retains the original author recusal. The final R/G `73441c…` review is a mechanical object/binding check, not self-approval of fixture semantics.

The mistakenly created, unreferenced G `c0a4dd4732b54968089e2a0ceb3409873012b0e8` used a wrong guard mode. Its correction record is retained. Only the corrected final G883230 is admitted by the mechanical bridge. No native execution or publication is attributed to the wrong-mode candidate.

Original 275e and c8 scale failures remain failures and cannot close P8-005/P8-006 or their dependent tasks. The 3ff study was still in progress in the preserved 13:38 observation. This checkpoint neither pools those studies nor reduces samples, workload, budgets, parity tables or acceptance gates.

## Cutoff and later work

The root supporting index and `root-support/actual-publication-and-native-transport.json` were packaged after cutoff and are labelled accordingly. They preserve only already observed pre-cutoff contents and tool responses, including the pending final-G launch; no later result is claimed.

The post-cutoff packaged #1800e6 independent review/inventory belongs to Round 29. Later final-G guard/CI results also belong to the next checkpoint. The c8 read-only cost-reader source is retained, but no complete returned phase analysis was available at cutoff; no phase totals are invented.

The superseded unreferenced first manifest `9af5080c97251fb6af908ac9c10ab7c675a24957` listed the known payloads before root supplied its final publication/transport record. The final manifest adds that existing record without extending the observation cutoff.

Before publication, a non-author should verify the exact proposed leaf list, manifest/README hashes, sole parent, absence of the new prefix in the parent, and preservation of every existing mode/OID. Root retains tree, commit, ref and PR publication authority.
