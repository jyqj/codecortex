# CodeCortex

A Rust MCP server for code graph indexing and analysis. CodeCortex builds a
semantic index of your codebase and exposes it through 14 MCP tools that AI
agents can call for hybrid search, impact analysis, architecture introspection,
and graph queries.

Pure code intelligence -- no UI, no CLI product, MCP-first.

## Quick Start

Build from source:

```bash
cargo build --release
```

Install into your AI agent (auto-detects Claude Code, Codex CLI, Cursor,
Gemini CLI, OpenCode, VS Code, Zed):

```bash
codecortex install
```

The MCP server starts automatically when the agent connects. You can also
launch it manually:

```bash
codecortex mcp --project_path /path/to/project
```

When launched from inside a directory tree containing `.git` or
`.codecortex.json`, the server discovers that project and auto-indexes it on
first connect (up to 50,000 files by default). If your MCP client starts
servers from another working directory, call `index(path)` once or launch
manually with `--project_path`.

## MCP Tools

All 14 tools are always available. No activation or domain system.

### Setup

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `status` | Check index health before querying | `aspect`: index / capabilities / schema / all | Index stats, available capabilities, graph node/edge schema |
| `index` | Point at a project and build/update the index | `path`, `full` (bool, default false) | Index build summary with file/symbol/edge counts |

### Discovery

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `search` | Find code by natural language or symbol name | `query`, `mode`: hybrid / symbol, `top_k`, `intent`, `exact`, `boost_files`, `recent_files`, `pinned_files` | Ranked search hits with file paths, line ranges, snippets |
| `context` | Build complete context for a task in one call | `task`, `max_symbols`, `include_source`, `intent` | Relevant symbols, relationships, and source grouped by file |

### Deep Dive

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `node` | Inspect a single symbol in detail | `symbol`, `include`: trail / source / outline / summary | Symbol metadata, caller/callee trail, source code, or outline |
| `explore` | Batch-inspect multiple symbols or trace data flow | `symbols[]`, `mode`: symbols / flow, `include_source`, `outline`, `max_depth` | Per-symbol relations and source, or flow paths between symbols |
| `trace` | Find the call-graph path between two symbols | `from`, `to`, `source_mode`: none / snippet / body / outline, `max_depth` | Shortest call path with optional function bodies at each hop |

### Analysis

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `relations` | Get callers, callees, refs, or type hierarchy | `symbol`, `kind`: callers / callees / both / refs / hierarchy, `limit`, `direction` | List of related symbols with edge metadata |
| `impact` | Understand change blast radius | `scope`: changes / tests / dead_code / circular / dependents, `files`, `base_branch`, `granularity` | Impacted symbols, affected tests, dead code list, or dependency cycles |
| `architecture` | Get high-level project structure | `aspect`: overview / communities / frameworks / routes / services / async / boundaries / env / unresolved, `filter`, `limit` | Architectural view matching the requested aspect |

### Utilities

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `files` | List indexed files or read a code region | `action`: list / region / expand, `path`, `start_line`, `end_line`, `context_lines` | File list, or source code for the requested line range |
| `graph_query` | Run a Cypher-subset query against the code graph | `query` (Cypher string) | Query result rows (MATCH/WHERE/RETURN/ORDER BY/LIMIT) |

> **Cypher subset limitations** (applies to `graph_query`):
> - `=~` regex is approximated via SQL LIKE -- only `.*`, `.+`, `.` wildcards work; character classes, alternation, and anchors return an explicit error.
> - `OPTIONAL MATCH` only works for single-hop patterns (one relationship).
> - Variable-length paths (`*1..N`) cap at 5 hops by default and only support CALLS / DEFINES / DEFINES_METHOD / CONTAINS_FILE / CONTAINS_MODULE edges. Multi-hop chains with different edge types are not supported.
> - `WITH`, `MERGE`, `CREATE`, `DELETE`, `SET`, `UNWIND` are not supported. `LIMIT` defaults to 50 when omitted.
>
> See [`docs/CYPHER.md`](docs/CYPHER.md) for the maintained subset reference.

| `ingest_traces` | Feed OTLP runtime traces to validate HTTP edges | `traces[]` (service_name, method, path, status_code) | Validation summary with matched/boosted edge counts |
| `adr` | Manage Architecture Decision Records | `action`: list / get / store / delete, `adr_id`, `title`, `status`, `context`, `decision` | ADR list or individual record |

## Recommended Usage Path

A typical agent workflow:

```
index(path) -> status() -> context(task) -> explore(symbols) -> trace(from, to) -> graph_query(cypher)
```

### Step-by-step guidance

1. **Start with `context` for any new task.** It returns the most relevant
   symbols, their relationships, and source code in a single call. This is the
   primary entry point -- prefer it over manual search + node chains.

2. **Use `explore` instead of multiple `node` calls.** When you need details on
   3+ symbols, one `explore(symbols)` call returns them all grouped by file.
   Use `mode="flow"` to discover data/control-flow paths between symbols.

3. **Use `trace(source_mode='body')` for complete flow understanding.** This
   returns the full function body and outgoing calls for every hop on the path
   -- one call gives you everything needed to understand how A reaches B.

4. **Use `impact` before editing code.** Run `impact(scope="changes")` to see
   the blast radius of your current diff. Run `impact(scope="tests")` to find
   which tests are affected.

5. **Use `relations` for targeted queries.** When you need just callers or
   callees of a specific symbol, `relations` is more efficient than `explore`.
   Use `kind="hierarchy"` for type inheritance trees.

6. **Fall back to `graph_query` only when structured tools do not cover your
   need.** Run `status(aspect="schema")` first to discover available node and
   edge types, then write Cypher.

### Anti-patterns to avoid

- Do not grep/find when `search()` is available -- it uses hybrid
  vector+FTS+grep fusion with ranking.
- Do not chain `search` + `node` when you want context -- `context(task)` is
  one round-trip.
- Do not loop `node()` over many symbols -- one `explore(symbols)` call
  returns them all.
- Do not use `trace(include_source=true)` for deep understanding -- use
  `trace(source_mode="body")` instead for complete function bodies.
- File changes are automatically detected and trigger incremental re-indexing
  (configurable via `auto_index.enabled` in `.codecortex.json`).

## Configuration

Create a `.codecortex.json` in your project root to customize behavior.
All fields are optional -- defaults work for most projects.

```json
{
  "indexing": {
    "include": ["**/*.py", "**/*.ts", "**/*.go"],  // extends (not restricts) indexing: known-language files are always indexed; include rescues unknown-language files that match these patterns
    "ignore": ["**/generated/**"],
    "max_file_bytes": 512000,
    "dirty_propagation": true,
    "memory_budget_fraction": 0.5
  },
  "search": {
    "vector_weight": 1.0,
    "lexical_weight": 1.1,
    "grep_weight": 0.8
  },
  "embeddings": {
    "provider": "hash",        // or "openai_compatible" — see below
    "dimensions": 256
  },
  "auto_index": {
    "enabled": true,
    "file_limit": 50000,
    "idle_timeout_secs": 60
  }
}
```

### Embedding Providers

The default `hash` provider uses a blake3-based sparse vector (zero network
dependency, deterministic, fast). It works well for symbol-name and
lexical-heavy queries but has a quality ceiling for semantic / natural-language
search.

For higher-quality vector search, configure an OpenAI-compatible endpoint:

```json
{
  "embeddings": {
    "provider": "openai_compatible",
    "base_url": "http://localhost:11434/v1",
    "model": "nomic-embed-text",
    "dimensions": 768
  }
}
```

Any endpoint that accepts `POST /embeddings` with `{ "input": [...], "model": "..." }`
works: OpenAI, Ollama (`ollama serve`), LiteLLM, vLLM, LocalAI, etc.

Run `status()` after connecting — the `diagnostics.embedding_provider` field
confirms which provider is active and warns when hash fallback is in effect.

### Repo Size Tiers

CodeCortex automatically detects your project size and adjusts output budgets:

| Tier | File count | Token budget | Search top_k | Max output chars |
|------|-----------|--------------|--------------|------------------|
| Tiny | < 500 | 4,000 | 5 | 18,000 |
| Small | 500 - 4,999 | 6,000 | 10 | 24,000 |
| Medium | 5,000 - 24,999 | 8,000 | 15 | 32,000 |
| Large | 25,000+ | 12,000 | 20 | 38,000 |

Budgets scale per-handler (e.g., `files` allows up to 10,000 items on Large
repos, `impact` up to 80 items).

### Environment Variable Overrides

| Variable | Effect |
|----------|--------|
| `CODECORTEX_EMBEDDINGS_PROVIDER` | `hash` or `openai_compatible` |
| `CODECORTEX_EMBEDDINGS_BASE_URL` | OpenAI-compatible endpoint URL |
| `CODECORTEX_EMBEDDINGS_API_KEY` | API key for the embeddings endpoint |
| `CODECORTEX_EMBEDDINGS_MODEL` | Model name for the embeddings endpoint |
| `CODECORTEX_EMBEDDINGS_DIMENSIONS` | Embedding vector dimensions |
| `CODECORTEX_EMBEDDINGS_TIMEOUT_SECONDS` | Embedding API timeout in seconds |
| `CODECORTEX_MEMORY_BUDGET_FRACTION` | RSS memory cap as fraction (0.1 - 0.95) |
| `CODECORTEX_DIRTY_PROPAGATION` | Enable/disable incremental dirty propagation |
| `CODECORTEX_DIRTY_PROPAGATION_MAX_FILES` | Maximum files reloaded by dirty propagation |
| `CODECORTEX_MAX_CONCURRENT_PARSE` | Cap parser worker threads |
| `CODECORTEX_USE_DIRECT_WRITER` | Enable experimental direct SQLite writer |
| `CODECORTEX_PPID_POLL_MS` | Parent-process death detection interval (0 to disable) |
| `CODECORTEX_STRICT_HASH` | Set to `1`/`true` to hash every file during incremental scans instead of using the mtime+size fast path |

## Architecture

7-crate workspace with strictly downward dependencies:

```
cc-model          Data types, config, error definitions (serde, thiserror, blake3, chrono)
    |
cc-db             SQLite index store (r2d2 pool, WAL mode, FTS5, 26 tables, schema v16)
    |
cc-parsers        Tree-sitter AST extraction + framework detection
cc-index          File scanning, incremental indexing, community detection (Louvain)
    |
cc-search         Hybrid search (vector + FTS + grep + RRF), Cypher subset engine
    |
cc-server         MCP server (rmcp), CLI (clap), CodeIndex engine, ImpactAnalyzer, FileWatcher
    |
cc-eval           Evaluation suite for retrieval quality benchmarking
```

### Data Flow

```
Source files
    |  (gitignore-aware scan, mtime+size fast path + hash confirmation)
    v
Tree-sitter parsers  -->  symbols, call edges, imports, test edges,
                          route edges, data flow edges, HTTP call edges,
                          semantic edges, dispatch sites
    |
    v
SQLite index (index.sqlite3)
    |
    +---> FTS5 full-text search
    +---> Hash / OpenAI-compatible vector embeddings
    +---> Regex symbol grep
    |
    v
RRF fusion + reranking  -->  ContextEnvelope  -->  MCP tool responses
```

### Key Internal Components

- **CodeIndex**: Core engine wrapping db + index + search. Handles
  `build_index`, `search_in_context`, `find_symbol`, `graph_query`,
  `detect_impact`, and more.
- **ImpactAnalyzer**: BFS reverse-caller expansion + community boundary
  detection + cross-service HTTP impact + historical co-change analysis.
  Git integration reads unstaged, staged, untracked, and base...HEAD diffs.
- **FileWatcher**: `notify`-based file watcher with adaptive debounce, burst
  backoff, and git dirty sanity poll. Automatically triggers incremental
  re-indexing on file changes. Controlled by `auto_index.enabled` in
  `.codecortex.json` (default: `true`).

## Language Support

30 auto-detected language identifiers (+ `Unknown` fallback) recognized across three confidence tiers:

### Full tree-sitter parsing (confidence 0.7+)

Python, JavaScript, TypeScript, TSX, JSX, Java, Go, Rust, C, C++

### Heuristic / generic fallback (confidence 0.3 - 0.5)

C#, PHP, Ruby, Swift, Kotlin, Dart, Scala, Lua, Vue, Svelte, Markdown,
SQL, YAML, TOML, HCL, Dockerfile, Bash, Protobuf, GraphQL, CMake

### Confidence tiers

| Tier | Score | Source |
|------|-------|--------|
| Generic | 0.3 | Regex-based extraction |
| Heuristic | 0.5 | Pattern matching with language awareness |
| TreeSitter | 0.7 | Full AST parsing |
| Semantic | 0.85 | Cross-reference resolved |
| Verified | 0.95 | Runtime-validated (via `ingest_traces`) |

### Semantic Framework Resolvers (16)

#### Full (15) — routes + handlers + cross-file resolution

**JavaScript/TypeScript**: Express, NestJS, Hono, React, Vue, Svelte/SvelteKit

**Python**: Django, Flask, FastAPI

**Go**: Gin / Echo / Fiber / Chi / Gorilla (unified)

**Java**: Spring / Spring Boot

**Rust**: Actix-web, Axum

**PHP**: Laravel

**Ruby**: Rails

#### Partial (1) — handler UID resolution only

**C#**: ASP.NET

### Detected Framework Signals

Recognized via manifest files and import patterns but without a dedicated
resolver (detection-only, no semantic enrichment):

Koa, Fastify, Next.js, Nuxt, Angular, Rocket, Remix, Vue Router, net/http

## Development

### Build

```bash
cargo build
cargo build --release    # optimized binary with thin LTO
```

### Test

```bash
cargo test               # all crates
cargo test -p cc-model   # single crate
cargo test -p cc-eval    # evaluation suite
```

### Binary Commands

```
codecortex mcp [--project_path PATH]   Start MCP stdio server
codecortex install [--force]           Install MCP config for detected AI agents
codecortex uninstall                   Remove MCP config from all AI agents
```

### Minimum Rust Version

1.88 (2021 edition). This matches the current dependency floor and is enforced
by CI.

## License

MIT
