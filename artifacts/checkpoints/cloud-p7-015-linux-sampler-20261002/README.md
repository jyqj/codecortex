# P7-015 Linux sampler subitem

Baseline: latest published combination `codex/cloud-p7-integration@d6462c163abe6ca92d2ae0a2cf967b1ad2433e22`. Source commit: `9a2aa55a24fc0bcbb5009fea31e5b770060672eb`. Sole source ownership: `crates/cc-eval/src/benchmark/sampler.rs`, including tests. P7-017 tests, lifecycle, dependencies and shared checklist are unchanged. D1/D2 remain unchanged, no provider/model/network invocation.

Before: `process_snapshot(pid)` always returned unavailable on Linux. After: the existing bounded probe reads `/proc/PID/stat` once and extracts PID-specific user/system CPU ticks, thread count and RSS pages. A single process record avoids combining separate per-process reads across exit/PID reuse. The parser handles comm names containing parentheses/whitespace/newlines and checks the reported PID. RSS multiplies by `sysconf(_SC_PAGESIZE)` with overflow checking. CPU ticks convert via actual `sysconf(_SC_CLK_TCK)` and checked u128 arithmetic; conversion overflow preserves raw CPU counters and returns None for unavailable nanoseconds. Nonpositive units, malformed data, negative RSS, RSS overflow, zero threads, zombies/dead/exited processes are unavailable. Legitimate measured zero CPU remains Some(0); missing values are never fabricated zero. Kernel release and raw unit/timebase remain explicit. No dependency or public snapshot field change.

Linux units follow [proc_pid_stat(5)](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html) and [sysconf(3)](https://man7.org/linux/man-pages/man3/sysconf.3.html). RSS is a kernel estimate and the method labels it as PID-only, not a process tree, continuous peak or precise memory census. The Linux conversion uses documented ticks, while existing macOS calibrated conversion and other-platform unavailable behavior remain intact. macOS was not executed on this Linux runner.

Validation:

- Targeted sampler tests: 8 passed / 0 failed / 0 ignored. Includes five new Linux cases, two of which re-exec this exact Rust test binary as isolated children, with bounded helper lifetime and kill/reap cleanup. No external interpreter or global environment mutation.
- Twenty additional targeted repetitions: 160 passed / 0 failed / 0 ignored, all raw logs retained. A real child allocates/touches 16 MiB, snapshots identify that child PID and nonzero RSS/threads, CPU counters remain monotonic and accumulate at least 50 ms, then kill/reap causes unavailable snapshots. Disabled public process/thread/resource probes are checked in a separate child environment.
- cc-eval default-feature library: 39 passed / 0 failed / 5 pre-existing ignored. No new ignores.
- Strict `cargo clippy -p cc-eval --lib --tests --no-deps --locked --offline -- -D warnings`, per-file rustfmt and git diff whitespace checks pass.
- Regression sensitivity: temporarily restore only the previous Linux-unavailable dispatch while retaining the new tests. The actual child test fails with `live child snapshot unavailable`, exit 101, 0 passed / 1 failed. This is a counterfactual dispatch regression check, not a claim that these new tests existed at the old SHA. Byte-identical source restoration is checked; restored targeted tests pass 8/0/0.

The initial baseline command did **not run**: sandbox startup reported disk-full. Its raw diagnostic is retained as not_run, never counted as a test result. Read-only disk inspection found full overlay storage; only rebuildable Cargo `target/debug/incremental` was removed, freeing 7.3 GiB. Subsequent builds use `CARGO_INCREMENTAL=0`. Source, existing artifacts, binaries, dependency caches and `/tmp/.git` were preserved.

Checklist recommendation: attach this receipt to **P7-015 Linux CPU/RSS sampling subitem completed**. Keep the whole P7-015 task todo/pending P7-014: queue occupancy, foreground fairness under actual wired backfill, DB contention, stale publish=0, and rebuild/delete/model-switch E2E are not certified here. This patch exposes the Linux measurement API; it does not claim that the remaining lifecycle workload is scheduled or that every resource observation has been integrated into a runner. V20 full 1k–100k/C1–16 or release performance certification remains not_run.

Reproduce on Linux at the source commit (from repository root):

```sh
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_BUILD_JOBS=5
export CARGO_INCREMENTAL=0
cargo test -p cc-eval --lib benchmark::sampler::process_probe_tests --locked --offline -- --nocapture
cargo test -p cc-eval --lib --locked --offline
cargo clippy -p cc-eval --lib --tests --no-deps --locked --offline -- -D warnings
rustfmt --check --edition 2021 crates/cc-eval/src/benchmark/sampler.rs
```

All snapshots/logs are actual observations on this machine, not best-of-selected samples. receipt.json includes exact hashes, commands, host/tick/page-size identity, all repeated child readings and counts. Raw expected-negative and environmental failures remain retained. No shared tasks/TODO change, merge or deployment.
