# Reviewed public DEV dataset

The dataset index is [public-dev-20261008.dataset-index.json](../../manifests/public-dev-20261008.dataset-index.json).
It records exact query, suite and source-manifest SHA256 values and byte-bound evidence.

| Repository | Native rows | Compatibility rows | Source files | Gold alternative spans |
| --- | ---: | ---: | ---: | ---: |
| express | 70 | 59 | 7 | 110 |
| requests | 91 | 83 | 20 | 141 |
| gin | 67 | 55 | 53 | 127 |
| typescript | 73 | 59 | 20 | 98 |
| serde | 14 | 14 | 15 | 18 |
| vite | 12 | 12 | 17 | 15 |

There are 327 native questions, 282 compatibility projections and 509 alternative spans across 132 selected files.
The 45 no-answer questions remain in native evaluation. The 305 component groups describe known correlation;
they are not a claim of statistical independence. Six real repositories cover Rust, Python, Go, JavaScript,
TypeScript and a JavaScript/TypeScript monorepo. The index keeps code-language labels, metadata-format labels
and English query language visible rather than treating translated questions as new independent samples.

The original 301 questions retain their fixed historical source/gold evidence. The 26 new Serde/Vite questions
have separate nonauthor reviews. Author inputs and canonical-only intermediates remain preserved; these
reviewed copies only apply the independently approved global-family mapping and explicit review metadata.
No question or gold was changed based on retrieval ranking.

From the repository root, validate a frozen native suite using the evaluator:
    
    cargo run --locked -p cc-eval --bin cc-eval -- validate --suite crates/cc-eval/benchmarks/public-v19/serde/reviewed-dev-20261008/suite.native.dev.json

Use the corresponding suite.compat.dev.json for compatibility validation. The index lists all twelve suites.
Actual admission evidence used the exact evaluator SHA recorded in the index, sixteen original V02 validations,
twelve new V02 validations and the unchanged V19 schema checker. A current local build is not that archived
binary; its new validation result must retain its own source/build identity. These validation commands do not
run retrieval or establish ranking quality.

All questions are public DEV and English. The source folders are selected byte-locked subsets, not full
upstream checkouts. Private data, protected held-out questions, Chinese/paraphrase coverage, zero-overlap
quality, graph retrieval quality and release qualification remain outside this certification. The original
approximately 600-question planning target remains a transparent coverage gap. Six Serde and eight Vite
reserved slots remain undrafted and unread.

Native required facets and spans differ from compatibility any-expected-file matching. A JS-to-TS question
records explicit source edges, while 58 inherited optional graph-constraint rows are retained without a graph
precision/recall claim. Metadata and source indexing capabilities need explicit runtime validation. See the
dataset index's gaps and source_gold_scope fields for the exact limits.
