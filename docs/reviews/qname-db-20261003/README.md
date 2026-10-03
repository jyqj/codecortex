# DB/index/lifecycle independent review — 2026-10-03

**Bounded DB relation/snapshot/lifecycle gate: PASS. Overall identity/PR-green/merge claim: REJECT.**
The new independent suites pass 7 index tests and 6 server tests, zero ignored.
The pinned product still fails all 3 `ci_schema_guard_contract` tests due to
schema-test migration omissions below. No qname mispublication counterexample
was established in the controlled cases. V19 and parent certification stay open.

Product: `4e3e5355d8e4c642574eb84bdf73b7373bf3c4a7` (PR 132).
Base: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`.
The parent's later report of docs-only head
`fd5146baf5a62f977ce5ed4682c16f485aef9da3` does not change this product target;
that later head was not fetched or tested here.
The parent separately reports parser review PR134 commit
`fb8ca8ae3b2cebf7d08255d37032158daafa63f2`: decorated classes can be recorded
as Function, publish wrong kind/qname through owner hints and lose nested
members. This finding was not independently executed in this DB suite; the
parent has assigned its separate repair. **SQL survivor/source/document
agreement does not establish AST semantic truth.** The database gate below is
conditional on correct original parser and owner semantics. It cannot certify
that reported decorated-class case or all identity publication.

Only these two newly authored integration suites and this review directory are
changed. No product, parser-owner, CI-repair, TODO, scorer or gold files changed.
The environment's `/workspace/.agents` is empty; this checkout has no `.agents`
or AGENTS.md. The design and implementation README were read as context;
author pass counts and fixtures are not independent evidence.

## Evidence and bounded decisions

`review-bounded-final.log` is the final receipt, Rust/Cargo 1.95.0, official
unchanged Cargo.lock (`ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`).
Toolchain came from `static.rust-lang.org`, dependencies from normal Cargo
registry fetch with `--locked`; cache/target reside in owned workspace paths.
No mirror, permission change, alternate credential route, env clearing or
old denied-path workaround was used.

| Check | Result and concrete observation |
| --- | --- |
| Real independent v24 cache | PASS: actual base CodeIndex full build of self-authored `review.py`, then SQLite `VACUUM INTO`; 4 symbols, schema24, no association table. Product reopens, reparses one unchanged file at25, exposes `Harbor.beacon`/`Ridge.beacon`, then reopen parses zero and preserves generation; model version explicitly asserted3. |
| Parser ambiguity / SQL survivor | PASS: duplicate original North method candidate suppresses its proof before SQL; same-name repeated top-level declarations retain two prepared candidates, actual SQL UID replacement leaves only second declaration authoritative, first qname omitted. No candidate deduplication. |
| Original-byte owner | PASS: CRLF/UTF8 fixture asserts exact source byte points at both owner endpoints; endpoints inside a multibyte character omit. Partial, legacy and no-symbol structures omit. |
| Leading documentation / neighbors | PASS: real top-level Python AST explicitly attaches leading comment; owner plus outside whitespace admits, same-name neighbor and span crossing into neighbor reject. Prepared source/document associations use the same real parser structure and byte slices. |
| Dirty-only | PASS: unchanged symbols leave persisted association JSON byte-identical. A deliberately changed symbol qname through dirty-only writer leaves the old proof untouched and hydration rejects its contradiction. This forged dirty unit is fault injection, not a claim real resolver renames declarations. |
| Transaction rollback | PASS: forged prepared doc_version rejects the real file replacement; prior generation and readable associations survive. |
| Cleanup | PASS: explicit chunks-delete trigger removes identities even with FK checks disabled in owned SQL fixture; actual incremental rename/removal revokes old paths and then all rows. |
| Cold and warm rejection | PASS: independent mutations of association row doc_version, JSON source digest/document version/tag, referenced symbol byte column, current files digest, and chunks source owner each fail cold SearchEngine plus previously warmed public final hydration while generation is deliberately held constant. |
| Missing authority | PASS: cold returns hits without qname; old cached qname cannot survive final hydration after association deletion, and errors rather than substituting a name lookup. |
| Document mirror hypothesis | PASS: single-field `document_manifest.reference_json.doc_version` mutation rejects both cold and warm public output. `verify_source_records` alone does not read that mirror, but subsequent candidate validation does. This is not a demonstrated public bypass. |
| SQLite snapshot | PASS: actual pooled read connection begins a transaction, reads joined identity/symbol/file/source/document data, a real writer commits replacement, same transaction still sees all old facts; transaction end exposes new facts. |
| Concurrent hydration | PASS: 80 actual batched hydration calls race 30 real replacements; each admitted qname matches the oracle for both locator AND original source identity. No mixed qname/source observed. |
| Generation/cache boundary | PASS: controlled epoch publication inside first `with_stable_generation` attempt forces exactly one retry and accepts only current generation. Actual incremental rename rejects old accepted envelope; cached public query publishes new identity. |

The original tests are in `crates/cc-index/tests/qname_db_independent_review.rs`
and `crates/cc-server/tests/qname_db_independent_review.rs`. The controlled
SQLite fault mutations affect one field at a time; generation is held fixed
solely to exercise warm final validation. No paired re-signing of symbols,
source, docs and associations is represented as an ordinary single-field fault.

## Code review: authority, transactions and fences

`documents/symbol_identity::prepare` requires a validated complete original AST
snapshot, exact owner span, unique owner and original parser endpoint candidate,
matching existing chunk name/kind and a document from the same proof. Owner
UTF8 slicing is validated before mapping endpoints. Breadcrumbs supply no
identity. The model's format1 row is distinct from chunk/document wire shape.

Both real writers call `symbol_identity_store::insert_on` after documents and
actual symbols are inserted: single-file/full SnapshotWriteTxn path and
multi-insert incremental path. They check original candidate ambiguity again,
all file/source/document/owner labels, and SQL-surviving symbol fields. A
non-survivor is omitted; a contradictory prepared proof aborts the transaction.
The association table's chunk/doc FKs and explicit chunks-delete trigger cover
normal deletion and FK-disabled full-build cleanup. Dirty-only symbol writes
do not fabricate source proofs; stale contradictions fail reads.

Cold `chunk_rows_by_ids_with_work` holds one pooled SQLite transaction through
all chunk batches, document decoding, and `load_on`'s four SELECTs per admitted
identity. Warm `verify_source_records` holds a separate transaction through its
manifest batches and association/symbol/current-file/source reads. The snapshot
is pinned by the first SELECT, not merely by BEGIN. Subsequent public candidate
validation uses its own read stage; it is not one cross-stage SQLite snapshot.
`EvidenceHydrator::finish` and outer context `with_stable_generation` recheck
persisted incarnation/index/evidence/semantic generation, discard mixed attempts
(including failed attempts), and preserve stable corruption errors. Existing
full rebuild/reopen generation publication is retained; this review exercised
normal fresh builds/reopen and controlled real file replacements. It did not
exercise old staging/GC/WAL/kill/denied/private-localdiag tests or certify every
crash/publication interleaving.

## Actual SQL cost scope

PASS for cold association accounting: a 4-row hydration with 3 admitted
identities and one omission measured **14 statements / 16 yielded rows / 642 VM
steps**, zero fullscan/sort steps. This equals one chunk-batch SELECT plus four
SELECTs per identity plus one absent-association probe. `query_on` resets and
merges actual SQLite outer-statement counters; these are measured work, not
estimated SELECT counts. They remain additional hydration work; no score, lane
budget or ranking inputs change.

Scope limitation: final `verify_source_records` passes `None` to `load_on`, so
its per-hit identity SELECTs and manifest reads execute on cold AND warm public
assembly without addition to `RetrievalCost.hydration`. Warm graph-window caches
reuse originating-query receipts. The type explicitly says receipts are
originating work, not cache-hit work/global limits, and SQL counters only cover
named statements. Thus a warm receipt is not the current request's total SQL
cost. The implementation README's broad sentence “association SELECTs are
charged” is true for originating row hydration, not all final validation. This
review does not infer a ranking/budget defect or claim total-cost completeness;
precise documentation should explicitly exclude final manifest/identity
validation or a future separate validation-work receipt should measure it.

## Reproduced CI migration omission and minimum separate repair

`ci-schema-raw.log`: 0 passed, 3 failed, no ignores, exit101 on the pinned product.
This is a required regression-test repair, not evidence schema25 failed to
initialize or rebuild. Our independently generated real24 fixture succeeds.

In `crates/cc-db/tests/ci_schema_guard_contract.rs`:

- line47 and67: expected schema24 is stale; change to25 (or current constant with
  a separate explicit assertion current==25), and rename the fresh-v24 test.
- line87: `[20,22,23,25]` incorrectly includes current25 in mismatch cases.
  Use e.g. `[20,22,23,24,26]`, or current+1 for the future version. Continue
  asserting exact `SchemaStatus::Mismatch { stored }`.
- Do not add semantic tables to `fixtures/p7-ci-schema-v21.sql`. It is correctly
  historical and lacks semantic_outbox. Labelling its v21 contents25 sends it
  into the current-schema physical FIFO-index maintenance branch and produces
  “no such table: main.semantic_outbox”. Removing25 from mismatch input fixes
  that malformed fixture usage. Genuine current25 fixtures should be created
  with the real full current schema, never historical content relabelled25.

The parent owns the separate CI fix; this package changes none of those files.
Until that separate fix is tested, a blanket all-green acceptance is rejected.

## Raw failures, exclusions and reproduction

All intermediate logs remain. The first compile attempts had independent test
API mistakes (`usize` FromSql, Arc return type); these were fixed in test files.
`review-first.log` also retains base/product shared-target local-crate artifact
mixing, resolved by cleaning only six local crate artifacts in the owned target.
`review.log` retains a wrong locator-only concurrency oracle: legacy chunk IDs
can recur between generations, so the final oracle also keys on source identity.
`review-second.log` retains a fault injection blocked by a real FK; final row
mutation uses non-FK doc_version instead. `review-third.log` retains a method
comment attachment assumption; final fixture uses real explicit top-level doc
attachment. `review-final.log` retains a failed fixture-text edit subsequently
corrected. These are test-development failures, not product counterexamples.
No failure was counted as a pass or erased.

Re-run only this bounded review with the official toolchain on PATH, owned
CARGO_HOME and CARGO_TARGET_DIR:

```sh
cargo test --locked --no-fail-fast -p cc-index -p cc-server --test qname_db_independent_review -- --nocapture
cargo test --locked -p cc-db --test ci_schema_guard_contract -- --nocapture
```

The second command is expected to fail on this fixed product. To regenerate
the independent24 cache, copy `baseline_fixture.rs` only into an owned checkout
at the exact base as `crates/cc-server/tests/qname_db_base_fixture.rs`, set fresh
`QNAME_DB_V24_ROOT` and `QNAME_DB_V24_OUT`, and run its exact test target. Use a
separate target directory for that checkout to avoid local-crate cache mixing.
The binary fixture SHA256 is
`4fa21e174c72a75a851520b4361d54a576c5ca822c0822e04ffab37896816ee9`.

Not run: public DEV/new gold, cc-eval/scorer, frozen DEV, broad runtime,
`semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts`,
old staging/GC/WAL/kill/denied42/private-localdiag paths, release/100k and V19.
No merge or deploy; parent items stay open.
