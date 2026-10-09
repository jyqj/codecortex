# Completed source-admission followups

This finite archive preserves 67 explicitly selected original records for three completed source-admission executions. Every member retains its original bytes and mode. The container uses sorted regular USTAR entries, zero owner/time fields, no PAX headers, and gzip level 9 with an empty filename and mtime 0.

| Scope | Exact execution source | Original v15 | Original taskplan |
|---|---|---|---|
| Cold formatting repair | `10ec828beb94200ec679302c45115760ebd55ee9` | exit 0, 311.562501536 s | exit 0 |
| Oracle formatting repair | `9ddae045f6925140a4d652c5f28f64add459eff1` | exit 0, 287.174001514 s | exit 0 |
| Rebuild chunk statements | `87c2274e497c3d0c9d40de4b538794318672e028` | exit 0, 285.928446610 s | exit 0 |

The `cold/`, `oracle/`, and `chunk/` directories include the original handoffs, exact CLI wrappers and commands, complete stdout/stderr, execution receipts, captured before/after input snapshots, materialized source proofs, fixed composition/pin records and the applicable peer records. Each original snapshot retains the coverage it actually recorded; this archive does not expand an earlier snapshot's scope. Existing canonical source reviews are referenced by their immutable review commits, repository paths, SHA256 and Git blob OIDs in `payload-manifest.json`, rather than duplicated.

The separate `oracle-analysis/` member examines the original 100 finite cost records from source `eded24951c3e99693410ce959a627ab575f66bec`, run `37956976406`, attempt 1. Its per-round nanoseconds are preserved. The observed ordering and output-lifetime boundary supports holding default performance integration; allocator/cache causality and full-parity speedup remain unproved. The successful original cost observation is not relabelled as the formatting-repair execution.

The original handoffs are historical snapshots. Their unexecuted Rust/probe fields describe their own creation time. This bundle does not claim later CI or probe outcomes. In particular, source admission does not establish Rust compilation, semantic test success, performance, release approval or task completion.

Excluded are source worktrees, private Git/history/object stores, full API response envelopes, native ZIP/ELF files, private transfer captures and signed URLs. The running Profile `6e1` work is not included. No guard, taskplan or native workload was rerun to create this package.

`selected-files.json` is the explicit input list; `payload-manifest.json` records every member's source, mode, bytes, SHA256 and Git blob OID. `readback-receipt.json` records full reopening, comparison with the unchanged source files and a byte-identical second serialization by the same builder. These are packaging checks, not a new task-acceptance gate.

This candidate has parent `c7096df66308a5b805d941e66aa0c9195da0ede7`. Publication is controlled separately by root. No ref is changed by this preparation. Formal progress remains **0 newly completed, 163 done, 29 remaining**.
