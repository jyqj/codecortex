# Native identity / adapter diagnostic (public DEV only)

Data integrity passes the checks below. Semantic retrieval quality remains failed and incomplete. No adapter mapping defect was found in these raw outputs; no production or scoring patch is justified inside this ownership. The PR adds independent miniature-source regression tests and aggregate evidence only.

## Locked inputs and scope

- Product: PR #131 `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` (the numeric PR prefix is not part of the SHA).
- Admission: PR #91 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`.
- New JS evidence: `535ff1b13b841af8021346a660c83c57919525e2`.
- New Python/Go evidence: `fff0931e5b470aa3b9111d6ce55444f9b40f98cf`.
- Read `.agents` and repository instruction/skill discovery: environment `.agents` empty; no repository AGENTS.md or SKILL.md at product base. Contracts inspected: `docs/internals/SOURCE_CHUNKS.md`, public model `search.rs`, normalizer/scorer/source verifier, and frozen admission protocol PREREGISTRATION.md metric definitions. No rejected/private diagnosis inputs used.

Root-safe evidence consists exclusively of counts, hashes, code references and newly authored source/gold examples. No admission query, answer identity, span coordinates, source body, or raw hit body is copied into this directory.

## Data integrity conclusion

`audit.py` verified 1,132 JS archive members and 965 Python/Go members against their SHA256/size manifests, including retained JS pilot evidence. Pilot measurements are excluded from the full-run denominator. All 16 full suites have exactly three unique repetitions per frozen query: **1,671 scheduled, 1,671 executed, zero missing, all Partial**. This is 903 native and 768 compat rows (301/256 queries). Frozen-query JSON values equal persisted evaluator-query values in all suites; their byte hashes differ because evaluator serialization changes formatting. Both hash sets are retained rather than asserting byte equality.

The 73 Python/Go source files were checked against admission SHA256 locks. All Requests 141 and Gin 127 alternatives have bounded, UTF-8-boundary-valid source byte spans containing their named symbol. This establishes coordinate and name presence, not independent certification of every gold role, kind convention, facet or chain. The original group report records 2,616 returned Python/Go hits, zero invalid and zero unverified source proofs. This audit independently compares the actual wire identity fields to every persisted native hit; no identity field was dropped by the adapter. It does not independently recompute all BLAKE3 source proofs or original binary identities.

## Semantic correctness conclusion

The native scorer requires exact path, every supplied symbol field, positive byte overlap, and no explicit invalid evidence (`crates/cc-eval/src/benchmark/metrics.rs:78`). Public top-level/metadata qname is copied as-is (`normalizer.rs:135`); breadcrumb, title, source text and owner spans are not substitute qualified identities. SearchHit has symbol name and kind but no dedicated qname field (`crates/cc-model/src/search.rs:7`); real hydration populates source/document metadata but no qualified identity (`crates/cc-search/src/plan.rs:513`). All inspected Requests/Gin wire hits lack both top-level and metadata qname. Thus an adapter cannot recover the required identity by mapping an existing public field.

| Predicate diagnostic, answerable rows only, any answer group | Requests | Gin |
| --- | ---: | ---: |
| Answerable rows | 249 | 165 |
| A retained hit has an answer path | 213 | 153 |
| Also correct name, valid evidence, positive span overlap | 159 | 111 |
| Also exact kind | 63 | 24 |
| Also every supplied qname (all native predicates) | 0 | 24 |

These are existential predicate counts, **not relaxed scores, simulated improvements, primary Top1 counts or candidate acceptance claims**. They distinguish actual absent/wrong retained evidence from identity mismatch. Requests has 36 answerable rows with no gold-path hit at all, and additional wrong-symbol/span evidence. Qualified-identity absence is nonetheless sufficient to force its complete zero: all 141 alternatives require qname; all 600 returned hits lack it; all 570 same-path hit/alternative pairs fail qname. Of those pairs, 63 fail only qname and 108 fail qname plus kind. Gin has no qname requirement; its 702 same-path pairs include 24 full matches, 108 kind-only failures, and many wrong-symbol/span pairs. Its low score therefore has both retained-evidence and exact-kind causes.

The kind incompatibility is source-backed: Requests has 85 alternatives labelled `function` whose named declarations are Python class methods; Gin has 81 alternatives labelled `function` covering Go receiver methods. Current parsers classify these as `method` (`crates/cc-parsers/src/python/mod.rs:107`, `crates/cc-parsers/src/go.rs:262`). This is an incompatible annotation/public taxonomy, not an established need to alias kinds in the scorer. Source-gold reviewers must decide the intended convention separately. No gold or parser changes are made here.

Compat checks only file paths, removes duplicate paths before ranking, and accepts supporting paths at Top1 (`metrics.rs:38`). Native ranks hits without path deduplication and greedily credits each group once; only primary groups satisfy Top1 (`metrics.rs:113`). Consequently compat ~0.77/0.80 and native 0/~0.07 can coexist on paired query projections without a scorer failure. They do not demonstrate native semantic correctness.

Partial answerable rows still receive their ordinary native score (`metrics.rs:172`); Partial is therefore not the Requests-zero mechanism. It separately blocks a complete-quality gate (`gate.rs:38`). Partial empty no-answer is incorrect; all 60 Python/Go no-answer repetitions remain failures. Statuses and original failures are retained.

Required groups implement group recall. Native-v1 exposes no separate required-facet metric, graph/ordered-chain correctness, Recall20, SymbolAccuracy or DuplicationRate; preregistration explicitly declares these unsupported. Facet/graph annotations do not make a hit certify an ordered chain. Span coverage unions all alternatives per file, including substitutes (`span_metrics.rs:17`); it is not alternative-conditional evidence coverage. These are frozen limitations, not newly implemented metrics.

## Self-authored verification and ownership boundary

`crates/cc-eval/tests/native_adapter_diagnostic.rs` creates miniature Python files and independent handgold. Five tests cover each strict identity negative, valid positive, supporting/duplicate/multiple groups, cross-file byte coverage, absent versus explicit qname mapping, Partial positive/negative rules, and a real index/search dispatch. The real micro class method returns `method`, verified source, and absent qname; it scores zero against the hand-authored qualified identity. A separate handgold control supplies that exact identity and scores one. This does not alter any corpus hit or propose reconstructing production qname.

Final checks: **26 passed** (21 existing benchmark_scoring + 5 new diagnostic tests), direct Rust 1.95, original Cargo.lock, default features, owned target/cache, no live providers. Initial handgold test failed because its manually entered end offset was one byte short; the self-authored test was corrected to the independently counted 26-byte text, with the first failure log preserved. No production behavior changed.

No corrected-data run is produced: there is no demonstrated public-field adapter mapping error to correct. Any public qname projection, source-gold kind correction, or new facet/chain scorer requires the respective owner/reviewer and a separately versioned proposal. This report does not authorize those changes, change budgets, relabel Partial, or turn the existing runs green.

## Reproduce

Extract the two original archives separately under `INPUTS/js/raw` and `INPUTS/pygo/raw`; extract their checkpoint directories under `INPUTS/js` and `INPUTS/pygo`. Extract admission protocol under `INPUTS/admission`. Materialize only the Requests/Gin source entries in admission `inputs_sha256` from their exact Git blobs to `INPUTS/sources/REPO/PATH`. Keep all original artifacts untouched. The script verifies these source bytes and frozen suite/query blobs again. Required Git objects are the input commits above and the explicit public source commits referenced by admission.

```
python artifacts/checkpoints/native-scorer-diagnostic-20261003/audit.py \
  --inputs INPUTS --repository CHECKOUT --output NEW_AGGREGATE.json
cargo test --locked --offline -p cc-eval \
  --test native_adapter_diagnostic --test benchmark_scoring
```

`aggregate.json` contains no query/gold bodies. `validation-receipt.json` records compiler/lock/aggregate/test hashes. No central TODO, ledger, old raw, scorer, normalizer, cc-search or shared worker file is modified.
