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
    "vector_top_k": 24,
    "lexical_top_k": 24,
    "grep_top_k": 12,
    "rrf_k": 50,
    "vector_weight": 1.0,
    "lexical_weight": 1.1,
    "grep_weight": 0.8,
    "rerank_window": 40
  },
  "embeddings": {
    "provider": "none",
    "dimensions": 256
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
| `dirty_propagation_max_files` | `200` | Max files a dirty propagation may touch; beyond this, suggests a full rebuild. |
| `memory_budget_fraction` | `0.5` | RSS cap as a fraction of system memory (0.1–0.95) for parallel parsing. |
| `max_concurrent_parse` | `null` | Max parallel parse threads. `null` uses the rayon default. |
| `use_direct_writer` | `false` | Experimental: bypass the SQL parser with a direct SQLite writer on full rebuild. |
| `dispatch_synthesis` | `true` | Synthesize event emitter-to-handler edges during indexing. |
| `event_fanout_cap` | `6` | Cap on handlers matched per emit site (narrowed by receiver/same-file first). |
| `event_denylist` | `[]` | Custom event names to exclude from dispatch synthesis. Empty uses built-in defaults. |

## Search

The three retrieval lanes are fused with Reciprocal Rank Fusion (RRF). The
weights bias the fusion toward one lane or another.

| Field | Default | Meaning |
|-------|---------|---------|
| `vector_top_k` | `24` | Max candidates retrieved from the vector lane per query. |
| `lexical_top_k` | `24` | Max candidates retrieved from the FTS5 lexical lane per query. |
| `grep_top_k` | `12` | Max candidates retrieved from the regex symbol grep lane per query. |
| `rrf_k` | `50` | RRF smoothing constant `k` in `1 / (k + rank)`. Higher values flatten rank differences. |
| `vector_weight` | `1.0` | RRF weight for the semantic vector lane (no-op when vector search is disabled). |
| `lexical_weight` | `1.1` | RRF weight for the FTS5 full-text lane. |
| `grep_weight` | `0.8` | RRF weight for the regex symbol grep lane. |
| `rerank_window` | `40` | Number of fused candidates passed to the reranker. |

## Embedding Providers

The `embeddings.provider` field selects the vector lane backend. Three values are
accepted:

| Provider | Behavior |
|----------|----------|
| `none` (default) | **Circuit break.** Vector search is disabled — search uses FTS + grep fusion only, with no embedding model and no network dependency. |
| `hash` | Blake3-based sparse vector (zero network dependency, deterministic, fast). Good for symbol-name and lexical-heavy queries; limited quality ceiling for semantic / natural-language search. |
| `openai_compatible` | Calls an external OpenAI-compatible `POST /embeddings` endpoint for higher-quality semantic vectors. |

### Circuit break (`none`)

When no embedding model is configured, CodeCortex cleanly skips the vector lane
rather than producing degraded or empty-vector results:

- The query embedder returns an empty vector, so the vector lane short-circuits
  before touching the database or building any vector cache.
- RRF fusion proceeds with the FTS and grep lanes only — results are still
  returned, ranked, and reranked normally.
- `status()` reports `diagnostics.embedding_provider = "none"` and an
  `embedding_notice` explaining that vector search is disabled and how to enable
  it.

To turn vector search on, set `provider` to `hash` (local, zero-dependency) or
`openai_compatible` (external model).

### OpenAI-compatible endpoint

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

Any endpoint that accepts `POST /embeddings` with
`{ "input": [...], "model": "..." }` works: OpenAI, Ollama (`ollama serve`),
LiteLLM, vLLM, LocalAI, etc.

Run `status()` after connecting — the `diagnostics.embedding_provider` field
confirms which provider is active.

## Auto-indexing

| Field | Default | Meaning |
|-------|---------|---------|
| `enabled` | `true` | Start the `FileWatcher` and re-index incrementally on file changes. |
| `file_limit` | `50000` | Maximum files auto-indexed on first connect. |
| `idle_timeout_secs` | `60` | Idle window used by the watcher's adaptive debounce. |

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

| Variable | Effect |
|----------|--------|
| `CODECORTEX_EMBEDDINGS_PROVIDER` | `none` / `disabled`, `hash`, or `openai_compatible` |
| `CODECORTEX_EMBEDDINGS_BASE_URL` | OpenAI-compatible endpoint URL |
| `CODECORTEX_EMBEDDINGS_API_KEY` | API key for the embeddings endpoint |
| `CODECORTEX_EMBEDDINGS_MODEL` | Model name for the embeddings endpoint |
| `CODECORTEX_EMBEDDINGS_DIMENSIONS` | Embedding vector dimensions |
| `CODECORTEX_EMBEDDINGS_TIMEOUT_SECONDS` | Embedding API timeout in seconds |
| `CODECORTEX_MEMORY_BUDGET_FRACTION` | RSS memory cap as a fraction (0.1 – 0.95) |
| `CODECORTEX_DIRTY_PROPAGATION` | Enable/disable incremental dirty propagation |
| `CODECORTEX_DIRTY_PROPAGATION_MAX_FILES` | Maximum files reloaded by dirty propagation |
| `CODECORTEX_MAX_CONCURRENT_PARSE` | Cap parser worker threads |
| `CODECORTEX_RESOLVER_CACHE_SIZE` | Resolver catalog `resolve_name` LRU cache capacity (default `8192`) |
| `CODECORTEX_USE_DIRECT_WRITER` | Enable the experimental direct SQLite writer |
| `CODECORTEX_PPID_POLL_MS` | Parent-process death detection interval (0 to disable) |
| `CODECORTEX_STRICT_HASH` | `1`/`true` hashes every file during incremental scans instead of using the mtime+size fast path |
