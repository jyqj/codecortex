# Durable incremental resolution (P2-C)

This is maintenance of the local code index, not a second indexing engine, agent runtime, or background network service. The seven crates and fourteen MCP input contracts remain unchanged. Default operation is offline.

## Changes and minimum work

`indexer_phases/dirty.rs` compares the existing PublicSurface fingerprints and consumed positional identities, and derives the existing typed resolution-dependency events. `DirtyPlanExplanation` reports body, public_surface, bound_address, configuration, inventory, candidate_set, resumed and rebased reasons, parsed-file count, selected dependents and file budget. A private name insertion with no actual dependent is not promoted into an interface invalidation. Known unchanged bodies retain the prior no-promotion contract; Unknown evidence remains conservative.

`indexer_phases/reconcile.rs` orchestrates the same pure bounded closure in `dirty_closure.rs`. It does not implement a competing dependency graph. Roots and known forwarded contributions are finite sets. Facades with declared forwarding or Unknown surfaces propagate conservatively, including an import route that was previously missing; recursive hashes are not used. The per-run file budget and 16-round safety cap still apply. Full compiler module/config/conditional semantics belong to P3, and dependency-storage/compaction cost certification to P2-D/P8.

## Durable replay basis, not a truncated work list

`resolution_frontier` has one versioned row in index.sqlite3. It stores the basis epoch, original roots, propagated contributions, completed consumers, typed invalidation events, change reasons and stop reason. A limited reverse lookup is a work window, never a complete list of the remaining consumers. Retaining the original cause and a basis-scoped completion set allows later windows to find consumers beyond limit+1.

A new relevant source/config/inventory change rebases the pending work: old causes are retained, completion proofs and propagated-contribution proofs are cleared. An old provider root can itself become a consumer requiring re-resolution. Removed providers are retained as invalidation causes; no file foreign key deletes the only record of their unfinished consumers.

No-change builds and empty event scopes resume the row. Process reopening reloads it. An enabled watcher polls for resolution debt even if its native event queue is empty, and uses the same acquire-before-drain/build-gate/split-build path. Disabled or zero-budget maintenance does not create a busy background loop. A manually invoked disabled incremental build reports disabled/incomplete when dependents remain. Explicit full rebuild remains a repair path and creates a new cache without old debt.

The representation is bounded: at most 200,000 combined root/contribution/completion/event entries and 16 MiB serialized payload. Overflow or corruption is a failure requiring explicit recovery, not permission to commit source hashes while dropping invalidation. This bound is not a 100k performance certification.

## One atomic publication

`WriteOps::write_reconciled_batch` delegates to the existing incremental writer. Its IMMEDIATE transaction validates the expected index epoch and next frontier before changing files. File facts, resolved edges, frontier acknowledgement/replacement and index-epoch advancement commit together. Failed validation, stale prepare, SQL failure or rollback cannot acknowledge unfinished work. There is no separate public frontier setter.

The indexed singleton summary and generation are read in one read-connection transaction, including a read pool of one. Query-path summaries are constant size and do not decode a potentially large frontier. Full frontier loads validate the version, canonical paths, bounds and payload digest; malformed state never becomes a normal empty queue. Deleting the original source root does not delete its debt row.

## Public freshness and caches

Index reports and index status contain `resolution_freshness`. MCP search and relationship tools read a freshness summary both before and after handler execution, outside the result caches. Object responses carry the final summary. A changed generation during a query returns `changed_during_query` and a retry explanation, not a snapshot-consistency claim. Incomplete legacy array responses fail with an explicit retry instruction rather than silently changing their output shape. Byte-cap truncation preserves freshness metadata outside the preview.

The summary's scope is **observed_resolution_invalidations**. Ready means no stored unresolved invalidation debt in that scope. It does not certify unobserved filesystem changes, parser/compiler completeness, postprocess atomicity, or a whole-query MVCC snapshot. Unknown language capabilities and independent source/scan errors keep their existing reports. Pending rows remain incomplete until actual replay has exhausted their consumers; one later normal/no-op scan cannot erase the earlier failure.

## Dirty reload target audit

| Category | Cleared/rebuilt state | Preserved syntax/local state |
|---|---|---|
| Calls | target ID/path/callee UID, resolution kind/confidence/strategy | Source caller identity, call syntax/signals; parser_exact local identity only when still present |
| References | target ID/path/UID, resolution kind/confidence/strategy | Reference source position and lexical scope; verified same-file parser target |
| Routes | handler ID/UID, resolution strategy/confidence | Route syntax, parser confidence and source-local metadata |
| Semantic relations | Cross-file target UID | Source UID and relation-extraction confidence; hierarchy relations regenerated per batch |
| Dispatch sites | Persisted handler UID | Enclosing function UID, event key, handler expression and extraction confidence |
| Imports | resolved_path recomputed from current module rules | Original import syntax, alias and forwarding declaration |
| Symbols | Existing declaration inventory retained | Body unchanged; catalog/resolution results are rebuilt through the existing phases |

The FileEdgesForReresolve destructure and SemanticRelation match remain exhaustive. Dispatch handler_symbol_uid is a real stored column: the old assertion that it existed only in memory was incorrect. Its new negative regression verifies that stale handler identity is cleared. The unsupported-binding marker is syntax evidence, not a stale target: dirty reload preserves it for unchanged Python source, preventing a later weak global-name fallback from inventing a target.

## Python call truth

`python/references.rs` replaces the former regex ref/call pass. It walks actual call/identifier AST nodes, assigns one lexical owner, preserves real byte positions, and does not scan declarations, comments or string text as calls. Formatted-string expressions, multiline calls, nested functions and default-value expressions have dedicated tests. A dynamic chained invocation is retained as an unsupported site at its own argument token rather than colliding with the inner callee ID. Parameter/assignment shadowing and decorated bindings are conservative; unsupported lexical bindings cannot be upgraded by the global/type-catalog fallback. This is static syntax/binding evidence, not Python execution or complete import/type inference.

Independent truth checks are required in addition to full/incremental comparison: both builds can agree on the same wrong graph. Current P2-C oracle compares fourteen specified tables (previous eleven plus resolution_frontier, semantic_edges and dispatch_sites); target IDs, UIDs, confidence and strategies are retained. Old runners and their eleven-table evidence are not relabeled.

## Upgrade, rollback and validation

Schema 21 (document manifests/spec stamps, per-file chunk policy stamps, original chunk byte coordinates, single module resolver, retained import context/Go package sets and explicit knowledge states) is incompatible with schema 20 and earlier: the cache needs a clean isolated full rebuild, both for the durable table and to remove old regex-invented Python/JS/TS facts and obsolete call positions. P2-D verification and remaining cost bounds are detailed in [INCREMENTAL_VERIFICATION.md](INCREMENTAL_VERIFICATION.md). Never test this by clearing a developer's daily index. Rollback uses the preserved prior source/binary and a separate cache rebuild, plus the existing durable-asset export/restore procedure.

The per-batch implementation report and raw receipts identify tested source, toolchains, commands and boundaries. Unit/storage, finite-closure, actual-parser/SQLite, watcher and public-MCP evidence are separate. Passing P2-C does not complete P2/G2, M1, embedding, large-repository performance or release certification.
