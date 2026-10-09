# Round 10: verified lane registry and original task progress

The original v15 CLI **passed on fixed G5 `27d6e8294e4b7ae337ee978ffada5b2dbeb43f31` in 188.259094 seconds**, under the unchanged 3,600-second bound. Its actual execution ended `2026-10-09T11:21:08.481179+00:00`. The recorded before/after HEAD, tree, registry, verifier and tracked-change identities are identical. Independent binding and execution reviewers accepted their respective scopes.

## Fixed chain and actual implementation

| Record | Commit | Scope |
|---|---|---|
| P5 | `fa6562ad70c6a6f2a7a1e65594df23f807e62f18` | Three-file lane registry and existing ablation-anchor change, based on merged PR181 |
| R5 | `365e2cc503a0119b2fba3cf98ce38e5aa65cfcd4` | Independent source review and actual P5 evidence;37 added archive files |
| G5 | `27d6e8294e4b7ae337ee978ffada5b2dbeb43f31` | Original v15 registry and four verifier identity constants |

The source review binds 1,092 product inputs,139 validation inputs and56 original-BASE differences. Its SHA-256 is `c74e75d90f9e94d7b4572f0e75f02ab172a01eb6dda28e3ec47aacd94e511dd0`. G5 registry SHA-256 is `7d4f24aa1068db11feb024edf276ff92a5f9ccee4ee9296fe59f2f16a13ccde1`; verifier SHA-256 is `b926b19fbb6a2609192f3ea11d230b9d1bbd432d9274ebae24bb7e05fad6a82d`. BASE, VERSION, history, validation rules, exclusions and CI remain unchanged.

Execution and QueryPolicy now obtain their five local lanes from one static registry. The original Vec-returning execution API, stable order, intent roles, capped budgets, semantic suffix and complete policy wire/fingerprint behavior remain. The existing local-retrieval ablation anchor was updated to the new wrapper while preserving its execution-only disable behavior.

P5's actual five selected checks all passed: full format check, strict workspace/all-target Clippy, 303 search-lib tests, one original mechanism-anchor control, and the original four-variant source preparation. The latter explicitly records `compiled:false`; no four-variant build, provider ablation or study success is claimed. Full raw logs and independent review are in R5's `artifacts/checkpoints/p8-lane-registry-20261009-50c/` archive. Older P2 failures and P4/G4/CI successes retain their own source identities.

## Task record and derived documents

After the G5 CLI finished, root ran the reviewed append script at HEAD G5. It added exactly **one evidence record and one implementation-notes suffix to P8-017**. Reversing those two appends reconstructs the complete original P5 task JSON byte for byte; every other task, top-level field, definition, status and dependency is unchanged.

The original `code_index_plan.py --write` and no-argument commands both exited0 on G5 with the prospective six-document changes. Their exact argv, HEAD, stdout, stderr and original output hashes are preserved under `round10-docs/plan-checks/`. The exact no-argument stdout became `PLAN-CHECK.json`. These are actual G5 prospective-document executions, not executions at a later publication commit. New task SHA-256: `d51f61a920d887d375e95ca529509d0750233ea22477e40f7d43b3caa985c495`.

This publication adds the current post-binding directory and the six documented progress views/ledger files. All G5 product inputs, validation inputs, guards and preexisting artifact bytes are unchanged. R5's manifest remains closed.

## Original completion accounting and remaining gates

**192 original tasks =163 done+16 in progress+12 todo+1 blocked. Remaining29. Newly fully completed by this session0. The requested minimum of ten original task completions is unmet.** Each of rounds1–10 has29 remaining and0 new original closures; `rounds.json` records those counts.

The local lane cleanup advances P8-017's implementation. Its original P8-016 hard dependency and full V18/V21 acceptance remain open. No test case, source review, PR operation, evidence record or partial subacceptance is counted as a completed original TODO.

The separate `final-study-state.json` records read-only metadata at11:20:32–11:20:33 UTC. All three original source-specific studies remained in progress, each with four successful preflight shard jobs and a running100k rep0; the later145 jobs were not present. Those metadata successes are not newly accepted raw shards, and no source's data is relabeled onto P5. Original full N30 scale and downstream acceptance remain incomplete. PR181's seven new remote workflows were still queued/pending in that snapshot; they are not claimed passed.

The prior correction to overbroad runtime-instrumentation requirements remains authoritative. Original actual mixed-path DB-wait observations retain their required meaning; a preceding availability probe is not relabeled as such an observation. Full backend tracing or repeated one-hour diagnostics are not imposed as new gates. Other owners' studies and implementations remain separate.

`archive-manifest.json` hashes every payload in this directory except itself. The task/doc files outside this directory are listed separately in the publication tree proof.
