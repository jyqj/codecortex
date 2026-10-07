# Independent resolver exclusion bind review

Target PR92 `ace2bc7983be2955831c9384e44d1bdd0749c909`; fixed production source `85692bb526ce5a9fdd7c92aa71d9392e498a7cf6`. Actual old-source reproduction runs its parent `ff6ceecd556465a09cc93c44fde26ac5a51f2aef` in a detached synthetic-only `/tmp` worktree. Identical new test source is used in both checkouts. Only this exclusive test/evidence changes; production, original tests, Cargo, ledgers and corpus are untouched.

Verdict: **bounded independent support** for the production fix. The old public read API actually returns `too many SQL variables` with 50,002 excluded paths on a real populated schema with absent signature baseline. The same real API on fixed source succeeds above that count, including 200,003 exclusions and a 50,000-file / 100,000-symbol database. This is execution of the implementation on both source revisions, not inspection of a generated SQL string. The legacy harness intentionally passes only when the expected old error occurs; it does not claim old production succeeded.

## Semantics and safety

The independent oracle uses Rust literal HashSet membership and sorts `(file_path, start_line)`, comparing actual resolver rows. Twenty-seven cases cover empty/full/nonmatching exclusions, repeated members, apostrophes/double quotes, backslashes/slashes, JSON-looking and SQL-injection-looking names, Unicode/emoji, composed versus decomposed accents, case differences, whitespace/control characters and path-prefix/wildcard-like text. Matching remains exact stored string equality: no glob/prefix matching, case folding, separator rewriting, dot-segment resolution or Unicode normalization. Rows are inserted in reverse path order and reverse line order, ensuring the observed ORDER BY matters. Ties at identical `(file_path,start_line)` remain unspecified as in the old query and are not declared stable.

`symbols.file_path` is NOT NULL; inserting a NULL path is rejected. Nullable symbol metadata stays NULL in returned rows. Exclusions are Rust `Vec<String>`: there is no nullable entry that could poison NOT IN. String `null`, `NULL` and JSON-looking text remain strings. One additional observation of API-representable embedded NUL strings preserves exact membership on this bundled SQLite, but NUL is not a valid OS pathname and is outside real-filesystem support.

Production uses `serde_json::to_string` and a bound `?1`; paths never become SQL syntax, and `json_each` expands serialized strings, not SQL. The empty slice retains the existing no-bind full scan. The SQL-looking input oracle and post-query row counts/integrity confirm no table mutation. Bundled SQLite JSON support is exercised by these successful real queries; no optional external SQLite deployment is certified. The existing explicit read transaction still encloses seed-token and row loading; the patch does not alter that snapshot boundary. Concurrent writers/aggregate-cache behavior are not newly exhaustively tested here.

## Bounded resource observations

One debug test process, compilation excluded, ran four tests in 13.48 seconds and peaked at **104,868 KiB (102.41 MiB) RSS**. This includes fixture insertion, exclusion/expected-set allocations, JSON serialization/parsing, returned rows and oracle comparison. Main release work may run concurrently; timings are observations, not performance gates or isolated production latency.

| Fixture | Exclusion / input | Observed API + oracle time |
|---|---|---|
| 22 paths / 44 symbols | 50,003 strings, JSON 950,043 bytes | 74 ms |
| 22 paths / 44 symbols | 200,003 strings, JSON 3,800,043 bytes | 320 ms |
| 22 paths / 44 symbols | one 4 MiB unmatched string | 185 ms |
| 50,000 paths / 100,000 symbols | all 50,000 paths excluded, zero rows returned | 204 ms |
| same | 25,000 paths excluded, 50,000 rows returned | 374 ms |
| same | empty exclusion, 100,000 rows returned | 429 ms |

Large fixture setup took 11,502 ms in that run. Separate initial fixed run also passed; logs retain both results. All measurement stages include oracle work; no query-only speedup is inferred. One parameter bounds bind count, **not** total memory or time: input serialization, SQLite JSON/IN-set materialization and output grow with input/output scale, subject to SQLite limits. No new input-size cap, cancellation or memory budget is introduced by the fix. Inputs larger than the measured count/byte cases, extreme escape expansion and OOM were not exercised, and arbitrary huge inputs are not promised safe.

No real repositories, services/providers, release quality or full V20 acceptance is tested. Existing failed performance evidence is not overwritten or dismissed. The review supports this narrowly scoped overflow repair and leaves broader release/resource acceptance to its owner.

## Replay and evidence

```sh
CARGO_TARGET_DIR=/tmp/resolver-review-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-db --test resolver_bind_independent_review -- --nocapture --test-threads=1
# On exact old parent with the identical new test copied into its own worktree:
RESOLVER_REVIEW_LEGACY=1 CARGO_TARGET_DIR=/tmp/resolver-review-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-db --test resolver_bind_independent_review -- --exact actual_legacy_overflow_or_fixed_large_inputs --nocapture
python3 artifacts/checkpoints/resolver-bind-independent-review-20261003/verify.py
```

Receipt records actual commands, source hashes, binary hashes and resource method. Raw local logs are copied verbatim; no database/binary bundle is committed. All fixture databases are created by tempfile and contain only generated strings/symbols.
