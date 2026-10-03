# V16 hand-vector production oracle — p7-v16-2d48f26-9bdab23f-cosine-v1-20261002

Exact source `2d48f2628ae7c745fcab1a21dd784eae4582a193`, unchanged production `c8c20b5b7d416372ee06ed5e248663c48912064e`; Rust test binary `efe5e0c0ed8eff9b99f698cfed2e7514aa10c375527464af2c61b52396bfe6c2` with semantic-http. This is a warm build, not a cold build. All 379 production/Cargo fingerprints match the independently reviewed source.

Five executions passed (5/0/0), three insertion orders each, 90 hand-gold scope/cosine cases. The public QueryHandle starts cold and makes one real loopback query POST per model; later scopes and cache hits make none. Document vectors flow through the production worker. No manual cache/database priming.

Hand gold scores are 0.96, 0.6 (equal-score doc-key ties), 0.8 and orthogonal0. The excluded document scores1, strictly exceeding all allowed Rust documents: filter-after-top-k cannot satisfy top1 gold. Language/path/file intersection and Some(empty) preserve range; final hydration remains in scope despite external soft hints and a preselect limit of1. Deletion removes the best hit; old-space recall is unavailable after explicit model switch and the new spec requires a new query POST.

Classification follows 06-VALIDATION V16 L1/L2. This new block is L2 and its V05 hydration subset is L2. Repeated synthetic checks are not independent quality questions; no live model, public heldout corpus, L3 product stdio, L4 or L5 claim. Bounded-memory evidence is not run, so full V16 stays pending. Full V05 and V11 remain pending.

Manifest/queries, normalized/raw fixture receipts, metrics, runner wall samples, explicit unmeasured resources, empty failures, comparison and gate are retained. Raw build/compiler/test logs stay local with filename/hash evidence. No source or key is sent beyond synthetic loopback.
