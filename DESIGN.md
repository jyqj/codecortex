# CodeCortex — Design Charter

> Version: 2.3 | Date: 2026-05-29
> Pure code indexing engine. No runtime/session/workflow/memory/skill/knowledge.

This document captures the *why* and the *boundaries*. For reference detail see:

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — crates, data flow, schema, components
- [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) — the 14 MCP tools and usage path
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) — `.codecortex.json` and env overrides
- [docs/LANGUAGES.md](docs/LANGUAGES.md) — language tiers and framework resolvers
- [docs/CYPHER.md](docs/CYPHER.md) — the read-only Cypher subset

---

## Dependency overview

A 7-crate workspace with strictly downward dependencies. No cycles.

```
cc-model -> cc-db -> cc-parsers / cc-index -> cc-search -> cc-server
                                                              ^
                                                          cc-eval
```

| Crate | Responsibility |
|-------|----------------|
| cc-model | Data types, config, errors (serde, thiserror, blake3) |
| cc-db | SQLite index store: r2d2 pool, WAL, FTS5, 25 tables (+5 FTS5), schema v18 |
| cc-parsers | Tree-sitter AST extraction + framework detection |
| cc-index | File scan, incremental index, Louvain community detection |
| cc-search | Hybrid search (vector + FTS + grep + RRF) + Cypher subset |
| cc-server | MCP server (rmcp), CLI (clap), CodeIndex, ImpactAnalyzer, FileWatcher |
| cc-eval | Retrieval-quality and latency evaluation harness |

## Design principles

- **MCP-first, single-purpose.** The product is code intelligence over MCP — not
  a CLI app, not a UI. The CLI exists only to start the server and install agent
  configs.
- **One database.** All state lives in `index.sqlite3`. There is no
  `runtime.sqlite3`, no session store, no telemetry sink.
- **Deterministic and offline by default.** First-class behavior requires no
  network: parsing, FTS, grep, and Louvain are all local. Vector search is
  *optional* and **circuit-breaks** when no embedding provider is configured —
  search degrades cleanly to FTS + grep fusion rather than failing or returning
  empty results. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md#embedding-providers).
- **Read-only graph queries.** The Cypher subset (`graph_query`) supports
  MATCH / OPTIONAL MATCH / WHERE / RETURN / ORDER BY / LIMIT / UNION and never
  mutates the index.
- **Strictly downward dependencies.** Crates compose in one direction; this keeps
  build subsets honest (each crate compiles and tests in isolation).
- **Incremental by default.** mtime+size fast path with hash confirmation, dirty
  propagation, and a file watcher keep the index fresh without full rebuilds.

## What is NOT in this project

- No session/task management
- No workflow/replay engine
- No memory/knowledge/skill system (ADR is repo metadata, not agent memory)
- No UI pin/working-set/overlay commands
- No learning/policy optimization
- No telemetry persistence
- No `runtime.sqlite3` (only `index.sqlite3`)
