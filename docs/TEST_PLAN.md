# Test Plan

## Unit Tests

1107 passed + 11 ignored in the latest `cargo test -q --workspace --all-targets` —
1086 crate/unit-target tests + 13 `type_catalog_bench`, 3 `mcp_dispatch_seam`,
and 5 `mcp_stdio` integration tests (the 11 ignored cases: the 5
real-workspace/incremental benchmarks in cc-eval, the 3 synthetic scale
benchmarks in `scale_bench`, the cc-db rebuild stress loop, and the 2
release-only micro-benchmarks in `type_catalog_bench` /
`graph_traversal_bench`).

| Crate | Tests | Coverage Focus |
|-------|-------|----------------|
| cc-db | 116 | Schema v4 rebuild-on-mismatch, chunk text encoding (incl. pre-compressed blob side-car), SQL injection, architecture, ADR, edges, frontier, graph, query, batch export fingerprints |
| cc-eval | 19 passed + 5 ignored | Assertion types (incl. field_equals, output_not_contains, field_matches_regex, array_contains_item, expected_symbols Recall@5 threshold with per-case `min_recall`, expect_error), corpus loading, fixture integration over the real MCP wire path, synthetic repo generator determinism, ignored real-workspace/incremental benchmark tests |
| cc-index | 317 | Framework resolvers (16, incl. cross-file), dispatch synthesis, multi-level Louvain community detection, resolver tier aliases, route-resolution provenance, dirty-closure status classification, framework detection signals, export-fingerprint contract, adaptive memory budget, staged-commit generation guard, config-linker signature gate |
| cc-model | 51 | Route normalization, data structures, enum round-trip, element-confidence matrix baselines, project root discovery, partial config defaults, external cache-dir paths, GraphExplain envelope, tool_graph_subsets catalog consistency + matrix snapshot |
| cc-parsers | 177 | Tree-sitter parsing for 10 languages, symbol extraction, AST-based Rust/C/C++ call graphs, spec-driven heuristic intra-file call edges, C/C++/Rust param/return data-flow |
| cc-search | 214 | Cypher parser/executor, variable-length path cap, regex validation, WHERE/Degree identifier validation, FTS5/RRF search, grep SQL scoping, search engine, result-cache Arc reuse, graph-aware result cache (epoch keying, degraded-result exclusion), fast-path kinds derived from catalog, preselect substring recall via trigram |
| cc-server | 191 | Engine lifecycle, impact analyzer BFS, confidence-threshold filtering, exposed explore/trace params, handler dispatch integration, stdio MCP E2E, output limits, UTF-8-safe truncation, graph trace, cycles, flow, build-gate serialization, watcher acquire-before-drain, graph_explain attachment |

## Eval Suite (cc-eval)

94 corpus cases covering all 14 MCP tools + error paths + boundary conditions, across Python/JS/TS/Rust/Go/Java/C/C++ — 24 of them gold accuracy cases carrying `expected_symbols` retrieval assertions (latest run: Avg Recall@5 1.00, Avg MRR 0.92). Every case is dispatched through the real MCP wire path: an in-process duplex JSON-RPC connection to the same rmcp `CodeCortexMcpServer` served over stdio, so schema deserialization, parameter sanitization, handler dispatch, and output budgeting are all under eval. Run with `cargo test -p cc-eval`.

### Corpus Cases

| Case | Tool | What It Validates |
|------|------|-------------------|
| status_basic | status | Index health, capabilities, schema output |
| index_build | index | Full index build on fixture project |
| search_hybrid | search | Ranked local fusion search returns relevant hits |
| search_symbol | search | Symbol-mode exact name lookup |
| search_rust_function | search | Rust function symbol search |
| search_go_function | search | Search for Go handler function (handleGetUser) |
| search_java_controller | search | Search for Spring controller (UserController) |
| search_golden_js | search | Golden test: search for user processing functions |
| search_golden_python | search | Golden test: search for user API functions |
| search_exact_symbol_name | search | Exact symbol search for calculate_total |
| context_task | context | Task-driven context extraction |
| context_flask_routes | context | Flask route context building |
| context_golden_refactor | context | Golden test: context for refactoring formatName |
| context_with_intent | context | Context query with intent=fix to prioritize bug-fix relevant symbols |
| context_graph_enriched | context | Context search path returns graph-enriched evidence |
| node_trail | node | Single symbol trail (callers + callees) |
| node_source_rust | node | Rust function source inspection |
| node_outline | node | File outline using include=outline for utils.js symbols |
| explore_symbols | explore | Batch symbol inspection |
| explore_flow_js | explore | JS symbol data flow exploration |
| explore_models_hierarchy | explore | Explore Python model classes (User, AdminUser) |
| explore_flow_mode | explore | Explore data flow from processUser |
| trace_path | trace | Call-graph path discovery |
| trace_cross_file_js | trace | JS cross-file call chain (getUserRoute → formatName) |
| trace_rust_intra | trace | Rust intra-file call chain |
| trace_same_symbol | trace | Trace from a symbol to itself (trivial path) |
| relations_callers | relations | Caller relationship extraction |
| relations_callees_process | relations | Callee extraction for processUser |
| relations_cross_file_python | relations | Cross-file callers of get_user in Python |
| relations_hierarchy | relations | Type hierarchy for User class |
| impact_dead_code | impact | Dead code detection |
| impact_dependents_utils | impact | File dependent analysis for utils.js |
| impact_circular | impact | Circular dependency detection |
| architecture_overview | architecture | Project structure overview |
| architecture_routes | architecture | Route extraction from frameworks |
| architecture_routes_express | architecture | Express route extraction |
| architecture_frameworks | architecture | Framework detection finds multiple frameworks |
| architecture_env | architecture | Environment variable references |
| architecture_services | architecture | Service bindings |
| files_list | files | File listing from index |
| graph_query_basic | graph_query | Cypher subset query execution |
| graph_query_callers | graph_query | Cypher query for callers |
| graph_query_aggregation | graph_query | COUNT aggregate with AS alias |
| graph_query_optional_match | graph_query | Anchored OPTIONAL MATCH preserves source on no target |
| graph_query_regex_regexp | graph_query | `=~` regex via SQLite REGEXP UDF |
| graph_query_union | graph_query | UNION combines two sub-queries |
| graph_query_variable_length_calls | graph_query | Variable-length `[:CALLS*1..3]` path traversal |
| ingest_traces_basic | ingest_traces | Runtime evidence ingestion |
| adr_list | adr | ADR listing |
| search_typescript_class | search | TypeScript class search (ItemsService) |
| node_outline_typescript | node | TypeScript file outline (ItemsController) |
| search_c_function | search | C function search (compute_area) |
| trace_c_intra | trace | C intra-file call chain (compute_area → multiply) |
| relations_callers_c | relations | C callers of multiply |
| search_cpp_method | search | C++ method search (has_funds) |
| trace_cpp_method | trace | C++ method call chain (withdraw → has_funds) |
| relations_callees_cpp | relations | C++ callees of withdraw |
| error_invalid_cypher | graph_query | Invalid Cypher query returns error |
| error_node_missing | node | Node lookup for nonexistent symbol returns error |
| error_trace_missing_symbol | trace | Trace between nonexistent symbols returns error |
| error_files_invalid_action | files | Invalid files action returns an explicit error contract |
| error_impact_no_index | impact | Impact with no changed files returns valid response |
| error_search_empty_query | search | Empty query returns empty or valid response |
| search_golden_c_compute_area | search | Golden: exact symbol lookup for C function compute_area |
| search_golden_cpp_transfer | search | Golden: exact symbol lookup for C++ free function banking::transfer |
| search_golden_fuzzy_calculate | search | Golden: fuzzy symbol lookup 'calculate' finds calculate_total |
| search_golden_fuzzy_create_user | search | Golden: fuzzy symbol lookup 'create_use' finds create_user (prefix match, not exact) |
| search_golden_go_handler | search | Golden: exact symbol lookup for Go gin handler handleGetUser |
| search_golden_java_list_users | search | Golden: exact symbol lookup for Spring controller method listUsers |
| search_golden_js_middleware | search | Golden: exact symbol lookup for Express middleware authMiddleware |
| search_golden_py_class_view | search | Golden: exact symbol lookup for Django class-based view UserListView |
| search_golden_rust_process_order | search | Golden: exact symbol lookup for Rust function process_order |
| search_golden_ts_items_service | search | Golden: exact symbol lookup for NestJS service class ItemsService |
| search_golden_hybrid_geometry | search | Golden: hybrid search for rectangle area surfaces both C geometry functions |
| search_golden_hybrid_graph_caller | search | Golden (graph-flavored): 'who calls formatName' must rank formatName top-5 and surface its caller processUser via the CALLS edge |
| search_golden_hybrid_logging | search | Golden: hybrid search for logging ranks the log method top-5 |
| search_golden_hybrid_order | search | Golden: hybrid search for order totals surfaces the Rust order functions |
| search_golden_hybrid_validate_email | search | Golden: hybrid search for email validation ranks validateEmail in top 5 |
| search_golden_hybrid_withdraw | search | Golden: hybrid search for withdrawal logic ranks the C++ withdraw method in top 5 |
| context_golden_bank_transfer | context | Golden: context for bank transfers surfaces the C++ account methods |
| context_golden_format_user | context | Golden: context for user-name formatting surfaces processUser and formatName |
| context_golden_order_total | context | Golden: context for order-total calculation surfaces the Rust order chain |
| node_golden_trail_process_user | node | Golden: node trail of processUser shows its callee formatName and caller getUserRoute |
| explore_golden_flow_route_chain | explore | Golden: flow exploration getUserRoute → formatName passes through processUser |
| trace_golden_c_perimeter_multiply | trace | Golden: C call path rectangle_perimeter → multiply exists |
| trace_golden_cpp_transfer_deposit | trace | Golden: C++ call path transfer → deposit exists with both endpoints |
| trace_golden_js_route_chain | trace | Golden: cross-file JS path getUserRoute → processUser → formatName, intermediate hop present |
| trace_golden_rust_order_total | trace | Golden: Rust call path process_order → calculate_total exists |
| relations_golden_hierarchy_user | relations | Golden: type hierarchy of User shows BaseModel ancestor and AdminUser descendant |
| relations_golden_py_get_user_callers | relations | Golden: cross-file Python callers of get_user include Flask and Django views |
| graph_query_golden_multiply_callers | graph_query | Golden: Cypher CALLS query finds both C callers of multiply |
| impact_golden_dead_code_known | impact | Golden: dead code scan finds the deliberately-unreferenced Go helpers |
| impact_golden_geometry_callers | impact | Golden: changing geometry.c impacts compute_area and rectangle_perimeter |
| impact_golden_utils_blast_radius | impact | Golden: changing utils.js impacts its cross-file callers |

### Assertion Types

- `is_success` — tool did not error
- `output_contains` — substring match on serialized output
- `output_not_contains` — negative substring match (fails if substring is found)
- `field_exists` — JSON path exists (supports dot-notation and array indices)
- `field_equals` — exact value match at JSON path (String, Number, Bool, Null)
- `field_matches_regex` — regex match on string value at JSON path
- `array_contains_item` — check that an array at a JSON path contains a specific value
- `min_results` — array at path has >= N items
- `expected_symbols` — retrieval quality: checks that expected symbol names appear in results; computes Recall@5 and MRR metrics. The per-case pass threshold is `min_recall` (default 0.7; single-symbol exact cases pin it to 1.0)
- `expect_error` — case-level flag: the tool is expected to return an error; `is_success` assertions are skipped

### Fixture Project

- 18 source files across 9 languages and 4+ framework resolvers:
  - JavaScript (4): routes.js, handler.js, middleware.js, utils.js
  - Python (4): app.py, api_views.py, models.py, config.py
  - Rust (2): lib.rs, api_handler.rs
  - Go (1): main.go
  - Java (1): UserController.java
  - TypeScript (2): app_controller.ts, types.ts
  - C (2): geometry.c, geometry.h
  - C++ (1): account.cpp
  - Server/framework (1): server.py
- Frameworks covered: Express, Flask, Spring, Go routers (Gin/Echo/Fiber/Chi/Gorilla)
- p95/max latency and output size tracked via `bench::run_benchmark()`
- Real workspace benchmark: `benchmark_real_workspace` copies the CodeCortex workspace (234 files in latest run) into a temp dir and writes `docs/benchmarks/real_workspace_latest.md` when requested
- Incremental index report benchmark: `benchmark_incremental_index_report_correctness` is ignored by default and covers full build -> no-op incremental -> one-file incremental update without writing benchmark artifacts
- Incremental latency benchmarks: `cargo test -p cc-eval bench_incremental -- --ignored --nocapture` runs three ignored scenarios (`bench_incremental_noop`, `bench_incremental_single_file`, `bench_incremental_dirty_closure`) over a synthesized 41-file TypeScript project; each prints p50/p95/max from `IndexReport.elapsed_ms` plus the `phase_timing` breakdown and hard-asserts counters and `dirty_propagation` status (see docs/BENCHMARK.md)
- Synthetic scale benchmarks: `cc-eval/tests/scale_bench.rs` runs ignored 1k/10k benchmarks (50k gated behind `CODECORTEX_BENCH_50K=1`) over the deterministic generator in `cc-eval/src/synth.rs`; ground-truth call-graph facts double as scale-correctness assertions (see docs/BENCHMARK.md)
- Recall@5 and MRR implemented via `expected_symbols` assertions (24 gold corpus cases active)

## Integration Testing

MCP server integration has three layers:
- Eval harness: runs all 94 corpus cases through the REAL MCP wire path — an in-process duplex JSON-RPC connection to the same rmcp `CodeCortexMcpServer` the binary serves over stdio, covering schema deserialization, parameter `sanitize()` validation, handler dispatch, and output budgeting (no stdio/child process).
- Dispatch seam (3 tests, `mcp_dispatch_seam.rs`): locks the wire-path contract — schema-invalid params are rejected, unknown tools error, and results arrive as unwrapped handler JSON.
- Stdio E2E (5 tests): launches the `codecortex mcp` binary via rmcp `TokioChildProcess`, lists the 14 tools, then exercises `index`/`status`/`search`, the graph tools (`context`/`node`/`explore`/`trace`/`relations`), the analysis tools (`impact`/`architecture`/`files`/`graph_query`/`adr`), and project-switch cache isolation over the real MCP stdio protocol.

## Pre-commit

No pre-commit hooks configured. Run `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets && cargo test -p cc-eval -- integration_fixtures_and_corpus`. For real-workspace performance, run `CODECORTEX_WRITE_REAL_BENCHMARK=1 cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture`.
