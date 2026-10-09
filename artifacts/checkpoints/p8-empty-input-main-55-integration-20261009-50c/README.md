# PR181 round 9: integration of main #185

This archive fixes the actual product commit **31a42daeb12da6936695abb08eb3912d4f3c6064 (P4)**, tree `286cca0c09c796227dfadc030af298b4f1c4e575`, with ordered parents B4 `27e60348f01d16298dd4d28f48dd1e2d47ee687c` and main `55aa2bcf355441585bcf980e1d6f4fab8eebe59d`. It preserves #175, #179, #183 and #185, the four original empty-input Rust files, and every prior artifact from both parents. It does not claim a future guard execution.

## Composition and independent source review

`independent-source-review.json` is the fixed-source canonical review, SHA-256 `79b3830e7ddb7a23074a25ef9e6c267621d0314bb12632c9dc5c2bcff050e11e`. The independent reviewer read 1,253 immutable Git blobs (25,032,129 bytes), recomputing blob IDs and SHA-256 values: **1,092 product inputs, 139 validation inputs, 53 original-BASE differences**. The main55 49 differences and the four original P2 files close this delta exactly. Seven main55 product changes do not overlap the four P2 paths. Canonical review covers source; actual P4 engineering results are in separate records.

P4 preserves the exact union of 22,076 artifact files (21,774 shared, 200 B4-only and 102 main55-only). Its 210 overlays on main55 are precisely 200 prior artifacts, four Rust files and six combined task/view documents. Main55 changes to P8-017/018 include both evidence and notes; B4's disjoint task changes and runtime interpretation correction are retained in full. Original task definitions, status, dependencies and acceptance are unchanged.

## Actual P4 engineering results

The fixed P4 checkout was clean before execution. Rust 1.95.0 ran the following commands with locked, offline dependencies where applicable. All nine exited 0; all 1,092 product input hashes and HEAD remained identical before and after.

| Scope | Observed result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| Workspace Clippy, all targets, `-D warnings` | exit 0 |
| `cc-index --all-targets` | 23 targets, 594 passed, 0 failed, 3 existing ignored |
| `cc-search --lib` | 301 passed |
| `cc-server` installer unit selection, single thread | 49 passed |
| `cc-server --test installer_cli`, single thread | 2 passed |
| `cc-eval --lib legacy_latency_ns_tests` | 2 passed |
| `cc-eval --bin p8-runtime-statistics` | 5 passed |
| `cc-eval --lib integration_fixtures_and_corpus` | 1 passed |

The raw argv, UTC times, environment overrides, before/after manifests and nine logs are in `engineering-checks/`; the supervisor and independent engineering review are included. These selected targets sum to 954 passed executions, not 954 distinct features or original TODOs. The three ignored index cases are two benchmarks and a child-process entry point. This is not an unfiltered workspace suite or a scale, platform, provider, quality or release certification. The old P2 unfiltered workspace exit 101, original G CI and P3/G3 results retain their own source identities and limitations; none is relabeled as P4 success.

## Original plan and prior-round records

The original plan `--write` and no-argument commands both exited 0 at HEAD B4 with the prospective P4 documents. Their actual stdout/stderr and receipt are preserved under `plan-checks/`. The exact no-argument stdout is P4's `PLAN-CHECK.json`. This is not claimed as execution at a later P4 or G4 checkout. The independent P4 document review verifies those scopes and the exact resulting bytes. The two round-8 reports under `prior-round8/` bind the already-published B4; their original timestamps and identities are preserved.

The round-8 runtime-attribution correction remains authoritative. The original §9 observations are retained, but mandatory full backend queue/service or pure-lock-wait instrumentation was an overstrong earlier interpretation. G275's 768-request backfill includes read pool checkout, SQL read and writer-acquire-plus-rollback probes; the last is not pure lock wait. More granular unmeasured backend telemetry is optional diagnostic work, not an invented completion gate. Original full-scale, statistics and task dependencies still apply.

## Original TODO accounting

**192 total = 163 done + 16 in progress + 12 todo + 1 blocked. Remaining: 29. Newly fully completed this session: 0.** Rounds 1–9 each retained 29 remaining and zero new original closures. The user's request for at least ten original TODO completions remains unmet. No definition or dependency was relaxed to alter this count.

`archive-manifest.json` hashes every payload file in this directory except itself. Subsequent source binding and its actual verification logs must be published in a separate post-binding directory so this manifest remains closed.
