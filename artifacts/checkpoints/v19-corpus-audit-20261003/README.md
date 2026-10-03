# V19 public corpus readiness audit

Audited PR58 base `83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad`. Scope is evidence recovery, read-only inventory and a future ownership plan. No production, gold, scoring, task ledger or authority gate changes. No live provider, private-source egress, merge or deployment. This is not a V19 quality pass.

## Missing historical freeze

`P5-GATE.json` points to `artifacts/benchmarks/p5e-g5-freeze-20261002/F0-FREEZE-RECEIPT.json` and `regression/validation.json`, binding target `0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6` and covered-source digest `d1f5a7af5ff9b0d025b6dd8dc475c110f178735d757d22e3546ba8470b45a167` (629 files). Those declarations are not raw byte verification.

Fetched all authorized remote branch heads; `remote-refs.txt` records observed remote heads/tags. The audit checks all reachable Git objects plus explicit historical target, its tree and path history. There are **zero objects for the freeze directory or F0-FREEZE-RECEIPT**. This proves absence within the inspected committed object/history scope, not absence from every former workstation or off-repository archive. No unapproved workstation directory was searched. Recovering the exact freeze requires its original retained archive from its owner, with F0 manifest and matching raw/binary/source hashes; summaries cannot regenerate it. A new run must have a new receipt and identity, never inherit the old freeze hash.

Direct omission evidence exists: committed `20261001-paused-github-sync/evidence-index.json` says `curated_complete_engineering_progress_not_full_raw_upload_not_G5`. Its `excluded_local` explicitly marks earlier original51 `raw` directories and normalized/cost/request files `local_not_uploaded`; `curate.py` skips raw directories. This checkpoint predates the missing 20261002 freeze, so it explains earlier raw omission, not proof of an identical upload operation for the later freeze.

All **102 retained original51 summary/manifest files** match their recorded upload SHA256. Both v1 and v3 summaries contain **51 questions / 153 measured rows** (three repetitions). Their observed engine HEAD `0a56a257f9a92c54d06ea5be0ce1d1763917a527`, source digest `4035cd30f4953d72b8e794153057b173b83f6bcfa00f64beea0f065f582f6805` and binary digest `60021653085a87f6b5cfe3ca0d31d9e607770b57b5c7fc93ce625d6a73e82677` differ from the declared final freeze; dirty-diff bindings are retained in audit.json. Per-suite normalized digests are claims whose underlying bytes were excluded. These older 153-row summaries cannot verify the required later 306 requests or their two-arm source mapping. S11's three no-answer failures remain visible in the older smoke gate; no failure is deleted or promoted to a pass.

## Current committed corpus

The eight manifests contain **56 dev rows, zero holdout**, and 29 distinct family strings; family strings are not a certified independent sample count. Four familiar suites have 11+8+18+14=51 rows. That arithmetic is not a recovered final raw run.

| Domain | Rows | Provenance / independence |
|---|---:|---|
| CodeCortex source subset | 14 | Nine hash-locked own-source files; seven families; null Git commit; authored source-read gold, explicitly not external human certification |
| Other seven manifests | 42 | Synthetic source fixtures; translations and related queries share families |
| Legacy TOML corpus | 94 cases | Functional sample-fixture tests; separate domain, not 94 external repository questions |
| Native chunk-policy ablation | 4 queries | Embedded authored synthetic sources; separate domain, not a public corpus import |
| Imported external public repositories with reviewed gold | 0 | No completed six-repository SHA/license/source/gold/split import found in committed current corpus |

The OCE README references methodology at `oce-ai/oce-benchmark@d4f10554a18e31599d1e46d5d56da6588d4aa86c` and says the 200-question code corpus is not bundled. An importer and an upstream reference are not an imported, licensed dataset. The referenced upstream tree could not be retrieved through the available public browser; no OCE question/count/license is certified here.

The conservative completed external multi-repository count is **0 / approximately 600**, leaving **600**. Existing 56 synthetic/own-source rows, legacy tests and untranslated/translated siblings are not subtracted from that target. Existing gold is usable for local development, with its explicit author-review limitation.

## Verified local entry and limits

At exact base SHA, built `cc-eval` offline with `--locked`, captured actual Cargo compiler-artifact executable and SHA256 in `validation.json`, then ran all eight `validate` commands: **8 exit-zero**, source/query locks valid, no lock or gold refresh. Build JSON and stderr are retained. This is input integrity validation, not search quality. No production retrieval baseline, raw historical replay, full workspace suite or live ablation was run for this audit.

Read-only rerun (from repo root):

```sh
python3 scripts/review/v19_corpus_audit.py --output /tmp/v19-audit.json
cargo build -p cc-eval --bin cc-eval --locked --offline
# Use the executable reported by Cargo if a non-default target directory is set.
<cc-eval-binary> validate --suite crates/cc-eval/benchmarks/manifests/p0-smoke.json
```

A **new current local baseline**, not yet executed, can use the existing product build-receipt helper and public stdio backend. Build exact source with a clean tracked tree; default product has no semantic HTTP opt-in. Build artifacts and `.codecortex` state belong in fresh temporary/output directories. Keep all raw rows, normalized digests, compiler artifact, product hash, source/corpus locks, commands and failure envelopes. Repeat for smoke, exact, intents and own-source subset without changing their gold:

```sh
python3 scripts/p7_stdio_build_receipt.py --package-kind default --offline --output-dir /tmp/v19-product-build
# Read the copied product executable from /tmp/v19-product-build/build-receipt.json.
<cc-eval-binary> run --backend mcp-stdio --binary <receipt-product> --suite crates/cc-eval/benchmarks/manifests/p0-smoke.json --output /tmp/v19-new-smoke --profile baseline
<cc-eval-binary> replay --run /tmp/v19-new-smoke
```

The `baseline` profile records known quality failures; exit-zero is integrity/availability, not quality acceptance. Use fresh outputs, preserve S11 no-answer and Partial source/intent limitations. This offline default-product baseline cannot demonstrate semantic live-model improvements or cost ablation. D1/D2 authorization remains required for those activities.

## Six repository ownership plan (proposal only)

`public-candidate-locks.json` contains actual upstream HEAD SHAs observed during this audit, immutable source URLs, primary LICENSE URLs, sizes and SHA256. Only public metadata and license files were read; source trees have not been imported. Review vendored/generated/dependency notices before any inclusion; an upstream root license alone does not license every bundled third-party asset. Do not follow upstream HEAD after this lock without a new corpus version.

| Owner / exclusive new corpus namespace | Public source | Role / root license | Planned independently reviewed questions |
|---|---|---|---:|
| A / serde | [serde-rs/serde](https://github.com/serde-rs/serde) | Rust, MIT OR Apache-2.0 | 100 |
| B / express | [expressjs/express](https://github.com/expressjs/express) | JavaScript, MIT | 100 |
| C / typescript | [microsoft/TypeScript](https://github.com/microsoft/TypeScript) | TypeScript, Apache-2.0 | 100 |
| D / requests | [psf/requests](https://github.com/psf/requests) | Python, Apache-2.0 | 100 |
| E / gin | [gin-gonic/gin](https://github.com/gin-gonic/gin) | Go, MIT | 100 |
| F / vite | [vitejs/vite](https://github.com/vitejs/vite) | Mixed JS/TS packages monorepo, MIT | 100 |

Each owner writes only its new namespace: immutable repository/source manifests, license inclusion/exclusion inventory, source-derived questions, literal independently verified gold and a review receipt. No owner edits common manifest registry, scorer, production, gate matrix or ledger. Integrator owns registry/aggregation and authority updates after review. Rotate reviewers B→A, C→B, D→C, E→D, F→E, A→F; gold author cannot sign their own independent review. This is a proposed division, not spawned or completed agent work.

Per repository target: 20 exact/API, 20 semantic/behavior, 15 architecture/facet groups, 20 cross-file call/data chains, 10 configuration/error paths and 15 hard-negative/no-answer questions. These are 100 **distinct reviewed question families**, with multilingual paraphrases attached rather than counted as new independent questions. If a repository cannot support 100 meaningful families, report the shortfall and add a separately licensed corpus; do not duplicate or fabricate questions. Hard negatives require source-reviewed absence and explicit scope, not low rank alone. Gold includes source SHA, path, stable symbol, line/byte spans, alternatives/facet group, and chain edges with evidence. Verify compatibility and native span/facet scoring separately.

Before any ranking/parameter inspection, register global family deduplication and a deterministic frozen split. Proposed split: 75 dev +25 holdout families per repo (450 dev /150 holdout total); this is a design proposal, **not a normative 150- or 600-holdout requirement**. Cross-language paraphrases and related query families stay in one split; review global cross-repo template leakage. Freeze source/gold/split SHA256 and reviewer decisions. Holdout gold stays inaccessible to tuning owners until the registered final evaluation. Do not backfill holdout based on scores.

Integrator preregisters top-k, native/compat metric definitions, no-answer denominator, macro-by-repository and micro aggregation, family-cluster paired bootstrap seed/iterations/CI, paired arms and practical/non-inferiority margins before the first result inspection. Public local lexical/multi-lane runs are independently feasible; real semantic quality/cost arms remain authorization-blocked. The six-source plan closes preparation scope only when imports and reviews actually exist; it does not close V19 now.

## Suggested authority updates for integrator

Keep V19.report and V19.current_local open: missing final raw not recoverable from inspected committed history; newly built validator passed eight input-lock checks but no current retrieval baseline. Keep V19.corpus open with 0 completed external reviewed questions /600 target and six pinned metadata-only candidate sources. Keep V19.holdout, facet/span, statistics open; proposed ownership/split/method are preparation only. Keep live ablation authorization-blocked. Link this audit and original omission index; do not rename older 153-row summaries as final 306-row evidence. No shared checklist was edited by this owner.
