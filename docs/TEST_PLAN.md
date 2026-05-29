# Test Plan

## Unit Tests

745 passed + 2 ignored in the latest `cargo test --workspace` — 739 crate unit tests + 5 `mcp_stdio` integration tests + 1 doctest (the 2 ignored are the real-workspace benchmark and an ignored doctest).

| Crate | Tests | Coverage Focus |
|-------|-------|----------------|
| cc-db | 56 | Schema, migrations (incl. v17→v18 trigram symbols_fts backfill), chunk text encoding, SQL injection, architecture, ADR, edges, frontier, graph, query, batch export fingerprints |
| cc-eval | 16 | Assertion types (incl. field_equals, output_not_contains, field_matches_regex, array_contains_item, expected_symbols Recall@5 threshold, expect_error), corpus loading, fixture integration, ignored real-workspace benchmark |
| cc-index | 223 | Framework resolvers (16, incl. cross-file), dispatch synthesis, multi-level Louvain community detection, resolver tier aliases, export-fingerprint contract, adaptive memory budget |
| cc-model | 31 | Route normalization, data structures, enum round-trip, project root discovery |
| cc-parsers | 170 | Tree-sitter parsing for 10 languages, symbol extraction, AST-based Rust/C/C++ call graphs, spec-driven heuristic intra-file call edges, C/C++/Rust param/return data-flow |
| cc-search | 134 | Cypher parser/executor, variable-length path cap, regex validation, WHERE/Degree identifier validation, vector cache + top-k heap, file-scoped vector streaming, grep SQL scoping, search engine, preselect substring recall via trigram |
| cc-server | 110 | Engine lifecycle, impact analyzer BFS, confidence-threshold filtering, exposed explore/trace params, handler dispatch integration, stdio MCP E2E, output limits, UTF-8-safe truncation, graph trace, cycles, flow |

## Eval Suite (cc-eval)

62 corpus cases covering all 14 MCP tools + error paths + boundary conditions, across Python/JS/TS/Rust/Go/Java/C/C++. Run with `cargo test -p cc-eval`.

### Corpus Cases

| Case | Tool | What It Validates |
|------|------|-------------------|
| status_basic | status | Index health, capabilities, schema output |
| index_build | index | Full index build on fixture project |
| search_hybrid | search | Hybrid fusion search returns relevant hits |
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

### Assertion Types

- `is_success` — tool did not error
- `output_contains` — substring match on serialized output
- `output_not_contains` — negative substring match (fails if substring is found)
- `field_exists` — JSON path exists (supports dot-notation and array indices)
- `field_equals` — exact value match at JSON path (String, Number, Bool, Null)
- `field_matches_regex` — regex match on string value at JSON path
- `array_contains_item` — check that an array at a JSON path contains a specific value
- `min_results` — array at path has >= N items
- `expected_symbols` — retrieval quality: checks that expected symbol names appear in results; computes Recall@5 and MRR metrics
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
- Real workspace benchmark: `benchmark_real_workspace` copies the CodeCortex workspace (212 files in latest run) into a temp dir and writes `docs/benchmarks/real_workspace_latest.md` when requested
- Recall@5 and MRR implemented via `expected_symbols` assertions (5 corpus cases active)

## Integration Testing

MCP server integration has two layers:
- Eval harness: creates a `CodeIndex`, builds the fixture index, and runs all corpus cases through the actual handler dispatch path.
- Stdio E2E (5 tests): launches the `codecortex mcp` binary via rmcp `TokioChildProcess`, lists the 14 tools, then exercises `index`/`status`/`search`, the graph tools (`context`/`node`/`explore`/`trace`/`relations`), the analysis tools (`impact`/`architecture`/`files`/`graph_query`/`adr`), and project-switch cache isolation over the real MCP stdio protocol.

## Pre-commit

No pre-commit hooks configured. Run `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets && cargo test -p cc-eval -- integration_fixtures_and_corpus`. For real-workspace performance, run `CODECORTEX_WRITE_REAL_BENCHMARK=1 cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture`.
