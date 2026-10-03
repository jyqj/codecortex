# Independent cache-key review of PR51

Frozen subject: `a7efaae70cd0828b1a1b2d811e20176d855394b3` (PR51, based on PR49). PR50 read ownership remains with the main owner. This branch adds only `crates/cc-server/tests/cache_key_review_behavior.rs` and this dedicated evidence directory. All production, Cargo, PR51 tests and ledger bytes remain unchanged.

Remote PR51 currently points to receipt commit `41cea9f5f108eab9f571e0b5ed84bf613f85f7d0`. Its delta from the frozen subject contains only benchmark evidence/ledger updates, with no production or test change. Its published `cache-key-requirements.json` explicitly marks derived-version axes not_run, immutable config/tier partial, sealed alternatives not runtime-selectable, and full_V11_closed=false; the formal map/gate also leave full V11/V16 open. This independent conclusion agrees with that declared scope. Referenced PR46/47 integrated lifecycle assertions were not independently replayed by this review; this supplement does not replace their evidence.

Conclusion: PR51's tested query/result-cache axes have real behavioral evidence; their provider-call/counter oracles are not merely duplicate hash implementations. The independent supplement now checks **distinct cached values consumed by production code** and a real incompatible-schema reopen. No additional production cache-key defect was established in this bounded review. It does **not** certify all C12/V11 dimensions: parser/project-model version changes, cross-build encoding/document/policy/selector/schema-spec changes and integrated public semantic model/instruction transitions remain unmeasured here. A documentation conflict is identified below.

## Authority and dependency partitions

`docs/roadmap/code-index-v2/02-CONTRACTS.md:92-107` (C12) is the broad authority; V11 at `06-VALIDATION.md:33` requires complete keys along with execution/freshness behavior. The Q5 query-vector ruling is explicitly recorded at `cc-semantic/src/cache.rs:554-578`. These cache types do not share one universal invalidation key:

| Cache/domain | Required/current dependency | Evidence and boundary |
| --- | --- | --- |
| Derived file/symbol/directory state | seed token plus parser/project-model/resolution-rule version | C12 requires these. `cc-db/src/seed_symbol_cache.rs:11-31` documents a seed-row aggregate; project model has a frozen version at `cc-model/src/project_model.rs`. PR51 does not change/test parser or rule versions. **NOT_RUN** version-change acceptance; raw input/ranking tests are not substitutes. |
| Chunk/source text | document version/source digest; current text-LRU also clears with observed index generation | `cc-model/src/identity.rs:109-120`; `cc-search/src/engine_cache.rs:170-201`. PR51 same-engine typed source update observes old text disappear and both result domains miss. Version-constant/renderer/parser migration is **NOT_RUN**. |
| Paid document artifact | namespace + space + exact embedded input digest + document spec; checksum/dimension/model validated on read | `cc-semantic/src/cache.rs:301-399,478-489`. Identical input/spec may legitimately reuse a vector across new document provenance, rename or DB rebuild. A document-version change is not automatically an artifact miss. PR51 snapshots a real document worker's artifacts but its query variants run on a separate component cache; this alone is not public semantic config-transition/re-embedding certification. |
| Query vector | namespace + QuerySpecDigest + exact query-byte digest; query spec includes full space, instruction, max_tokens, tokenizer | `cache.rs:554-606`, `spec.rs:192-204,279-329`. No document/index/evidence/semantic epoch belongs here under Q5: document publication changes visible recall, not pure query encoding. PR51 observes real service/factory/adapter counts for namespace, model, dimension, instruction, budget and byte axes; unknown tokenizer rejected before provider/cache. New supplement proves distinct values survive the matching production consumer. |
| Local result | incarnation/index_epoch + request/hard scope/query/policy/config domain | `cc-model/src/generation.rs:15-17`; `cc-search/src/engine_cache.rs:253-333`. Request fields: strategy, intent, query, top_k, path_prefix, grep, preselect limit; languages/files are framed optional unordered sets; boost/conversation/recent/pinned/overlay are ordered optional hints. Domain also contains selection/budget/document version, query config, search/ranking fingerprint and repo tier. PR51 tests current request fields and empty/set-order/hint-order behavior with real SQLite cache counters and warm Arc reuse. Static version/tier/all config changes remain bounded source review, not a full mutation matrix. |
| Graph result | local dependencies + evidence_epoch + graph limits/token budget/top_k-only mode | `generation.rs:18-26`, `engine_cache.rs:337-356`, `engine_graph.rs:106-110`. PR51 tests six graph limit fields/token budget and evidence-only writes; top_k-only mode is outside PR51's changed-axis matrix. |
| Dense recall / final context | document generations, space/query spec and selector/budget/rerank dependencies | Dense scans manifest/artifacts each query (`cc-server/src/semantic_wiring.rs:865-920`). Ordinary local/graph caches explicitly bypass reads/writes when `request.semantic` is present (`engine.rs:207,225`; `engine_graph.rs:110,193`); their local keys are not being used as a dense-result cache. C12 freshness remains a separate generation/read-view obligation. Full public dense/final-context lifecycle is **NOT_RUN** by this supplement. |

Control cancellation/deadline remains per-call validation, not a reason to reuse or replace the deterministic query key. This review does not re-certify all V11 cancellation/fairness behavior.

## Oracle assessment

PR51 `p7_v11_cache_key_matrix.rs:355-592`: `QueryCacheKey::new` is used to inspect the produced vector, but expected miss/hit also independently constrains actual HTTP-adapter posts and factory invocation, plus repeated warm requests. A missing tested key component would cause a wrong hit and fail those counts even if inspection reused the same key constructor. Thus the whole test is not tautological. Its constant `[1,0,...]` response leaves **wrong-value consumption** unproved.

The new `production_service_adapter_and_consumer_preserve_distinct_cached_values` exercises the production `QueryEncodingService`, production provider factory/OpenAI-compatible response adapter with a synthetic injected transport, and a second production `encode_queries` cache consumer. The consumer's provider panics if contacted. The test never constructs a QueryCacheKey or hashes expected keys. Eleven cases cause exactly nine adapter POSTs/factory calls; deliberately distinct valid finite vector markers are consumed as:

`10, 10, 20, 30, 40, 50, 60, 70, 80, 90, 10`

Cases cover base, identical warm with a changed would-be supplier, namespace, same-dimensional model, dimension, instruction, token budget, leading byte, composed/decomposed Unicode, and return to the original key. Cache cardinality grows only on the nine misses, well below capacity. This distinguishes valid reuse from cross-axis conflation and demonstrates the original value persists rather than the latest value leaking into a warm hit. Markers are fixture outputs independent of the key formula. Changing an instruction is a key-isolation test, not proof that the product applies an instruction to its HTTP input (public config currently exposes no instruction key).

The transport is an in-memory HTTP seam, **not actual query TCP**. PR51's unchanged document setup uses real loopback HTTP and actual product stdio; do not transfer that network label to component query tests.

PR51 local/graph oracles use actual counters and Arc reuse, plus a fresh engine recomputation. This independently tests cache behavior/equivalence but not the correctness of the production ranking algorithm itself. Graph changes need not alter values on a tiny fixture; the miss counter is the key-axis observation. PR51 public config test observes changed lane weight after `set_project`, and full rebuild changes incarnation/source bytes. It proves a new domain honors config; since each immutable engine owns a new cache, it does not demonstrate every fingerprint field can mutate in a shared retained cache.

## Schema supplement and remaining boundaries

The new schema test warms a real SearchEngine on source `111`, drops its owning DB/engine, sets only the **synthetic** DB's user_version to incompatible 1, changes source to `222`, and reopens via public `CodeIndex::new`. Production migration/reset creates a different incarnation and advances index/evidence epochs (observed 5→6 / 1→2 before reindex). The rebuilt result has `222`, no `111`, first miss=1 then warm hit=1. No key, generation or incarnation is manually seeded. This is a real new-domain invalidation observation; an old pinned query concurrently straddling reopen is not tested. Schema 21→22 is additive in `index_migrate.rs:38-80`, so this test must not imply every schema difference rebuilds.

Remaining **NOT_RUN**, not evidence of a confirmed cache defect:

- actual parser/project-model/resolution-rule version upgrades and persisted derived-input migrations;
- compiled encoding/document/render/selection/budget/retrieval-policy version changes; metric admits only Cosine and versions are sealed in this binary;
- a second admitted tokenizer (current gate rejects unsupported labels), graph top_k-only variant, every repo tier and all immutable search/ranking/query config fields;
- public semantic model/dimension/query-spec changes while preserving/retiring resources, document-template-only config changes/re-embedding costs, semantic-epoch freshness through dense/final context under concurrency;
- adjacent additive schema migration and old live handles during incompatible reopen.

Documentation conflict: `crates/cc-semantic/docs/ENCODING-SPACE.md:30-33` and `spec.rs:87-92` say dimension does not enter SpaceDigest. Current `spec.rs:167-170` hashes the whole serialized VectorSpace, whose fields include dimension; it **does** enter the current digest. The observed dimension cache miss is consistent with the implementation. Owner should align prose with the chosen frozen format; this evidence branch changes neither format nor docs.

## Executed evidence

Rust 1.95.0 and 1.99.0 each pass the two new independent tests and the three unchanged PR51 tests. Strict 1.99 clippy for the new test target and per-file rustfmt/whitespace pass. Both source/test snapshots and all raw successful observations are retained. The first new compile attempt used the wrong DB facet (`writes` instead of `admin`); a subsequent synthetic HTTP fixture omitted JSON Content-Type and was rejected by the real adapter before caching. Both failed attempts are retained and excluded from passing results; only new fixture code was corrected.

Reproduce:

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=5
export CACHE_KEY_REVIEW_OUTPUT=/tmp/cache-key-review-independent
cargo +1.95.0 test -p cc-server --features semantic-http --test cache_key_review_behavior --locked --offline -- --nocapture
export P7_KEYS_EVIDENCE_DIR=/tmp/cache-key-review-pr51
cargo +1.95.0 test -p cc-server --features semantic-http --test p7_v11_cache_key_matrix --locked --offline -- --nocapture
```

Repeat with `+1.99.0`. Only synthetic providers, isolated fixture files and loopback traffic; no real services/credentials. Verify retained evidence with `python3 artifacts/checkpoints/cloud-cache-key-review-20261003/verify.py`. Parent may attach this bounded review receipt to its ledger; this branch does not change acceptance status.
