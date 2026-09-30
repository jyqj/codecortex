# Incremental verification and bounded work (P2-D)

## Evidence boundaries

An incremental/full equality result proves agreement on the fourteen declared SQLite tables, not compiler correctness. The comparisons retain target IDs/UIDs, confidence, strategies, ambiguity, semantic edges, dispatch sites and durable resolution debt. An intermediate incomplete build is `incomplete_not_certified`; it cannot pass final parity by omitting the frontier. Unknown PublicSurface remains unknown even after all observed invalidations have drained.

`cc-eval mutation-case --case <case.json> --output <new-directory> --shrink-attempts 32` runs a self-contained initial source map and ordered Write/Delete/Rename stages through two actual parser/SQLite projects. Each stage can reopen the incremental engine, retain a partial budget result or request bounded settlement. Independent column predicates/counts are authored with the fixture, never inferred from the generated graph. Case/schema/path/size limits fail closed. The evaluator never executes user source or reads retrieval gold into the product.

On a valid failing comparison the original case/result are retained. The reducer deletes stages only while preserving the exact first failure signature; invalid inputs and infrastructure errors are not accepted as reproductions. It records its evaluation budget, whether deletion-minimality was reached, and a final standalone replay. This is bounded stage reduction, not global source minimization. The intentionally incorrect count in the reducer self-test verifies the harness; it is not a product defect or product accuracy result.

## Language and syntax scope

The P2-D fixed sequences cover Python, TS, Rust and Go provider body/signature changes, rename, equal-evidence same-name competitors, deletion, missing-name restoration, reopen and repeated no-op. Some fixtures deliberately exercise the existing global-name heuristic without language-valid imports: they do not certify compiler module rules. Separate P2-A/B/C import, forwarding cycle, config, package/test isolation and independent target regressions remain mandatory. Full tsconfig/Cargo dependency aliases/Python namespace/Go build conditions are P3 work.

JS/TS no longer supplements real calls with regex matches over function text. Actual AST expression sites own calls once; declaration/string/comment text cannot generate calls. Await arguments, template substitutions and nested receiver calls are traversed; dynamic callable expressions remain unsupported instead of masquerading as a lexical callee. Call references are projected from real call records and syntax identifiers. Targets are resolved by the existing ladder, not stamped parser_exact because a first same-named symbol exists. This remains a static analyzer, not a JS runtime, full type checker or complete lexical-binding implementation.

## Storage and catalog work

`surface_dependents_bounded` and `resolution_dependents_with_work` preserve an overflow witness, globally deduplicate paths and bound result memory. The closure retains enough slack for earlier promotions in the same invocation; original causes/completion proofs remain persisted, so an output window cannot erase the rest. `dirty_plan.dependency_sql` measures only these reverse-dependency statements: statement count, VM steps, returned rows and SQLite scan/sort counters. It is not bytes read, all indexing work or an asymptotic proof.

Completed-path exclusion can still require SQL work proportional to an affected/completed prefix. Frontier payload replacement is also proportional to retained causes/completions, bounded at the existing 200k-entry/16MiB limits. Do not describe either as constant-time; P8 must assess larger practical fanout and resources. The cost regression varies unrelated files separately from the fixed affected set, measures both an empty and a long completed prefix, and verifies old dependency rows disappear on replacement/removal.

Catalog policy stays single-source: dead slots above `max(live,4096)` refuse cache parking; the next seed load compacts. Old name/UID buckets are cleaned with file removal, and a healthy token-validated catalog is reused. The unit churn probe and 1k-file repeated real-build probe test different layers. Release-only 1k/5k disposable builds record full/no-op/body/API-resume samples and storage counts. These observations are not p95/p99, 100k, concurrent load or whole-process RSS certification; raw labels include the profile and exact command.

## One fingerprint and safe rebuilds

The compatibility names `get_export_fingerprint(s)` now delegate to PublicSurface's versioned canonical encoding: KnownEmpty has a fingerprint, Unknown/missing evidence does not. The legacy SQL and cc-index export_name hash formulae have been removed. Export names remain valid display and resolver input, not a parallel invalidation policy.

The current schema 21 includes document manifests/spec stamps and per-file policy stamps and preserves original chunk byte coordinates ([SOURCE_CHUNKS.md](SOURCE_CHUNKS.md)), removes obsolete cross-language path guesses and includes persisted import context, Go package sets and explicit multilingual ProjectModel/module outcomes (see [PROJECT_MODEL.md](PROJECT_MODEL.md)); it requires an isolated rebuild from earlier cache formats to remove persisted JS/TS false call/reference facts and outdated call positions. Full-rebuild WAL/SHM paths append suffixes to the complete database filename; replacing an extension left stale WAL pages on custom filenames. The regression keeps many incremental writes before a full rebuild and checks the resulting identities. This does not extend the rebuild guarantee to independently writing external processes.

Final command results, exact covered file set, fixed-input retrieval pairs and limitations live in the P2-D roadmap gate/implementation report. This document describes contracts; it does not self-certify an unrun platform or test.
