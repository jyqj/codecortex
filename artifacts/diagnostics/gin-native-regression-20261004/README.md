# Gin native regression diagnosis — 2026-10-04

**Blocking quality regression is real.** Fixed public DEV native answerable cases: **8 down / 2 up / 45 unchanged**; three repetitions agree for every Top1 case. Correct first hits drop 27→21 of 55, or .490909→.381818 (−.109091). All 12 no-answer cases fail all repeats on each arm; all 732 original rows remain Partial and source quality FAIL. No statistical significance claim: preregistered cluster CI is not implemented.

## Evidence and binding

Base delivery `07b87a8bb1235439a6aae26ca2c916d217bd71a4`; unchanged archive SHA256 `86a88f17993f0adf1e728879fc048cb44204d3fe2060a7140d75c53be6682613`, prefix `artifacts/checkpoints/public-dev-paired-gin-20261004/`. Baseline `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`, candidate `37dd042eaa1209a86e0cafdcd92ae77e036e76f5`, product `90858afae647a513537bf118932a7ba5020ee98b`.

`binding.json` rehashes every archived file, each arm's recorded src/Cargo/lock bytes against its Git commit, and the 53 Gin source files. Explicit **eight src directories** plus Cargo/lock match candidate to product; evaluator src matches between arms. Gin v2 native bytes are rederived with the admitted byte-only kind changes from the original author input and compared semantically to serialized run queries; compat original rows also match. Both arms bind the same v2 input lock. Serialized run query snapshots are reserialized JSON, so their raw SHA256 need not equal original input-file SHA256. Original executables were not archived: original binary pins remain build-receipt evidence, not a new binary rehash claim.

`scorer-replay.json`: **732/732 scores exactly reproduced**, **2964/2964 raw hit projections exactly match** actual unmodified Rust normalizer. Archived verified spans/validity are preserved during this replay; no fresh Gin indexing or suite retrieval occurred. Our independent builds use official Rust/cargo 1.95, original lock and default features, separate pinned worktrees/targets/cache/fixtures. All microtest returned spans are freshly source-verified.

No AGENTS.md or local SKILL.md/.agents/skills was present in workspace or these pinned checkouts. Normal origin fetch supplied missing evidence/author objects. No product, gold, scorer, ranking, budgets, central task state, or old raw artifact was changed.

## Why Top1 fell

**New accurate Go declaration names enter unchanged retrieval/reranking, allowing receiver/container/type mentions to compete as requested targets.** `boundaries.rs` now follows `type_spec/type_alias` name edges and classifies interface/type_alias by underlying Go AST. Previously these type chunks often had no name. With a name, `SearchPlan::build_hit`'s existing exact-symbol token rule grants **+0.18** to Context, Engine or Binding mentioned in the natural-language query. Breadcrumb/name also participate in overlap and chunk FTS. This is a real ordering change, not scorer inference from a title.

| Case suffix | Old primary | Candidate winner | Retained primary rank | Change |
|---|---|---|---:|---|
| f0005 | Abort | Context | 2 | down |
| f0012 | ShouldBindJSON | Context | 2 | down |
| f0035 | Data | Context | 2 | down |
| f0048 | Header | Context | 2 | down |
| f0068 | Value | Context | 2 | down |
| f0072 | DisableBindValidation | Binding | 3 | down |
| f0081 | Routes | Engine fragment | 2 | down |
| f0096 | getReadHeaderTimeout | Engine fragment | 2 | down |
| f0029 | previously no matching first hit | BindingBody interface | 1 | up |
| f0076 | previously competing Engine fragment | trySplit | 1 | up |

All IDs are `v19.gin.<suffix>.en01`. These are safe identifiers/symbol summaries; no query or source body is copied here.

For every down case, candidate winner score exceeds the retained primary score **before selection/packing**. New winners all have exact-symbol +.18; none is the requested primary. Primary name/kind/span remain identical; six primary scores are byte-identical numerically. Routes changes −.000370370 in lexical RRF; getReadHeaderTimeout changes −.008830601 in lexical RRF. Those secondary lexical shifts do not explain away the new type winner. The trySplit up case is a very small **lexical rank swap** (rank 4/5 exchange; +.000370370), not evidence of a name/type matcher guard fix.

Native scorer failures at candidate rank 1: **name 8/8, kind 8/8, span overlap 8/8, path 1/8**. Valid source evidence passes 8/8; optional qname passes 8/8 because **zero Gin answer alternatives constrain qname**. Retained primary at rank 2 or 3 passes the complete unmodified scorer conjunction. `predicates.jsonl` enumerates *every native row/hit/alternative* with all predicate booleans, safe raw projection, original digest, SHA256 and raw-relative reference. `cases.json` includes all 67 cases, repeats, scores, packing and selection receipts. Full original raw stays in the original archive.

Thus **none of these eight Top1 losses is a chunk-span break, dropped primary, normalizer/name-type guard rejection, newly-required-qname mismatch or packing-induced disappearance**. Packing still affects deeper hit availability and all quality remains Partial: this diagnosis does not certify complete retrieval or attribute every Recall/nDCG change to one cause. Compact explanation labels reduce wire bytes; qname adds identity bytes, but those effects do not account for these eight rank-1 mismatches.

## Independent microtest proof and taxonomy contract

`micro_driver.rs`, `run_micro.py`, `check_micro.py` use a self-authored small Go program, real CodeIndex build/search, real MCP backend, actual normalizer/source verifier and **unchanged native scorer** on both pinned versions. Three variants are retained: a basic control, a stronger lexical-container control (both arms already fail the broad method query), and a receiver competition fixture with one extra state field.

The receiver fixture reproduces **Top1 1→0 and MRR 1→.5 on both engine and MCP**. Primary method score stays `.4538207028196103`; old unnamed type `.28480193291848743` becomes named type `.4648019329184875`, exactly +.18, with identical text/source proof. Primary is retained at rank 2. Literal method-only query remains correct in both arms. Correct interface and named-type (`type_alias` product taxonomy) synthetic contracts improve 0→1 in every variant, showing the new taxonomy/identity is useful and should not be reverted to old false labels.

For fixed v2 Gin, **no candidate hit fails kind alone while passing path/name/qname/span/evidence**. There is no observed old-gold/new-taxonomy contract conflict explaining these Top1 losses. We did not compare old v1 gold or invent aliases. Synthetic interface/type_alias checks use their actual declaration kind; a stale broad “function” contract would fail exactly as the unchanged scorer requires. That would be a contract issue to document independently, never an automatic gold or alias edit.

## Minimal next implementation study for root

Evidence supports a new separately authorized slice at **query target/receiver interpretation feeding exact-name boosts**, preserving accurate declaration labels and current budgets. Before touching production, establish synthetic call-target versus receiver/container controls (class-only, interface-only, callable-only, mixed prose, same-name different kinds, split type fragments). Existing boost grants any matching query token the same exact-name reward; a generic target-identity rule should distinguish a requested callable from its named receiver without hiding direct type searches. This diagnosis proposes the surface, not a gold-derived weight/threshold or validated repair.

Primary source locations at candidate: `crates/cc-parsers/src/chunker/boundaries.rs:57` (Go kind), `:109` (declaration name), `:128` (compatible hint guard), `:369` (boundary identity); `crates/cc-parsers/src/chunker.rs:169` (name into chunks); `crates/cc-db/src/index_db_write_batch.rs` and `src/sql/index_v1.sql:128` (chunk FTS name/breadcrumb); `crates/cc-db/src/index_db_retrieval.rs:239` (lexical bm25); `crates/cc-search/src/plan.rs:380` (overlap), `:444` (exact-symbol bonus), `:516` (qname metadata); `crates/cc-search/src/selection/coverage.rs` (first-ranked anchor retained); `crates/cc-search/src/selection/budget.rs:269` (compact labels); `crates/cc-eval/src/benchmark/normalizer.rs:137` and `metrics.rs:78` (projection and native predicates).

Same-profile native nDCG .481059→.448182 and MRR .559091→.492424 remain adverse; Recall10 .454444→.457475 is separately small positive. Compat Top1 .8→.836364 remains a different file-level metric; no cross-formula subtraction is used.

## Reproduction

Unpack the original fixed archive to a new scratch directory. Run `python3 analyze.py <unpacked-root>` and `python3 run_replay.py <unpacked-root>` after the isolated micro builds; the latter links `replay_driver.rs` against the exact candidate lib receipt. For micro fixtures, `python3 run_micro.py baseline [base|inversion|receiver]` and corresponding candidate then `python3 check_micro.py`; outputs require fresh owned destinations and never overwrite old evidence. Current script's pinned scratch paths are explicit for this cloud diagnostic.

Deliver via normal commit/push/draft only; no merge/deploy, holdout, Requests diagnostics, broad rerun, old post_index or prohibited staging/private/100k experiments. Draft creation is attempted once; a Forbidden response stops the draft path.
