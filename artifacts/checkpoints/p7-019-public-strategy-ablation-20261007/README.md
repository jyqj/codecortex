# P7-019 public strategy comparison: scoped engineering evidence

This checkpoint records the public-policy ablation mechanism at fixed source
`93356fc87e9534c286192bde1fc88aa95abe878b`. Its complete inventory is **772
crate/Cargo inputs**, with manifest SHA-256
`ef5a880e8633c605cd52ae50fc2b4dd7193a761602da98ac7f97c5dcdff43187`.
The source delta is seven paths, including the development CLI, runner,
reporting, focused controls and documentation. Production retrieval weights,
budgets, scorer, gold, original tests and task status authority are unchanged.

## What was implemented and exercised

The development `cc-eval ablate-strategies` path accepts a locked suite, exact
source snapshot, built product and local build receipt. Each cell uses the same
source, binary, configuration, readiness target and input population. The only
public request difference is `retrieval_strategy`:

| Policy | Actual product meaning |
|---|---|
| `local` | Local retrieval lanes |
| `auto` | Local plus optional semantic recall, with explicit local fallback |
| `semantic` | Local plus a required semantic port; local lanes remain |

**Dense-only is not supported by the current public API.** This work does not
invent a hidden production flag or change weights to claim that counterfactual.
The entire P7-019 task remains open. The original brief's real corpus/provider
quality leg still requires its P8 live prerequisites; this checkpoint provides
no live authorization, paid-provider execution or public holdout certification.

The mechanism checks actual query/lane/semantic deadlines, semantic candidate
limit, source candidate budgets, complete hard scope, token budget and final
whole-response byte cap. Equal top_k alone cannot establish comparable cells.
Checked point-in-time readiness requires the exact dense space and real
published/pending/failed coverage. In the semantic-enabled experiment **all
cells, including local, backfill and wait for that same dense state**. This is
a query-policy comparison, not a semantic-disabled indexing-cost ablation.

## Canonical execution and preserved earlier observations

`validation-summary.json` lists each actual command, exit code, source identity
and original log hash. The canonical sequence targeted-cleaned the eight
workspace packages while preserving external dependency caches, explicitly
bound both Cargo target/build directories, then rebuilt the default and
`semantic-http` products plus the final runner. The workspace compiler-artifact
audit and copied product digests accompany those executions.

The final sequence includes seven focused protocol controls, the actual
local/auto default-product stdio smoke, the complete three-policy loopback
runner, narrow strict Clippy and workspace format checking. The loopback runner
owns its literal local listener and returns the same constant unit vector for
every input. It does not see gold. Its three authored development questions,
one warm-up and two measured repetitions per cell produce **27 acquired public
response Values and 18 measured rows**. These are mechanism observations, not
semantic benefit evidence. All output caps and cost counts in the summary are
derived from the retained response files and comparison report.

Earlier observations, including source `73d30ad7`, the first failed Clippy run
and its later correction, remain byte-for-byte in the archive. Another task
subsequently reproduced a cross-worktree Cargo cache mistake. Accordingly the
earlier shared-target runs are labeled **noncanonical observations** here.
Static equality of product inputs is retained as static evidence; it does not
retroactively authenticate reused products. Nothing in the old logs or receipts
was rewritten to turn them into the final canonical run.

The first canonical Clippy check also returned 101: its metadata view lacked
`ablation::strategy`, even though the frozen source declares the module and the
actual runners had just exercised it successfully. That failed log remains
separate. A targeted `cc-eval` cleanup and the unchanged Clippy command produced
the recorded recheck; no source or test assertion was changed. Product and
runner execution, the initial lint failure and the lint recheck have distinct
receipts. This is not a claim that every transitive dependency build is hermetic.

The initial default stdio smoke used temporary project/raw directories which
were not retained; its original log and product binding are preserved. The
full three-policy runner retains its input, locked suite, plan, readiness
observations, acquired response Values, normalized rows, scores and reports.
The final canonical source and products apply only to the fixed source above;
a later combined checkout needs its own execution receipts.

## Reading costs, effects and raw responses

The original retrieval work and final-assembly validation work remain separate
observations with their existing coverage statements. Provider billing and
current-query cache attribution are not public response fields, so the report
keeps them **null/unknown**, including the local arm. Originating receipts are
not summed into an invented current-request or monetary cost.

Paired effects use existing per-question score means, then query-family means
and the fixed-seed bootstrap. Translations and repeated requests do not become
independent samples. Every effect row is labeled `fake`, with no quality
interpretation; this demonstrates reproducible arithmetic and population
checking. Reported adapter times include verification and artifact persistence,
so they are not product-only latency measurements or an SLA.

`profile-raw` retains acquired returned/rejected JSON Values, including warm-up.
It is not a complete wire capture. A transport `Err` has no returned Value and
is handled by the generic runner/manifest; the harness does not claim to retain
all bytes of every possible transport error. The successful canonical run's
27 acquired Values are all retained and hash checked.

## Files and reproducibility

- `validation-summary.json`: canonical command/build bindings, observed cells,
  historical-cache qualification and remaining limits.
- `source-delta.json`: seven changed source/document paths with hashes.
- `checkpoint-files.json`: exact lengths and hashes of every included original
  observation plus the two direct summaries.
- `raw-observations.tar.gz`: lossless, deterministically constructed originals.
- `verify_storage.py` and `STORAGE.md`: bounded read-only verification; no
  extraction, payload execution, test rerun or gate approval.

Complete source snapshots are reproducible from the fixed Git commits and
input manifests. Large executable copies are not committed; their actual
hashes, Cargo compiler-artifact messages and build records are retained.
These are scoped local provenance records, not a hermetic-build or remote
attestation claim. Verification of this storage does not mark P7-019 or G7 done.
