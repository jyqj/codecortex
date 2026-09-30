# Captured module resolution (P3)

P3-D removes the parser-crate filesystem resolver and its generic JS/Python probe generator. All supported importers consume the single captured module layer; unmodeled language rules report Unsupported without fabricated cross-language paths. Vue/Svelte script importers share local JS/TS rules, not arbitrary component transformations. See [input safety and costs](MODULE_INPUT_SAFETY.md) and `MODULE_CAPABILITIES.json` for machine-checked current versions and actual test locations.

## Shared data and publication

`cc-model::module_inputs` contains syntax-only import context, ordered package targets, captured TOML/configuration, compact Rust module declarations and Python root descriptors. `ProjectModel` is immutable. Discovery consumes the existing WalkManifest/event scope and captured source content; module lookup performs no filesystem IO, package-manager invocation, Python execution, build script, compiler invocation or network access.

Import context distinguishes static import, require, dynamic import, type-only import and type-only require. Rust imports carry inline-module scope and cfg predicates. All import writers and dirty reload persist/validate `imports.context_json`. The same module specifier used with different conditions is identified by `(import_string, request_key)` in ResolutionManifest, not deduplicated into one decision. File outcomes retain target IDs/UIDs, strategy and confidence. Types are not runtime calls.

Configuration documents now include package.json, Cargo.toml, pyproject.toml, go.mod and go.work, using the content-digest cache and publication verification established in P3-A. TOML uses the pinned offline dependency. Raw input, effective-config, metadata-payload and per-file evidence limits remain explicit. Project-model format is 3; database schema is 21 (document manifests/spec stamps; see DOCUMENTS.md). The metadata key retains its compatibility name `project_model_inputs_v1`, while the payload version/digest and schema enforce incompatibility. Older caches require a separate full rebuild. Never clear the user's daily index to verify migration.

Changed configuration and module declarations enter the existing ModuleConfig/negative-lookup dependencies and durable resolution frontier. No second dependency or recovery engine is introduced. Acknowledgement follows facts/frontier publication, not a global transaction across every postprocess action and OS file. Exact configuration inputs are rechecked before commit; the filesystem is not an atomic snapshot.

## Typed module outcomes

Resolved contains exactly one internal file or package-set target. Unresolved is a missing target under modeled local rules; Unsupported is a known unimplemented rule; External denotes declared remote/unindexed dependencies; Ambiguous retains conflicting candidates; Unknown denotes insufficient configuration/profile/environment knowledge. Counts remain separate from symbol/call confidence and frontier freshness. A successful Go package has NULL imports.resolved_path but a populated ResolutionManifest.modules[].resolved_package. Consumers must not equate every NULL file path with an unresolved module.

## TypeScript and JavaScript scope

The resolver uses the nearest captured ts/jsconfig and owning package manifest, retaining the P3-A ownership policy rather than claiming tsc files/include/references project selection. Node10, bundler, Node16/NodeNext and local_compat are recorded separately. local_compat is an explicit CodeCortex policy, not a compiler mode.

Workspace package candidates come from declared package workspaces over the admitted manifest inventory. Supported patterns are path segments, `*` and `**`; matching uses bounded dynamic programming. The nearest declared workspace bounds lookup; duplicate matching package names are Ambiguous with candidate manifests, not map-order choices. Self references and declared file/link dependencies are supported inside the project. External or unlinked packages are External; a missing manifest for a declared local file/link is Unresolved. Installed node_modules, registry access and dependency installation are outside this batch.

Package exports/imports preserve JSON key order. Exact subpaths precede the most specific wildcard. Condition objects evaluate active keys/default in source order, not a fixed types/import/require priority. Active static/type/require/dynamic contexts, package type and .mts/.cts/.mjs/.cjs formats are recorded. Duplicate JSON keys, mixed invalid map keys, invalid targets and escaping paths are rejected. Null exports block access. Arrays may advance past invalid targets, but a selected valid target whose source file is absent does not cause a retry at another array element. Internal-import redirects have cycle/depth bounds.

JS extension substitution selects admitted TS/declaration alternatives; relative directory resolution consults captured package entry fields before index files where allowed. ESM relative imports require an explicit extension; the implementation does not apply CJS-style directory guesses. No arbitrary dist-to-src/source-map reconstruction is performed. Paths/baseUrl retain defining origins and target order. Version-conditioned typesVersions, package-based extends, arbitrary assets/rootDirs/custom suffixes, complete Node interop and all compiler-version defaults are not certified. Unsupported cases remain visible.

## Rust scope

Cargo workspace/member names, excludes, crate aliases, ordinary declared path dependencies and workspace-inherited dependency aliases are projected from captured TOML. Old Cargo compatibility tests call the new model; the former filesystem loader is no longer a production path. The previous convenience of referring to a workspace member without a dependency is retained only as `rust_workspace_alias_heuristic`, not declared dependency proof.

A crate uses its declared lib path or conventional lib/main entry. Root-level standalone lib.rs/main.rs files can define an explicitly labelled standalone module-root policy. The model does not claim every Cargo target/bin/example/test profile. Multiple incompatible owners are not silently merged.

Compact tree-sitter facts record module declarations, inline-module scope, simple path attributes and cfg predicates. External module selection distinguishes foo.rs and foo/mod.rs and reports dual-file ambiguity. crate/self/super and supported dependency aliases follow declared module locations. Unknown platform cfg, cfg_attr/macro expansion, optional/development/target-specific dependency profiles and dependency feature overrides are Unsupported instead of becoming unconditional edges. The declared profile evaluates local Cargo default features and all/any/not/feature predicates; it is not host-target inference or Cargo's complete feature unification.

Rust declaration facts are cached by scanner content hashes. Changed files may run an additional compact tree-sitter pass during discovery; this batch does not claim a single AST pass for the whole pipeline. Unchanged production builds reuse those facts without rereading/reparsing each Rust source. Module locations have file and logical-module indexes, so a query does not scan every module. Catalog construction still costs work per build; this is not constant-time incremental indexing. Module traversal has explicit depth/file limits.

## Python scope

The default static root policy is project root plus src; setuptools package-dir/find.where and Poetry from roots can explicitly change it. Relative dots are bounded by the selected package/source root. Package __init__.py and ordinary modules are resolved separately. Namespace portions are accumulated in root order until a regular package/module is found; a regular package in a later root takes precedence over earlier namespace portions, and child lookup stays within that package. A namespace directory is never fabricated as a source file. A concrete namespace submodule can be resolved.

Python source, module initializers, import hooks and build backends are never executed. Runtime sys.path mutations, custom finders/loaders, arbitrary package-dir mappings and dynamic __all__ are outside this static subset. No inference of runtime export execution is made.

## Go and watcher recovery scope

[GO_MODULES.md](GO_MODULES.md) defines the admitted portable package profile, local workspace/replace rules, package membership, compact fact caching, bounded dependency refinement and full-tree watcher reconciliation. Optional independent Rust/Python/Go fixture comparisons use scripts/module_oracles.py; only actual recorded runs count, and missing tools remain not_run.

## Symbol binding and forwarding

An explicitly imported name that is absent from its module cannot fall through to a coincidentally matching global symbol. Existing lexical/same-file rules still precede import resolution. Forwarded names are traced through actual PublicSurface declarations, resolved by the captured module model. The build loads only bounded reachable forwarding surfaces, not the whole repository per import. Named exports, forwarding aliases and cycles are visited deterministically; budgets fail explicitly. Scope/condition-sensitive macro forwarding is not expanded.

This change preserves the old multi-language forwarding/cycle assertions by replacing their previous reliance on weak global-name fallback with actual paths. It does not weaken the negative missing-member test. On import invalidation, both the target and the resolver-derived call classification are reset, preventing incremental/full differences caused by stale `call_kind`.

## Retrieval interaction found during batch verification

The expanded Rust inline-module coverage includes real test helpers. Initial fixed-corpus averages concealed R04's loss of first rank offset by R08's improvement. Raw per-query records were retained and reviewed; no source symbols or answers were deleted. The graph-enrichment seed selection now depends on direct query evidence rather than already boosted file priors, and connectivity's rerank contribution is bounded by that support. Search policy version and independent synthetic seed/support tests changed together. The final paired runner rejects any negative per-case quality delta rather than trusting equal aggregate means. Details are in SEARCH.md and the P3-B quality-review artifacts; this is a development-corpus regression fix, not a new holdout certification.

## Verification and limitations

`cc-index/tests/p3b_modules.rs` supplies independently authored targets and negative cases; `cc-eval/tests/p3b_modules.rs` covers parser/SQLite, typed context round-trip, configuration-only retarget, restart, fourteen-table parity and an explicit real MCP process. `p3b_cost.rs` separately measures indexed package updates and Rust snapshot/cache lookup in release mode. Existing P0/P1/P2/P3-A regressions remain enabled. Classic remains the unsupported-mode negative; NodeNext now has positive format/condition tests. The Cargo rename regression now requires the actual declared dependency target rather than its former unsupported null result. Historical failures and artifacts remain unchanged.

Cost observations do not prove speedup, whole-process memory limits, p95/p99, 100k scaling or external compiler equivalence. The report and frozen command receipts declare actual toolchains and platform. G3 is established only by the exact batch gate and frozen receipts. Public holdouts, embedding/OCE, full compiler equivalence, cross-platform CI and release certification remain separate gates.

Primary references consulted for the implemented subset: [TypeScript module reference](https://www.typescriptlang.org/docs/handbook/modules/reference.html), [Node packages](https://nodejs.org/api/packages.html), [Rust modules](https://doc.rust-lang.org/reference/items/modules.html), [Rust conditional compilation](https://doc.rust-lang.org/reference/conditional-compilation.html), [Python import system](https://docs.python.org/3/reference/import.html). The implementation states narrower capability boundaries than these specifications.
