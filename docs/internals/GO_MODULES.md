# Go module/workspace inputs and typed module outcomes (P3-C)

## Model and supported profile

Go manifests and compact syntax facts extend the existing immutable ProjectModel, admitted FileCatalog, configuration cache and durable resolution frontier. Discovery consumes the scanner's existing walk or event scope. There is no second index, package-manager invocation, network lookup or execution of indexed code.

The bounded manifest projection supports module/go/toolchain, require, use and replace directives, their ordinary block forms, quoted paths and line comments. Workspace use members and local relative replacements must remain inside the indexed project. go.work replacement declarations override member replacements. Conflicting local roots, duplicate package declarations and duplicate visible modules do not acquire map-order winners. Missing local inputs, malformed configuration, external dependencies and unsupported version selection stay distinct.

The declared profile is **admitted, portable, unconditional, non-test source files**. A package has its declared package name and a sorted set of files; the import-path suffix is not assumed to be its name. Main packages are not importable. Filename platform conditions, go:build/+build conditions and cgo are retained, and unresolved selection produces Unknown. _test.go is excluded from production package membership. Dot imports and blank imports do not become invented namespace bindings. Complete lexical shadowing, types, methods, generated files, vendor trees, module-version selection, version-specific replacements, target selection and dependency feature/build profiles are not compiler-certified. Unknown directives and uncaptured environments are not treated as empty valid configuration.

Packages with more than 4096 admitted members return Unknown (`go_package_member_budget_exceeded`) and at most 64 diagnostic candidate previews, not a selected truncated package. Snapshot/captured input limits and selected workspace dependency work remain separate; see [MODULE_INPUT_SAFETY.md](MODULE_INPUT_SAFETY.md).

## Result and storage contract

ModuleResolution retains import_string, request_key, rule/strategy, conditions, configuration dependencies and probes. Status is one of Resolved, Unresolved, Unsupported, External, Ambiguous or Unknown. A Resolved result contains exactly one of resolved_path or resolved_package. A package has directory, name and admitted files; it cannot be replaced by an arbitrary representative file. Ambiguous/Unknown results can expose bounded candidate descriptors, never an invented successful target.

The legacy imports.resolved_path column remains NULL for a successful Go package result. Read ResolutionManifest.modules and packages_resolved coverage to distinguish that from a missing target. Actual call and reference edges point to concrete current symbol files and identities. The internal go-package lookup key never becomes a physical file path or fabricated database file. Go namespace selectors cannot bind a same-file function merely because its leaf name matches, and exported functions are not classified as constructors just because their names begin with uppercase letters.

TS external/unlinked packages are External; a declared local file/link whose manifest is missing is Unresolved. Duplicate workspace package names are Ambiguous. Declared external Rust registry/git dependencies are External rather than implicit local aliases. For Python, an unobserved external environment is Unknown; a missing submodule inside a known regular package, or a missing relative module, is Unresolved. Unsupported remains for known unimplemented rules, not as a universal bucket.

Schema **21** (document manifests, per-file chunk policy and original coordinates; [SOURCE_CHUNKS.md](SOURCE_CHUNKS.md)), ProjectModel payload **3**, and the retained metadata key project_model_inputs_v1 form the current format contract. Earlier caches require an isolated rebuild; tests never clear the developer's daily index. The historical key name does not change the payload version check.

## Incremental dependencies and bounds

Known local Go imports record consumer ancestor go.mod/go.work candidates, workspace/member/replacement manifests, target ancestor module boundaries and package member sources. Negative ancestor dependencies cover later configuration creation. Compact source fact changes (package/conditions) invalidate the existing dependency graph without treating every body change as configuration. TargetSurface and existing FileInventory dependencies cover actual members and new files.

Only complete unconditional Go outcomes whose modules all resolve to known package sets can drop the conservative ModuleConfig wildcard; source files with no imports also do not need that wildcard. Unknown/external/conditional or budget-limited outcomes retain conservative invalidation. Other languages keep their existing fallback. This is a measured narrowing, not a claim that every configuration edit has constant cost.

Go and Rust share the bounded source-capture primitive. Existing scanner content hashes permit compact fact reuse on unchanged files. go_source_reads and go_fact_cache_hits describe this compact capture stage, not scanner hashing, configuration discovery or commit verification IO. Changed files can still undergo an additional compact tree-sitter pass. A query reads only captured facts; capture and catalog construction still scale with project inputs.

Existing configuration bounds remain 1 MiB per file, 16 MiB aggregate and 1024 captured configuration entries. Compact source reads are bounded at 4 MiB; Go package selection stops above 4096 members with explicit incomplete knowledge. Resolution evidence and persistent frontiers retain their independent limits. Configurations are verified before publication, with acknowledgement after facts/frontier publication; this is not an atomic OS snapshot or a transaction spanning all postprocessing.

## Watcher reconciliation

Native event classification distinguishes rename sides and preserves rescan/error signals. Folder changes, incomplete removals, renames, queue-overflow signals and watcher errors request a full-tree scan rather than claiming complete scoped paths. Protected/cache/build directories do not cause self-triggering reconciliation loops. Ordinary configuration content changes remain scoped even when excluded from source indexing.

The watcher consumer now uses the rescan flag when choosing the build scope. A failed publication requests another full reconciliation rather than acknowledging and losing the drained batch. The queue is process-local; durable dependency work still lives in the existing frontier. Controlled event tests and real watcher ticks do not prove every operating system's native overflow/delivery behavior.

## Verification layers

cc-index/tests/p3c_modules.rs contains independent package-set, local/external/ambiguous/unknown, ignored-configuration, boundary and immutable-snapshot fixtures. cc-eval/tests/p3c_modules.rs exercises actual parsers, SQLite, configuration-only changes, exact target/reference identities, full/incremental parity, reopen and an explicit real MCP child with restart. Watcher tests include synthetic native events and the actual consumer retry path. p3c_cost.rs independently measures 100/1000 unrelated Go files and 25 consumers with a five-file budget.

scripts/module_oracles.py consumes only the fixed authored benchmarks/modules/p3c-fixtures.json. Rust emits metadata and dep-info without running a produced program or build script. Python PathFinder is given explicit directories; the fixture initializers raise if executed. Both compare to authored expected inputs, also consumed by the product tests. Optional Go go-list is network-disabled and uses a local toolchain; absent Go is not_run, not passed. TypeScript compiler comparison remains not_run. Version strings, commands, hashes and raw results are retained per run. These selected memberships do not certify complete compiler/runtime semantics, cross-platform behavior, public holdouts, 100k/RSS/tail latency, G3 or a release.

Normative references reviewed during this batch: Go modules reference (https://go.dev/ref/mod), go/build documentation (https://pkg.go.dev/go/build), and rustc command options (https://doc.rust-lang.org/rustc/command-line-arguments.html). Actual validation receipts, not the reference links, establish which tools ran locally.
