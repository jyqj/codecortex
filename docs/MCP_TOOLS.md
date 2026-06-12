# MCP Tools

All 14 tools are always available — there is no activation step or domain system.
Parameters below list the most useful options; run `status(aspect="capabilities")`
for the authoritative runtime surface. Unknown parameter names are rejected with a
`-32602` invalid-params error naming the field, so typos fail fast instead of
silently running with defaults.

> **Migration note:** earlier releases silently ignored unrecognized parameter
> names; every tool now rejects them. The JSON-RPC error message carries the
> serde diagnostic verbatim, e.g.
> ``failed to deserialize parameters: unknown field `qurey`, expected one of `query`, `mode`, `top_k`, ...``
> Clients that relied on the old lenient behavior should rename or drop the
> offending parameter using the reported field name and `expected one of` list.

## Setup

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `status` | Check index health before querying | `aspect`: index / capabilities / schema / all | Index stats, available capabilities, graph node/edge schema |
| `index` | Point at a project and build/update the index | `path`, `full` (bool, default false) | Index build summary with file/symbol/edge counts |

## Discovery

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `search` | Find code by natural language or symbol name | `query`, `mode`: hybrid / symbol, `top_k`, `intent`, `exact`, `boost_files`, `recent_files`, `pinned_files`, `path_prefix` | Ranked local search hits with file paths, line ranges, snippets |
| `context` | Build complete context for a task in one call | `task`, `max_symbols`, `include_source`, `intent` | Relevant symbols, relationships, and source grouped by file |

## Deep dive

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `node` | Inspect a single symbol in detail | `symbol`, `include`: trail / source / outline / summary | Symbol metadata, caller/callee trail, source code, or outline |
| `explore` | Batch-inspect multiple symbols or trace data flow | `symbols[]`, `mode`: symbols / flow, `include_source`, `outline`, `max_depth`; symbols mode adds `max_callers`, `max_callees`, `max_source_per_file`; flow mode adds `max_paths`, `exact`, `file_path`, `max_candidates` | Per-symbol relations and source, or flow paths between symbols |
| `trace` | Find the call-graph path between two symbols | `from`, `to`, `source_mode`: none / snippet / body / outline, `max_depth`, `max_snippet_lines` | Shortest call path with optional function bodies at each hop |

## Analysis

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `relations` | Get callers, callees, refs, or type hierarchy | `symbol`, `kind`: callers / callees / both / refs / hierarchy, `limit`, `direction` | List of related symbols with edge metadata |
| `impact` | Understand change blast radius | `scope`: changes / tests / dead_code / circular / dependents, `files`, `base_branch`, `granularity`, `confidence_threshold`, `max_nodes` / `max_per_layer` (changes-scope BFS caps) | Impacted symbols (with `truncated` + `total_impacted_discovered` when the blast-radius BFS is capped), affected tests, dead code list, or dependency cycles |
| `architecture` | Get high-level project structure | `aspect`: overview / communities / frameworks / routes / services / async / boundaries / env / unresolved, `filter`, `limit` | Architectural view matching the requested aspect |

## Utilities

| Tool | When to use | Key params | Returns |
|------|-------------|------------|---------|
| `files` | List indexed files or read a code region | `action`: list / region / expand, `path`, `start_line`, `end_line`, `context_lines` | File list, or source code for the requested line range |
| `graph_query` | Run a Cypher-subset query against the code graph | `query` (Cypher string) | Envelope `{ results, row_count, truncated, truncated_reason, limit_applied }` (see [CYPHER.md](CYPHER.md)) |
| `ingest_traces` | Feed OTLP runtime traces to validate HTTP edges | `traces[]` (service_name, method, path, status_code) | Validation summary with matched/boosted edge counts |
| `adr` | Manage Architecture Decision Records | `action`: list / get / store / delete, `adr_id`, `title`, `status`, `context`, `decision` | ADR list or individual record |

## Graph explainability (`graph_explain`)

Graph read tools attach an additive `graph_explain` envelope when there is
something to report: `impact` (incl. `scope="circular"`), `trace`, `relations`
(incl. `kind="hierarchy"`), `graph_query`, and the graph-enrichment summary
inside `context`/`search` responses. Fields: `edge_kinds_used` (what was
actually traversed), `declared_edge_kinds` (the tool's static edge-kind
contract), `synthetic_edge_count` / `runtime_evidence_edge_count`, `truncated`
plus a stable `truncated_reason` token naming the first clipping cause, and
`read_errors` (capped at 8) for DB reads that degraded to partial results
instead of failing the call. A clean, untruncated run omits the field
entirely. See [ARCHITECTURE.md](ARCHITECTURE.md#graph-explainability-graphexplain)
for the per-tool edge-kind matrix.

## Recommended usage path

A typical agent workflow:

```
index(path) -> status() -> context(task) -> explore(symbols) -> trace(from, to) -> graph_query(cypher)
```

1. **Start with `context` for any new task.** It returns the most relevant
   symbols, their relationships, and source in a single call. Prefer it over
   manual search + node chains.
2. **Use `explore` instead of multiple `node` calls.** For 3+ symbols, one
   `explore(symbols)` call returns them all grouped by file. Use `mode="flow"`
   to discover data/control-flow paths between symbols.
3. **Use `trace(source_mode='body')` for complete flow understanding.** It
   returns the full function body and outgoing calls for every hop — one call to
   understand how A reaches B.
4. **Use `impact` before editing code.** `impact(scope="changes")` shows the
   blast radius of your current diff; `impact(scope="tests")` finds affected
   tests.
5. **Use `relations` for targeted queries.** When you need just callers or
   callees of one symbol, `relations` is leaner than `explore`. Use
   `kind="hierarchy"` for type inheritance trees.
6. **Fall back to `graph_query` only when structured tools fall short.** Run
   `status(aspect="schema")` first to discover node and edge types, then write
   Cypher.

## Anti-patterns to avoid

- Don't grep/find when `search()` is available — it uses ranked FTS5 + grep +
  preselection fusion with ranking.
- Don't chain `search` + `node` when you want context — `context(task)` is one
  round-trip.
- Don't loop `node()` over many symbols — one `explore(symbols)` call returns
  them all.
- Don't use `trace(include_source=true)` for deep understanding — use
  `trace(source_mode="body")` for complete function bodies.
- Don't manually re-index after edits — file changes are auto-detected and
  trigger incremental re-indexing (`auto_index.enabled` in `.codecortex.json`).

## CLI commands

```
codecortex mcp [--project-path PATH]   Start MCP stdio server
codecortex install [--force]           Install MCP config for detected AI agents
codecortex uninstall                   Remove MCP config from all AI agents
```
