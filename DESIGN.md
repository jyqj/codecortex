# CodeCortex — Code Index Engine

> Version: 2.2 | Date: 2026-05-28
> Pure code indexing engine. No runtime/session/workflow/memory/skill/knowledge.

---

## Architecture

```
cc-model          data types, config, error (serde, thiserror, blake3, chrono)
    |
cc-db             SQLite index store (IndexDb, FTS5, WAL)
    |
cc-parsers        tree-sitter AST extraction (Python/JS/TS/Java/Go/Rust/C/C++)
cc-index          file scan, incremental index, community detection
    |
cc-search         hybrid search (vector + FTS + grep + RRF), Cypher subset, graph queries
    |
cc-server         MCP server (rmcp), CLI (clap), CodeIndex engine, ImpactAnalyzer, FileWatcher
```

All crate dependencies flow downward. No cycles.

## Crates

### cc-model
Data types, config, error definitions. Minimal dependencies: serde, serde_json, thiserror, tracing, blake3, chrono.
- `ProjectConfig` (loaded from `.codecortex.json`)
- `IndexPaths` (project_path, workdir, index_db, logs_dir)
- `ContextEnvelope`, `ContextNode`, `ContextSpan` (search result packaging)
- `Intent`, `Language`, `SymbolKind` (enums)
- `ImpactReport`, `ImpactedSymbol`, `RiskLevel` (impact analysis)
- `SearchRequest`, `SearchHit` (search I/O)

### cc-db
SQLite persistence for the code index. Single database: `index.sqlite3`.
- `IndexDb`: connection pool (r2d2, 4 readers + 1 writer), WAL mode
- 26 tables: files, chunks, symbols, imports, symbol_refs, resolution_attempts, call_edges, test_edges, route_edges, route_nodes, diagnostics, literal_index, scopes, communities, repo_frameworks, file_frameworks, data_flow_edges, co_change_edges, http_call_edges, semantic_edges, infra_nodes, infra_edges, dispatch_sites, runtime_evidence, adr, metadata
- FTS5 full-text search on chunks, diagnostics, literals, files
- Schema versioning via `user_version` pragma (v16, incremental migration support)

### cc-parsers
Tree-sitter AST extraction for 30 auto-detected language identifiers (+ `Unknown` enum fallback; 10 identifiers with full tree-sitter, rest via generic/heuristic).
- Full tree-sitter: Python, JavaScript, TypeScript, TSX, JSX, Java, Go, Rust, C, C++
- Heuristic/generic fallback: C#, PHP, Ruby, Swift, Kotlin, Dart, Scala, Lua, Vue, Svelte, Markdown, SQL, YAML, TOML, HCL, Dockerfile, Bash, Protobuf, GraphQL, CMake
- Extracts: symbols, call edges, imports, test edges, route edges, data flow edges (type_ref, env_access, param_pass, return_flow), HTTP call edges, semantic edges, dispatch sites
- Confidence tiers: Generic (0.3), Heuristic (0.5), TreeSitter (0.7), Semantic (0.85), Verified (0.95)

### cc-index
File scanning and incremental indexing.
- Gitignore-aware file discovery (via `ignore` crate)
- Incremental: mtime+size fast path with hash confirmation; `CODECORTEX_STRICT_HASH=1` disables the fast path for strict scans
- Dirty propagation: re-parse dependents when exports change
- Community detection (Louvain algorithm)
- Framework detection
- Memory-budgeted parallel parsing (via `rayon`)

### cc-search
Hybrid search engine.
- Vector search (hash embedder or OpenAI-compatible API)
- Lexical search (FTS5)
- Grep search (regex on symbols)
- Reciprocal Rank Fusion (RRF) combining all three lanes
- Reranking with file-path / breadcrumb / recency boosts
- Cypher-only read-only query engine (MATCH/WHERE/RETURN/ORDER BY/LIMIT/UNION)

### cc-server
CLI + MCP server. Contains the `CodeIndex` engine struct.

**CodeIndex** (~1900 lines across engine.rs + engine_query.rs) wraps cc-db + cc-index + cc-search:
- `new(project_path)` / `set_project` / `close` / `reopen`
- `build_index` / `build_auto_index` / `index_status`
- `search_in_context(query, top_k, intent)` -> `ContextEnvelope`
- `find_symbol` / `file_symbols` / `list_indexed_files` / `summarize_file`
- `graph_query` / `callers` / `callees` / `trace_path` / `symbol_refs`
- `detect_impact` / `analyze_impact` / `find_impacted_tests`

**ImpactAnalyzer**: BFS reverse-caller expansion + community boundary detection + cross-service HTTP impact + historical co-change analysis. Git integration: unstaged + staged + untracked + base...HEAD.

**FileWatcher**: `notify`-based file watcher with adaptive debounce, burst backoff, and gitignore filtering. Integrated into the MCP server lifecycle: when a project path is supplied or discovered, `run_mcp_server()` starts the watcher on connect, and `tool_index()` restarts it when the project path changes. Controlled by `auto_index.enabled` in `.codecortex.json` (default: `true`).

## MCP Tools (14 tools, no domain system)

All tools are always available. No activation required.

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `status` | Index health, capabilities, graph schema | `aspect`: index/capabilities/schema/all |
| `index` | Set project and build/update code index | `path`, `full` |
| `search` | Hybrid search or symbol lookup | `query`, `mode`: hybrid/symbol |
| `context` | Build complete task context in one call | `task`, `include_source` |
| `node` | Inspect single symbol with trail | `symbol`, `include`: trail/source/outline/summary |
| `explore` | Batch explore symbols or flow paths | `symbols[]`, `mode`: symbols/flow |
| `trace` | Call path between two symbols | `from`, `to`, `source_mode`: none/snippet/body/outline |
| `relations` | Callers, callees, refs, type hierarchy | `symbol`, `kind`: callers/callees/both/refs/hierarchy |
| `impact` | Change impact, tests, dead code, cycles | `scope`: changes/tests/dead_code/circular/dependents |
| `architecture` | Project architecture insights | `aspect`: overview/communities/frameworks/routes/services/async/boundaries/env/unresolved |
| `files` | List files, read/expand code regions | `action`: list/region/expand |
| `graph_query` | Cypher escape hatch | `query` |
| `ingest_traces` | Ingest OTLP runtime traces to validate HTTP edges | `traces[]` |
| `adr` | Manage Architecture Decision Records | `action`: list/get/store/delete |

## CLI Commands (3)

`mcp` — start MCP stdio server
`install` — install MCP configuration for detected AI agents
`uninstall` — remove MCP configuration from all detected AI agents

## What is NOT in this project

- No session/task management
- No workflow/replay engine
- No memory/knowledge/skill system (ADR is repo metadata, not agent memory)
- No UI pin/working-set/overlay commands
- No learning/policy optimization
- No telemetry persistence
- No runtime.sqlite3 (only index.sqlite3)
