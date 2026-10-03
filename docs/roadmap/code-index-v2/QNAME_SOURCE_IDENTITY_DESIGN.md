# Source-bound qname association: proposed design, awaiting review

Base: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` (PR 131 integration head).
This change set contains diagnostic tests and a design only. Production remains
schema 24 / module model 3. It does not claim to fix public identity or close V19.

## Why the current indexed relation is insufficient

Parser `SymbolRecord` contains qname, symbol ID/UID and line/byte-column
coordinates. Original `ChunkSource` contains a source snapshot, exact chunk
slice, owner byte span and signature span. The document entity key is a hash of
file path, snapshot and owner span, not a parser symbol UID. No persisted
authority binds these two records. A query-time name/line/containment lookup
would introduce a best-effort relation that the index transaction never proved.

The diagnostics use real Python/Go parsers, source bytes, full indexing and
public context output. Exact matching means both endpoints, including UTF-8
byte columns; it never means line overlap. Even exact coordinate observations
in a diagnostic are not sufficient persistence authority.

## Proposed write authority

Add a typed prepared association alongside file write units, created while
the original parser output and checked source snapshot are available. Admit
only complete, validated AST source structures. For each chunk, require its
owner to equal a declaration boundary, not a nearby or containing declaration.
Require the parser declaration envelope to equal that boundary at both byte
endpoints. Wrapped/decorated declarations need an explicit parser envelope;
do not expand arbitrary symbols by containment. Match the exact parser record
in that file. Require a nonempty bounded parser qname and symbol ID/UID. Zero
or multiple eligible parser records means omission. Do not deduplicate
ambiguous records by matching names or UIDs to manufacture uniqueness.

The source structure identity must equal the scanner-confirmed original
snapshot; the file content hash must equal its raw digest. Invalid UTF-8
boundaries, partial/fallback structures, missing owners or symbols, and
malformed proofs cannot produce associations. A parser qname that does not
preserve lexical scope requires parser correction or explicit omission before
it becomes public qualified identity. In particular, the nested Python
baseline test records `inner` with no `outer` container. Breadcrumbs cannot
repair that at hydration time.

## Proposed durable representation and version decision

Request schema **25**, retaining module model **3** and the existing
rebuild-on-mismatch rule. No adjacent additive migration or legacy backfill is
proposed. An additive table alone cannot prove identities for already-indexed
unchanged files; reparsing is needed. Use isolated owned caches for migration
tests, and never reset daily indexes.

The new `chunk_symbol_identity` table would contain:

| Field | Binding |
| --- | --- |
| chunk_id (primary key) | current chunk locator; deletion cascade plus explicit trigger where existing staging disables FKs |
| file_path | exact canonical indexed source path |
| doc_key, doc_version | exact current document manifest reference |
| snapshot_id, content_digest | original indexed source identity |
| owner_start, owner_end | exact original declaration envelope |
| symbol_id, symbol_uid | admitted parser record, with current symbol-row consistency check |
| qname | exact parser-provided qualified name |

Keep this separate from `ChunkSource` and document rendering: existing source
proof wire shape, model input, encoding key and ranking inputs stay unchanged.
The schema rebuild supplies the reparse/invalidation boundary; no persistent
cache-format bump is proposed. A compact association format tag can be stored
in the typed row if the implementation review requires it.

The association must be inserted and validated in the same real file
transaction as symbols/chunks/documents, in shared write helpers used by full
and incremental builds. Confirm each proposed symbol survives the real SQL
write semantics: an in-memory parser candidate that was replaced/dropped is
not authority. Do not introduce a second independent indexing path. Removal,
rename and replacement revoke associations atomically. Dirty-only units must
not replace a proof with a lookup derived from current disk or partial facts.

## Proposed read authority

Hydrate the association with chunk/source/document records on the same read
connection and SQLite snapshot. Check file, document reference, source identity,
owner envelope and referenced current symbol record before returning qname.
Absence is a supported omission. Corrupt or contradictory stored associations
must fail the existing strict hydration contract, rather than falling back to
name matching. Carry the admitted qname through the internal candidate row;
only then add `metadata.qname` to the public hit. Keep `SearchHit` constructors
and kind taxonomy unchanged.

The query generation fence remains authoritative for concurrent index
replacement and reopen. Warm final-hit validation must also verify the qname
association, so a cached hit cannot retain identity after deletion/rename or
generation change. Public disk/source freshness rejection remains unchanged
and does not certify a qname by itself.

## Implementation acceptance after design approval

- Real Python same-name classes, nested scopes, decorated declarations,
  oversized methods with split body chunks; real Go methods on different types.
- Owner endpoints inside UTF-8 characters, malformed source proof, hard-scope
  rejection, no symbol, ambiguous records and legacy/no-proof omission.
- Full/incremental parity, delete, rename, stale indexed snapshot versus current
  disk, reopen, cached output and concurrent generation changes.
- Complete public API output through the unmodified native evaluator with a
  separately authored micro gold: baseline missing identity scores 0, correct
  identity scores positively, wrong qname still scores 0.
- Preserve old hit fields/source bytes and score traces in the same fixture;
  no ranking, score, budget, Partial, taxonomy, public DEV or frozen gold edits.

These are pending checks, not passed receipts. Parent items/V19 remain open.
