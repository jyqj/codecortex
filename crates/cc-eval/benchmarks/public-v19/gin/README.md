# Gin V19 candidate author shard

Base: PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`. Public source: `gin-gonic/gin@43fe48e8a0f44af783116cdb010725e6bb50255f`. Protocol: PR65 `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6`. No HEAD tracking, ranking inspection, production changes or provider runs.

**100 source-read author drafts; 0 independently reviewed/accepted families.** The six author target buckets have 20 exact/API, 20 behavior, 15 architecture/facets, 20 cross-file chains, 10 config/error and 15 bounded no-answer. These distinct factual intents are candidate independence claims pending independent/global review. A discarded protobuf positive/negative duplicate was replaced before its block commit. Translations/paraphrases are not counted. Source/task-equivalent global components still require designated reviewer adjudication; shared boilerplate or broad topics alone do not establish equivalence.

## Protocol migration and custody blocker

The original opaque-ID migration uses original author declaration order, not hash results: `v19.gin.f0001` through `f0100`, one `en01` variant each. All extra metadata is under `annotations.v19`. Categories map to actual protocol/evaluator names. Native groups use the first group as primary grade3 and other required facets as grade2; factual answers, source evidence, spans and chains are unchanged. This preregistration alignment is recorded with original gold/query and per-record SHA256 commitments in `provenance/id-migration.json` and `gold-fact-commitments.json`.

The fixed protocol hash rule yields **67 dev / 33 intended holdout**, without quota adjustment. Current public files contain 67 native dev rows and 55 compat dev rows (12 dev no-answer omitted from compat). Every native required facet references its own answer group; each graph edge includes pinned source-node evidence. Proposed global components are singleton pending designated equivalence review; later conflicting associations must quarantine components rather than silently move seen dev into holdout.

**Holdout custody is blocked; eligible confirmatory holdout count is 0.** Pre-protocol commits published 80 draft question/gold bodies and body-bearing author scripts, including old provisional holdout records. All 100 drafts were authored in a shared cloud workspace, which provides no independent custody boundary. Therefore the 33 intended holdout records are explicitly quarantined for confirmatory holdout and only their counts/exact-byte SHA256 commitments are published in the current tree. An untracked cloud `/tmp` preservation copy retains originals and quarantine bodies; it is not restricted storage, reviewed holdout or access control. The hashes are commitments to preservation bytes, not custody certification. Authors who saw these drafts cannot tune/evaluate their held-out arm.

Body-bearing old query/script files were removed from the current tree, with original commits/hashes preserved. **Public Git history remains exposed; deletion does not restore secrecy.** No history rewrite, force push or relabeling to claim an untouched holdout occurred. `corpus-receipt.json` reports contamination, quarantine, draft/reviewed/accepted counts and custody status. Independent custodian/global reviewer must adjudicate replacements/new version before accepting any holdout; cannot fix exposure by declaring the same bodies private.

## Source and license

53 explicitly MIT-noticed implementation/helper Go files are copied byte-for-byte. `source-manifest.json` and `provenance/inventory.json` retain every tracked upstream file's bytes/SHA256/inclusion reason. The 1099-byte root LICENSE matches locked SHA256 `b104efb2c7700691650f27034e8541c5ae0ed9af54d884287f19a7467ca2fe7f`.

BSD-noticed `tree.go`, `path.go` and their tests are excluded pending separate notices; other test files, generated protobuf, certificate/template fixtures, dependencies, docs/build/CI assets and binaries are excluded. Files without an explicit Gin MIT header (including internal/fs/fs.go and binding/plain.go) are conservatively excluded. No vendor/dependency implementation is bundled. Source boundaries were set before gold and never changed based on ranking.

Committed suites use clearly labelled snapshot `commit:null`, because admitted copied source has no Git metadata. Original upstream SHA is independently locked in provenance and every evidence span. `verify.py` checks every admitted byte against a clean checkout at the exact SHA. `verify_git_suite.py` also runs actual evaluator validation on ephemeral suites using the real clean Git checkout with mandatory source.commit. Thus snapshots are not presented as passing Git cleanliness checks themselves. Both native/compat public dev suites passed snapshot and actual Git-source validation with identical admitted locks.

## Reproduction and limits

From repository root, using a clean public Gin checkout at the locked SHA and an actual cc-eval build:

```sh
cargo build -p cc-eval --bin cc-eval --locked --target-dir /tmp/gin-eval-target
python3 crates/cc-eval/benchmarks/public-v19/gin/scripts/verify.py /tmp/gin-public
python3 crates/cc-eval/benchmarks/public-v19/gin/scripts/verify_git_suite.py /tmp/gin-public /tmp/gin-eval-target/debug/cc-eval
/tmp/gin-eval-target/debug/cc-eval validate --suite crates/cc-eval/benchmarks/public-v19/gin/suite-native-dev.json --suite crates/cc-eval/benchmarks/public-v19/gin/suite-compat-dev.json
```

Read the checker and schema from protocol fixed commit, without changing protocol/shared registry. Run `check.py --shard gin:native=<queries.native.dev.jsonl> --shard gin:compat=<queries.compat.dev.jsonl> --evaluator <binary> --suite <suite-native-dev.json> --suite <suite-compat-dev.json> --output <counts-only-receipt>`. No `--custodian` flag was used here. Checker returned zero errors and `format_checks_passed_not_source_gold_review`. Exact protocol file hashes and separate SHA256/BLAKE3 input locks are in corpus-receipt. `freeze` was an explicit authoring/versioning operation, not global split freeze; `validate` never rewrites locks. Settings now match protocol top_k10, timeout30000ms, repetitions3, warmup0, seed20261003. No run was performed.

Full draft source evidence: 217 line/UTF-8 byte spans and 71 directed call/data/dispatch edges, all bound to SHA/path/symbol and span hash. Byte spans are zero-based half-open [start,end). No-answer asserts only the specifically recorded whole-file scope and hashes, with near-miss evidence and author full-file inspection; it never infers absence from retrieval rank. Public native gold is outside indexed source; compat keeps deduplicated explicit paths in frozen primary-first group order. It makes no facet/span/chain claim. Native facet coverage and graph correctness metrics are not implemented; contract/source checks do not certify these metrics or retrieval quality.

## Delivery and review

Original 20/40/60/80 author blocks were committed/pushed with counts-only validation receipts. They were candidate author blocks, not independently reviewed complete intake blocks. The final block adds the remaining 20 drafts and protocol migration. New current corpus does not replace missing historical 306 raw requests and does not establish a live V19 quality pass.

Review status stays unsigned/pending. Proposed PR60 reviewer rotation assigns F / Vite author to Gin E, subject to integrator assignment. Reviewers must source-check literal facts, required versus alternative facets, exact spans, dispatch/data edges, absence scopes, semantic independence, global association and contamination/custody. Main integrator alone owns registry/authority gates. Only counts/hash/status should be sent to the tuning owner.

PR60 `gh pr view 60` returned Forbidden. That GraphQL operation and PR creation remain paused by instruction; no alternate channel, credential or permission change was used. Authorized successful git source reads/commits/pushes continued. No PR, merge, deployment or force push is claimed.
