# Source-bound public qname implementation receipt

Base: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`; design review:
`bc22dc9cd662e12341f3c2e9d31a0a6d4a90195c`; recovery checkpoint:
`697d6c4927bc989f47752bae4be90f81e8334715`. The parent approved schema 25,
module model 3 and rebuild-on-mismatch. PR 132 remains draft for independent
parent review. CHECKPOINT.md records the earlier unfinished recovery boundary;
the source/receipt audit described there is now completed.

The source-bound format-v1 association is prepared from the scanner-confirmed
original parser snapshot and exact complete AST declaration envelope. All
original candidates at the same endpoints remain visible to ambiguity checks;
none are deduplicated into authority. Shared real file writers require exact
source/document/name/kind/qname agreement and the actual SQL-surviving symbol
before inserting the association in that transaction. Non-surviving or missing
proof omits identity. Contradictory persisted relationships fail closed on cold
and warm public hydration, using the caller's read snapshot and query generation
fence. Existing hit name/kind labels cannot acquire another entity's qname.

Python qnames now use the native dotted lexical ancestor convention; local
functions remain Function without receivers, direct class members remain Method.
Decorated functions have one canonical wrapper declaration; call references use
that same symbol, so SQL UID replacement cannot discard its owner envelope.
Indexed source is used for identity; current disk only supplies the existing
independent public freshness acceptance/rejection. Adjacent whitespace retained
by the partition is allowed, explicitly attached documentation is bounded, and
same-name neighboring declaration bodies are rejected.

Executed with direct Rust 1.95, official Cargo.lock and --locked, normal platform
network/proxy, and owned build/index caches. `validate.py` reproduces the bounded
commands and standalone native driver without editing cc-eval or its gold/scorer.

| Check | Passed | Receipt |
| --- | ---: | --- |
| workspace fmt / all-targets clippy, warnings denied | yes | fmt.log / clippy.log |
| real parser / transaction / public / lifecycle tests | 17 | product-tests.log |
| parser / search library regressions | 183 / 298 | parser-search-regressions.log |
| schema / epoch / physical FIFO regressions | 5 / 10 / 1 | corresponding logs |
| native independent micro gold, missing identity | recall10 0 | native-scores.json |
| native independent micro gold, correct identity | recall10 1 | native-scores.json |
| native independent micro gold, wrong qname | recall10 0 | native-scores.json |
| actual MCP JSON-RPC, correct / wrong qname | recall10 1 / 0 | public-mcp-current.json / native.log |

The native driver reads complete production engine API output and independently
executes the real MCP server over JSON-RPC using the unchanged evaluator runner.
`public-context-current.json` and `public-mcp-current.json` retain both complete
outputs. It normalizes them with the
unchanged evaluator and verifies every hit against original fixture bytes. It
freezes every pre-existing public hit field, including scores/reasons/score trace,
plus original source and document identities against the actual v24 baseline.
It never fills qname into hit metadata. Wrong qname is an independently wrong
gold alternative evaluated against the same actual output.

The actual saved v24 real-parser synthetic cache, its SHA-256 and original rows
are retained as `v24-original-fixture.sqlite3` and `.json`. Tests copy it only to
new owned namespaces and prove v25 rebuild/reparse on unchanged source, then
zero-parse unchanged incrementals and reopen. Other tests cover same-name Python
classes, nested/async/decorated functions, split Python/Go methods, original
UTF-8/CRLF bytes, invalid UTF-8 owner endpoints, same-name neighbor rejection,
missing/ambiguous/legacy/partial/no-symbol omission, hard scope, transaction
rollback, SQL survival, source/doc/symbol/tag corruption, delete/rename, changed
disk with an old indexed snapshot, pinned handles and actual concurrent
incremental queries across multiple source generations.

Association SELECTs are charged to the existing hydration SQL work bill. This
does not change ranking or lane budgets; it is additional read work. Owner maps
retain all candidates and avoid repeated whole-file scans. No throughput,
storage-overhead or 100k certificate follows from these bounded tests.

Not run: public DEV/Requests re-scoring, holdout/V19, release/100k certification,
other platforms, all-features/provider tests, the explicitly excluded old-runtime
EROFS/GC-WAL/kill/staging/denied/private-localdiag scenarios. No daily database
was migrated/reset; no new additive migration or compatibility backfill was
invented. Public taxonomy, ranking/scores, Partial, scorer and frozen gold are
unchanged. Parent independent review is pending; V19/parent items remain open.

`source-manifest.json` binds the actual workspace sources/lock and local receipt
driver by content hash; it deliberately avoids a circular final commit SHA.
