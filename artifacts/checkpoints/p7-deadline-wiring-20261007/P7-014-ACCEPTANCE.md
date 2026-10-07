# P7-014: retention fix and current wiring evidence

**Status: bounded production fix accepted; original task responsibilities remain
open for reconciliation.** Source `5af7ac0089ee7522e78ff2ce2468f881c8cf70f2`
depends on the accepted P7-012 tree. Only `semantic_wiring.rs`, the new
`p7_gc_retention_config.rs`, and the GC paragraph in `docs/CONFIGURATION.md` change
relative to `aee4aeacef0166b400c9268c78ec800085c97573`.

## Actual defect, unchanged regression and repair

The public GC grace is a `u64`, while the runtime/GC contract uses a signed `i64`.
Previously only zero was rejected: `2^63` wrapped to a negative retention floor.
The fixture was committed first at `9bb2f4a2e9e8468432a197a652e6219f3366f69a`.
After rebuilding the affected packages, original production yielded 3 passes and
1 failure. The same fixture bytes yield 4 passes with the checked conversion.

Assembly now rejects an unrepresentable grace as a key-specific configuration
error before cache-root resolution or provider construction. Existing zero
rejection stays intact. The disabled master switch remains inert. Values 1, 3600
and `i64::MAX` are exercised against actual cache creation and GC: fresh objects
survive, the exact configured-age boundary expires them, and DB generation stays
unchanged. Documentation states the supported range.

The first baseline and first fixed attempt are also retained. The first fixed
attempt failed compilation with E0432 because the shared target supplied an
incompatible `cc-semantic` artifact as fresh. Both initial attempts are excluded
from canonical before/after proof. A targeted clean preceded the canonical
baseline; raw Cargo artifact events document the builds. No source-map-only or
fresh-flag-only certification of all transitive dependencies is made.

[Independent GC review](p7-014-retention-independent-review.json) accepts the exact
source, fixture and documentation scope. The reviewer did not rerun Rust and did
not certify this whole task. Actual final execution is separately recorded in
[p7-014-receipt.json](p7-014-receipt.json).

## Current applicable engineering matrix

| Matrix | Actual result | Counting boundary |
| --- | --- | --- |
| New GC retention regression, `--features semantic` | 4 passed | Four new functions; canonical original production was 3 passed / 1 failed. |
| Original parameter contract, `semantic-http` / `semantic` / default | 12 / 9 / 8 passed | Same existing target across three profiles; repeated functions are not new independent cases. |
| Existing runtime and status unit scopes | Runtime 11 passed, 2 original ignored; status 10 passed | Historical ignored counterexamples remain unchanged and are not passes. |
| Original default `mcp_stdio` | 9 passed | Legacy public stdio compatibility. |
| Scoped strict Clippy and workspace formatting | Both exit 0 | No behavior-test credit. |
| Unchanged actual lifecycle stdio driver and original gate map | 13 state checkpoints; 129 raw stdio records; 8 loopback HTTP attempts | One actual lifecycle run. These counts are separate from Rust functions and semantic-quality questions. |

Across the Rust profile matrix there are **63 successful executions**, representing
**46 distinct successful functions**, plus two ignored historical functions. The
original five-state requirement is covered by the actual lifecycle: unconfigured,
disabled, backfilling, failed/degraded, and ready. It also checks cold/warm ready
queries, clean restart, three authority revocations, and the unmodified three-attempt
provider-failure path with its real 30-second backoff. Ten tool-list responses each
contain 14 tools. Existing symbol mode and configuration/schema/sanitize/handler
paths are covered by the original parameter and legacy targets.

The lifecycle product SHA is
`d20d2262d9c64553a9d5e796d31430c1e52041d161f776c57cc529c531fee965`,
recorded before and after execution. The raw verifier checks status responses
against the 13 original gate rows, matches checkpoints back to actual RPCs, and
recomputes HTTP attempt/cost counts. Four document and four query posts are observed;
reported token usage is synthetic and paid currency remains null. No real-provider
authorization or semantic-quality conclusion follows from this loopback run.

## Original responsibility reconciliation

The original P7-014 brief assigns wiring rows 1/2/3/6/7/10/13. The
[implementation order](../p7-implementation-planning-20261002/IMPLEMENTATION-ORDER.md)
requires a thirteen-row reconciliation at closure. This round preserves all
original owners in [p7-014-wiring-reconciliation.json](p7-014-wiring-reconciliation.json).
It does not treat the five-state minimum as permission to delete those obligations.

Current code implements full configured assembly/teardown, bounded post-index
worker scheduling, outer degradation projection, the GC grace configuration,
scalar status counts, and an explicitly configured-space transition. Standalone
recovery/reconcile helper definitions and the explicit GC helper do not by
themselves establish all composition-root scheduling responsibilities. The parent
must reconcile the original explicit/post-index minimum and remaining scheduling
ownership with P7-015/016 evidence.

GC counters are returned but are not persisted or exposed in status. The original
wording makes auxiliary persistence conditional and allows a justified P7-016
evidence-only disposition; that decision must be explicit. Space-switch triggering
is present, but its JSON log remains append-only without a cap. The original
as-needed cap likewise needs an explicit disposition. No implementation of either
missing feature is claimed here. Opportunistic reclaim and bounded desired-set
competition retain their original P7-016 and P7-015 owners.

Until that reconciliation is complete, P7-014 remains in progress. Task state,
original acceptance text, gold fixtures and the reviewed-source registry are not
modified by this checkpoint.
