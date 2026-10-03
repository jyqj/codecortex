# Independent first20 TypeScript public-dev review

Author input is frozen at `4c01f627bd6df73e70bfe1dbdc42b2a9234e797a`, upstream at `ed4807212c28c90777c1d7ef2bf8e47af5d08519`, and protocol at `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6`. This reviewer is separate from `typescript-independent-author-cloud`.

Repository and language admission were checked against official source, rather than inferred from the historical compiler layout. The official Microsoft/TypeScript Git commit endpoint confirms the exact SHA and tree `82e7df2ae99d151725fc2331a37887109b11c8b7`. All 20 admitted files under `packages/typescript/src` match the pinned official raw files byte for byte. They are actual TypeScript API/AST source; the primary language of the entire repository is not asserted. All three license artifacts match their fixed official origins: root Apache-2.0, complete upstream NOTICE, and separately retained vscode-uri MIT license. Generated, vendor, test, dependency and unlisted files remain excluded. Source/license admission for the declared 20-file scope is accepted.

The first20 review sequence is the 15 current `01-migration` public-dev rows followed by the first five current `02` public-dev rows. Results are 20 accept, zero reject, zero needschange: 17 positive and three bounded no-answer cases. Facts, exact named symbols and spans, primary/required-secondary facets, actual import-bound call targets and branch semantics were manually checked against source. The three absence scopes were read completely; token absence is supplementary. Twenty distinct core obligations are locally accepted within this first20 scope; shared files or broad topics alone do not imply equivalence.

The remaining 53 public-dev rows and global cross-repository component associations are unreviewed. No 100-component certification is made. All 27 custody-blocked holdout bodies remain unread; historical candidate bodies and author scripts were not read. Ranking runs, accepted confirmatory holdout cases and formal combined complete-20 blocks remain zero. This is a 20-dev review block, not a formal mixed dev/holdout completion.

`dev-020-review.json` records opaque row/family/component and evidence hashes plus decisions and counts; it contains no question or answer excerpts. Its SHA256 is `398e58dd12fac85ce62c43ba93815b26af74fef41714e261f85d2430e0f24e98`. Official-source proof is in `upstream-source-admission.json`, SHA256 `e7d952edcaba0e3d04779d4fa2342f0a478551becf7ed94a387530a082fc0679`.

Replay from the repository root:

```sh
python3 crates/cc-eval/benchmarks/public-v19/reviews/typescript/review_dev.py --check
```

Replay verifies SHA256, spans, structure, projections and frozen decisions. It does not prove semantic truth automatically or claim an independent native BLAKE3/evaluator run. Manual review goes beyond schema acceptance. Source access succeeded without permission rejection; no rejected channel or author PR creation was retried. The reviewer's independent draft PR contains only this namespace and supplies exact-SHA/count/hash evidence for the integration owner. No author shard, protocol, product, dependencies, CI or shared checklist was changed.
