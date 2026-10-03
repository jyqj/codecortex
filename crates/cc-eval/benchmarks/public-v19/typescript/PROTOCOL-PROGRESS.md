# Protocol migration and additional source-derived candidates

This supplements the preserved original first-20 README and records. Protocol resource fixed at PR65 `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6` was read through the existing permitted Git connection. No Forbidden GraphQL retry or credential/channel change occurred.

Monotonic opaque serials f0001–f0100 were reserved before later drafting. First20 aliases preserve original JSONL order and original record hashes at `269c1f8a4897e4f2a7fd4a09d5c5c611ca674835`. Their original question file hash remains `84d5e33414d22063b81a9b058db204775401e9fac2a2b1113abc73773711c4af`. Migration keeps source facts/answer alternatives/byte spans; versioned native primary/secondary grades now follow protocol conventions (principal3, remaining required facets2). This is pre-ranking author metadata revision pending independent review, not quality-driven gold tuning. Original gold/grade representation remains retained at its original identity.

Formal candidate split uses SHA256(`codecortex-public-v19-split-v1\n` + canonical global-family key), first64bits <2^62 for holdout. This replaces the preliminary quota-selected salt in the legacy block. Counts are expected75/25, never rebalanced. Every family currently has its own candidate global component because the authored obligations differ; source-equivalent/counterpart/global dedup decisions still need the designated independent reviewer. Sharing a file, codec or broad subsystem alone is not semantic equivalence. This is not a frozen global association graph.

New public block files contain dev native/compat questions only. Holdout exact byte commitments and counts appear in corpus-receipt.json. Local `.custody-blocked` preparation is deliberately ignored by Git and is **not restricted custody or access control**: this workspace is shared. Holdout authors know their drafts; access audit and independent restricted custodian are required. No untouched/accepted confirmatory holdout count is claimed. The initial five formal holdout candidates were already in public Git history, and their exposure remains explicitly recorded; deleting or relabelling them would not fix it. No new holdout text or holdout spec is staged in later public commits. An ignored workspace directory does not fix contamination and is not represented as a private store.

Per-block source manifests lock the exact admitted public SHA, file bytes/SHA256/BLAKE3 and inclusion reason, plus exclusion/license lists. Additional sources are hand-written clients, codecs, timing/cache modules, binary AST infrastructure and AST cloning/visitor/guard helpers. Their generated imports are excluded; the source snapshot is intentionally not a standalone package or clean Git checkout. Each suite labels snapshot commit=null and provenance retains the real SHA. Native answer groups and required facets preserve precise line-byte evidence; ordered typed call/constructor/data-flow/guard-passing edges are annotations, not a claim of implemented graph scoring. Compat is a separately validated ordered explicit-file projection and omits no-answer families.

Validation is source-only: no ranking, search run, provider or parameter inspection. Original validator build at PR60 is reused because production/scorer/Cargo are unchanged. `blocks/prepare_block.py` is explicit authoring; `blocks/verify_public.py` verifies saved public dev records read-only, including pinned archive bytes and actual native/compat validation. Protocol's byte-identical checker is retained under protocol-reference and emits hashes/counts/error codes only. Example reproduction after installing root requirements-validation.txt and fetching the pinned public archive:

```sh
python3 crates/cc-eval/benchmarks/public-v19/typescript/blocks/verify_public.py
# Run the original protocol checker with installed validation dependencies:
PYTHONPATH=crates/cc-eval/benchmarks/public-v19/typescript/.validation-deps \
python3 crates/cc-eval/benchmarks/public-v19/typescript/protocol-reference/check.py \
 --shard typescript:native=crates/cc-eval/benchmarks/public-v19/typescript/blocks/02/queries.native.dev.jsonl \
 --shard typescript:compat=crates/cc-eval/benchmarks/public-v19/typescript/blocks/02/queries.compat.dev.jsonl \
 --output /tmp/typescript-protocol-check.json
```

Each block is author-complete **candidate preparation**, with0 independent accepted families and0 reviewed complete blocks. Separately held/reviewed holdout and independent source review remain blocked/pending; counts must not be represented as corpus acceptance or historical306/raw/live quality recovery. The integrator alone owns the shared manifest/registry/authority ledger. Draft PR creation stays paused after GraphQL Forbidden.
