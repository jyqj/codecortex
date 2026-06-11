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

- `IndexDb` — r2d2 read pool (default 4 readers, clamped to 1–64) + a dedicated
  Mutex-guarded writer connection, WAL mode. Each read connection carries a
  prepared-statement cache of 64; hot point reads go through `prepare_cached`
- `UnitOfWork` (`unit_of_work.rs`) — the multi-statement write seam: it holds
  the write connection for its whole lifetime, runs an `IMMEDIATE` transaction,
  exposes only typed write methods (never the raw connection), bumps
  `index_epoch` exactly once on commit, and rolls back when dropped uncommitted
- Two full-rebuild strategies — temp-db (`rebuild_with_temp_db`) and
  `DirectWriter` (`rebuild_with_direct_writer`) — are thin build adapters over
  one shared `run_rebuild_protocol`: snapshot an epoch floor → build the
  replacement database in a temp file → under the write lock, finalize the
  generation as `max(floor, live) + 1` and atomically rename temp → main →
  reopen the writer, rebuild the read pool, checkpoint the WAL
- WAL management: full rebuilds checkpoint-truncate the WAL; long
  incremental-only sessions checkpoint via `checkpoint_wal_if_large` once the
  WAL exceeds 16 MB
- 21 tables: metadata, files, chunks, symbols, imports, symbol_refs,
  call_edges, test_edges, routes, literal_index, communities, frameworks,
  data_flow_edges, co_change_edges, http_call_edges, semantic_edges,
  infra_nodes, infra_edges, dispatch_sites, runtime_evidence, adr
- 5 FTS5 virtual tables under a dual maintenance model: the two trigram
  mirrors — `symbols_fts` (name) and `file_paths_fts` (file_path), which
  accelerate the substring symbol and path-token lookups in file
  preselection — are kept in sync with their base tables (`symbols` / `files`)
  by insert/delete/update triggers, so no write path populates them directly;
  `chunks_fts`, `files_fts`, and `literal_fts` have no triggers and are
  maintained at the application layer (`delete_file_data` + the shared insert
  helpers). Rebuild strategies must populate data through those helpers so the
  application-maintained tables stay in sync
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
File scanning and incremental indexing, organized as a phase pipeline:
scan/diff → parse → dirty closure → framework enrichment → resolve → write →
postprocess → analysis (phase headers in `crates/cc-index/src/indexer.rs` and
`indexer_phases.rs`; ordering invariants in `build_plan.rs`).

- Scan/diff: gitignore-aware discovery (via the `ignore` crate); mtime+size
  fast path with hash confirmation; `CODECORTEX_STRICT_HASH=1` disables the
  fast path for strict scans
- Parse: memory-budgeted parallel parsing (`rayon` + `memory_budget.rs`)
- Dirty closure (`dirty_closure.rs`): a fixpoint loop promotes importers of
  export-changed files to re-resolution, bounded by a file budget and a round
  cap; reloaded edge data passes through a per-category dirty-reload policy
  (`dirty_reload_policy.rs`) deciding whether stored target UIDs are cleared,
  regenerated, or kept
- Enrichment + resolve: framework resolver enrichment (`framework_resolvers/`),
  then symbol catalog / type catalog / semantic-edge resolution (`resolver/`,
  `type_catalog.rs`), then cross-file framework resolution
- Write: incremental atomic batch or full rebuild (see cc-db)
- Postprocess (after write): test edges, dispatch synthesis (below), and
  Louvain community detection — each guarded by its own input signature so
  unchanged inputs are skipped
- Analysis: git co-change (skipped when `HEAD` is unchanged), infrastructure
  pass (gated on a path+mtime+size signature over the infra candidate set),
  ADR indexing
- Full and incremental builds share one orchestration (`build_plan.rs`):
  `prepare` is read-only (scan → parse → resolve → snapshot) and produces an
  owned `PreparedBuild`; `commit` consumes it to run write → postprocess →
  analysis, so the two modes cannot drift apart

#### Dispatch synthesis
Synthetic edges for dynamic dispatch (event emitter → handler, JSX/Vue
component rendering, state-setter re-render chains, field-backed observers,
interface dispatch), run as a postprocess round after the index write.

- Each pass is declared exactly once as a `SynthesisPassSpec` in
  `dispatch_synthesis/mod.rs`: id, signature gate, the synthetic call kinds and
  semantic-edge prefixes it owns, and its compute function. `registry()` lists
  passes in execution order; the cleanup set used when synthesis is disabled is
  derived from the owned declarations instead of being repeated by hand.
- Compute/apply separation (`synthesis_pipeline.rs`): every pass computes its
  `EdgeDelta` against the committed snapshot through the read pool (no write
  lock); all deltas are then applied atomically in a single `UnitOfWork`.
- The cross-pass overlay (`PassContext::prior_deltas`) covers CALL edges only;
  passes that consume semantic edges read committed state.

### cc-search
Hybrid search engine.

- **Retrieval lanes** (`lanes.rs`) — the seam between the engine and the
  retrieval strategies: each lane implements the `RetrievalLane` trait and is
  registered in `default_lanes()`. Three adapters today — **lexical** (FTS5
  over chunks), **grep** (regex/substring over symbols), and **graph**
  (call-graph expansion from seed symbols) — feed RRF in deterministic order;
  adding a lane needs no `plan.rs` / `engine.rs` edits.
- **Two-level caching** (`engine.rs`) — an LRU result cache keyed by
  `(index_epoch, query_hash)` and an LRU chunk-text cache keyed by chunk id.
  Every cc-db write transaction bumps the persisted index epoch, so both
  caches self-invalidate without manual hooks.
- **Preselection** — a 7-layer file scoring strategy (working set, recent,
  pinned, overlay, FTS summary, symbol/path tokens, graph-neighbor expansion);
  the layer list and constants live in the header of `preselect.rs`.
- **RRF** (Reciprocal Rank Fusion) combines local retrieval lanes, followed by
  reranking with file-path / breadcrumb / recency boosts
- **Cypher** read-only query engine (MATCH / OPTIONAL MATCH / WHERE / RETURN /
  ORDER BY / LIMIT / UNION); see [CYPHER.md](CYPHER.md). Variable-length
  `CALLS` traversals take a lazy-BFS fast path (`cypher/fast_path.rs`); see
  [ADR-0001](adr/0001-cypher-traversal-lazy-bfs-fast-path.md).

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
    |                     route edges, data-flow edges, HTTP call edges,
    |                     semantic edges, dispatch sites
    v
Dirty closure + framework enrichment + resolution
    |  (symbol/type catalogs, cross-file UID binding)
    v
SQLite index (index.sqlite3)
    |  write: incremental batch  OR  full rebuild (temp-db / direct writer)
    |
    |<--  Postprocess (runs after the write, reads the committed index,
    |     writes back): test edges, dispatch synthesis (synthetic
    |     call/semantic edges), Louvain communities — each behind its
    |     own input signature
    |<--  Analysis: git co-change, infra pass, ADR indexing
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
- **GraphReadModel** (`cc-server`, `graph_read_model/`) — shared read path for
  trace/flow/cycles/impact: adjacency loading, neighborhood BFS, semantic-edge
  projection, and HTTP/async bridge synthesis. Process-global caches are keyed
  by a `GraphReadGeneration` (db identity + `index_epoch` + `evidence_epoch`);
  each cache slot declares its `EpochSensitivity` (`IndexOnly` slots survive
  evidence-only bumps, `IndexAndEvidence` slots are evicted by any bump), so
  invalidation policy is part of the slot declaration, not call-site
  discipline. Caching stays in cc-server per ADR (cc-db owns the persisted
  epoch vector and typed queries).
- **symbol_resolution** (`cc-server`, `symbol_resolution.rs`) — single
  `resolve()` pipeline for symbol-name → candidate disambiguation shared by
  `trace_path`, `explore_flow`, and `type_hierarchy`; per-tool differences
  (exact vs LIKE match, file filter semantics, kind filter) are pinned
  explicitly in `ResolutionOpts` presets (`for_trace` / `for_flow` /
  `for_type_hierarchy`) rather than re-implemented inline.
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
