# TypeScript V19 candidate block 01

New public-corpus preparation, based on PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`. Upstream is **only** `microsoft/TypeScript@ed4807212c28c90777c1d7ef2bf8e47af5d08519`. The locked tree has `packages/typescript/src`, not the historical `src/compiler` layout. No upstream HEAD is followed.

20 distinct candidate obligations: 4 exact/API, 4 behavior, 3 architecture/facets, 4 cross-file chains, 2 configuration/error, 3 scoped no-answer. This is a representative first block; 80 remain. No claim of source exhaustion. All are **candidate_pending_independent_review**. The author does not certify independent review; review-request.json lists checks for another author.

15 dev / 5 holdout. Family IDs use semantic slugs; SHA256 IDs and related-obligation cluster hashes determine splits with the fixed recorded salt. The salt was selected solely to produce 75/25 candidate counts before any retrieval inspection. Related filesystem, diagnostic, encoding and client-option obligations stay together. Any later block must preserve existing assignments and this salt; do not reroll it. This is not a frozen global split. Integrator owns cross-repository deduplication and final split approval. Send tuning owners counts, hashes and coverage only; do not send holdout question or gold text.

## Source and license admission

Only the seven explicitly listed source files are admitted; they remain byte-identical to the pinned public tar archive. No binary, dependency, full repository or private source is included. Vendor, generated output (including generated enum modules), bundled declaration libraries, test material and all unlisted files are excluded. These files provide source evidence, not a standalone buildable TypeScript package; excluded generated types and dependencies are not silently included.

LICENSE.txt exactly matches PR60's 9197-byte Apache-2.0 lock `a7d00bfd54525bc694b6e32f64c7ebcf5e6b7ae3657be5cc12767bce74654a47`. Full upstream NOTICE.txt is retained. The admitted path module references vscode-uri source at `edfdccd976efaf4bb8fdeca87e97c47257721729`; the corresponding Microsoft MIT LICENSE.md is retained too. provenance.json locks each license artifact. All other notices in upstream NOTICE are retained for attribution; their libraries are not admitted merely because a notice is present. Independent license scope review remains pending.

## Contract and verification

suite.candidate.json is a shard-local native evaluator suite, not a shared registry entry. Its source commit is null because the imported, byte-locked partial snapshot is not an upstream Git checkout. Every gold annotation separately binds the real upstream SHA and SHA256 file/span hashes. Native source/query digest fields use **BLAKE3**, as required by manifest.rs; these are not SHA256.

Each positive answer facet has exact UTF-8 byte offsets [start,end), symbol and 1-based inclusive lines. Multiple required facets use distinct primary groups; equivalent alternatives would share one group. Cross-file calls have token spans and from/to facet indices. Native validation and scoring do not certify graph-chain semantics; retained annotations require reviewer examination. Three no-answer cases are limited to complete named files with full-range source-read rationale and absent-token checks. Token absence is corroboration, not a substitute for the full source review.

Reproduce from repository root:

```sh
SHARD=crates/cc-eval/benchmarks/public-v19/typescript
python3 -m pip install --target "$SHARD/.validation-deps" -r "$SHARD/requirements-validation.txt"
curl -fL https://codeload.github.com/microsoft/TypeScript/tar.gz/ed4807212c28c90777c1d7ef2bf8e47af5d08519 -o /tmp/typescript-v19.tar.gz
python3 "$SHARD/verify.py" /tmp/typescript-v19.tar.gz
cargo build -p cc-eval --bin cc-eval --locked --message-format=json
# Use Cargo's reported compiler-artifact executable if the target directory differs:
target/debug/cc-eval validate --suite "$SHARD/suite.candidate.json"
```

verify.py reads saved gold without regenerating it, verifies schema/locks/spans/edges/splits, and compares all admitted source plus root license/notice to the pinned archive. author.py records the source-derived obligations and can regenerate this first block; this is explicit authoring, not part of validation. The initial native offline build lacked reqwest; the subsequent --locked build obtained public crates.io dependencies. build.jsonl, build.stderr, native-validate.log and validation.json retain the evidence and executable SHA256. No retrieval/ranking/provider run occurred.

This block is new corpus preparation. It does not recover historical 306 raw rows, certify live quality, approve a global split, or close the 100-family target.
