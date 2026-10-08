# Vite public DEV source and gold corpus

This directory adds **149 manually authored public DEV questions** against
[`vitejs/vite@10033218d239c927cdc375970b5741cce408e81b`](https://github.com/vitejs/vite/tree/10033218d239c927cdc375970b5741cce408e81b).
All 149 final questions received a complete source-based review from a different
agent before their native content lock was written. The native and compatible
suites both passed the actual release `cc-eval validate` command.

This is an additive `public-dev-20261008` corpus. It does not change the historical
301 accepted DEV questions, historical v19 split assignments, or any holdout
body. The compatible representation below is a projection of the same 149
questions and contributes **zero additional questions**.

## Fixed inputs and review scope

| Item | Recorded value |
| --- | --- |
| Upstream commit | `10033218d239c927cdc375970b5741cce408e81b` |
| Upstream tree | `dfb60bc145174f00dc27cab056786bbbe582daf0` |
| Native questions | 149 |
| Compatible rows | 149, all answerable native IDs |
| Family labels | 138; related variants share 9 of these labels |
| Admitted source files | 148 |
| Admitted source bytes | 1,565,563 |
| Source tree paths accounted for | 2,841, through admission and exclusion records |
| Gold answer alternatives | 195 |
| Gold and hard-negative evidence spans | 344, including 281 distinct spans |
| Retained attribution observations | 11 |
| Author | `/root/build_validation` |
| Independent reviewer | `/root/p8_corpus_closeout` |

The source domain includes Vite production TypeScript, executable JavaScript,
build and package configuration, selected sibling-package production source, and
the shipped vanilla JavaScript/TypeScript starter files. It includes distractor
files across that declared domain. Tests, documentation, other templates,
declaration adapters, dependencies, binary assets, and the identified
vendor-derived pretty-format and source-map decoder implementations are excluded.
[source-manifest.json](source-manifest.json) records every admitted and excluded
Git path, each admitted file's Git blob and SHA-256, source acquisition, and
license provenance. The upstream checkout was clean, detached at the full
commit, and had no submodules. Upstream installation and build scripts were not
executed for corpus preparation.

Each native question has manually selected primary or supporting source spans,
required facets, and a separately identified hard negative. Spans use UTF-8 byte
offsets `[start, end)`; annotations also record source lines, file SHA-256, span
SHA-256, and the full upstream commit. Required primary groups have grade 3;
required secondary groups have grade 2; optional supporting groups have grade 1.
Question and gold bodies remain outside the indexed upstream checkout.

The [complete independent review](review/vite-full149-v2-independent-review.json)
contains a source-reading conclusion for every ID and binds the exact final
question and manifest bytes. The author packet retains its original `pending`
annotation to avoid changing accepted bytes; this separate non-author receipt
records final acceptance. The earlier
[20-question review](review/vite-first20-independent-review.json) and
[license correction review](review/vite-license-correction-independent-review.json)
are retained with their original scope.

The [correction receipt](review/correction-receipt.json) records nine questions
changed after full independent review: one gold span was extended to include the
complete native-invoke branch; five optional evidence groups were promoted to
required secondary groups because their facts are requested in the question;
and three phrasings were narrowed to match the inspected code. No ranking result
was consulted to make these corrections. All other rows, the first 20 exact
question bytes, family assignments, and admitted source files remained unchanged.

## Category and family accounting

| Category | Native questions |
| --- | ---: |
| `file_exact_match` | 2 |
| `configuration_lookup` | 20 |
| `semantic_feature` | 35 |
| `error_handling` | 31 |
| `component_location` | 6 |
| `api_usage` | 15 |
| `cross_language` | 5 |
| `symbol_location` | 5 |
| `architecture_understanding` | 19 |
| `call_chain` | 11 |
| **Total** | **149** |

The five cross-language questions inspect WASM/JavaScript glue, the JavaScript
executable's TypeScript build entry, HTML-to-JavaScript startup, JavaScript/CSS
styling, and TypeScript changes to HTML/package JSON. They establish evidence
from the inspected source, not execution of those language runtimes. The query
language labels are 141 TypeScript, 5 mixed, 2 JSON, and 1 HTML.

Families are transparent related-question labels, not certified independent
statistical observations. The repeated labels are `f0002`, `f0020`, `f0027`,
`f0035`, `f0065`, `f0086`, `f0089`, `f0096`, and `f0123`, within the
`p8dev20261008.vite` namespace. The two three-row families and seven two-row
families account for 11 related variants beyond the 138 labels. Complete ID
membership is recorded in
[author-identity-checks.json](review/author-identity-checks.json), and the
independent review preserves these relationships. Shared files across different
labels remain a possible source of correlation. A family label never adds to
the question denominator.

## License and source use

The pinned Vite repository declares MIT. Its actual source retains additional
attributions, including explicitly marked MIT material and an ISC section based
on node-graceful-fs. Other comments identify Svelte, Node.js, StackOverflow, and
Vue origins or references without a separate license declaration in the selected
comment. The manifest preserves those observations and immutable notice hashes;
it does not assign an unstated license to a fragment or claim all source is
exclusively first-party MIT. This corpus stores questions and provenance, and
does not redistribute the upstream source bodies. Use is local authoring,
independent review, and local evaluation under the previously authorized public
repository and fixed-commit scope.

## Recreate the checkout and validate

The suites expect a clean upstream checkout in `../upstream-vite`, alongside the
CodeCortex repository. From the CodeCortex repository root, create that fresh
sibling checkout with the fixed revision:

```sh
git init ../upstream-vite
git -C ../upstream-vite remote add origin https://github.com/vitejs/vite.git
git -C ../upstream-vite fetch --depth=1 origin 10033218d239c927cdc375970b5741cce408e81b
git -C ../upstream-vite checkout --detach FETCH_HEAD
git -C ../upstream-vite status --porcelain --untracked-files=all
```

Then use `cc-eval` to verify both existing locks:

```sh
cc-eval validate \
  --suite crates/cc-eval/benchmarks/public-dev-20261008/vite/native.dev.suite.json \
  --suite crates/cc-eval/benchmarks/public-dev-20261008/vite/compat.dev.suite.json
```

The [validation summary](validation/validation-summary.json), command receipts,
and raw logs record the actual successful execution. The session used release
`cc-eval` SHA-256
`554d1baefeadd367614a0755a94b2db5a6194b284a83c03b7d8fe172368dd33d`,
built from CodeCortex `fb772551cff6b4620a6fcdb94c57b78350cebb33` with the
798-source-input manifest identified in
[frozen-binaries.json](validation/frozen-binaries.json). Both suites use one
repetition, no warmup, seed 42, top-k 10, a 30,000 ms timeout, and explicitly
disabled auto-indexing and semantic/network/query egress options.

`freeze` was executed as an explicit authoring operation only after independent
acceptance. Normal reproduction uses `validate`; refreshing a lock would require
a separately reviewed corpus change. The successful validation kept source,
question files, and suite locks unchanged.

The compatible file clears native answer groups and derives `expected_files`
from their alternatives in stable primary-first order. Every identity field and
annotation is preserved. The unchanged repository functions `query_rows`,
`check_projection`, and `source_check` verified this projection and native source
spans with no errors; see
[projection-check.json](validation/projection-check.json).

| Content lock | BLAKE3 |
| --- | --- |
| Admitted source inventory | `37f77d1ffc690a75d80dc02d6c6298a195ed24e39da040f5fb52b77dec8a9977` |
| Native query bytes | `0e0c00d4eb4afa6356a988485c31cb2c3c38ab0c4a35a77530b6d892be721dce` |
| Compatible query bytes | `5f1674d0ad79ec9b5a2971558dfeee51ffc363328a80440255fb088a433ce325` |

Source admission, gold acceptance, and lock validation do not certify retrieval
quality, upstream runtime correctness, an ordered graph-path score, holdout
performance, or completion of an original P8 task. Those outcomes need their
own execution and acceptance evidence.
