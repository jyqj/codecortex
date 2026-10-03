# Independent V16 production-oracle review

Frozen candidate PR45: `b973de5c542db2ba46ddb0798b03d67627e42275`.
Original oracle file SHA256: `806e000aebc0c697f09e8082ceae7ae90d4fa51a1ce0fadee177bb6a09055dc9`.
Production source is unchanged from `c8c20b5b7d416372ee06ed5e248663c48912064e`; the crates delta contains three added integration tests only. No production, original oracle, resource-author file or ledger was changed by this review.

## Findings

The original range gold implements a fixture-local predicate and **does not call production HardScope::passes** or repo_path helpers. Its fixture scores are literal hand values and **do not call production cosine/scoring helpers**. It obtains document IDs from SQLite to compare the contract's doc-key tie order, but independently asserts the complete literal fixture path set and one document per file before comparing scores. That identity lookup is a sanctioned observation, not reuse of production scope/scoring as an oracle. No shared-predicate false positive found for the declared original six scope cases.

Two proof limits are material:

1. All original query/document vectors have unit norms (up to float tolerance). An intentionally wrong raw-inner-product scorer returns the same original hand gold within tolerance. Independent negative-control test demonstrates this directly. Consequently the original fixture alone cannot certify the cosine norm division or scale invariance. This is an oracle coverage gap, **not a failure of the actual production cosine implementation**.
2. The original three seeds rotate file creation order, followed by one full index build. The production scanner sorts paths (`cc-index/src/scanner.rs`, scan_with_manifest path sorting), and no row insertion/scan-order observation is captured. The phrase “three insertion orders” is stronger than the retained evidence; call them three file-creation rotations. The independent variant establishes three actual SQLite insertion orders via separate one-file commits and asserts `document_manifest ORDER BY rowid` after every commit.

Five rounds × three fixtures × six scope/cosine cases = **90 case receipts**, not 90 independent quality questions or a count of distinct Rust assert invocations. The original report already correctly sets independent quality query count to zero. Repeating this fixture does not remove the two blind spots above.

## Independent production variants: 2 passed / 0 failed / 0 ignored

New `p7_v16_oracle_independent_review.rs` imports the production interfaces under test but no production scope predicate, path helper, cosine helper, ranking helper or author gold/fixture. Its expected admitted paths are literal per-case lists. It uses integer vectors with analytic norms: e.g. east query [5,0], document [30,40], dot150 / (norm5 × norm50) = 0.6. Gold includes nonunit scores 0.8, 0.6, 0, -0.6 and north-direction 1, 0.8, 0.6, plus Python 0.28/0.96. Scaling changes raw dot ranking, so the original blind spot would fail here. Score tolerance is 1e-12.

Three independently observed insertion orders each execute:

- Separate real incremental file commits through CodeIndex/indexing handler and real caller-driven document worker. Eight actual document POST inputs per original model, no manual vector-cache or manifest seeding; exact literal catalog and every rowid insertion prefix are asserted.
- A cold public QueryHandle search using production query encoding/HTTP, then literal-gold exact recall for k=0/1/2/5/99 (including a tie at the top-k boundary and the negative candidate).
- Strict component prefix excludes a stronger `scope_extra` sibling and `outside` vector before top1; path/language/file intersection, exact-file prefix, case mismatch, empty language/file sets, and Python-only cases use literal expected path lists.
- Cold north query changes direction and score order with one additional real query POST; all warm scope variants reuse cache.
- Real deletion removes the best document, changes top2, and prevents public hydration from returning the deleted path. Public checks require a complete nonempty semantic lane and an actual rrf:semantic contribution in hydrated score traces. External soft hints and preselect1 cannot escape hard Rust scope or erase all hits.
- Explicit model change/set_project/indexing switch; retained old recall is unavailable/semantic_space_not_active with zero candidates and no extra query POST. New-model public query requires a fresh query POST and preserves the independent nonunit exact gold; warm new-model search reuses cache.

Production variants use real SQLite, worker, registered production provider factory, query encoding, blocking HTTP transport, cache/manifest, exact backend, QueryHandle and hydration. Only synthetic source/query strings and an in-process 127.0.0.1 endpoint with synthetic file credentials are used. No live/paid provider, outside endpoint or source egress. Progress deadlines retain five seconds. This is in-process L2, not executable stdio L3 or quality L4/L5. Bounded memory is not measured.

## Independent retained-receipt audit

`scripts/review/p7_v16_receipt_independent_audit.py` checks each raw file hash against commands.json and audits all five rounds/15 fixtures/90 cases. It **never consumes result.gold**, production predicates or scoring helpers. Expected result path sets and score values are separately literal; actual legacy chunk paths, score order/doc-key ties, coverage, deletion, cold/after-delete hydrated scope and retained semantic contribution, and old-space rejection are checked. Both saved receipt representations (lane_receipts/lanes) are interpreted explicitly. Result: 15 fixtures and 90 case receipts checked successfully. Recorded query_posts=30 is an author execution counter, not independently reexecuted by this offline audit; the new variant independently proves its own nine query POSTs across three fixtures.

No replay of the original five executions is claimed. Their saved receipts pass an independent interpretation; the new variants provide freshly executed independent production evidence. Targeted semantic-http clippy `-D warnings` and the new file's rustfmt check pass. Full suite/CI/resource memory/L3/live-model/quality holdout were not rerun.

## Reproduction

```sh
cargo test -p cc-server --features semantic-http --locked --offline --test p7_v16_oracle_independent_review -- --nocapture
python3 scripts/review/p7_v16_receipt_independent_audit.py
cargo clippy -p cc-server --features semantic-http --locked --offline --test p7_v16_oracle_independent_review -- -D warnings
rustfmt --edition 2021 --check crates/cc-server/tests/p7_v16_oracle_independent_review.rs
```

Rust 1.95.0; PATH=/workspace/.cargo/bin:$PATH, RUSTUP_HOME=/workspace/.rustup, CARGO_HOME=/workspace/.cargo, CARGO_TARGET_DIR=/tmp/p7-017-build, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0.

Ledger/report suggestion for integrator: retain original hand-scope/top-k/delete/space L2 success; rename its insertion-order claim to creation-order rotations; add this independently executed nonunit cosine, actual insertion order and retained-receipt audit block. Keep full V16/V05/V11 and P7-014 open for their remaining gates. No current production counterexample found; do not turn these synthetic functional checks into quality-query counts.
