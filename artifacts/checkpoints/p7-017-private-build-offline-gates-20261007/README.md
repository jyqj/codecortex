# P7-017 private builds and isolated offline gates — 2026-10-07

Both actual products and their original test runners built successfully from fixed source `a213a4cfab0e0e87198bc279acacda51e4ff62cf`. Both executions of the unmodified isolated gate failed while reading the reported child PID under `/proc`. **This checkpoint does not accept P7-017.** No task status is changed.

## Actual results

| Product profile | Copied product SHA-256 | Product build exit | Runner build exit | Original isolated gate |
|---|---|---:|---:|---|
| default | `08a651297e39ee066ca9c016e80781deaed5f866e3aedc7846795d378cd3a695` | 0 | 0 | 101; 0 passed / 1 failed |
| semantic | `84122fdb41ca247e8eb05046915c04b2a794327b562e8f4975d24a6c11f065ed` | 0 | 0 | 101; 0 passed / 1 failed |

The complete crate/Cargo input manifest has 768 entries and SHA-256 `487c8d0ec38378c8974725c43fd3f30946ba5bf4ed335aec947990df1125a9fe`. Every execution records the same source before and after. The original `benchmark_adapters.rs` SHA-256 remains `0d47366fa35cec4a48c44175e3da6cdcbbcbf5f2bda39a780bedef4c8064fcbe`. The builder, helper, Python controls and original test hashes are in `source-bindings.json`; the Python controls were not reexecuted by this checkpoint.

Each product build starts with a distinct absent `cargo-target` and sets both `CARGO_TARGET_DIR` and `CARGO_BUILD_BUILD_DIR` to that private path. Product Cargo artifacts report `fresh=false`, exact selected feature lists (`[]` or `[semantic]`), dev/debug0 profile, immutable source and actual compiler invocation. Each original test runner was compiled in its product's private directory and reports `fresh=false`; default runner Cargo features are `[default, eval-http]`, and semantic-product runner Cargo features are `[default]`, matching the original CI command shapes. Copied runner and product hashes were checked immediately before and after each real gate invocation. No shared workspace target was used for these builds.

## Original gate and observed failure

The actual executable was the compiler-selected copy of `benchmark_adapters`, filtered to `p7_offline::real_stdio_default_disabled_semantic_contract --ignored --exact --nocapture`. The source fixture covers ordinary local configuration and explicit `semantic.enabled=false`, executing all 14 old tools and checking source, errors, cache absence and reopen/persistence when it reaches completion. Both present runs aborted at the initial child `network_status(pid)` call (`benchmark_adapters.rs:305:79`, ENOENT). The product had initialized MCP, but the gate had not reached the tool assertions or emitted complete case observations. Those later checks are **not claimed as passed**.

The separate Python visibility probe records `getpid()=5` with `/proc/self/status` outer `Pid=775017` and `NSpid=775017 5`; its child reports inner PID 6 but outer PID 775029. This demonstrates the observed PID-namespace/proc visibility mismatch. It does not substitute another PID, edit the original gate, mount or rewrite `/proc`, or infer an acceptance result. No unrestricted rerun replaced either failure.

## Network scope

The saved `deny_network_exec.py` applies an inherited process-local seccomp filter, closes any inherited socket descriptors and refuses socket standard streams. Each actual wrapper execution separately observes IPv4/IPv6 `EPERM`, repeats those probes in an exec child, observes `NoNewPrivs=1`/`Seccomp=2`/two filters, and confirms no socket descriptors before exec. The unmodified product spawn still uses `env_clear` and fixture-only HOME/XDG paths with no key.

These are network-denial preconditions and actual failure observations. They do not count product socket attempts, prove the unfinished 14-tool matrix, or certify system-wide isolation. Any separate CI run with `network_scope=not_isolated_not_claimed` remains a distinct local behavior receipt.

## Evidence and cleanup

`validation-summary.json` lists all six actual commands, their raw-log hashes, product and runner identities, scope boundaries and failures. The archive contains every included original run file unchanged: build/source receipts, raw Cargo and gate output, per-exec network probes, independent proc visibility observations and the exact scratch capture/cleanup helpers.

Only owned private Cargo intermediates were removed after recording each terminal gate result. Copied executable and raw-evidence hashes were checked during cleanup. A later reappearance of the default target is documented separately; this checkpoint does not claim those paths remained absent. The four large copied executable bodies and disposable build caches are excluded from Git. Rebuilding from the recorded commit can reproduce the procedure; matching product hashes is not promised.

Run `python3 verify_storage.py` to verify the fixed archive and direct original files without extraction or execution. Storage validation is separate from the failed behavioral gates.
