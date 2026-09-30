# Chunk budgets, invalidation and model-input projection (P4-B/P4-D)

## One source budget

`cc-model::chunk_policy::ChunkPolicy` is consumed by every parser owned by `ParserRegistry`, including the JS/TS projection inside Vue/Svelte. Registry and build entry points reject invalid configurations before parsing. P4-D removes the line-only `Chunker::new` compatibility constructor: independent callers use `Chunker::from_policy`, and invalid values fail instead of being silently bounded. Production parser construction retains the complete captured policy and validates it at the public parse boundary.

Indexing fields: `chunk_line_budget` (default 80, range 1..10000), `chunk_byte_budget` (16384, range 4..1048576), `chunk_char_budget` (16384 Unicode scalar values, range 2..1048576), `chunk_token_budget` (4096 estimated units, range 1..262144), and `chunk_merge_min_bytes` (256, range 0..1048576; zero disables merging). The minimum scalar and byte limits accommodate a CRLF pair and a four-byte UTF-8 scalar. Characters are not graphemes or display cells. Estimated tokens mean ceil(UTF-8 bytes / 4), labelled `utf8-bytes-div-ceil-4-v1`; no exact tokenizer is loaded or claimed.

Every symbol, statement, header, gap and tail uses the same predicate. Effective bytes are min(byte budget, estimated token budget * 4), in addition to scalar and occupied-line limits. AST, heuristic-symbol and line fallback paths only produce `SourceStructure`; all three then enter the same `Chunker::from_structure` → `Partition` → `coalesce` implementation. The shared fallback always advances at a UTF-8/CRLF boundary. Mid-line pieces carry half-open byte ranges and public source evidence rather than claiming a complete line. Syntax-error ASTs remain explicitly partial. Fatal parse failures and unsupported encodings are errors/skips, not successful empty files. Opaque source is not decoded lossily.

## Same-domain coalescing

Partitioning retains exact source order and removes duplicate coverage even for duplicate, contained or crossing candidate intervals. Coalescing joins only adjacent pieces with identical symbol owner and structural parent domain when at least one is below the threshold and the result fits every budget. Distinct symbols, documentation units and independent branch/loop/exception constructs remain barriers, including same-parent and brace-free branches. Compact AST Control boundaries and descendant-barrier propagation prevent an expression wrapper from hiding these boundaries. It creates neither repeated parent bodies nor invented delimiters. Closing fragments remain separate when ownership or a hard budget prevents safe merging.

## Configuration and cache consistency

Each build captures indexing configuration through the existing bounded `.codecortex.json` reader, allowing a long-lived MCP session to observe chunk policy changes. Existing defaults-plus-environment and warning behavior for unreadable or malformed settings is retained. Numeric out-of-range budgets fail rather than silently reverting to default chunk sizes.

`files.chunk_policy` stores the policy fingerprint in the same transaction as that file's chunks. Scalar and batched writers include it. File-state aggregates and cache include it too (aggregate format 4); unchanged source bytes with a new chunk policy no longer take the mtime/hash skip path. A scoped event widens to an admitted-tree scan when committed policy stamps differ. A failed file retains its old stamp; successful siblings cannot acknowledge its work. Repair, no-change and restart cases use the same indexing pipeline, not a second queue.

`PreparedBuild` carries the actual policy and rejects a commit under different chunk rules; the report carries that prepared policy through later stages. `IndexReport.chunk_policy` is not a guarantee that failed files match. A config change during preparation may require the next build; this is not an atomic filesystem or whole-query snapshot. Tests use isolated databases. Schema 21 additionally versions document projections and requires an isolated rebuild from 20 and earlier; ProjectModel remains version 3.

## Source and embedding input are separate

`cc-index::documents::render::render` consumes a verified `SourceSnapshot` and `ChunkRecord`. It checks identity, exact slice, lines and owner/signature bounds, then returns a separate `EmbeddingInput`. Format version 1 includes escaped path, language, owner breadcrumb and signature metadata before untouched source. `source_range` locates original bytes inside the projection. `input_hash` hashes exactly the rendered text; `render_key` additionally includes options and source identity. Rename and signature options are explicit inputs.

Metadata truncation is bounded and explicit. The source body is complete or rendering fails with a request to rechunk. Combined input bytes and estimated tokens have independent limits. Rendered headers do not enter `chunks.text`. P4-B supplies the render API; P4-C prepares and persists its inputs with current document versions (see [DOCUMENTS.md](DOCUMENTS.md)). No provider call, scheduled embedding work or vector identity is implied; those consumers remain P6/P7.

## Evidence scope

Unit cases, SQLite full/incremental/restart checks, public MCP and development-corpus regressions are separate evidence layers. P4-D compares merge-disabled and production merge policies with identical parser/search inputs, and only reports document reduction after path/symbol/facet/source gates pass. Release cost receipts add native/ps RSS snapshots, full/no-op/single-file parse counts, manifest/model-input bytes and large-file boundary proxies. They do not certify transient peak RSS, tokenizer counts, 100k scale, production tail latency, public holdout quality or release readiness. P4-A source fidelity checks remain in force.
