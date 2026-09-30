# Project model and captured module resolution (P3-A / P3-B / P3-C)

## Scope and ownership

`cc-model::project_model` contains the immutable admitted `FileCatalog`, captured config inputs, anchored paths, diagnostics and module decisions. `cc-index::project_model` discovers inputs once per prepare. `module_resolution` resolves from this snapshot without filesystem operations. Existing parse and dirty-reload paths share it; this is not a second indexing engine. Discovery is currently implemented in `project_model/mod.rs`, not a parallel `discover.rs` engine.

The scanner remains the source-file admission authority. Full discovery consumes its shared WalkManifest. TS config roots use admitted JS/TS ancestors; package/Cargo/Python/Go manifest candidates use ancestors of the corresponding admitted source catalog. Scoped discovery starts from committed roots and configuration events; it does not launch a second directory walk. JSON configs may be semantic inputs even if excluded as source by ignore rules. The loader and watcher share one config-path policy: ordinary hidden directories such as .config are supported, while protected/generated/cache directories (.git, node_modules, target, vendor, dist, build, virtualenv/cache and IDE directories) are explicit Unsupported inputs. The watcher admits potential JSON/JSONC config events without reading them; this broad admission is conservative and is not a claim that every JSON file is indexed.

`nearest_config` is CodeCortex's nearest-directory ownership policy, with tsconfig before jsconfig in one directory. It does not reproduce tsc's complete files/include/exclude/references project selection. P3-B extends the captured descriptors to ordered workspace package maps, Node format contexts, Cargo/module declarations and Python source roots. Production no longer uses the previous filesystem Cargo alias loader. See [MODULE_RESOLUTION.md](MODULE_RESOLUTION.md) for exact modes, inference boundaries and unsupported cases.

P3-B also admits bounded Cargo.toml/pyproject.toml configuration events independently of source ignore rules. Rust syntax-fact cache hits and actual source reads are reported separately. Normal parse and dirty reload consume the same captured language-specific module model.

P3-C adds captured go.mod/go.work and Go package sets, typed external/ambiguous/unknown outcomes, selective known-local Go configuration dependencies and watcher full reconciliation/retry. See [GO_MODULES.md](GO_MODULES.md) for profile limits and evidence layers.

## Configuration and resolution contract

JSONC scanning respects strings/escapes, comment bodies, line positions and trailing commas. Relative extends and ordered arrays are supported; each option retains the config that defined it. Per-capture memoization and content digests prevent per-import IO and mtime/size cache collisions. Shared option values and deduplicated diagnostics prevent repeated inheritance from exponential expansion.

Local resolution supports paths with one wildcard, exact keys before longest-prefix patterns, declared fallback target order, baseUrl, local relative modules, JS/MJS/CJS extension substitutions, and admitted JSON when resolveJsonModule is true. Equal-prefix overlapping patterns are Unsupported rather than picking a sorted-map winner. baseUrl anchors paths when present; otherwise the defining config directory does. Returned probes feed existing negative dependencies.

The mode is recorded as node10, bundler or explicit local_compat policy for the local subset. local_compat is not a TypeScript compiler mode. P3-B adds the documented local/workspace Node16/NodeNext package-condition subset. Classic, package-based extends, custom suffix/rootDirs semantics and arbitrary assets remain unclaimed. Active conditions select ordered captured package branches, without claiming full Node/TypeScript runtime interop. Missing/invalid/cyclic/unsupported configs cannot silently fall back to a more distant valid config. Unavailable imported bindings cannot be upgraded by global-name/type fallback into invented calls. This restriction is conservative, not a full lexical scope/type system.

Normative references consulted on 2026-09-28: TypeScript [module reference](https://www.typescriptlang.org/docs/handbook/modules/reference.html), [extends](https://www.typescriptlang.org/tsconfig/extends.html) and [paths](https://www.typescriptlang.org/tsconfig/paths.html). These inform the local subset; the current compiler's full defaults and version-dependent behavior are not certified.

## Persistence, recovery and publication

`project_model_inputs_v1` is a bounded digest-checked metadata envelope for roots and parsed config inputs. Changed config digests feed existing ModuleConfig dependencies and the durable resolution frontier; configs need not produce source chunks. Module decisions/probes/conditions are stored in each ResolutionManifest, not fabricated symbol identities. Module counts are separate from symbol/call resolution counts.

Captured config contents are verified again before committing. A changed captured config rejects the prepared result. Input acknowledgement follows successful facts/frontier publication: a crash before acknowledgement repeats invalidation instead of losing it. This is not a single transaction covering acknowledgement, all postprocessing and OS files. Newly appearing unobserved files/config roots and hostile concurrent symlink swaps are outside an atomic filesystem-snapshot guarantee; watcher events and subsequent full reconciliation remain necessary.

Owned configuration files are excluded from the older heuristic config-link writer. Otherwise an ignored config could be reinserted after scanning, removed on the next build and continually rebase the frontier. The scoped ignored-config/restart/full-parity regression preserves this failure mode and the repair.

Schema 21 additionally persists document manifests/spec stamps and per-file chunk policy stamps and preserves original chunk slices and coordinates (see [SOURCE_CHUNKS.md](SOURCE_CHUNKS.md)) and requires an isolated rebuild from schema 20 or earlier; imports retain syntax/context, module evidence includes package sets and explicit knowledge states, and the project input payload is version 3. Do not clear a developer's daily cache to test migration. Preserve prior binary/source and use separate caches for rollback and evidence. No live provider, network request or compiler process is required.

P3-D unifies bounded configuration/source reads in cc-model, rejects non-regular inputs before blocking open and uses descriptor-relative no-follow reads on Unix. Other-platform and concurrency guarantees are narrower; see [MODULE_INPUT_SAFETY.md](MODULE_INPUT_SAFETY.md). Workspace/Python owner lookups walk ancestor keys instead of all unrelated roots. The module parser remains disk-free; snapshot capture is not free or an atomic filesystem view.

## Bounds and cost interpretation

Config input is limited to 1 MiB per file, 16 MiB total bytes, 1024 inputs/roots and depth 32; effective configurations also have an aggregate 16 MiB bound. Paths have 256 patterns, up to 32 targets each, and bounded strings. Size/depth/cycle/protected-path failures remain explicit; symlinked configs are unsupported. ResolutionManifest and frontier retain their existing independent bounds. Many matching dependencies and completed prefixes still cost work; they are not O(1).

`IndexReport.project_model` reports input digest, catalog size, config roots/inputs, discovery reads, parse-cache hits, root probes, inventory source, modes and diagnostics. `config_reads` counts discovery content reads, NOT publication verification reads, all filesystem metadata calls, or Rust/Go compact source-declaration capture (reported separately). Publication rechecks every captured config. Whole-file catalog/ancestor-set construction is still O(files); no claim of constant-time incremental indexing is made.

Tests: `cc-index/tests/p3a_project_model.rs` (independent snapshot/config contracts), `cc-eval/tests/p3a_project_model.rs` (SQLite/full-incremental/real stdio), `cc-eval/tests/p3a_cost.rs` (explicit release measurements), plus retained P0/P1/P2 regressions. The implementation report and frozen receipts, not this capability document, establish which toolchains/platforms actually ran. No RSS, p95/p99, 100k, holdout or G3 certificate follows from a P3-A batch pass.
