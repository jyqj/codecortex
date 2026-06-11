# Architecture

CodeCortex is a pure code-intelligence engine: it builds a semantic index of a
codebase and exposes it over MCP. There is no UI, no session/workflow/memory
system, and a single on-disk database (`index.sqlite3`). For the design
philosophy and explicit non-goals, see [`DESIGN.md`](../DESIGN.md).

## Crate layout

A 7-crate Cargo workspace with strictly downward dependencies (no cycles):

```
cc-model      Data types, config, error definitions (serde, thiserror, blake3)
    |
cc-db         SQLite index store (r2d2 pool, WAL mode, FTS5, 21 tables + 5 FTS5, schema v3)
    |
cc-parsers    Tree-sitter AST extraction + framework detection
cc-index      File scanning, incremental indexing, community detection (Louvain)
    |
cc-search     Ranked local search (FTS5 + grep + preselect/RRF), Cypher subset engine
    |
cc-server     MCP server (rmcp), CLI (clap), CodeIndex engine, ImpactAnalyzer, FileWatcher
    |
cc-eval       Evaluation suite for retrieval-quality and latency benchmarking
```

### cc-model
Data types, config, and error definitions. Minimal dependencies: `serde`,
`serde_json`, `thiserror`, `tracing`, `blake3`.

- `ProjectConfig` — loaded from `.codecortex.json` (see [CONFIGURATION.md](CONFIGURATION.md))
- `IndexPaths` — project_path, workdir, index_db, logs_dir; `CODECORTEX_CACHE_DIR`
  can move per-project caches out of the repo into a stable hashed subdirectory
- `ContextEnvelope`, `ContextNode`, `ContextSpan` — search-result packaging
- `Intent`, `Language`, `SymbolKind` — enums
- `ImpactReport`, `ImpactedSymbol`, `RiskLevel` — impact analysis
- `SearchRequest`, `SearchHit` — search I/O

### cc-db
SQLite persistence for the code index. Single database file: `index.sqlite3`.

- `IndexDb` — r2d2 read pool (default 4 readers) + a dedicated Mutex-guarded
  writer connection, WAL mode
- 21 tables: metadata, files, chunks, symbols, imports, symbol_refs,
  call_edges, test_edges, routes, literal_index, communities, frameworks,
  data_flow_edges, co_change_edges, http_call_edges, semantic_edges,
  infra_nodes, infra_edges, dispatch_sites, runtime_evidence, adr
- 5 FTS5 virtual tables: full-text search on chunks, literals,
  files, plus two trigram mirrors — `symbols_fts` (name) and `file_paths_fts`
  (file_path) — that accelerate the substring symbol and path-token lookups in
  file preselection; each is kept in sync with its base table (`symbols` /
  `files`) by insert/delete/update triggers, so no write path populates it directly
- A `REGEXP(pattern, text)` scalar UDF backs Cypher `=~`; the compiled pattern is
  cached as SQLite auxiliary data so a constant pattern compiles once per
  statement, not once per row
- Schema versioning via the `user_version` pragma (v3). The current strategy is
  rebuild-on-mismatch for on-disk indexes.

### cc-parsers
Tree-sitter AST extraction across 30 auto-detected language identifiers (+ an
`Unknown` fallback). See [LANGUAGES.md](LANGUAGES.md) for the full matrix.

- Extracts: symbols, call edges, imports, test edges, route edges, data-flow
  edges (type_ref, env_access, param_pass, return_flow), HTTP call edges,
  semantic edges, dispatch sites
- Confidence tiers: Generic (0.3), Heuristic (0.5), TreeSitter (0.7),
  Semantic (0.85), Verified (0.95)

### cc-index
File scanning and incremental indexing.

- Gitignore-aware file discovery (via the `ignore` crate)
- Incremental: mtime+size fast path with hash confirmation;
  `CODECORTEX_STRICT_HASH=1` disables the fast path for strict scans
- Dirty propagation: re-parse dependents when exports change
- Community detection (Louvain modularity maximization)
- Framework detection
- Memory-budgeted parallel parsing (via `rayon`)

### cc-search
Hybrid search engine.

- **Lexical** search — FTS5 over chunks.
- **Grep** search — regex over symbols.
- **Graph** lane — call-graph expansion from seed symbols.
- **Preselection** — trigram-backed symbol substring recall narrows candidates.
- **RRF** (Reciprocal Rank Fusion) combines local retrieval lanes, followed by
  reranking with file-path / breadcrumb / recency boosts
- **Cypher** read-only query engine (MATCH / OPTIONAL MATCH / WHERE / RETURN /
  ORDER BY / LIMIT / UNION); see [CYPHER.md](CYPHER.md)

### cc-server
CLI + MCP server, home of the `CodeIndex` engine. See [MCP_TOOLS.md](MCP_TOOLS.md)
for the tool surface.

### cc-eval
Fixture- and corpus-driven evaluation harness for retrieval quality (Recall@5,
MRR) and latency. See [TEST_PLAN.md](TEST_PLAN.md) and [BENCHMARK.md](BENCHMARK.md).

## Data flow

```
Source files
    |  (gitignore-aware scan, mtime+size fast path + hash confirmation)
    v
Tree-sitter parsers  -->  symbols, call edges, imports, test edges,
                          route edges, data-flow edges, HTTP call edges,
                          semantic edges, dispatch sites
    |
    v
SQLite index (index.sqlite3)
    |
    +---> FTS5 full-text search
    +---> Regex symbol grep
    +---> Trigram-backed symbol preselection
    |
    v
RRF fusion + reranking  -->  ContextEnvelope  -->  MCP tool responses
```

## Key internal components

- **CodeIndex** (`cc-server`, ~2400 lines across `engine.rs` + `engine_query.rs`)
  wraps cc-db + cc-index + cc-search. Lifecycle and shared infrastructure stay
  on `CodeIndex` itself; the query surface is grouped into three zero-cost
  borrowed views:
  - lifecycle: `new(project_path)` / `set_project` / `close` / `reopen`,
    `build_index` / `build_auto_index` / `index_status`
  - `.search()` -> `SearchOps`: `search_in_context(query, top_k, intent)` ->
    `ContextEnvelope`, `task_symbols`
  - `.graph()` -> `GraphOps`: `find_symbol` / `file_symbols` /
    `list_indexed_files` / `summarize_file` / `graph_query` / `callers` /
    `callees` / `symbol_refs`
  - `.impact()` -> `ImpactOps`: `detect_impact` / `analyze_impact` /
    `find_impacted_tests`
- **ImpactAnalyzer** — BFS reverse-caller expansion + community boundary
  detection + cross-service HTTP impact + historical co-change analysis. Git
  integration reads unstaged, staged, untracked, and `base...HEAD` diffs.
- **FileWatcher** — `notify`-based watcher with adaptive debounce, burst
  backoff, gitignore filtering, and a git dirty sanity poll that backfills missed
  changes. Wired into the MCP server lifecycle: started on connect when a project
  path is supplied or discovered, and restarted by `index()` when the project
  path changes. Controlled by `auto_index.enabled` in `.codecortex.json`
  (default: `true`).

## Confidence tiers

| Tier | Score | Source |
|------|-------|--------|
| Generic | 0.3 | Regex-based extraction |
| Heuristic | 0.5 | Pattern matching with language awareness |
| TreeSitter | 0.7 | Full AST parsing |
| Semantic | 0.85 | Cross-reference resolved |
| Verified | 0.95 | Runtime-validated (via `ingest_traces`) |
