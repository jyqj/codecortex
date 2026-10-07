# Public strategy ablation, v1

P7-019 now has an explicit execution path in the development `cc-eval` CLI:

```sh
cc-eval ablate-strategies --plan /absolute/path/plan.json --output /new/run/directory
```

This is an engineering comparison of the existing public `retrieval_strategy`
values. `local` uses local lanes. `auto` adds optional semantic recall and may
fall back to local. `semantic` requires an available semantic port and still
retains local lanes. **The public API does not currently expose dense-only
retrieval.** The report records that capability as unsupported; this command
does not implement a new product mode or certify the full P7-019 quality gate.

## Fixed inputs and budgets

The plan names one locked suite, one complete isolated source snapshot, one
binary and its existing ablation `BuildReceipt`. Before spawning any product,
the source inventory, build result/options and actual binary SHA256 are checked.
Every cell uses that same binary, source input, configuration, seed, repetition
count, query population and top_k. Its only request difference is the explicit
strategy. The original factorial ablation command and gold/scorer are unchanged.
The observed populations must also equal the complete locked suite, including
warm-up requests; two equally truncated profiles cannot establish comparability.

Example for the default product (the named paths must actually exist):

```json
{
  "schema_version": 1,
  "suite": "suite.json",
  "source_snapshot": "source-snapshot",
  "binary": "codecortex",
  "build_receipt": "build-receipt.json",
  "strategies": ["local", "auto"],
  "network": "disabled",
  "readiness": {
    "expected_space": null,
    "timeout_ms": 30000,
    "poll_interval_ms": 100,
    "max_polls": 300
  }
}
```

A source snapshot is the exact regular-file inventory listed by the build
receipt, such as a dedicated copy of `crates/`, `Cargo.toml` and `Cargo.lock`.
Passing the live repository with unrelated artifacts does not silently exclude
those extra bytes. The receipt is local provenance, not cryptographic proof
that a remote build used those inputs.

The public search API does not accept an arbitrary output byte limit. The
harness checks the budget it actually reports: configured output maximum,
whole-response limit, token budget, used UTF-8 bytes and token estimate. It also
checks query/lane/semantic deadlines, semantic candidate limit, every numeric
source candidate budget and the complete public hard-scope projection. Those
identities must match for the same input across all cells and repetitions.
Only the two documented explanatory labels `budget.units` and `hard.semantics`
are removed from equality, because normal packing can shorten their prose.
Every scope value remains part of comparison. Truncated or unknown scope
receipts cannot establish equality.

## Readiness and network scope

The runner uses public `status(aspect="capabilities")`, including the checked
point-in-time database identity. With `expected_space=null`, it requires the
admitted file count and local availability. With a pinned semantic space, **all
cells**, including local, wait for the same nonzero desired/published coverage,
no pending or failed work, and actual ready states. A different space or failed
state is an error. Poll count and elapsed time both have explicit bounds. File
counts are never substituted for vector counts. A final observation detects
coverage/space changes before closing, without claiming the entire experiment
was an atomic snapshot.

Version 1 deliberately admits only `network=disabled` with semantic disabled,
or `network=loopback_only` with explicitly enabled semantic/network/HTTP flags
and an HTTP endpoint using a literal loopback IP and explicit port. DNS names,
external addresses, user-info and redirects expressed in the URL are refused.
A semantic-enabled comparison must pin its expected 64-character space digest.
The server still enforces its own credential/configuration/query-network gates;
a harness plan cannot authorize those gates on its behalf. This implementation
does not call a paid or external provider.
The loopback contract is for caller-owned fake fixtures and is labeled
`profile=fake` on cell reports and effect/cost rows. An endpoint address alone
does not attest how the caller implemented that fixture. The included stdio
control owns its listener and returns the same unit vector for every input.

## Outputs and interpretation

Each cell retains the normal benchmark raw results, rows, scores and gate.
`profile-readiness.jsonl` records actual status observations.
`profile-raw/` and `profile-queries.jsonl` also retain warm-up and rejected public
responses, so a strategy or budget mismatch does not replace the only copy of
the response with an error string. Readback recomputes assertions from the raw
bytes and verifies their hashes. Merely changing a serialized budget assertion
cannot authorize an equal-budget result.

`comparison.json` reports actual policies, per-input budgets, original cell
gates and the unsupported dense-only capability. Partial, unavailable and
error lane receipts remain unchanged. Existing quality gates continue to see
Partial as Partial. Missing cost fields stay null. Cost receipts describe
originating work; they do not prove the cost of the current cache hit.

`strategy-costs.jsonl` retains every successful or rejected response observation,
including warm-up. It includes the original retrieval/validation work and
semantic coverage, plus explicit `provider_cost_units`, `provider_cost_unknown`,
`cache_reuse` and `cache_reuse_unknown` columns. The current public response does
not expose provider billing or current-query cache attribution, so those values
remain null/unknown even in the local arm. The comparison counts available and
missing observations without adding originating-work receipts into a fictional
total request cost.

Only complete, comparable cells with successful integrity/availability gates
produce paired effect intervals. The existing scorer first averages each
question's repetitions. Candidate-minus-local deltas are then grouped by query
family; translations therefore do not become independent samples. The report
includes each case delta, family mean, seed, 2,000-draw bootstrap convention and
category/split strata, making the arithmetic reproducible. Failed cells retain
their raw data and gates while the paired result is explicitly unavailable.
Every fake effect row carries `profile=fake` and no semantic-benefit
interpretation. These intervals demonstrate the comparison mechanism.

The time measured through this adapter includes response verification and
artifact persistence and is labeled accordingly. It is not a product-only
latency certificate. Formal quality still needs independently reviewed public
corpora, proper compat/native reports, hard negatives, holdout custody and
stratified paired confidence intervals. Loopback vectors and protocol controls
provide engineering evidence only.

## Validation targets

`p7_strategy_ablation` supplies focused protocol controls for policy forwarding,
remote-endpoint rejection, dense readiness, output/scope budget mismatches,
Partial preservation, rejected-raw retention and observation replay. Its
explicitly ignored stdio smoke requires `CODECORTEX_BENCH_BINARY` and
`P7_STRATEGY_BINARY_SHA256`; it exercises a pinned real product and source
verification for local/auto. A complete build receipt must accompany any
published run. Test code existing here is not evidence that it has run.

`p7_strategy_stdio` adds an explicitly ignored three-policy end-to-end control,
compiled with the existing `cc-eval/semantic` feature. It requires a real
`semantic-http` product plus `P7_STRATEGY_SOURCE_SNAPSHOT`,
`P7_STRATEGY_BUILD_RECEIPT` and a new `P7_STRATEGY_EVIDENCE_DIR`. Its owned
loopback provider returns constant unit vectors, never sees gold, retains
request/input-hash observations, and exercises the full runner across
`local`, `auto` and `semantic`. It verifies all three gates, budgets, raw replay,
cost unknowns and recomputed paired intervals. This is the public-policy
mechanism matrix; it does not provide the dense-only counterfactual sketched
in the earlier task brief.

The original P7-019 task brief separates the fake mechanism leg from the real
corpus/provider V19 quality leg. The latter remains in the P8 live scope.
The frozen factorial mechanism remains available for separately reviewed
counterfactual sources; this command neither invents production flags nor
changes weights to manufacture an absent public mode.
