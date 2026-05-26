# CodeCortex — Code Index Engine

> Version: 2.0 | Date: 2026-05-25
> Pure code indexing engine. No runtime/session/workflow/memory/skill/knowledge.

---

## Architecture

```
cc-model          zero-dep data types, config, error
    |
cc-db             SQLite index store (IndexDb, FTS5, WAL)
    |
cc-parsers        tree-sitter AST extraction (Python/JS/TS/Java/Go/Rust)
cc-index          file scan, incremental index, community detection
    |
cc-search         hybrid search (vector + FTS + grep + RRF), Cypher subset, graph queries
    |
cc-server         MCP server (rmcp), CLI (clap), CodeIndex engine, ImpactAnalyzer, FileWatcher
```

All crate dependencies flow downward. No cycles.

## Crates

### cc-model
Data types, config, error definitions. Zero external dependencies beyond serde.
- `ProjectConfig` (loaded from `.codecortex.json`)
- `IndexPaths` (project_path, workdir, index_db, logs_dir)
- `ContextEnvelope`, `ContextNode`, `ContextSpan` (search result packaging)
- `Intent`, `Language`, `SymbolKind` (enums)
- `ImpactReport`, `ImpactedSymbol`, `RiskLevel` (impact analysis)
- `SearchRequest`, `SearchHit` (search I/O)

### cc-db
SQLite persistence for the code index. Single database: `index.sqlite3`.
- `IndexDb`: connection pool (r2d2, 4 readers + 1 writer), WAL mode
- 25 tables: files, chunks, symbols, imports, call_edges, test_edges, route_edges, route_nodes, infra_nodes, infra_edges, etc.
- FTS5 full-text search on chunks, diagnostics, literals, files
- Schema versioning via `user_version` pragma (rebuild on mismatch)

### cc-parsers
Tree-sitter AST extraction for 9 language families.
- Python, JavaScript, TypeScript/TSX/JSX, Java, Go, Rust, Markdown
- Extracts: symbols, call edges, imports, test edges, route edges
- Confidence tiers: Generic (0.3), Heuristic (0.5), TreeSitter (0.7)

### cc-index
File scanning and incremental indexing.
- Gitignore-aware file discovery (via `ignore` crate)
- Incremental: mtime + hash change detection
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
- Cypher subset query engine (MATCH/WHERE/RETURN/ORDER BY/LIMIT)
- Legacy `GraphQueryEngine` fallback for non-Cypher queries

### cc-server
CLI + MCP server. Contains the `CodeIndex` engine struct.

**CodeIndex** (~600 lines) wraps cc-db + cc-index + cc-search:
- `new(project_path)` / `set_project` / `close` / `reopen`
- `build_index` / `build_auto_index` / `index_status`
- `search_in_context(query, top_k, intent)` -> `ContextEnvelope`
- `find_symbol` / `file_symbols` / `list_indexed_files` / `summarize_file`
- `graph_query` / `callers` / `callees` / `trace_path` / `symbol_refs`
- `detect_impact` / `analyze_impact` / `find_impacted_tests`

**ImpactAnalyzer**: BFS reverse-caller expansion + community boundary detection + cross-service HTTP impact + historical co-change analysis. Git integration: unstaged + staged + untracked + base...HEAD.

**FileWatcher**: `notify`-based file watcher with debounce, burst backoff, and gitignore filtering.

## MCP Tool Domains (4 domains, 31 tools)

### meta (always active)
`list_tool_domains`, `activate_domain`

### core (active by default)
`set_project`, `build_index`, `index_status`, `search`, `find_symbol`, `list_files`, `file_symbols`, `list_communities`, `list_frameworks`, `index_capabilities`, `callers`, `callees`, `analyze_impact`, `summarize_file`

### context (activate on demand)
`search_in_context`, `prepare_edit_region`, `expand_code_region`

### graph (activate on demand)
`graph_query`, `trace_path`, `symbol_refs`, `find_impacted_tests`, `get_dependents`, `find_dead_code`, `find_references`, `get_architecture`, `find_route_handlers`, `find_async_consumers`, `find_service_bindings`, `list_package_boundaries`

## CLI Commands (22)

`init-project`, `index`, `status`, `search`, `search-in-context`, `analyze-impact`, `mcp`, `index-capabilities`, `find-symbol`, `graph-query`, `callers`, `callees`, `trace-path`, `watch`, `list-files`, `file-symbols`, `list-communities`, `list-frameworks`, `clean`, `summarize-file`, `symbol-refs`, `install`

## What is NOT in this project

- No session/task management
- No workflow/replay engine
- No memory/knowledge/skill system
- No pin/working-set/overlay
- No learning/policy optimization
- No telemetry persistence
- No runtime.sqlite3 (only index.sqlite3)
