# Gin V19 candidate author shard

Independent author namespace based on PR60 commit `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`. Public upstream is locked to `gin-gonic/gin@43fe48e8a0f44af783116cdb010725e6bb50255f`; never follow HEAD. This is a new candidate corpus, not recovered historical 306 raw requests or a live quality result.

The first complete block contains 20 distinct source-read families: 4 exact/API, 4 behavior, 3 architecture/facets, 4 cross-file chains, 2 configuration/error and 3 bounded no-answer. The deterministic family hash rule yields 15 dev / 5 holdout in this block. `evidence/summary.json` is the current aggregate. Translations and paraphrases are not separate families. Related facts must be checked by the independent reviewer and integrator for global duplication before admission. These are **candidate** splits, not a globally frozen split.

## Source and license

53 MIT-noticed implementation Go files are copied byte-for-byte and locked by per-file SHA256 in `provenance/inventory.json`. The 1099-byte LICENSE matches PR60's `b104efb2c7700691650f27034e8541c5ae0ed9af54d884287f19a7467ca2fe7f`. The source inventory records every tracked upstream file's inclusion/exclusion. Third-party BSD-noticed `tree.go`, `path.go` and tests are excluded pending separate notice review; all tests, generated protobuf, certificates, dependency manifests, docs/assets and binaries are excluded. There is no vendor directory imported and no dependency implementation bundled. Admitted files have a Gin MIT header and no generated-code marker.

The committed evaluator suites use a snapshot `commit: null`: the byte-identical curated source directory has no Git metadata. This is the evaluator's explicit snapshot representation, **not** a claim of unknown upstream provenance. The immutable upstream SHA is in provenance and every evidence record; `verify.py` checks every admitted byte against a clean checkout at that SHA. A reviewer can also validate an ephemeral suite with `source.root` set to the clean upstream checkout and `source.commit` set to the upstream SHA; the admitted file list and digests are identical.

## Contract and reproduction

`questions/{dev,holdout}.jsonl` follow actual `benchmark/schema.rs` Query contract. Gold is outside admitted source. Native answer groups require each listed facet (alternatives currently contain one precise function); literal answers, line/byte spans (0-based start, exclusive end), source SHA, evidence SHA256 and chain edges are retained in annotations and `gold/evidence.json`. No-answer rows have empty native answers and expected_files, with explicit file scope, whole-file hash, author source inspection and near-miss evidence. No low-rank inference was used.

From repository root, with public Gin checkout at the locked SHA:

```sh
python3 crates/cc-eval/benchmarks/public-v19/gin/scripts/verify.py /tmp/gin-public
cargo build -p cc-eval --bin cc-eval --locked --target-dir /tmp/gin-eval-target
/tmp/gin-eval-target/debug/cc-eval validate --suite crates/cc-eval/benchmarks/public-v19/gin/suite-dev.json --suite crates/cc-eval/benchmarks/public-v19/gin/suite-holdout.json
```

`author.py 20` regenerates first-block candidate inputs from the source-read declarations; explicit `cc-eval freeze` then restores evaluator BLAKE3 locks. `freeze` is only input authoring, not global split freezing. `verify.py` and `validate` do not retrieve, index, run providers or tune. No production/scorer/shared registry files are changed. `top_k` and suite settings are contract placeholders, not preregistered final evaluation parameters. Native graph-chain scoring is not implemented; annotations preserve chain evidence for independent review. Facet/span support is represented and input-validated, not certified for retrieval quality.

## Review and remaining work

Author has not signed independent review. Proposed audit rotation assigns Vite author F to review Gin E. `review/README.md` records required checks and stays unsigned. Independent reviewer must read literal answers, verify bounded absence and chain call/dispatch edges, reject duplicates/ambiguous facets and determine admission. Holdout question/gold bodies must not be sent to production tuning owners; report only counts, hashes, coverage and issues. Main integrator owns global registry, cross-repository deduplication, final splits and authority gates. Current candidate total: 80 families (59 dev / 21 holdout), with 16 API, 16 behavior, 12 facets, 16 chains, 8 config/error and 12 no-answer. Remaining target: 20 families and independent review of all candidates. PR60 metadata read via gh was denied (Forbidden); that call and PR creation remain paused, as directed. Authorized git commits/pushes continue without changing credentials or permissions.
