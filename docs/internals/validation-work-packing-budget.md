# Optional final-validation work under output pressure

`evidence_summary.source_freshness.validation_work` reports the named SQL work
of the current final-assembly attempt. Its two SQL objects, versions, coverage
and meaning remain the same as documented in `QUERY_EXECUTION.md`. It is
optional diagnostic metadata; source identity, proof, freshness completeness
and generation remain separate fields. A missing work object never means zero
cost or a completed attempt.

## Packing order

The response keeps the existing byte and token limits, source priority and
explicit omission accounting. Duplicate rendered source views yield first. If
the response still exceeds its cap, the packer checks whether omitting the
optional work receipt alone preserves all current hit and retrieval metadata.
This check precedes the lossy lane/hit metadata projection, so diagnostic cost
reporting cannot displace the public score contributions when its omission is
sufficient to fit the response.

At that boundary and before each additional body eviction, the same reversible
probe checks whether removing the whole
`validation_work` object and setting `source_freshness.details_omitted: true`
is sufficient to fit the current response, including that marker and the
self-measured byte/token receipt. If it is sufficient, the packer keeps every
current complete body and metadata field and records the diagnostic omission.
If it is not
sufficient, it restores the exact work object and the previous value or absence
of `details_omitted`, then follows the existing metadata, optional-reference and
evidence eviction order. The later body probe retains the receipt whenever a
smaller body set can accommodate it. No SQL subfield is zeroed, shortened or
partially reported.

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

The subsequent PR143 CI run `37629938552`, job `112821255708`, exposed an
earlier packing boundary in the unchanged
`p7_v05_v11_independent_review` public BM25 oracle. After duplicate rendering
was compacted, the real response occupied 16,020 bytes under the same
16,000-byte cap. Omitting the complete optional receipt reduced it to 15,578
bytes with the omission marker included. The old metadata projection instead
removed `stage_a_layer_scores` before reaching the body-protection probe.
The new early probe protects the complete existing metadata without a
score-field exception or a changed budget.

`packing_validation_work` includes three narrower controls over actual indexed
source with explicitly synthetic output pressure: a fixed cap that keeps the
complete body by omitting the optional work object, and a failed omission probe
that must restore the entire producer receipt and an absent, false, true or
null pre-existing marker. The latter also checks inputs without a work object,
graph-induced Partial status and source/body preservation. The third control
starts from unprojected hit metadata and full retrieval lanes, proves duplicate
rendering yields first, and then verifies byte-for-byte value equality of the
hits and retrieval metadata when the early receipt omission fits. All three
check actual serialized bytes, self-accounting and idempotence; expanded
budgets do not recreate an omitted receipt.

Commands:

```sh
cargo test --locked -p cc-eval --test diag_p5e_fix_review_boundary_20261003
cargo test --locked -p cc-eval --test packing_validation_work
cargo test --locked -p cc-server --test p7_v05_v11_independent_review
```

Failure/reproduction logs, original hashes, stage observations and the final
scoped validation receipt are kept in
`artifacts/checkpoints/validation-work-packing-budget-fix-20261007/`.
The separate early metadata regression, unchanged public oracle, source
identity and subsequent validation results are recorded in
`artifacts/checkpoints/validation-work-bm25-metadata-fix-20261007/`.
Historical source admissions and review records are not modified by this fix.
