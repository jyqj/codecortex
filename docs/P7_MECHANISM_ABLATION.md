# P7-019 source-isolated fake mechanism comparison

P7-019's original task brief separates the fake engineering leg from the later real-provider quality leg. This harness closes the engineering question with actual separately compiled MCP binaries. Its fake effect intervals do not certify held-out semantic quality, a release benefit, or a performance SLA.

The existing `ablate-strategies` runner compares public `local`, `auto`, and `semantic` policies. Public `semantic` includes local retrieval and therefore cannot supply the dense-only arm. The new `ablate-mechanisms` runner uses two fixed source replacements in complete isolated build snapshots. It adds no production configuration, environment variable, feature, or route.

## Declared counterfactuals

The replacements are versioned in `crates/cc-eval/src/benchmark/ablation/mechanism_controls.json`. The verifier compiles this exact file into itself and rejects plans that change either replacement. All other source bytes must match the reference snapshot.

| Cell | Local candidate registry | Semantic query dispatch | Observed candidate lanes |
| --- | --- | --- | --- |
| `none` | Removed | Removed | Empty; actual negative control |
| `local` | Original | Removed | `exact_symbol`, `path`, `lexical`, `grep`, `graph` |
| `dense_only` | Removed | Original | `semantic` only |
| `hybrid` | Original | Original | Five local lanes plus `semantic` |

The local factor replaces the entire `default_lanes` registry with an empty registry. It does not run local lanes and then hide their output. The semantic factor changes policy resolution to effective `local` before the query handle dispatches the semantic port. The requested policy remains the actual caller's `semantic` value. Only the source-isolated runner accepts this declared counterfactual effective policy; public strategy validation is unchanged.

Every cell receives the same queries, `retrieval_strategy=semantic`, top-k, locked source corpus, configuration, time budgets, candidate budgets, hard scope, and whole-output byte/token budget. The runtime must report the expected effective policy and exact lane set for every request, including warm-up requests. Disabled lanes remain visible as disabled; complete, partial, timeout and error states are never rewritten.

Dense-only refers to candidate retrieval. The experiment retains common planning, source validation, hydration, ranking, and output packing. All four cells provision the same configured semantic index and wait for the same complete pinned vector space. Thus the local cell is a query-retrieval counterfactual, not an offline or zero-indexing-cost product configuration.

## Build and run

Start from a clean checkout of the precise source SHA being reviewed. Rust and Cargo must be available in `PATH`; dependencies may be pre-cached. The script verifies that the checkout is exactly the requested SHA and that its build input worktree is clean.

```bash
python3 scripts/p7_mechanism_build.py \
  --source "$PWD" \
  --expected-head "$EXACT_SHA" \
  --output "$RUNNER_TEMP/p7-019-build" \
  --jobs 2 \
  --discard-targets

P7_MECHANISM_BUILD_MANIFEST="$RUNNER_TEMP/p7-019-build/mechanism-build.json" \
P7_MECHANISM_EVIDENCE_DIR="$RUNNER_TEMP/p7-019-evidence" \
cargo test -p cc-eval --features semantic --test p7_mechanism_stdio \
  -- --ignored --nocapture --test-threads=1
```

The build script snapshots the tracked workspace manifest, lockfile, Cargo/toolchain configuration when present, and every tracked crate file. These are the complete Cargo inputs for the product binary build; unrelated documentation, repository history, and historical evidence are excluded. Source files have exact SHA-256 inventories. Snapshot symlinks, undeclared changes, aliased source paths, reused binary content, and shared or aliased Cargo targets are rejected.

Each cell builds `cc-server`'s `codecortex` binary with the same `semantic-http` feature, compiler, profile, jobs, flags and controlled compiler environment. The actual Cargo and rustc executable bytes are hashed. Compiler wrappers are explicitly disabled, and Cargo's intermediate build directory is explicitly placed inside the same cell target. Its source root, binary, build receipt, and target directory are distinct. The complete inventories are compared before and after Cargo and before and after execution. Only the target output position is excluded from the existing versioned semantic build-option projection.

`--discard-targets` removes only each target created by this script, after copying the successful binary and saving the command and build logs. A distinct target directory with a small position receipt remains so its canonical identity can still be verified. It never deletes an input, source snapshot, binary, receipt, cache shared with another task, or an existing output directory. `--prepare-only` validates and materializes snapshots without compiling; it cannot emit a successful build manifest.

The generic development CLI can run a previously locked plan:

```bash
cc-eval ablate-mechanisms --plan plan.json --output new-run-directory
```

A plan names the immutable `mechanism-build.json`, locked suite and pinned readiness policy. The first version permits a literal loopback HTTP endpoint with explicit opt-in; the fixed integration test starts that endpoint and supplies a synthetic file credential. There are no live-provider credentials or calls in this acceptance path.

## Actual positive and negative controls

`p7_mechanism_stdio` performs four independent index lifecycles and 36 real MCP search requests: three fixed queries, one warm-up and two measured repetitions in each cell. The loopback provider returns the constant four-dimensional vector `[1, 0, 0, 0]` for every input and has no gold access. This deliberately supplies a mechanical dense signal with no semantic interpretation.

The test verifies all of the following against actual raw product responses:

- The `none` binary returns no candidate lanes and no hits for every query.
- The local and hybrid cells execute exact-symbol retrieval and retain exact first results for two source symbols.
- A fixed token absent from the corpus produces no local hit and positive dense candidates in the dense-only and hybrid cells.
- The dense-only cell contains exactly one semantic lane, which actually completes and emits candidates. The hybrid cell retains its original local lanes.
- Actual hybrid output is rejected when labeled dense-only. Reusing the hybrid source or binary for the dense-only cell is also rejected by build verification.
- All observations preserve identical per-input effective budgets and the complete expected request population.
- Raw metrics and paired bootstrap effects replay exactly. Category/split strata include a fixed authored `holdout` partition, which is explicitly marked fake and does not represent an independent public semantic holdout.

The same locked queries and configurations are used in every cell. There are no per-query weights, selected best runs, or label-conditioned vector rules. Cell and measured-query order are seeded.

## Evidence and interpretation

Retain both build and execution output directories as CI artifacts. The build directory contains the reference and four source snapshots, every full source inventory, four binaries and receipts, and raw Cargo stdout/stderr plus exact commands, timestamps and exit codes. A receipt is recorded local provenance; it is not remote attestation of a compiler or host.

The execution directory contains the locked corpus/query/plan inputs, fake-provider request digests, and an `acceptance.json` that becomes `passed: true` only after all actual fixture controls pass. Each cell retains before/after readiness, every warm-up and measured raw MCP response and SHA-256, effective budget observations, lane-execution witnesses, normalized results, source verification, costs and resources. `comparison.json` reports population/budget integrity, raw gates and paired effects. A failed cell or incomplete population prevents a paired-effect success report; failed raw observations remain available.

Cost fields preserve the public contract: provider billing and current-query cache reuse are `null` with explicit unknown flags. Originating-work and validation-work receipts remain separate. Indexing the same configured semantic space in every arm does not establish zero provider cost for the local query cell, and cached originating work is not additive current-request work.

The effect calculation reuses the existing scorer and paired reporting: repetitions are averaged within a query, translations share their query-family sampling unit, and the seeded family bootstrap uses 2,000 draws with the recorded percentile indices. It reports exact regressions and category/split effects without requiring synthetic benefits. These are reproducible engineering observations. Real-provider corpus quality and the V19 benefit criterion remain a separate later evaluation under the original task brief.
