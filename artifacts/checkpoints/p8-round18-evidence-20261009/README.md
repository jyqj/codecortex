# Round 18 evidence archive — 2026-10-09

Status: `fixed_payload_set_ready_for_object_review`. This is a fixed, evidence-only candidate archive. Creating its Git objects does not publish a branch or activate a workflow. Root and an independent reviewer must check the actual objects before publication.

The cutoff is **2026-10-09 06:44:11 UTC**. The base is Round 17 commit `9574382bd6c20ba99628576640b98342912b5b68` (tree `9fd5a6ceb996c08f56f08552a72576d102955959`). The frozen input spec is `e72bfa7b0c4829fe66c4201233485b3ef5874fa3`. There are exactly **42 preserved payloads, 2,906,648 bytes**, plus this README and `manifest.json`: **44 new leaves**, all under `artifacts/checkpoints/p8-round18-evidence-20261009/`.

**Original TODO ledger: 192 total, 163 done, 29 remaining; this round closes 0.** The 29 remaining are 16 in progress, 12 todo, and 1 blocked. At the cutoff, main was `60e5ce3ccc86f5cccb40f6f9c32bbc7b77bf1e9d`; the original 802,282-byte task blob and all task definitions, hard dependencies and acceptance requirements remain unchanged.

## Original CI and selected controls

`pr173-ci/` preserves the complete original check, security and MSRV logs, parser, parser output, and independent/root reviews. These belong to PR 173 run **37890756949, attempt 1**, with head `275e8799d4947d297329073eaa3ca675d3fd0777` and actual CI checkout `d53ea0be17a53d1354ee1d6f0a111c5e588e91f5`. Its 12 non-artifact roots equal the head; the later main and current synthetic merge are separately identified rather than substituted for the actual checkout.

The original Python invocations passed **46 resource-harness, 409 P8, and 181 source-integrity methods**, with zero errors, failures or skips. All three original CI jobs succeeded. Rust log totals are **3,281 passed executions, zero failed executions, and 132 original ignored executions** across repeated original command executions; these are not a count of unique tests and the ignored executions are not relabelled as passes. The original v15 and historical proof outputs retain their original acceptance limits.

`surface-window-controls/` contains the original **11597533828** ZIP, full member inventory, author and independent root audits, audit source/input and complete custody receipts. The fixed-source run executed six commands and **10 selected Rust methods**: six new window controls, three prior dependency controls and one prior stale-build control, with zero failures or ignored methods in this targeted population. The 54 surface and 36 dependency SQL fixtures record bounded query work; they do not establish 100k performance, whole-build wall-clock gains, or formal N150 acceptance. No controls, product workload, Cargo build, or statistics were rerun to assemble this archive.

The maintainer merged PR 170 at 06:04:11 UTC. `main/root-owner-merge-and-ledger-review.json` records that external action and the source relationships. This archive does not describe it as an agent merge or reattribute a23 measurements to the changed main.

## Failed original a23 formal study

The original scale study remains **source `a23bb72d3c954f385b99fe81ce9189885c208557`, run 37872522779, attempt 1**. Its terminal API and complete official annotation are preserved. Four rep-0 shards (1k, 5k, 10k, 50k) were delivered; the 100k rep-0 job ended in failure and the later 145 measurement jobs did not run. The original aggregate reports **146 missing shard pairs** and failed. The original failed matrix and its **11599136237** ZIP retain their original bytes.

The 100k check ran from **02:23:45 to 06:12:02 UTC**. Its sole official failure annotation says that the hosted runner lost communication with the server. The underlying CPU, memory, network or runner-process cause and native 100k terminal outcome are unknown. An unavailable job log is recorded as an unavailable transport, not as proof of a product timeout or performance failure.

`public-aggregate-log-derivative.log` is explicitly a safe derivative, **not the complete original log**: only five private blob-storage URI tails were replaced, and the root receipt verifies all other original bytes unchanged. The original log's byte length and digest remain in the reports; its private URI-containing body is not included here. Earlier uncertainty reports are preserved alongside the later official annotation, without rewriting history.

This study cannot close P8-005/P8-006 or satisfy the complete N150 population. Other runs, attempts or sources cannot supply its missing samples.

## Inactive N150 receiver drafts and recovery boundary

`n150-receiver-draft/` preserves both drafts, their plans, author records and independent review. The first draft received an actual memory-only Python AST parse/compile check; that check did not execute or approve its transport or replay logic. A concrete page-glob defect was subsequently found: transfer receipts would have been read as pagination bodies. Version 2 excludes transfer files with exact numeric page matching and validates the complete pages and unique IDs. Its mechanical change and independent **static-only** acceptance are preserved; no execution of version 2 is claimed.

Both workflow bodies are archival files below this artifact directory. They are **not active GitHub workflows**. The draft is bound to the permanently incomplete original a23 run and **must not be published or executed for that run**.

The preserved contract requires all **301 original packages** (build + 150 capacity + 150 shard), the original build validator, all 150 original shard validations and the unmodified original aggregate/combine. A successful remote matrix is the conditional 302nd package and must be compared; remote aggregate failure or absence is recorded separately and is not invented as a new task requirement. Here the missing measurement shards themselves prevent acceptance.

`recovery/original-task-source-boundaries.json` separates source-specific historical results from current source claims. It neither relabels old measurements nor invents a requirement that every task domain rerun under one SHA, require paid providers, add P8-020 as a dependency, or run a second N150 for every downstream task. A later independently registered complete study is a separate population. Any Round 19 registration, trigger, future run ID or result is outside this archive's cutoff.

## Byte custody and scope

Every one of the **40 UTF-8 payloads** was read in full and independently hashed for SHA-256 and Git blob SHA-1 against the frozen OID. The **two binary ZIPs (115,365 bytes total)** retain their exact original Git blob OIDs. Their complete original-byte and native Git GET equality proofs are included; the archive author checked those full custody reports and **did not redownload the binary bodies**.

`manifest.json` lists every payload path, OID, mode, size, SHA-256 and verification method. No source, active workflow, guard, task definition/status or existing artifact is changed. The inventory and archive verification are evidence management, not new product tests or TODO completions.
