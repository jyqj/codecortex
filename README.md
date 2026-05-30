# CodeCortex

A Rust MCP server for code-graph indexing and analysis. CodeCortex builds a
semantic index of your codebase and exposes it through 14 MCP tools that AI
agents call for hybrid search, impact analysis, architecture introspection, and
graph queries.

Pure code intelligence — no UI, no CLI product, MCP-first.

## Quick start

Build from source:

```bash
cargo build --release
```

Install into your AI agent (auto-detects Claude Code, Codex CLI, Cursor, Gemini
CLI, OpenCode, VS Code, Zed):

```bash
codecortex install
```

The MCP server starts automatically when the agent connects. You can also launch
it manually:

```bash
codecortex mcp --project-path /path/to/project
```

When launched from inside a directory tree containing `.git` or
`.codecortex.json`, the server discovers that project and auto-indexes it on
first connect (up to 50,000 files by default). If your MCP client starts servers
from another working directory, call `index(path)` once or launch manually with
`--project-path`.

## The 14 tools at a glance

| Group | Tools |
|-------|-------|
| Setup | `status`, `index` |
| Discovery | `search`, `context` |
| Deep dive | `node`, `explore`, `trace` |
| Analysis | `relations`, `impact`, `architecture` |
| Utilities | `files`, `graph_query`, `ingest_traces`, `adr` |

All tools are always available — no activation or domain system. A typical
workflow:

```
index(path) -> status() -> context(task) -> explore(symbols) -> trace(from, to) -> graph_query(cypher)
```

See [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) for full parameters, the recommended
usage path, and anti-patterns.

## Documentation

| Doc | Contents |
|-----|----------|
| [docs/README.md](docs/README.md) | Documentation index |
| [DESIGN.md](DESIGN.md) | Design charter, principles, non-goals |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, data flow, schema, internal components |
| [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) | The 14 MCP tools, usage path, anti-patterns |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | `.codecortex.json`, embedding providers, env overrides |
| [docs/LANGUAGES.md](docs/LANGUAGES.md) | Language tiers and framework resolvers |
| [docs/CYPHER.md](docs/CYPHER.md) | The read-only Cypher subset (`graph_query`) |
| [docs/TEST_PLAN.md](docs/TEST_PLAN.md) | Test suite and eval corpus |
| [docs/BENCHMARK.md](docs/BENCHMARK.md) | Benchmark metrics and how to run them |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Build, test, lint, and MSRV |

## Highlights

- **30 language identifiers**, 10 with full tree-sitter parsing; 16 semantic
  framework resolvers (Express, Flask, Spring, Gin, Axum, Rails, …). See
  [docs/LANGUAGES.md](docs/LANGUAGES.md).
- **Hybrid search** — FTS5 + regex grep + optional vector, fused with Reciprocal
  Rank Fusion. Vector search circuit-breaks cleanly when no embedding provider is
  configured. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md#embedding-providers).
- **Impact analysis** — BFS reverse-caller expansion, community boundaries,
  cross-service HTTP impact, and git co-change analysis.
- **Incremental indexing** — mtime+size fast path with hash confirmation, dirty
  propagation, and an auto-indexing file watcher.

## License

MIT
