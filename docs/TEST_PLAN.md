# Test Plan

## Unit Tests

725 tests across 7 crates (+ 1 doctest, 1 ignored). Run with `cargo test`.

| Crate | Tests | Coverage Focus |
|-------|-------|----------------|
| cc-db | 53 | Schema, migrations, SQL injection, architecture, ADR, edges, frontier, graph, query |
| cc-eval | 14 | Assertion types (incl. field_equals, output_not_contains, field_matches_regex, array_contains_item, expect_error), corpus loading, fixture integration |
| cc-index | 212 | Framework resolvers (16, incl. cross-file), dispatch synthesis, community detection |
| cc-model | 29 | Route normalization, data structures, enum round-trip |
| cc-parsers | 160 | Tree-sitter parsing for 10 languages, symbol extraction, Rust parser coverage |
| cc-search | 160 | Cypher parser/executor, regex validation, vector cache, search engine, graph queries, parity tests |
| cc-server | 97 | Engine lifecycle, impact analyzer BFS, handler dispatch integration, output limits, graph trace, cycles, flow |

## Eval Suite (cc-eval)

49 corpus cases covering all 14 MCP tools + error paths + boundary conditions. Run with `cargo test -p cc-eval`.

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
| ingest_traces_basic | ingest_traces | Runtime evidence ingestion |
| adr_list | adr | ADR listing |
| error_invalid_cypher | graph_query | Invalid Cypher query returns error |
| error_node_missing | node | Node lookup for nonexistent symbol returns error |
| error_trace_missing_symbol | trace | Trace between nonexistent symbols returns error |
| error_files_invalid_action | files | Invalid action falls through to default list behavior |
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

- 15 source files across 7 languages and 4+ framework resolvers:
  - JavaScript (4): routes.js, handler.js, middleware.js, utils.js
  - Python (4): app.py, api_views.py, models.py, config.py
  - Rust (2): lib.rs, api_handler.rs
  - Go (1): main.go
  - Java (1): UserController.java
  - TypeScript (2): app_controller.ts, types.ts
  - Server/framework (1): server.py
- Frameworks covered: Express, Flask, Spring, Go routers (Gin/Echo/Fiber/Chi/Gorilla)
- p95/max latency and output size tracked via `bench::run_benchmark()`
- Recall@5 and MRR implemented via `expected_symbols` assertions (4 corpus cases active)

## Integration Testing

MCP server integration is tested via the eval harness, which creates a `CodeIndex`, builds the fixture index, and runs all corpus cases through the actual handler dispatch path.

## Pre-commit

No pre-commit hooks configured. Run `cargo test && cargo clippy` before committing.
