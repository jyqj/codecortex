# Round 10: one local lane registry for execution and policy

The actual product is **P5 `fa6562ad70c6a6f2a7a1e65594df23f807e62f18`**, tree `0e6fdcfd2ac7a727184daf5a0e290cf00c55abbc`, parent main `b9412406e11422d7cf914458a8bfbbd58cf94eaa` (merged PR181). It changes exactly three product files and retains all other source, validation, guard, task and artifact bytes from that parent.

## Problem and implementation

The execution registry and QueryPolicy separately listed the same five local lanes. P5 gives both consumers one static borrowed registry. The existing `default_lanes() -> Vec<&'static dyn RetrievalLane>` interface remains; it copies references from the shared registry. QueryPolicy obtains lane IDs directly from the same view without constructing another registry Vec or executing a lane.

The exact order remains `exact_symbol`, `path`, `lexical`, `grep`, `graph`. Intent roles, timeout caps, optional semantic suffix, policy version, serialized labels, field order and fingerprint behavior remain unchanged. Two new controls cover 112 intent/strategy/budget combinations and two complete fixed JSON contracts; the existing fusion-order and semantic-unavailable controls remain.

The first two-file candidate was retained when independent review found the existing ablation mechanism's literal source-text consumer. The final v2 also updates only `local_retrieval` on/off text in `mechanism_controls.json`. Its off variant still empties the execution Vec before any local lane runs while keeping the shared registry and policy obligations. The complete `semantic_dense` control and the original strict replacement/validation logic are unchanged. The authoring directory preserves v1/v2 patches and manifests. The old JSON contracts were derived from the fixed original source and serde declarations; no old-binary execution is invented.

## Actual fixed-P5 verification

All five commands ran on clean P5 with Rust 1.95.0 and fixed locked/offline Cargo inputs where applicable. All exited0, and all 1,092 product input hashes remained identical before and after.

| Actual command scope | Result |
|---|---|
| `cargo fmt --all -- --check` | exit0 |
| Workspace/all-targets Clippy, warnings denied | exit0 |
| `cargo test -p cc-search --lib --locked --offline` | 303 passed,0 failed,0 ignored |
| Existing `fixed_controls_match_current_product_source_once` in cc-eval lib | 1 passed,66 filtered |
| Original `p7_mechanism_build.py --prepare-only` at fixed P5 | exit0;four source variants prepared |

The Rust calls total 304 passed executions. The 112 combinations run inside one regression test; they are not 112 original TODOs or independent study samples. The receipt, five complete logs, supervisor and independent execution review preserve actual argv, environment, times and input manifests. This is not an unfiltered workspace suite or a runtime/scale/release certification. Prior P4 tests, old P2 failures and original CI retain their distinct source identities.

The seven original preparation JSON files are preserved. Their reference inventory has 1,092 inputs; the four source-inventory differences are exactly: `none` changes both controls, `local` changes only query policy's semantic control, `dense_only` changes only the local execution wrapper, and `hybrid` changes nothing. `compiled:false` is retained. No variant binary, build receipt or successful mechanism-build manifest was produced. Full prepared source copies can be reproduced from fixed P5 and these unchanged controls; this archive preserves the actual preparation metadata, not a claim that those variants were compiled or benchmarked.

## Independent source review and later binding

`independent-source-review.json`, SHA-256 `c74e75d90f9e94d7b4572f0e75f02ab172a01eb6dda28e3ec47aacd94e511dd0`, binds the actual P5 product: **1,092 product inputs,139 validation inputs,56 original-BASE differences**. Three newly changed paths extend the prior53 differences. New semantics were reviewed by `/root/todo_audit`, independently of author `/root/pr_audit`. Unchanged prior source semantics retain their fixed R4 provenance; earlier executions are not relabeled as P5.

This archive does not yet claim a subsequent v15 CLI result. The registry and four original verifier binding constants must be installed in a separate fixed commit, then the unchanged original CLI executed there. Its original BASE, VERSION, history, source domains, exclusions, validation logic and CI remain in force.

## Original TODO accounting

**192 total =163 done+16 in progress+12 todo+1 blocked;29 remaining;0 newly fully completed original tasks.** This is a concrete implementation advance for P8-017. Its original P8-016 hard dependency and full acceptance remain open. The requested minimum of ten original task completions is still unmet; no status, definition or acceptance condition is changed to manufacture closures.

`archive-manifest.json` hashes all payload files in this directory except itself. Later binding/CLI evidence belongs in a separate directory so this manifest remains closed.
