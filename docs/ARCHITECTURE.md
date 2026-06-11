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
cc-db         SQLite index store (r2d2 pool, WAL mode, FTS5, 21 tables + 5 FTS5, schema v4)
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
- `GraphExplain`, `GraphExplainCollector` (`graph_explain.rs`) — the shared
  explainability envelope graph tools attach additively (see
  [Graph explainability](#graph-explainability-graphexplain) below)
- `EdgeKindSet` + `tool_graph_subsets` (`graph_catalog.rs`) — typed per-tool
  edge-kind subset declarations, validated against the graph catalog by tests

### cc-db
SQLite persistence for the code index. Single database file: `index.sqlite3`.

- `IndexDb` — r2d2 read pool (default 4 readers, clamped to 1–64) + a dedicated
  Mutex-guarded writer connection, WAL mode. Each read connection carries a
  prepared-statement cache of 64; hot point reads go through `prepare_cached`.
  The public method surface is split by capability into three zero-cost
  borrowed views (same pattern as `CodeIndex`); lifecycle (`open` /
  `open_with_read_pool_size`) stays on `IndexDb` itself:
  - `.reads()` -> `ReadOps`: all queries — symbol/file/graph/retrieval reads,
    `generation`, `stats`, `get_metadata`, `get_file_state`, `read_conn`.
    Multi-symbol adjacency reads are batched (`caller_rows_by_uids` /
    `callee_rows_by_uids` / `symbol_degree_details_batch`); cc-search's graph
    consumers (enrichment, preselect, graph lane) all use the batched
    variants, so graph enrichment issues three queries per search instead of
    three per resolved symbol
  - `.writes()` -> `WriteOps`: every epoch-bumping mutation — batch writes,
    edge/evidence writes, `set_metadata`, `begin_unit_of_work`; the only
    public path to write methods (compile-time write isolation)
  - `.admin()` -> `MaintenanceOps`: rebuild protocols (`rebuild_with_temp_db` /
    `rebuild_with_direct_writer`), `checkpoint_wal*`, `instance_id`
- `UnitOfWork` (`unit_of_work.rs`) — the multi-statement write seam: it holds
  the write connection for its whole lifetime, runs an `IMMEDIATE` transaction,
  exposes only typed write methods (never the raw connection), bumps
  `index_epoch` exactly once on commit, and rolls back when dropped uncommitted
- Epoch invariant (`epoch_rules.rs` declares the full table → clock map; audit
  tests verify each write method against it):

  | Tables | Clock | Why |
  |---|---|---|
  | all index content (files, symbols, edges, routes, chunks, ...) | `index_epoch` | parsed/post-processed content; invalidates index-derived caches |
  | post-process artifacts (communities, frameworks, infra, co_change, test_edges) + `adr` | `index_epoch` | consumed as index content by context/graph output |
  | `runtime_evidence` | `evidence_epoch` | continuous ingestion must not evict index-only cache slots |
  | `http_call_edges.confidence` via `boost_http_edge_confidence` | `evidence_epoch` | the one exception: evidence-driven boost, not an index change |

  The two clocks advance independently: every `UnitOfWork` commit bumps
  `index_epoch` exactly once, while `boost_http_edge_confidence` bumps only
  `evidence_epoch`. Downstream cache slots declare which clocks they key on
  via `EpochSensitivity`
  ([`graph_read_model/cache.rs`](../crates/cc-server/src/graph_read_model/cache.rs))
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
- Schema versioning via the `user_version` pragma (v4). The current strategy is
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
  cap (`DIRTY_CLOSURE_MAX_ROUNDS` = 16); reloaded edge data passes through a
  per-category dirty-reload policy (`dirty_reload_policy.rs`) deciding whether
  stored target UIDs are cleared, regenerated, or kept. How the phase ended is
  classified by `DirtyClosureResult::status()` and surfaced as
  `dirty_propagation` on `IndexReport` (and thus the MCP `index()` response;
  omitted for full builds): `normal` (fixpoint converged), `partial_closure`
  (round cap hit or budget exceeded after round 1 — a complete-round prefix of
  the closure was kept), `budget_exceeded` (round-1 direct importers alone
  exceeded `indexing.dirty_propagation_max_files`; nothing was re-resolved,
  cross-file references may be stale, a full rebuild is advised), or
  `disabled` (config off)
- Enrichment + resolve: framework resolver enrichment (`framework_resolvers/`),
  then symbol catalog / type catalog / semantic-edge resolution (`resolver/`,
  `type_catalog.rs`), then cross-file framework resolution. Name resolution
  walks a declared ladder (`RESOLVE_LADDER` in `resolver/resolve_core.rs`):
  self-member → scope → same-file → imports → suffix → global-unique →
  call-site signals (arg-count, then receiver; metadata-less candidates
  survive as wildcards) → import-distance. Each result's `winning_step`
  names the `resolution_strategy` persisted on the edge/ref (e.g.
  `fuzzy_arg_count`, `...:upgraded_from=...`); its `candidate_count` feeds
  the confidence penalty but is not persisted. Route-handler resolution
  (`resolver/route_resolve.rs`) records its own tier provenance on the route
  record: `route_dotted` (0.85), `route_ladder:<ladder strategy>` (the
  ladder's confidence), or `route_global` (0.5); framework-resolver-resolved
  handlers (NestJS, ASP.NET, …) leave it NULL
- Write: incremental atomic batch or full rebuild (see cc-db). Chunk payloads
  are zstd-compressed during `prepare` (lock-free) and carried as a
  `PreparedBuild` side-car (`chunk_blobs`), so the write transaction only
  binds pre-computed blobs; the transaction-side compression fallback is kept
  for chunks missing from the side-car. The config-linker pass also runs here
  behind a signature gate: the expensive scan half (`scan_config_tokens`:
  project walk + tokenize) is skipped when the config-file set signature
  (paths + mtime + size, persisted as `last_config_sig` metadata) is
  unchanged — cached raw tokens are then re-resolved against the current
  symbol/file catalog (`resolve_config_links`), and when the signature is
  unchanged AND the batch wrote nothing the whole pass is skipped
- Postprocess (after write): test edges, dispatch synthesis (below), and
  Louvain community detection — skip logic is declared per pass as a
  `PassGate` adapter (`pass_gate.rs`: DB-signature, file-signature,
  HEAD-string, and a pair gate coupling two gates into one round), so
  unchanged inputs are skipped through one seam instead of hand-rolled
  checks. Gates now separate decision from
  bookkeeping: a gate decides skip/run during the lock-free compute stage and
  hands out a `DeferredSignatureRecord` that the apply stage persists, so
  signatures are only recorded once the corresponding delta actually landed
- Analysis: git co-change (skipped when `HEAD` is unchanged), infrastructure
  pass (gated on a path+mtime+size signature over the infra candidate set),
  ADR indexing — same `PassGate` registry
- Full and incremental builds share one orchestration (`build_plan.rs`):
  `prepare` is read-only (scan → parse → resolve → chunk compression →
  snapshot) and produces an owned `PreparedBuild`; the commit half consumes
  it, so the two modes cannot drift apart. The commit is itself **staged into
  three lock scopes** (module docs in `build_plan.rs` are the contract):
  1. `commit_write` — generation guard + `phase_write`, under the caller's
     write lock;
  2. `compute_postprocess` — postprocess/analysis COMPUTE with **no lock
     held**: signature scans, synthesis passes, Louvain, git/infra/ADR
     analysis read the just-committed snapshot through the read pool and
     produce typed deltas (`PostprocessPlan` / `AnalysisPlan`);
  3. `apply_postprocess` — applies the deltas in short DB transactions under
     a brief write lock and produces the `IndexReport`.

  Freshness guard: `PreparedBuild` records `index_epoch` at prepare START;
  stage 1 re-reads it and refuses to write on mismatch, and stage 3 rechecks
  the post-write epoch before applying deltas — either mismatch surfaces as
  `CcError::StalePreparedBuild`, and cc-server's shared build driver
  (`run_split_build` in `handlers/core.rs`) re-runs the whole
  prepare+commit sequence once. Stage-2 correctness (no concurrent build
  mutating the DB between write and apply) relies on the caller holding the
  per-project build gate across all three stages; the epoch recheck exists
  for cross-process writers. Eventual consistency is accepted: readers may
  briefly observe stage-1 index content whose postprocess artifacts
  (synthetic edges, communities, test edges, co-change/infra/ADR) are not
  yet refreshed — every stage-3 apply bumps `index_epoch`, so epoch-keyed
  caches converge as the deltas land. The bundled `commit` (and the
  `&mut self` builds that call it) composes the same three stage functions
  inline, so each stage has exactly one implementation

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
- **Three-level caching** (`engine.rs`) — an LRU result cache keyed by
  `(index_epoch, query_hash)`, an LRU graph-aware result cache, and an LRU
  chunk-text cache keyed by chunk id (capacities via
  `CODECORTEX_SEARCH_RESULT_CACHE_SIZE` / `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`
  / `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`). The result cache stores the final
  immutable hit list as `Arc<[SearchHit]>` — `SearchEngine::search()` returns
  that Arc, so a cache hit is a pointer clone with no per-hit text copy.
  `search_with_graph_context` (the default agent entry point via
  `search_in_context`) has its own post-enrichment cache: the key covers both
  DB epochs (`index_epoch`, `evidence_epoch` — runtime-evidence ingestion
  alters edge confidence embedded in enrichment nodes), the request hash, the
  `GraphEnrichLimits`, the token budget, and a per-engine ranking fingerprint
  (full `RankingConfig` plus `graph_weight`/`graph_top_k`). Degraded results
  (`graph_explain.read_errors` non-empty) are never cached, so a transient DB
  failure is not served for the rest of the epoch pair. Every cc-db write
  transaction bumps the persisted index epoch, so all caches self-invalidate
  without manual hooks.
- **Preselection** (`preselect.rs`) — a file scoring strategy with 8
  registered layer adapters: 6 primary layers (working set, recent, pinned,
  overlay, FTS summary, symbol/path tokens), a gated `FallbackLayer` (fires
  only when the primary layers scored nothing), and graph-neighbor expansion
  (seeds off everything before it). Each layer is a `PreselectLayer` adapter
  registered in `default_preselect_layers()` (same seam style as the
  retrieval lanes); scoring constants live in `RankingConfig`.
  `PreselectResult` carries a per-layer score breakdown (`layer_scores`),
  surfaced in hit reasons as `preselect:<layer>:+<score>` so a file's
  preselect score is auditable.
- **RRF** (Reciprocal Rank Fusion) combines local retrieval lanes, followed by
  reranking with file-path / breadcrumb / recency boosts
- **Cypher** read-only query engine (MATCH / OPTIONAL MATCH / WHERE / RETURN /
  ORDER BY / LIMIT / UNION); see [CYPHER.md](CYPHER.md). Variable-length
  `CALLS` traversals take a lazy-BFS fast path (`cypher/fast_path.rs`); see
  [ADR-0001](adr/0001-cypher-traversal-lazy-bfs-fast-path.md). Eligibility is
  gated by `FastPathConfig` (whose eligible edge kinds reference the
  `tool_graph_subsets::CYPHER_FAST_PATH` catalog declaration) and
  ineligibility is a typed `FastPathIneligibility` surfaced to callers as
  `fast_path` metadata on `graph_query` responses.

### cc-server
CLI + MCP server, home of the `CodeIndex` engine. See [MCP_TOOLS.md](MCP_TOOLS.md)
for the tool surface.

### cc-eval
Fixture- and corpus-driven evaluation harness for retrieval quality (Recall@5,
MRR) and latency. The runner drives the REAL MCP wire path: corpus cases go
through an in-process duplex JSON-RPC connection to the same rmcp
`CodeCortexMcpServer` the binary serves over stdio, so schema
deserialization, parameter `sanitize()` validation, handler dispatch, and
output budgeting are all exercised — schema drift fails eval, not just E2E
tests. A deterministic synthetic repository generator (`synth.rs`) plus the
`scale_bench` test target provide a 1k/10k/50k-file scale benchmark matrix
whose ground-truth facts double as correctness checks. See
[TEST_PLAN.md](TEST_PLAN.md) and [BENCHMARK.md](BENCHMARK.md).

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

- **CodeIndex** (`cc-server`, ~2800 lines across `engine.rs` + `engine_query.rs`)
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

  Every `CodeIndex` carries a **per-project build gate** (a `Mutex<()>`
  created at construction, surviving idle eviction) that serializes
  prepare+commit pairs across all build entry points — manual MCP `index()`,
  watcher ticks, and auto-index on connect. Lock-ordering rule: the gate is
  acquired BEFORE any `CodeIndex` RwLock acquisition in the build flow, and
  no thread may block on the gate while holding the RwLock (`&mut self`
  builds only `try_lock` and surface "busy" as an error). For the gate to
  work, every entry point must share ONE `CodeIndex` instance per project:
  `ProjectSession` keys a 16-slot LRU by normalized project path and reuses
  the cached instance on both `index_for_project_path` and
  `set_active_project`.
- **GraphReadModel** (`cc-server`, `graph_read_model/`) — shared read path for
  trace/flow/cycles/impact: adjacency loading, neighborhood BFS, semantic-edge
  projection, and HTTP/async bridge synthesis. Process-global caches are keyed
  by a `GraphReadGeneration` (db identity + `index_epoch` + `evidence_epoch`);
  each cache slot declares its `EpochSensitivity` (`IndexOnly` slots survive
  evidence-only bumps, `IndexAndEvidence` slots are evicted by any bump), so
  invalidation policy is part of the slot declaration, not call-site
  discipline. Caching stays in cc-server per ADR (cc-db owns the persisted
  epoch vector and typed queries). DB read failures on these paths are no
  longer silently degraded to empty results (`unwrap_or_default`): they are
  recorded on a `GraphExplainCollector` and surfaced as `read_errors` in the
  tool response (see below).
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
  (default: `true`). The poll loop is **loss-free by acquire-before-drain**:
  a tick first secures the build slot (the `auto_indexing` flag, then the
  per-project build gate via `try_lock`) and only then calls `drain_pending`
  — if the gate is busy, events stay queued (`has_pending` peeks without
  consuming) and the batch is indexed on a later tick instead of being
  dropped.

## Graph explainability (GraphExplain)

Graph read tools attach one shared, additive envelope
(`cc-model/src/graph_explain.rs`) instead of inventing per-tool truncation
metadata:

- `edge_kinds_used` — edge kinds actually traversed/projected (dispatch
  tokens such as `call` / `http_bridge`), deduplicated in first-seen order
- `declared_edge_kinds` — the tool's static edge-kind contract from
  `tool_graph_subsets` (see matrix below); contract metadata only, it never
  makes an otherwise-empty envelope worth attaching
- `synthetic_edge_count` / `runtime_evidence_edge_count` — how many traversed
  edges were synthesized (HTTP/async bridges) or runtime-validated
- `truncated` + `truncated_reason` — stable token for the FIRST cause that
  clipped the result (`output_budget`, `default_limit`, `max_depth`,
  `max_paths`, `max_expansions`, `max_nodes`, `max_per_layer`,
  `result_limit`, `bridge_cap`, `db_error:<op>`, …)
- `read_errors` (capped at 8, overflow counted in `read_errors_dropped`) —
  DB reads that failed but were degraded to a partial/empty result instead of
  failing the tool call; previously these were silent `unwrap_or_default`
  sites in the graph read paths

The envelope is attached additively (existing fields are untouched) across
`impact`, `trace`, `graph_query`, `relations`, `type_hierarchy`, circular
dependency analysis, and search graph enrichment (`context`/`search`); an
empty envelope serializes as `{}` and is omitted entirely.

### Per-tool edge-kind subsets

`tool_graph_subsets` (`cc-model/src/graph_catalog.rs`) is the single
declaration point for which catalog edge kinds each tool surface consults —
machine-checked by a catalog-consistency matrix snapshot test. Declarations
are visibility-only: they do not change what tools traverse.

| Tool surface | Edge kinds |
|---|---|
| CYPHER_FAST_PATH | CALLS |
| CYCLES | IMPORTS, CALLS |
| IMPACT | CALLS, TESTS, HANDLES, HTTP_CALLS, ASYNC_CALLS, CO_CHANGE |
| RELATIONS | CALLS, SEMANTIC, REFERENCES |
| SEARCH_ENRICH | CALLS, REFERENCES, TESTS |
| TRACE | CALLS, HANDLES, HTTP_CALLS, ASYNC_CALLS |
| TYPE_HIERARCHY | INHERITS, IMPLEMENTS, DEFINES_METHOD |

`FastPathConfig::DEFAULT.eligible_edge_kinds` in cc-search references
`CYPHER_FAST_PATH` directly, so the Cypher fast-path gate and the catalog
declaration cannot drift (ADR-0001, 2026-06-11 update).

## Confidence tiers

| Tier | Score | Source |
|------|-------|--------|
| Generic | 0.3 | Regex-based extraction |
| Heuristic | 0.5 | Pattern matching with language awareness |
| TreeSitter | 0.7 | Full AST parsing |
| Semantic | 0.85 | Full AST parsing + richer intra-file semantic extraction |
| Verified | 0.95 | Runtime-validated (via `ingest_traces`) |

Per-element-kind deviations from these tier defaults are single-sourced in
`ParserTier::element_confidence` (`cc-model/src/lib.rs`); see the matrix in
[LANGUAGES.md](LANGUAGES.md). Cross-file resolution happens later in cc-index
and assigns `resolution_confidence` separately.

## Extension points

Each seam below is a trait or declarative spec with a single registration
point; adding an implementation does not require edits elsewhere.

| To add a… | Seam (trait/type) | Registration point | Reference adapter |
|---|---|---|---|
| retrieval lane | `RetrievalLane` | `default_lanes()` in [`cc-search/src/lanes.rs`](../crates/cc-search/src/lanes.rs) | `GraphLane` |
| preselect layer | `PreselectLayer` | `default_preselect_layers()` in [`cc-search/src/preselect.rs`](../crates/cc-search/src/preselect.rs) | `GraphNeighborLayer` |
| framework route resolver | `FrameworkResolver` | `default_registry()` in [`cc-index/src/framework_resolvers/mod.rs`](../crates/cc-index/src/framework_resolvers/mod.rs) | [`fastapi.rs`](../crates/cc-index/src/framework_resolvers/fastapi.rs) |
| framework detection signal | `FrameworkSignalSpec` | `signal_registry()` in [`cc-index/src/framework_registry/mod.rs`](../crates/cc-index/src/framework_registry/mod.rs) | [`import_marker.rs`](../crates/cc-index/src/framework_registry/import_marker.rs) |
| language (no tree-sitter grammar) | `LangSpec` | [`cc-parsers/src/lang_spec.rs`](../crates/cc-parsers/src/lang_spec.rs) + `ParserRegistry` in [`cc-parsers/src/lib.rs`](../crates/cc-parsers/src/lib.rs) | `CSHARP_SPEC` |
| synthetic-edge pass | `SynthesisPassSpec` | `registry()` in [`cc-index/src/dispatch_synthesis/mod.rs`](../crates/cc-index/src/dispatch_synthesis/mod.rs) | [`event_emitter.rs`](../crates/cc-index/src/dispatch_synthesis/event_emitter.rs) |
| postprocess skip gate | `PassGate` | [`cc-index/src/pass_gate.rs`](../crates/cc-index/src/pass_gate.rs), consumed by `phase_postprocess_compute` / `phase_analysis_compute` (`indexer_phases.rs`) | `DbSignatureGate` |
| multi-statement write | `UnitOfWork` | [`cc-db/src/unit_of_work.rs`](../crates/cc-db/src/unit_of_work.rs), entered via `IndexDb::writes().begin_unit_of_work()` | dispatch-synthesis apply in [`cc-index/src/synthesis_pipeline.rs`](../crates/cc-index/src/synthesis_pipeline.rs) |

- **Retrieval lane** — implement `RetrievalLane` for a unit struct and append
  it to the `default_lanes()` vec. Order is the deterministic RRF fusion
  order; no `plan.rs` / `engine.rs` edits needed.
- **Preselect layer** — implement `PreselectLayer` and append to
  `default_preselect_layers()`. Order is execution order: the fallback gate
  reads the scores of earlier layers, and graph-neighbor seeds off everything
  before it.
- **Framework route resolver** — create
  `cc-index/src/framework_resolvers/<framework>.rs`, implement
  `FrameworkResolver`, and add one `registry.register(...)` line in
  `default_registry()`.
- **Framework detection signal** — add a submodule under
  `framework_registry/` exporting a `SPEC: FrameworkSignalSpec` (id + detect
  fn) and list it in `signal_registry()` (same declarative style as
  `SynthesisPassSpec`). Order is execution order: import markers run first so
  the `require()` fallback sees the same scan's declared-import detections.
  `detect_file_frameworks_conn` is orchestration-only — it iterates the
  registry, aggregates, and persists. Repo-level signals (package manifests)
  and activation literals live in their own submodules outside the per-file
  registry.
- **Language without a tree-sitter grammar** — declare a
  `static <LANG>_SPEC: LangSpec` in `lang_spec.rs` (language, grammar-name
  tag, extensions, qualified-name separator), then wire a `SpecDrivenParser`
  field and a `match` arm into `ParserRegistry` in `cc-parsers/src/lib.rs`.
- **Synthetic-edge pass** — add a submodule under `dispatch_synthesis/`
  exporting a `SPEC: SynthesisPassSpec` (id, signature gate, owned call kinds
  / semantic prefixes, compute fn) and list it in `registry()`. Execution
  order matters: interface dispatch runs last.
- **Postprocess skip gate** — implement the `PassGate` trait (or reuse
  `DbSignatureGate` / `FileSignatureGate` / `StringCacheGate` / `PairGate`)
  and consult it from the pass's compute stage in `indexer_phases.rs`; the
  gate's `DeferredSignatureRecord` travels with the pass's typed delta and is
  persisted in the apply stage only after the pass's writes landed.
- **Multi-statement write** — `UnitOfWork` is the seam itself, not a trait to
  implement: add a typed write method on `UnitOfWork` instead of exposing the
  raw connection. Callers obtain one via `IndexDb::writes().begin_unit_of_work()`;
  commit bumps `index_epoch` exactly once, drop without commit rolls back.
