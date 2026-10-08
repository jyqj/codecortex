# Serde public DEV corpus, 2026-10-08

This entry adds **150 reviewed public DEV questions** from a real, fixed Serde
checkout. It supplies the Rust coverage for the P8-002 corpus expansion. It uses
the existing Native query/suite schemas and evaluator; the original 301-question
corpus, historical preparation protocol and holdout partitions are unchanged.

The complete final question packet was reviewed question by question by a
different agent from its author. Both final suite entries were then explicitly
frozen and validated by the existing release `cc-eval`. The native validator
reported `150 queries, 60 admitted files, locks valid`; the compatible entry
reported `149 queries, 60 admitted files, locks valid`.

## Fixed source and admitted scope

| Item | Fixed value |
| --- | --- |
| Repository | <https://github.com/serde-rs/serde> |
| Commit | `6693a89cca77e0151437da1c7f890090b9ebf04c` |
| Git tree | `48fd8cbe29a82dc7683136b6dca6f9c75dad11b7` |
| Admitted files | 60 files, 863,738 bytes |
| Full tree accounting | 361 paths: 60 admitted, 301 excluded |
| Source manifest SHA-256 | `b7e45236c36f09c16a1787fe1dc259d3bc75523fda3a2f7d491334d5a6462f44` |
| Native source digest | `f0a3504d95c17ddcce0f3d0165a465ba9914fbd470daa77453ee1d134dadcb8f` |
| Source mode | Actual clean, detached Git checkout; no submodules |
| License | MIT OR Apache-2.0, with upstream license files retained here |

[source-manifest.json](source-manifest.json) records every admitted file's Git
blob, mode, size and SHA-256, every excluded path and its reason, and the
authorization and local usage record. Production Rust files from `serde`,
`serde_core` and `serde_derive` are admitted with their relevant package/build
configuration and repository overview. Test fixtures, CI, generated/dependency
outputs and the separate `serde_derive_internals` package are excluded from this
declared domain.

The upstream `serde/src/core` symlink is excluded; the canonical
`serde_core/src` files are indexed directly. Eight upstream package license
symlinks remain recorded as Git link-target bytes in the manifest. They are
metadata, not admitted index inputs. The two complete root license files are
preserved in [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
Question and gold files are outside the upstream checkout. Upstream source
bodies are read from that fixed checkout and are not copied into this corpus.

## Questions and independent review

The native entry contains 145 Rust-labelled and 5 TOML-labelled questions,
including one explicitly scoped no-answer question. Nine categories cover
symbol/file/component location, API use, configuration, error handling, semantic
behavior, architecture and source call chains. Mixed JavaScript/TypeScript
coverage is supplied by the separate Vite entry.

There are 118 transparent family labels for these 150 questions. Related views
share family labels where their intent/evidence overlaps. These labels do not
certify statistically independent observations, and variants are not counted as
additional families. The [original D2 target](../../../../../docs/roadmap/code-index-v2/09-BENCHMARK.md)
is approximately 600 reviewed
questions across at least six real repositories; this entry contributes 150
questions to that overall review, without inventing a 600-family requirement.

Every question records byte-bounded source gold, file/span hashes, facets,
hard-negative evidence and scope. All 367 gold and hard-negative spans were
independently checked against the actual fixed source. The independent reviewer
also accounted for the entire 361-path Git tree and inspected the relevant code
for every question. The one no-answer question is restricted to the serializer
subtree; an independently executed `rg` probe found no `IgnoredAny` or
`fn visit_map` there. That result is limited to the declared scope.

The acceptance authority is the complete, non-author receipt:

- [review/serde-full150-v2-independent-review.json](review/serde-full150-v2-independent-review.json): all 150 per-ID source/gold/facet/negative decisions and accepted v2 corrections; SHA-256 `2c4cd78aaf31dbe1b042a9100c81c915cad357548cbf7841e6676b6267104fb7`.
- [review/serde-full150-v2-mechanical-review.json](review/serde-full150-v2-mechanical-review.json): independent source, span, identity, admission and license-link checks.
- [review/serde-first20-independent-review.json](review/serde-first20-independent-review.json): retained earlier review of the first 20 questions.

The author is `/root/p8_corpus_closeout`; the non-author reviewer is
`/root/build_validation`. They worked in separate worktrees and read the fixed
public source. This is a public DEV author/reviewer separation, not a claim of
independent holdout custody.

The immutable author annotations still say `review_status: pending`. The
separate independent receipt accepts their exact final bytes. Changing that
author field after review would change the reviewed packet, so acceptance is
recorded externally. Four v2 edits narrow two query scopes and correct two facet
descriptions: forwarded `FlatMapDeserializer` methods, local
`deserialize_in_place_body` checks, optional identifier mode selection, and the
distinction between an empty tuple and unit syntax. Gold spans, grades, groups,
hard negatives and families are unchanged from the fully read v1 packet. The
receipt records all four exact before/after differences.

## Native and compatible entries

| Entry | Questions | Scoring | Query file SHA-256 |
| --- | ---: | --- | --- |
| [suite.native.dev.json](suite.native.dev.json) | 150 | `codecortex-native-v1` | `4d5ac21c0b42a385ef348da069a1bcd3c9ae547b3ebb3c77b673bd749455f7ec` |
| [suite.oce-compat.dev.json](suite.oce-compat.dev.json) | 149 | `oce-compat-v1` | `89f2e7786c2c806a3dfb1e49973c826f50c46ef51cf4a60090d0353432b91128` |

The query-file SHA-256 values above bind the reviewed raw JSONL bytes. The
evaluator's `queries_digest` fields are its own normalized query locks; they
are respectively `1d9c3e16f096badd5f007e099df94258c57d1417c98325534f07cf4b3ba8e390`
and `c43cb44bd0addaa3742ab2ac825339def8bff48439dca3948bfc583953e6b61f`.
Both lock types are retained and serve their existing purposes.

The compatible entry is a deterministic projection of the same 149 answerable
native questions. It retains IDs, query text, family, split, category, language,
difficulty, scope and annotations. `expected_files` contains deduplicated gold
alternative paths in stable primary-groups-first order; `answers` becomes empty.
The native no-answer question is omitted because this compatible file-ranking
profile does not represent it. Native facets remain as provenance in annotations
and are not scored by that compatible entry. This projection creates no new
questions and is not the canonical external OCE benchmark.

The unchanged original helpers `query_rows`, `source_check` and
`check_projection` from `scripts/p8_corpus_audit.py` accepted the inputs with zero
errors. [validation/projection-check.json](validation/projection-check.json)
records their source hash, signatures, exact digests and omitted ID. Both entries
use the same source inventory, three repetitions, no warmup, seed `20261008`,
30-second query timeout and top-k 10. Existing evaluator schema and scorer
behavior are unchanged, so the parent corpus registration can list these suite
paths alongside the previous reviewed entries.

## Reproduce the locked validation

From the CodeCortex repository root, a fresh source checkout can be placed at the
relative location expected by both committed suites:

```sh
mkdir -p ../session/upstream-serde
git -C ../session/upstream-serde init
git -C ../session/upstream-serde remote add origin https://github.com/serde-rs/serde.git
git -C ../session/upstream-serde fetch --depth 1 origin 6693a89cca77e0151437da1c7f890090b9ebf04c
git -C ../session/upstream-serde checkout --detach FETCH_HEAD
git -C ../session/upstream-serde status --short
cargo run --release -p cc-eval --bin cc-eval -- validate \
  --suite crates/cc-eval/benchmarks/public-dev-20261008/serde/suite.native.dev.json \
  --suite crates/cc-eval/benchmarks/public-dev-20261008/serde/suite.oce-compat.dev.json
```

An existing checkout at that path can be reused when its full commit, admitted
bytes and clean status match. Validation uses the committed locks. `freeze` is
an explicit authoring operation and is not needed to verify this published
entry.

[validation/freeze-validate-receipt.json](validation/freeze-validate-receipt.json)
binds four actual successful commands and their complete logs. They used the
fixed release evaluator built from CodeCortex
`fb772551cff6b4620a6fcdb94c57b78350cebb33`, binary SHA-256
`554d1baefeadd367614a0755a94b2db5a6194b284a83c03b7d8fe172368dd33d`.
The native and compatible freeze/validate commands all returned exit 0. These
short validation durations are input checks, not retrieval speed measurements.

The separately reviewed and frozen first-20 packet previously received a real
MCP input/protocol check by the parent agent. The author received only the
60/60 source readiness, no parse errors, 60 completed measurement records and
Partial usability result. Per-question scores and ranks were not disclosed to
the author and were not used for question selection or corrections. That first
20 run does not certify this final 150-question packet. The author receipt
preserves this history; the final product run and aggregate P8 acceptance require
their own parent integration evidence.

No provider service was called for these corpus checks, no holdout body was read
or authored, and source call-chain evidence does not certify an ordered graph
metric or held-out generalization. Overall six-repository admission and task
status are recorded by the parent corpus registration.
