# PR61 V11 cache consumption — independent bounded review

Subject PR61: `6f3cee1f2d23b43af5271bcaeaea67b931fd4fb7`.
Author executed test source: `744789649d1e0f2f3b41077ec3d1f5caacbff0cc`.
Git comparison confirms no crates/Cargo/CI differences between those subjects.
Only a new independent review test and this evidence directory are added here;
the author test, production, matrix/ledger and public-v19 holdout/gold are untouched.

## Recommendation to the main integrator

Preserve bounded `closed_declared_subset` for these three rows of the authoritative
37-row `P7-REMAINING-GATES.md` at the fixed subject:

| Row | Independent conclusion | Actual limit |
| --- | --- | --- |
| V11/immutable | Accept immutable search/ranking/query domains and supported tiers. Original 50-field inventory, all five query-policy values and 5 tiers replayed; analytic symbol bonus +2.0 consumes the captured value despite caller mutation +9.0. | Only 13 author score/output activations, not all 50 scoring branches. Distinct engines own distinct LRUs: this is domain isolation, not a collision test for a shared mutable-config cache. No such shared domain is admitted. |
| V11/graph-extra | Accept actual top-k-only/context separation and top-k changes, plus unordered file/language sets. Original Arc/counter assertions replayed; independent literal `{a.py,z.py}` output oracle confirms scope before/after permutation. | Original fixture has no cross-file call edge; actual graph-neighbor enrichment is established by other bounded receipts, not this extra-axis test. Duplicate-set normalization and every language are not added claims. |
| V11/dense-context | Accept current supported semantic-only publication, ordinary warm cache retention, dense cache bypass, final hydration/selection/rerank/packing consumption. New final envelopes match a reopened cold engine for local and real encoded dense queries. | No final-envelope LRU exists. These are recomputation/consumer proofs, not final-LRU key collision proofs. No alternative selector/budget version or metric/tokenizer implementation is admitted. Cross-build rows remain not run. |

Authority: `02-CONTRACTS.md` C12, lines 92–107, requires final context to depend on
all used generations and selector/budget/rerank specs. Current code meets this by
reassembling each final envelope: `engine.rs::search_in_context_with` enters
`with_stable_generation`, `assemble_context_once` hydrates current sources,
selects with current intent/top-k, then calls the real budget packer. No cached
final envelope is returned. `SearchEngine::new` clones immutable configs into an
instance with fresh LRUs; graph/local caches bypass get/put when semantic is
present (`engine.rs`, `engine_graph.rs`). This structural review is combined with
observed consumer results; structure alone is not reported as runtime acceptance.

No product defect or false cache hit was found in these executed supported cases.
Full V11 and P7-014 remain open: this review neither closes the two cross-build
rows nor substitutes for lifecycle/quality/concurrency requirements elsewhere.

## Independent oracles and execution

Rust 1.95.0 (`59807616e`, x86_64-unknown-linux-gnu), locked dependencies:

- `semantic`: unchanged author 3/3; new independent 2/2.
- `semantic-http`: unchanged author 3/3 plus new independent 2/2.
- Strict clippy for the new target with semantic-http: pass; scoped rustfmt and
  `git diff --check`: pass. This is not a whole-workspace or second-toolchain claim.

New three-file fixture is authored locally, not derived from a retrieval gold:
`a.py/b.py/z.py` have literal returns `111/222/999`. Exact source-byte equality,
literal complete path sets and an analytic +2.0 score delta prevent a fresh/warm
comparison alone from hiding identical wrong outputs. Original tier receipts
again show preselection `120/120/120/150/200` and 50 fields/13 activations.

A real SemanticRuntime worker publishes three document embeddings through the
production DB path. Incarnation/index/evidence remain fixed and semantic epoch
changes. The retained warm handle and a new CodeIndex opened on the same durable
DB produce equal final hits, rendered prompt, selection receipt and spans for
local retrieval. Production QueryEncodingService then populates the query cache;
the actual recall emits nonzero dense candidates and final dense hits/selection/
rendered prompt match the reopened cold engine. A pinned z.py request consumes
reranking and selects literal z.py in both domains. The original author test
independently records dense result/graph misses with no hits; it remains unchanged.

The real packer receives the resulting dense envelope, not invented JSON. At
16 KiB the final HTTP-profile envelope is 13,510 bytes; at 8 KiB it is 8,038 bytes,
with byte count and ceil(bytes/4) estimate recomputed from actual serialization.
The tighter pack explicitly omits all three full hit bodies into continuation
references; this is not a claim that source bodies fit in 8 KiB. An insufficient
2 KiB budget fails closed with `required context metadata exceeds output budget`.
This directly exercises supported runtime budget tightening, not a fake spec bump.

Raw new-review JSON is from the final semantic-http run; semantic success is
preserved separately in its log. PID-named raw author receipts retain both
profiles. Receipt records source/test/lock hashes and actual test binary hashes.

## Retained setup failures

Attempt 1 failed compilation because this new test incorrectly assumed CacheStats
Serialize, ContextSpan PartialEq and matching integer widths. Only test projection
and comparison types were corrected. Attempt 2 wrongly trimmed the fixture's final
newline; the oracle now compares exact original bytes. Attempts 3/4 assumed 2/4 KiB
could contain required metadata; raw errors are retained. Final test asserts the
2 KiB fail-closed result and exercises a valid 8 KiB tight budget. No production
behavior or author assertions changed, and no product counterexample is claimed.

Verify checked-in provenance and numeric/literal receipts:

```sh
python3 artifacts/checkpoints/cloud-v11-consumption-review-20261003/verify.py
```

Re-run the actual consumers with ordinary official Cargo (no live provider):

```sh
cargo +1.95.0 test --locked -p cc-server --features semantic --test p7_v11_cache_consumption --test v11_consumption_independent_review
cargo +1.95.0 test --locked -p cc-server --features semantic-http --test p7_v11_cache_consumption --test v11_consumption_independent_review
```
