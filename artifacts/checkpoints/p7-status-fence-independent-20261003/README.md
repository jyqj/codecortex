# Independent bounded PR54 status-snapshot repair review

Executed fixed production candidate: **`3dceedf3dcee851b3b2e4d4938bba11d76c63326`**. Review-test source: **`7def2c401e0d302484d62bc6bbd0ac3dd6be1cd4`**. Candidate status module and all 380 tracked src/Cargo files are unchanged by this task (`fixed-candidate-proof.json`). Only new review files and this evidence directory were added; production, original tests/assertions, ledger, dependencies and cache.rs were not edited.

## Result

**40 bounded command runs; 120 passed / 0 failed / 0 ignored**:

| Group | Repetitions | Test executions |
| --- | ---: | ---: |
| Exact-source four-case observer fixture, semantic | 10 | 40 |
| Same four-case fixture, semantic-http | 10 | 40 |
| Original unchanged complete p7_v11_generation_public target, semantic-http | 20 | 40 |

The original product test exercises actual built-product stdio with synthetic loopback, including both search/context crossed-generation requests and the three-attempt direct fence. All original assertions, including the previously failed line337, are byte-identical to PR49/PR52. Its repair-check runs did not fail. Strict target clippy with -D warnings, rustfmt and diff checks pass. Every one of the 40 run receipts/raw/log digests was verified; actual product/test binary digests and source/module hashes are retained. Warm builds and repeated functional cases are not cold-build, quality or probability certification.

The old PR52 result remains **84 passes / 1 original-test failure** at its old fixed tree. It is not overwritten, removed, relabelled or counted as a pass here. The inherited prior evidence remains in `artifacts/checkpoints/p7-v05-v11-independent-20261003` and PR52.

## What was genuinely forced

The independent fixture uses a real parsed index, one-connection SQLite read pool, actual cc-server SemanticRuntime/document queue/publication transaction and a gated synthetic FakeProvider. No vector cache is inserted by hand, no status result is fabricated and no live provider/credential/source egress occurs.

1. **Legacy counterexample, deterministic:** release a held real document worker immediately after the old root snapshot and before the old separate semantic projection. The database/coverage reaches semantic epoch2 with one visible document; old code returns ready with epoch1. All twenty fixture rounds reproduce this. These passing counterexample assertions certify reproduction of the old defect, not old-code correctness.
2. **Candidate crossed snapshot:** release that real worker at the existing candidate observer after root reads. First attempted root epoch1 is discarded; attempt two returns epoch2, equal to independent stable coverage/generation with published=eligible=1, pending=0 and ready. Every round uses exactly two observer calls. Incarnation/index/evidence stay equal; the actual change is semantic publication.
3. **Continuous real churn:** after initial actual publication, every root observer rewrites/rebuilds the real source, schedules the real worker and waits for its next actual provider call/publication. Each of the three snapshots crosses a changed generation. Exactly three attempts and four actual provider calls produce a retryable error, null generation, unavailable index/query and **no ready or dense publication claim**. This is neither manual epoch SQL nor an unbounded synthetic retry loop.
4. **Stable/configured-space oracle:** absent writes, root generation equals independently read coverage generation and counts. An intentionally different configured model against the still-old active space remains partial/backfilling, configured dense_published=0, with semantic_configured_space_pending; the unchanged database generation is still reported coherently.

The repair covers a real production consistency window: old root/stats/freshness acceptance happened before semantic pending/coverage/configured-space reads. New code places those reads and worker failure observation inside the outer three-attempt generation fence and discards the entire attempted semantic result when crossed. This is a production-code repair, not a test-only wait or weakened assertion.

## Test instrumentation boundary

The observer is private, so the new isolated test module compiles the **exact non-test prefix** of candidate capability_status.rs, plus a child test declaration. Prefix identity is checked before replay and recorded in `source-binding.json`. The old prefix is compiled separately with only an entry rename, observer parameter and callback insertion before its semantic projection; that instrumentation is documented and not called the old product binary.

An adapter dereferences the **real production QueryServices**. Its private query_encoding_active method cannot be called externally, so the bridge **panics if invoked**. All fixture configs explicitly disable network_opt_in/query authority, proving this unrelated branch is not used rather than emulating its behavior. Public setters install actual recall/wiring/degradation slots equivalently to the private attach helper. Actual worker and DB classes come from the candidate library. Thus this is **L2 exact-source observer instrumentation**, not a widened production API or an L3 black-box claim. The separately run unchanged original target provides actual L3 stdio evidence and enabled query wiring coverage.

Preliminary compile errors (private attach access and non-Serialize coverage logging) are retained in `preliminary.log`; only new fixture composition/logging was corrected. The first executed four-case fixture passed; that preliminary success is excluded from canonical counts. No production timeout, assertion or callback logic was altered.

## Original assertion and residual limits

In the original single-document test fixture, no additional document writes/publications are introduced after the ready snapshot. Correctly pairing that ready observation with its publication generation makes its existing same-generation recovery assertion meaningful, and twenty full repetitions pass. **A later legitimate query/status can generally observe a higher epoch**; readiness is not a future-generation pin. Independent conclusions concern a single accepted snapshot's internal generation/count/configured-space consistency, not universal cross-request equality.

This review does not close complete V11/V05/P7-014, all cache-key/resource/thread axes, every auxiliary-only bookkeeping race, arbitrary mutable runtime/config/degradation interleavings or live-provider behavior. Query-auth bridge path is explicitly outside the L2 fixture. The candidate's cache/resource integrations are source-bound but not independently re-certified by this status-focused task. D1/D2 unchanged.

After execution, remote `codex/cloud-p7-v11-generation-snapshot-fix@c4452f986f0826f4d3936d91042e3616c088a664` was inspected: its extra commit is documentation/evidence only, with no crates/Cargo/scripts diff. The executed conclusion remains explicitly 3dceedf, without relabelling the run SHA.
