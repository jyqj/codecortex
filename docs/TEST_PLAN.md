# Test Plan

## Unit Tests

586 tests across 7 crates. Run with `cargo test`.

| Crate | Tests | Coverage Focus |
|-------|-------|----------------|
| cc-db | 32 | Schema creation, migrations, SQL injection safety, edge queries |
| cc-eval | 8 | Assertion types (incl. field_equals), corpus loading, fixture integration |
| cc-index | 212 | Framework resolvers (16, incl. cross-file), dispatch synthesis, community detection |
| cc-model | 26 | Route normalization, data structures |
| cc-parsers | 135 | Tree-sitter parsing for 10 languages, symbol extraction |
| cc-search | 130 | Cypher parser/executor, search engine, graph queries |
| cc-server | 42 | MCP tool dispatch, handler logic, budget tiers |

## Eval Suite (cc-eval)

29 corpus cases covering all 14 MCP tools. Run with `cargo test -p cc-eval`.

### Corpus Cases

| Case | Tool | What It Validates |
|------|------|-------------------|
| status_basic | status | Index health, capabilities, schema output |
| index_build | index | Full index build on fixture project |
| search_hybrid | search | Hybrid fusion search returns relevant hits |
| search_symbol | search | Symbol-mode exact name lookup |
| context_task | context | Task-driven context extraction |
| node_trail | node | Single symbol trail (callers + callees) |
| explore_symbols | explore | Batch symbol inspection |
| trace_path | trace | Call-graph path discovery |
| relations_callers | relations | Caller relationship extraction |
| impact_dead_code | impact | Dead code detection |
| architecture_overview | architecture | Project structure overview |
| architecture_routes | architecture | Route extraction from frameworks |
| files_list | files | File listing from index |
| graph_query_basic | graph_query | Cypher subset query execution |
| ingest_traces_basic | ingest_traces | Runtime evidence ingestion |
| adr_list | adr | ADR listing |
| search_rust_function | search | Rust function symbol search |
| trace_cross_file_js | trace | JS cross-file call chain (getUserRoute → formatName) |
| trace_rust_intra | trace | Rust intra-file call chain |
| relations_callees_process | relations | Callee extraction for processUser |
| impact_dependents_utils | impact | File dependent analysis for utils.js |
| architecture_routes_express | architecture | Express route extraction |
| node_source_rust | node | Rust function source inspection |
| explore_flow_js | explore | JS symbol data flow exploration |
| context_flask_routes | context | Flask route context building |
| graph_query_callers | graph_query | Cypher query for callers |
| context_golden_refactor | context | Golden test: context for refactoring formatName |
| search_golden_js | search | Golden test: search for user processing functions |
| search_golden_python | search | Golden test: search for user API functions |

### Assertion Types

- `is_success` — tool did not error
- `output_contains` — substring match on serialized output
- `field_exists` — JSON path exists (supports dot-notation and array indices)
- `field_equals` — exact value match at JSON path (String, Number, Bool, Null)
- `min_results` — array at path has >= N items

### Known Limitations

- Fixture is 112 LOC (4 JS + 2 Python + 1 Rust) with Express + Flask routes
- p95/max latency and output size tracked; recall/MRR not yet implemented
- Fixture covers 3 languages but only 2 framework resolvers

## Integration Testing

MCP server integration is tested via the eval harness, which creates a `CodeIndex`, builds the fixture index, and runs all corpus cases through the actual handler dispatch path.

## Pre-commit

No pre-commit hooks configured. Run `cargo test && cargo clippy` before committing.
