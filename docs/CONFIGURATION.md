# Configuration

Create a `.codecortex.json` in your project root to customize behavior. Every
field is optional — the defaults work for most projects.

```json
{
  "indexing": {
    "include": ["**/*.py", "**/*.ts", "**/*.go"],
    "ignore": ["**/generated/**"],
    "max_file_bytes": 512000,
    "chunk_line_budget": 80,
    "dirty_propagation": true,
    "dirty_propagation_max_files": 200,
    "memory_budget_fraction": 0.5,
    "max_concurrent_parse": null,
    "use_direct_writer": false,
    "dispatch_synthesis": true,
    "event_fanout_cap": 6,
    "event_denylist": []
  },
  "search": {
    "lexical_top_k": 24,
    "grep_top_k": 12,
    "rrf_k": 50,
    "lexical_weight": 1.1,
    "grep_weight": 0.8,
    "rerank_window": 40,
    "graph_weight": 0.6,
    "graph_top_k": 12
  },
  "ranking": {
    "graph_rerank_weight": 0.3,
    "overlap_weight": 0.35
  },
  "auto_index": {
    "enabled": true,
    "file_limit": 50000,
    "idle_timeout_secs": 60
  }
}
```

## Indexing

| Field | Default | Meaning |
|-------|---------|---------|
| `include` | `[]` | **Extends** (does not restrict) indexing. Known-language files are always indexed; `include` rescues unknown-language files that match these glob patterns. |
| `ignore` | `[]` | Glob patterns to exclude, on top of gitignore-aware discovery. |
| `max_file_bytes` | `512000` | Files larger than this are skipped. |
| `chunk_line_budget` | `80` | Maximum lines per code chunk for symbol extraction. |
| `parse_timeout_micros` | `null` | Per-file parse timeout in microseconds. `null` means no timeout. |
| `db_read_pool_size` | `null` | SQLite read connection pool size. `null` derives from repo size tier (4–12). |
| `dirty_propagation` | `true` | Re-parse dependents when a file's exports change. |
| `dirty_propagation_max_files` | `200` | Max files a dirty propagation may promote. Exceeding it degrades to a no-op (round 1; full rebuild advised) or keeps a partial closure (later rounds), surfaced as `dirty_propagation` on the index report. |
| `memory_budget_fraction` | `0.5` | RSS cap as a fraction of system memory (0.1–0.95) for parallel parsing. |
| `max_concurrent_parse` | `null` | Max parallel parse threads. `null` uses the rayon default. |
| `use_direct_writer` | `false` | Experimental: bypass the SQL parser with a direct SQLite writer on full rebuild. |
| `dispatch_synthesis` | `true` | Synthesize event emitter-to-handler edges during indexing. |
| `event_fanout_cap` | `6` | Cap on handlers matched per emit site (narrowed by receiver/same-file first). |
| `event_denylist` | `[]` | Custom event names to exclude from dispatch synthesis. Empty uses built-in defaults. |

## Search

Local retrieval runs three lanes — FTS5 full-text, regex symbol grep, and a
call-graph expansion lane — over trigram-backed preselection. Lane results are
fused with Reciprocal Rank Fusion (RRF), then reranked with file-path /
breadcrumb / recency boosts.

| Field | Default | Meaning |
|-------|---------|---------|
| `lexical_top_k` | `24` | Max candidates retrieved from the FTS5 lexical lane per query. |
| `grep_top_k` | `12` | Max candidates retrieved from the regex symbol grep lane per query. |
| `rrf_k` | `50` | RRF smoothing constant `k` in `1 / (k + rank)`. Higher values flatten rank differences. |
| `lexical_weight` | `1.1` | RRF weight for the FTS5 full-text lane. |
| `grep_weight` | `0.8` | RRF weight for the regex symbol grep lane. |
| `rerank_window` | `40` | Number of fused candidates passed to the reranker. |
| `graph_weight` | `0.6` | RRF weight for the call-graph retrieval lane (seed symbols + 1-hop call-edge expansion). `0.0` disables the lane. |
| `graph_top_k` | `12` | Max candidates contributed by the call-graph lane per query. |

## Ranking

Scoring weights for search result ranking, under a top-level `"ranking"` key.
Every field is optional — omitted fields keep the built-in defaults shown
below, which reproduce the historical hard-coded behavior exactly. Most
projects never need to touch these.

### Hit rerank weights

Bonuses folded into each hit's final `rerank_score` after RRF fusion.

| Field | Default | Meaning |
|-------|---------|---------|
| `graph_rerank_weight` | `0.3` | Weight of the graph connectivity score's contribution to the final `rerank_score` (0.0 disables). |
| `overlap_weight` | `0.35` | Weight of query-token/text overlap added to the fused score. |
| `symbol_exact_bonus` | `0.18` | Bonus when a query token exactly matches the chunk's symbol name. |
| `path_prefix_bonus` | `0.05` | Bonus when the file path starts with the requested path prefix. |
| `doc_file_bonus` | `0.08` | Bonus for project documentation files (README, docs/, ADRs). |
| `working_set_boost` | `0.22` | Bonus for files in the caller's working set (`boost_file_paths`). |
| `recent_file_boost` | `0.12` | Bonus for recently-edited files (`recent_file_paths`). |
| `pinned_context_boost` | `0.20` | Bonus for pinned context files (`pinned_file_paths`). |
| `overlay_neighbor_boost` | `0.10` | Bonus for overlay/dirty-buffer files (`overlay_file_paths`). |
| `stage_a_weight` | `0.04` | Multiplier mapping the preselect (stage-A) file score into rerank. |
| `stage_a_cap` | `0.25` | Cap on the preselect file-score contribution to rerank. |
| `dsl_name_bonus` | `0.25` | Bonus when a `name:` DSL filter matches the hit's symbol name. |

### Preselect file scoring

Per-file scores used by the file preselection stage that narrows candidates
before chunk-level search. The four context layers score
`max(floor, scale / rank)` over the caller-provided file list.

| Field | Default | Meaning |
|-------|---------|---------|
| `preselect_working_set_floor` | `2.0` | Working-set layer score floor. |
| `preselect_working_set_scale` | `5.0` | Working-set layer rank-decay scale. |
| `preselect_recent_floor` | `1.2` | Recent-files layer score floor. |
| `preselect_recent_scale` | `3.5` | Recent-files layer rank-decay scale. |
| `preselect_pinned_floor` | `2.2` | Pinned-files layer score floor. |
| `preselect_pinned_scale` | `4.0` | Pinned-files layer rank-decay scale. |
| `preselect_overlay_floor` | `1.5` | Overlay (dirty-buffer) layer score floor. |
| `preselect_overlay_scale` | `3.0` | Overlay (dirty-buffer) layer rank-decay scale. |
| `preselect_fts_base` | `1.4` | FTS summary layer: score is `base + 1 / (1 + |bm25|)`. |
| `preselect_symbol_exact_bonus` | `2.0` | Per-token bonus for an exact symbol-name match. |
| `preselect_symbol_fuzzy_bonus` | `1.2` | Per-token bonus for a substring symbol-name match. |
| `preselect_path_token_bonus` | `1.0` | Per-token bonus for a path component match. |
| `preselect_graph_neighbor_base` | `0.8` | Base score for 1-hop call-graph neighbor files (clamped to `preselect_graph_accum_cap`). |
| `preselect_graph_edge_increment` | `0.1` | Per-edge increment added on top of the graph-neighbor base score. |
| `preselect_graph_accum_cap` | `1.2` | Cap on a file's accumulated graph-neighbor score (base + increments). |
| `preselect_fallback_score` | `0.2` | Score for recently-indexed files when nothing else matched. |
| `preselect_explicit_scope_score` | `10.0` | Short-circuit score given to explicitly scoped files (`file_paths`). |

### Graph retrieval lane

Seed and expansion scores for the call-graph retrieval lane feeding RRF.

| Field | Default | Meaning |
|-------|---------|---------|
| `graph_neighbor_decay` | `0.5` | Score decay per hop from a seed symbol to its call-graph neighbors. |
| `graph_seed_exact_score` | `1.0` | Seed relevance for an exact symbol-name match. |
| `graph_seed_fuzzy_score` | `0.5` | Seed relevance for a substring symbol-name match. |

## Auto-indexing

| Field | Default | Meaning |
|-------|---------|---------|
| `enabled` | `true` | Start the `FileWatcher` and re-index incrementally on file changes. |
| `file_limit` | `50000` | Maximum files auto-indexed on first connect. |
| `idle_timeout_secs` | `60` | Idle-session eviction: after this many seconds without MCP activity, the active project's `CodeIndex` is closed (DB handles released) and transparently reopened on the next call. Not related to watcher debounce — the watcher computes its own adaptive debounce from repo size (500ms base + 100ms per 500 files, capped at 3000ms). |

## Repo size tiers

CodeCortex detects project size and adjusts output budgets automatically:

| Tier | File count | Token budget | Search `top_k` | Max output chars |
|------|-----------|--------------|----------------|------------------|
| Tiny | < 500 | 4,000 | 5 | 18,000 |
| Small | 500 – 4,999 | 6,000 | 10 | 24,000 |
| Medium | 5,000 – 24,999 | 8,000 | 15 | 32,000 |
| Large | 25,000+ | 12,000 | 20 | 38,000 |

Budgets scale per-handler (e.g. `files` allows up to 10,000 items on Large
repos, `impact` up to 80 items).

## Environment variable overrides

Environment variables take precedence over `.codecortex.json`.

### Indexing

| Variable | Default | Effect |
|----------|---------|--------|
| `CODECORTEX_MEMORY_BUDGET_FRACTION` | `0.5` (config) | RSS memory cap as a fraction, clamped to 0.1 – 0.95; overrides `indexing.memory_budget_fraction` |
| `CODECORTEX_DIRTY_PROPAGATION` | `true` (config) | Enable/disable incremental dirty propagation |
| `CODECORTEX_DIRTY_PROPAGATION_MAX_FILES` | `200` (config) | Maximum files reloaded by dirty propagation |
| `CODECORTEX_MAX_CONCURRENT_PARSE` | unset (rayon default) | Cap parser worker threads |
| `CODECORTEX_USE_DIRECT_WRITER` | `false` (config) | Enable the experimental direct SQLite writer |
| `CODECORTEX_CACHE_DIR` | unset (in-repo) | Store project index caches under this directory instead of `<project>/.codecortex`; each project gets a stable hashed subdirectory |
| `CODECORTEX_STRICT_HASH` | off | `1`/`true`/`yes` hashes every file during incremental scans instead of using the mtime+size fast path |
| `CODECORTEX_RESOLVER_CACHE_SIZE` | `8192` | Resolver catalog `resolve_name` LRU cache capacity |
| `CODECORTEX_COMMUNITY_MAX_EDGES` | `2000000` | Edge-count cap for Louvain community detection; a graph above the cap skips detection and assigns every symbol to community 0 instead of risking OOM |

### Search and graph caches

| Variable | Default | Effect |
|----------|---------|--------|
| `CODECORTEX_SEARCH_RESULT_CACHE_SIZE` | `32` | LRU capacity of the core search result cache, keyed by `(index_epoch, query_hash)` |
| `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE` | `32` | LRU capacity of the graph-aware search result cache (`search_with_graph_context`), keyed by `(index_epoch, evidence_epoch, query/limits/budget/ranking fingerprint)` |
| `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE` | `512` | LRU capacity of the decompressed chunk-text cache |
| `CODECORTEX_GRAPH_CACHE_SIZE` | `16` | Per-project slot count of the process-global graph adjacency caches (`GraphReadModel`) |
| `CODECORTEX_BRIDGE_EDGE_LIMIT` | `10000` | Max HTTP call edges / route nodes loaded when synthesizing cross-service bridge edges; hitting the cap logs a truncation warning |
| `CODECORTEX_CYPHER_FAST_PATH` | enabled | Exactly `0` disables the lazy-BFS fast path for variable-length `CALLS` traversals (ADR-0001); `graph_query` responses then report `fast_path.reason = "disabled(CODECORTEX_CYPHER_FAST_PATH=0)"` |

### Server lifecycle

| Variable | Default | Effect |
|----------|---------|--------|
| `CODECORTEX_PPID_POLL_MS` | `5000` | Parent-process death detection interval in milliseconds (`0` disables the watchdog) |

### Evaluation / benchmarks (cc-eval only)

| Variable | Default | Effect |
|----------|---------|--------|
| `CODECORTEX_WRITE_BENCHMARK` | off | `1` makes the fixture and synthetic scale benchmark tests persist their reports under `docs/benchmarks/` |
| `CODECORTEX_WRITE_REAL_BENCHMARK` | off | `1` makes the ignored real-workspace benchmark persist `docs/benchmarks/real_workspace_latest.md` |
| `CODECORTEX_BENCH_50K` | off | `1` enables the otherwise-skipped 50k-file synthetic scale benchmark (`bench_synthetic_50k`) |
