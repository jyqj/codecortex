# Merge of the current owner runtime cleanup

This increment merges owner `29682890c89511dd6f477a6bf48bd969aa1537af`
into the independently reviewed supplement `8c1e943c84208bd3ddde4bb36e29448fcba14f6c`.
The fixed merged product is `d381f413bee3606bdc7410a73be855a0aab4aabb`, tree
`854202cf6b0eaf5506d2fdc9394e3cc5b7d0bbf2`, with those two ordered parents.

## Preserved behavior and evidence

The merged runtime keeps the supplement's terminal-row accounting, admission and
interrupt handling, bounded draining, partial-constructor state and all owned-writer
seal predicates. It adds the owner's explicit `parity_exit_code` report field and
its defensive handling of `ProcessLookupError` around product killing. The latter
is not presented as a newly reproduced Linux defect: current CPython already
suppresses the normal POSIX process-disappearance race internally.

The supplement's 25 runtime controls and all six additional owner lifecycle controls
are retained, for 31 runtime controls. The original owner class AST is identical;
all prior supplement class ASTs are identical. The imported owner controls explicitly
use FakeProduct with real executor threads, and remain protocol-control evidence.
They are not production workload observations.

All 56 added owner artifact paths retain their exact original Git bytes. The
execution-599 checkpoint, round6 runtime cleanup originals, original successful and
failed logs, review records and archives keep their original identities. Both prior
main review documents are copied exactly into the adjacent history directory.

## Actual combined validation

The combined source passed **387 P8 Python tests** in **86.626 seconds**. The
[original receipt](all-p8-receipt.json) records HEAD `8c1e943c` and MERGE_HEAD
`29682890`, together with separate before/after hashes for the two changed inputs.
Those bytes match the subsequently published product. The full 64,326-byte log has
SHA-256 `1e38609b91dfe68623d144e42121bbd0c6b985ff4615774daacc1a4350fb5b1b`.
Previous 381-test and owner runs remain prior-source observations.

The full staged whitespace check returned 2 because inherited original CI logs and
patch-context lines contain trailing whitespace. Its original output is preserved;
the evidence bytes were not rewritten. The two changed runtime/control paths pass
their scoped whitespace check. A later clean working-diff check must not be described
as validation of every historical log's whitespace.

The separate product review checks the actual GitHub commit/tree/ordered parents,
the 56 original artifact copies, the two source deltas, all 31 runtime methods and
the original 387 individual test results without rerunning them.

## Admission and original task scope

All 1,087 Rust/Cargo inputs are unchanged from the owner 599 candidate and the prior
supplement. The validation inventory remains 136; only two digests change. All 15
prior source-review records, eight prior incremental review records, 30 reviewed
source paths and all prior incremental history remain; two new independent records
are added for this merge. Admission pins and their actual full source proof follow
in separate commits; this directory does not preclaim that later execution.

The original roadmap remains **192 total / 163 done / 29 unfinished**. This merge
completes zero original TODOs. No prerequisite, sample population, timeout, budget,
acceptance rule, runtime observation, quality or release status is changed here.
