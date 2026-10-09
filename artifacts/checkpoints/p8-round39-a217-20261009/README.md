# Round 39: source admission, original CI, and the ten-task packet

This append preserves 40 explicitly selected original records (4,723,774 bytes). It does not change source code, task definitions, thresholds, original studies, or task status. The ledger remains 163 completed and 29 remaining, with zero newly closed tasks.

The bundle contains:

- 21 isolated-profile source-admission records for `6e1b22b094a002620e6a656f88f29135b38bdae0`, including the original successful v15 and task-plan executions, their complete input snapshots and streams, the actual source proof, and scoped peer records. Source admission is not Rust compilation or completion of the new 1,350-slot study.
- The nine original G6B CI records selected by its frozen public manifest. Their own capture times and then-pending soak remain unchanged; later main integration does not turn that historical snapshot into an all-green claim.
- Six original ten-task packet records. The packet records cold 4/150 at its own cutoff and zero profile samples. It preserves the original hard dependencies and does not mark any task done.
- Four root operation records: Round 37 and Round 38 closeouts, the subsequent actual cold 5/150 intake and limited Chunk cost observation, and an explicit correction withdrawing unsupported precision from a previously written Oracle artifact capture timestamp. The timestamp correction does not change GitHub artifact creation time, original execution times, ZIP bytes, measurements, or statistics.

The source-admission records retain their original scope and execution identities. The newer root record supplies subsequent observations without rewriting the packet's earlier snapshot. Neither Oracle nor Chunk's finite mechanism cost result is represented as an established full-parity or rebuild speedup.

`selected-files.json` is the complete finite source/target/hash/mode list. `payload-manifest.json` binds every regular USTAR member and the archive; `readback-receipt.json` records complete byte and mode comparisons with the original files and a byte-identical second serialization. The existing canonical profile review is referenced by its immutable review commit, path, and blob rather than copied again.

The archive uses sorted regular USTAR members, original source modes, zero timestamps and owner fields, and deterministic gzip. It excludes guard views, private Git caches, raw ZIPs, private transfer references, and the 67 source files already preserved by parent `95e54f22340d0cd1a0b16e1de01ad81fc9bf5934`. Packaging verification is not an additional task acceptance requirement.

This directory is prepared as an artifact-only candidate on that exact parent. The packager does not create or move a branch reference; actual publication is recorded separately by the root agent.
