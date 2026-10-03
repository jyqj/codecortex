# Original source snapshots and hierarchical chunks (P4-A)

## Source-bound symbol identity (schema 25)

`chunk_symbol_identity` records format-v1 parser declaration associations in
the same file transaction as symbols, original chunks and documents. Prepare
uses the scanner-confirmed original snapshot and complete validated AST
boundaries: both declaration byte endpoints must match exactly one original
parser record, with the same file, name and kind as the chunk's existing labels.
No breadcrumb/title/name concatenation or current-disk lookup supplies qname.
The chunk must belong to the declaration body or its explicitly attached
documentation; adjacent whitespace retained by the source partition is allowed,
but a same-name neighbor's body is not. Ambiguous, missing, partial/fallback or
legacy proof yields no association. Transaction insertion additionally requires
that the exact symbol survived the actual SQL uniqueness/replacement rules.

The stored association binds source snapshot/digest, exact owner coordinates,
document key/version, symbol ID/UID/name/kind and parser qname. Cold hydration
and warm final-hit validation check this relationship with current source,
document and symbol rows on one SQLite read snapshot, within the existing query
generation fence. Contradictory persisted relationships fail closed. Valid
relationships add `metadata.qname`; absence does not trigger inferred identity.
Document rendering, source proof shape, old hit fields and ranking remain intact.

Python native qnames now preserve dotted lexical ancestry. A function nested in
a function or method remains `Function` and has no receiver; a direct class
member remains `Method`. A decorated function has one canonical wrapper symbol,
so its inner node cannot replace the declaration envelope in SQL. Other parser
languages retain their existing qname conventions. The module model stays v3;
schema 25 requires reparsing through existing rebuild-on-mismatch, not legacy
backfill or an in-place additive migration. Tests use owned caches only.

The original P4-A historical description follows. Implementation scope and
executed receipts are in
`artifacts/checkpoints/qname-source-identity-implementation-20261003/`.

`cc-model::source::SourceSnapshot` borrows original bytes, computes a versioned encoding-aware snapshot identity and a separate raw-byte BLAKE3 digest, and builds one LF-based line-offset index. Invalid UTF-8 has an opaque raw identity but is never lossily converted into source text. The current production parser interface accepts UTF-8 only. CRLF, BOM and the final newline remain part of the original slice. Byte spans are half-open; occupied line ranges are 1-based inclusive, byte columns are zero-based. An EOF point after LF does not create an extra occupied line.

`SourceStructure` owns bounded symbol/block/statement/comment coordinates copied during the existing parser task. `chunker/boundaries.rs` traverses a borrowed tree with a cursor and does not invoke a parser. The full AST is released with the existing task; no Node or Tree is retained in ParseOutcome. Traversal has 250000 visited-node, 50000-boundary and 512-depth limits. Syntax errors or truncation are explicit partial capability, not exact semantic proof. Rust/Go compact project-model extraction is a separate existing pass; a single language-parse test does not claim the entire index pipeline parses only once.

The chunker keeps small leaf symbols whole, but projects container members individually even when the containing class fits the budget, preserving method retrieval identities without duplicating the parent body. It follows nested declarations for oversized parents, retains owner breadcrumbs and signature spans, and splits large bodies at available block/statement boundaries. Exact fragments come from the original snapshot, not token reconstruction or lines().join(). The shared fallback advances within the configured line/scalar/estimated-token limits and the byte ceiling (16 KiB by default), without cutting a UTF-8 character or CRLF pair. Adjacent declaration documentation is attached only across a contiguous whitespace gap; detached comments stay separate. Consecutive sibling comment nodes form one bounded documentation unit without crossing a blank source line, declaration or parent scope; a file header is not fragmented into unrelated one-line results. A literal first Python body docstring is kept with its decorated signature when the combined prefix fits; dynamic f-/b-strings are not treated as docstrings. Arrow-function signature coordinates end at the function body rather than including its implementation. Container header breadcrumbs also carry up to 16 directly declared member names, computed once per file and capped with the entire breadcrumb at 1024 UTF-8 bytes; this is metadata, never reconstructed source or query-specific text. P4-B adds same-domain small-fragment merging, configurable combined budgets and a separate model-input renderer; see [CHUNK_POLICY.md](CHUNK_POLICY.md). Broader quality ablations remain P4-D work.

Vue/Svelte uses one byte-position-preserving script projection: non-script bytes become spaces, CR/LF are retained and script bytes stay at their original offsets. Chunk text is then sliced from the original component, never from the projection. Template structure remains explicitly unmodeled. Non-AST parsers use labelled heuristic/line fallback; their confidence is not upgraded.

Each real `ChunkRecord.source` contains its snapshot identity, raw byte span, slice digest, boundary method, owner and signature coordinates. `chunks.source_json` stores this optional evidence through scalar and batched plain/zstd writes. Synthetic/test/legacy records may have null evidence and must not be described as verified original source. The fields are orthogonal to old `chunk:<path>:<index>` IDs; Search hydration now validates the stored slice digest and exposes indexed-source proof in hit metadata.source_evidence, an additive public projection needed to verify mid-line chunks. The benchmark byte verifier (public-v3, retained by v4) checks the entire fixture's raw identity, exact slice and independently counted line coordinates; malformed proof is rejected without legacy fallback. Prior products without byte metadata retain the unchanged normalized-line compatibility verifier. P4-C adds index-local document keys/versions, a current manifest and bounded public disk verification; see [DOCUMENTS.md](DOCUMENTS.md). Asynchronous/vector publication and a global snapshot remain outside this batch. No claim of durable storage of an entire file's syntax boundary tree is made; that tree is transient parse output. Existing files rows supply indexed path and observed mtime/size; these observations do not enter content identity.

The parse phase checks that retained or reread text matches the scanner's content hash before accepting the result. A mismatch becomes a parse error and is not accepted as new text under an old digest; this is not an atomic filesystem snapshot or a new automatic retry scheduler.

Current schema 22 adds semantic manifest/outbox/space objects in place to the adjacent schema 21. The v21 upgrade preserves legacy schema objects, rows, FTS and incarnation; schema 20 and earlier still require an isolated rebuild to establish the v21 chunk/document evidence. Original source evidence remains separate from that policy. Do not clear a developer's daily index for migration testing. The module model remains version 3. Input and MCP contracts are retained; whole-query freshness, release quality, general encoding support, peak RSS and 100k certification are not implied.

当前 search envelope 的代际保护在 `QueryHandle::search_in_context_with` 的 `with_stable_generation` callback 内：hydrate、selection、packing 与 `verifier.finish()` 使用同一 accepted generation。dispatch 的 `finalize_search_response` 从 envelope 提取该 generation，并通过 `attach_observed` 注解序列化窗口的 freshness；不再在 fence 接受后做旧的单次硬检查。检查意图仍是拒绝混合代际结果，但 fence 接受后的变化是显式 freshness 事实，错误语义并非旧 helper 的逐字等价。CI guard 按具体 production 函数体检查该调用链，排除注释占位，并由 `ci_generation_guard_contract.rs` 的真实异步查询/重建以及 engine generation-fence 回归提供行为证据；它不是全程序静态分析。

Primary API references checked 2026-09-28: Rust str documentation (https://doc.rust-lang.org/std/primitive.str.html#method.lines), tree-sitter Node byte positions (https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.Node.html) and basic parsing (https://tree-sitter.github.io/tree-sitter/using-parsers/2-basic-parsing.html). Repository lockfiles and actual test receipts, not current online package versions, identify the executed code.
