# Independent PR58 minimum-gate and lifecycle review

Fixed production/review baseline: **83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad**. New test/audit source frozen at **ed6663e9af270552d3217e64363416f8f5daa1a5**; finite replay runner frozen at **2a4e4e7a073bf39cfe7866a520549f600a6dd68a**. Only this new review/evidence directory changes. No production, main oracle, shared matrix, ledger, tasks, dependency, scope or hydrator edit. `fixed-source-proof.json` binds 413 pre-existing paths, including 388 source/Cargo paths, all original server tests and the authoritative gate documents to the baseline.

## Block A: V05/V16 minimum requirement recommendations

Authority is the literal V05/V16 rows in `06-VALIDATION.md`, with its SHA in `gate-review-map.json`. The map has one explicit row per minimum requirement, evidence SHA, assertion/sample boundary and `not_run`. Recommendations are for the parent integrator; this directory does not change acceptance state. No V20 numeric scale or V19 corpus size is imported into these functional requirements.

| V05 minimum | Independent assessment |
|---|---|
| BM25 contribution monotonicity | Covered by frozen 2ae064d strict-positive ordered pair, independently read raw FTS, and five original observations; no universal final-rank claim. |
| Soft preselection outside hit | Frozen public DSL cases plus new literal all-lane cases with outside hints/preselect1; legal hits remain reachable. |
| Some(empty) | New explicit files/languages empties have zero candidates in all six lanes and zero final hits; exact dense counterpart is frozen separately. |
| Language/path intersection | Literal caller files/path/language versus DSL conflicts and intersections; complete final fixture sets. |
| DSL | Frozen public path/lang repetitions/conflicts, new function/class/name/absent-name L2 cases, and new actual stdio search/context function/name cases. |
| Every lane obeys hard domain | All six raw lanes nonempty in the unrestricted positive, including a real parsed nonseed cross-file graph neighbor; literal constrained sets exclude sibling/outside paths. |
| Final hydrate obeys domain | Nonempty literal byte slices, current observed doc versions, complete final sets, actual product two-tool byte/line/public-span checks. |

The unchanged PR58 test passed five semantic and five semantic-http rounds: **10 tests / 110 case rows / 660 lane receipts / 1,480 raw candidates / 210 final hits**. The new receipt auditor never consumes author `allowed_literal`, `expected_final_literal`, HardScope predicates, path/scoring helpers or author gold. It independently specifies paths, full final chunk identities, literal source and byte-derived line boundaries. Five synthetic negative mutations are rejected: legal final omission, empty source, wrong line, wrong byte span and erased cross-file graph. These are oracle sensitivity controls, not mutated production executions.

This is L2 real worker/SQLite/recall/search plus separate L3 executable stdio. Authored SemanticRequest scope at the L2 seam is not newly called public dispatch proof; that conjunction uses the frozen public cases and the new narrow two-tool public function/name fixture. No kind/type-alias/all-language exhaustive assertion or Windows filesystem certification is implied. The minimum table has no mandatory number of repositories, all possible symbol kinds, 100k rows or C8/16.

| V16 minimum | Independent assessment |
|---|---|
| Hand cosine/top-k, tie, scope before top-k, delete, distinct spaces | Frozen 2edf2009af review: nonunit analytical positive/zero/negative gold, k0/1/2/5/99, actual observed rowid insertion orders, stronger outside negatives, real delete and model transition. Its seven source/oracle fingerprints exactly match this baseline. The frozen author 15 fixtures/90 cases are freshly raw-hash/scoring audited, not rerun or counted as new quality questions. |
| Bounded memory | Frozen ed978e69 read-allocation before/after, legal/oversized/growing-input tests; all four checked source fingerprints still match. Fresh unchanged exact-backend bounded-batch unit: 11 rows, dim4, batch4, k3, four keyset requests; 1/0/0, 226 deliberately filtered unrelated unit tests. Source inspection confirms one retained batch, one loaded vector and bounded top-k heap (transient k+1). |

These cover the declared exact-backend/cache-read minimum. The specific read buffer bound is distinct from JSON diagnostic allocation, C/SQLite allocation, whole-process RSS and continuous peak RSS. No ANN, quality, V20 C8/16/100k/release or cross-build metric/tokenizer certification. Parent may accept these explicitly bounded minimum rows without inventing additional scale. Full integrated current-run gate/CI decisions remain parent-owned; prior checkpoints retain their original source attribution.

## Block B: formal V18 lifecycle map

`gate-review-map.json` contains 13 named lifecycle rows with expected semantic/dense states, exact per-stage document/query POST deltas, raw paths, and closed/not-run boundaries. `validate_results.py` checks every row against the independently recorded real stdio and HTTP files, and verifies all canonical artifact hashes. Three complete executions pass: **39 state checkpoints**, plus **6 actual public function/name/source responses**. None is a semantic-quality query count.

| Stage per lifecycle execution | Semantic / dense | Document POSTs | Query POSTs |
|---|---|---:|---:|
| Fresh unconfigured / explicitly disabled | not_configured / disabled | 0 | 0 |
| Fresh authority absent | port_attached_unverified / disabled | 0 | 0 |
| Actual lazy provider assembly failure (absent synthetic key reference) | failed / disabled | 0 | 0 |
| Held document HTTP | backfilling / partial, worker pin1 | 1 | 0 |
| Released publication, search/context cold then warm | ready / ready, one document | 1 | 2 |
| Clean process restart of same project/cache; no-op index | ready / ready | 0 | 2 |
| Same-project query opt-in revoke | ready / ready, authority false | 0 | 0 |
| Same-project old network opt-in revoke | ready / ready, authority false | 0 | 0 |
| Same-project disable after ready | not_configured / disabled | 0 | 0 |
| HTTP500 attempts1/2 | backfilling / partial | 1 / 2 | 0 |
| Actual third HTTP500, durable retry exhausted | failed / partial, failed1/published0 | 3 | 0 |

Deltas on a multi-stage lifecycle are relative to the start of that stage family; table values are not summed as disjoint charges. Actual total per execution is **4 document POSTs +4 query POSTs**, one successful document publication, three unsuccessful document attempts, four successful query encodings. Across three executions: 12+12 POSTs. Query cache is bounded **in-process**: restart legitimately encodes each distinct tool marker once again; repeats within the process add zero POSTs. Durable document artifacts/manifest survive clean restart and add no POST. Revoking network authority does not erase already published document coverage; `ready` coverage coexists with `network_authorized=false`. Fresh cold authority absence is therefore a different state.

No manual SQL queue changes, clocks, provider injection or runtime-state forcing. Terminal HTTP failure follows the unchanged three-attempt ceiling and actual 30s backoff, with local source still accessible. Nonlexical fresh semantic queries after revoke correctly return an unavailable lane with zero candidates and empty hits/spans, rather than inventing local matches. Queries containing `needle` verify genuine local source fallback. Both search/context explicit semantic requests when unconfigured/disabled return actual RPC **-32603**, `semantic recall is not configured`; invalid input's -32602 sanitizer contract remains in the independently hash-audited frozen parameter suite.

Synthetic HTTP responses report one token per input for test bookkeeping. Those values are **not real provider usage or currency cost**. Failed HTTP500 attempts have unknown potential real billable usage, not a zero-cost claim. Every attempted POST is retained. Status reads and warmed/revoked requests are checked against actual transport counts. Headers are validated for the public dummy authorization marker; real credentials and authorization header contents are not emitted.

Existing 027fc86d public evidence is locked to actual c8c20b5 source: full 14-tool/schema/default/legacy arrays in three profiles, feature/authority/bypass gates, actual query positive/cache, headers/body deadline and MCP cancel. The retained 1,573 raw records and associated log hashes pass a fresh audit. Existing 2ae064d's 89 raw records also pass; its original V11 failure remains retained. Old lifecycle summary 2b8eb20 lists temporary raw logs without complete raw stdio/HTTP payloads in this checkout: it remains **historical**, not sole lifecycle proof. The new actual stdio supplement supplies complete raw proof at 83a6b54.

## Preliminary mistakes and retained evidence

Scaffolding failures are kept separately and excluded from canonical totals:
- Initial harness assumed unavailable semantic requests were parameter -32602 errors; actual contract is -32603 SemanticUnavailable.
- Initial receipt auditor assumed one chunk per file and chunk suffix0; the parsed Python import/function correctly produce two chunks.
- Initial query trace assertion used an incorrect field name; actual published score bill is `score_trace`.
- Initial revoked nonlexical query assumed a local match. Exact empty output is the correct result; a separate lexical query proves local availability.
- A proposed line-coordinate counterexample was **retracted**. Byte34 is the newline at the end of source line1, so span[34,77) correctly has line boundaries1–4. Requiring this partial byte slice to equal the full line slice was an invalid new oracle premise. Preliminary raw interpretation remains, while canonical byte slicing, byte-derived line boundaries, and both public spans pass. No product defect was found.
- Draft formal state labels were corrected against actual receipts: assembly failure dense is disabled; warm old-authority revoke retains ready document coverage. Final expected map is validated for all three executions.

No existing assertion was altered, deleted, ignored or given a larger timeout. Only the new independent harness/oracle assumptions were corrected before its canonical source freeze.

## Reproduction and limits

Set CARGO_HOME=/workspace/.cargo, RUSTUP_HOME=/workspace/.rustup, PATH=/workspace/.cargo/bin:$PATH, CARGO_BUILD_JOBS=5, CARGO_INCREMENTAL=0. From repository root, `python3 artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/replay.py` executes the bounded 16-command matrix at the fixed source; set P7_GATE_REVIEW_OUTPUT to a new absolute directory (which must not exist) because committed runs are immutable; the final reproducer adds only this output-directory option, while canonical execution remains attributed to runner2a4e4e7. `audit_frozen.py` and `scripts/review/p7_v16_receipt_independent_audit.py` audit retained evidence. `validate_results.py` validates the formal mapping and all new receipt hashes. The exact batch command is in its receipt; strict cc-server all-lane clippy passes with semantic-http/-D warnings. Python sources parse successfully and git diff checks pass.

**Not run:** P7-016 SIGKILL/crash-point matrix, interrupted process restart, hot reload, arbitrary concurrent configuration changes, live/paid providers or source egress, whole workspace/current release/CI, V19 semantic quality or V20 performance. Full P7-014 acceptance remains separate. D1/D2 unchanged. No merge, force push or deployment.
