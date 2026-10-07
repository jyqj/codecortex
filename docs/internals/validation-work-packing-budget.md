# Optional final-validation work under output pressure

`evidence_summary.source_freshness.validation_work` reports the named SQL work
of the current final-assembly attempt. Its two SQL objects, versions, coverage
and meaning remain the same as documented in `QUERY_EXECUTION.md`. It is
optional diagnostic metadata; source identity, proof, freshness completeness
and generation remain separate fields. A missing work object never means zero
cost or a completed attempt.

## Packing order

The response keeps the existing byte and token limits, metadata projection,
source priority and explicit omission accounting. After compacting existing
metadata and duplicate source views, the packer removes optional references
before selecting the complete source bodies that fit.

Before each additional body eviction, it checks whether removing the whole
`validation_work` object and setting `source_freshness.details_omitted: true`
is sufficient to fit the current response, including that marker and the
self-measured byte/token receipt. If it is sufficient, the packer keeps every
current complete body and records the diagnostic omission. If it is not
sufficient, it restores the exact work object and the previous value or absence
of `details_omitted`, then follows the existing evidence eviction order. This
preserves all original numeric and explanatory values when a smaller body set
can accommodate them. No SQL subfield is zeroed, shortened or partially reported.

This check does not replace the existing oversized-freshness diagnostic
projection or the minimal-envelope fallback. It does not change the hydrator,
SQL execution, source verification, original retrieval-cost receipt, generation
receipt, or Partial status. A diagnostic-only omission does not itself claim
that source evidence is partial. An earlier body/graph omission remains Partial.
Repeated packing does not recreate omitted diagnostics or source bodies.

## Regression and controls

PR143 CI run `37623178087`, job `112798257508`, exposed the original fixed
`diag_p5e_fix_review_boundary_20261003` gate: at its unchanged effective
16,000-byte limit, the new receipt increased omitted hits from two to three.
The fixed seven-hit fixture lost `scope/impl.py` from fix/refactor results.
The receipt bypassed the existing optional-detail policy because the complete
freshness object was below the large-diagnostic size threshold.

The original boundary file and its exact two-omission expectation remain
unchanged. It covers actual parser/index/MCP source checks, implementation,
test/interface facets, shrinking/expanding caps, numeric-width stress, exact
scope projections and real legacy/generation producer preservation.

`packing_validation_work` adds two narrower controls over actual indexed
source with explicitly synthetic output pressure: a fixed cap that keeps the
complete body by omitting the optional work object, and a failed omission probe
that must restore the entire producer receipt and an absent, false, true or
null pre-existing marker. The latter also checks inputs without a work object,
graph-induced Partial status and source/body preservation. Both controls check
actual serialized bytes, self-accounting and idempotence; expanded budgets do
not recreate an omitted receipt.

Commands:

```sh
cargo test --locked -p cc-eval --test diag_p5e_fix_review_boundary_20261003
cargo test --locked -p cc-eval --test packing_validation_work
```

Failure/reproduction logs, original hashes, stage observations and the final
scoped validation receipt are kept in
`artifacts/checkpoints/validation-work-packing-budget-fix-20261007/`.
Historical source admissions and review records are not modified by this fix.
