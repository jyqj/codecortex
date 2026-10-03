# Bounded parallel candidate — BLOCKED, not validated

Exact experiment base: PR114 `52a50730b58e2b59351449fc80f65771b24c26a8`, verified by GitHub connector on 2026-10-03; draft/unmerged. Original local checkout `ff458bc591b4e7e444af4464d6eef2513cdb335c` left untouched. Candidate isolated in `/workspace/bounded-parallel`, branch `perf/bounded-parallel-backfill-20261003`.

Candidate changes only queue, runtime, wiring. Fixed joined scoped worker width = 2 only for explicit existing per-project >=2, else1. One shared max_batch claim budget; claims linearize via a short admission mutex, independent lease/token and existing renew/retry/hand-back/CAS. No DB transaction spans provider or join. Runtime cursor/running/pin/factory lifetime unchanged. Shared degradation forwarded after workers join, also on drain error. Old serial consumer API retained. Candidate handler errors propagate after retry and physical joins, concurrent errors logged; provider retry stops subsequent claims. No new config/cache/DDL/FIFO SQL/versions/DEV/central TODO/direct_writer edits.

These are implementation intentions, **not verified correctness**. No synthetic tests authored/executed, no compile result, fmt, Clippy, or AB evidence; no ready/concurrency/IO/RSS claim. Source hashes and uncommitted patch retained for parent review. `git diff --check` passed (read-only).

## Hard stop / smallest environment boundary

`source /workspace/.cargo/env` adds cached Cargo shim to PATH but does not configure rustup's home. The first `cargo fmt --all` tried creating `/home/agent/.rustup` and returned explicit EROFS. The following check in the already-submitted shell command independently returned the same startup error, recorded in check.log. Neither reached Rust compilation. This was an invocation mistake; existing official cache is present at `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/`, but it was not invoked as a bypass.

User explicitly prohibits retrying a RO-refused action or changing roots. Therefore no RUSTUP_HOME/CARGO_HOME reassignment, direct-toolchain fallback, alternate fixture root, permission or credential change was attempted after refusal. No tests, build, instrumented pipeline, 100k, commit/push/PR, merge, forcepush or deployment followed. Parent must resolve whether/how validation may continue under that constraint. Automatic approval review did not reject any action; this is a real filesystem EROFS plus the user's stop rule.

TODO remains incomplete and identifies the remaining correctness/test/performance/delivery work. This branch is uncompiled and uncommitted, not a completed or performance-accepted candidate.
