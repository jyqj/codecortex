# Qname association design diagnostic receipt

Base: `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`.
Toolchain: direct `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/`
executables; `rustc 1.95.0 (59807616e 2026-04-14)`.
Official repository Cargo.lock, `--locked`, normal platform network/proxy.
Owned build cache `/workspace/scratch/qname-target`; isolated index cache
`/workspace/scratch/qname-cache`. No runtime bypass or forbidden scenarios ran.

Command: `cargo test --locked -p cc-index --test qname_owner_diagnostic -p cc-server --test qname_public_diagnostic -- --nocapture`, with direct Rust 1.95
`RUSTC`/`RUSTDOC`, `CARGO_HOME=/workspace/.cargo`, the above `CARGO_TARGET_DIR`
and `CODECORTEX_CACHE_DIR`, and `CARGO_BUILD_JOBS=5`.
`CODECORTEX_QNAME_DIAGNOSTIC_OUT` saved the complete public context baseline
to `public-context-baseline.json`.

Result: 4 tests passed, zero failures/ignored/filtered; see `tests.log`.
The public context contains two method hits with original source evidence
and document identity, and neither has metadata.qname. The test independently
checks exact indexed symbol coordinates for `Alpha.needle` and `Beta.needle`.
Additional real parser tests verify decorated Python/UTF-8/CRLF and split Go
methods, and record that nested Python `inner` currently lacks its outer scope.

This is baseline diagnosis, not implementation acceptance. Schema remains 24,
module model remains 3, production code and lockfile are unchanged. Schema 25
and the association design await parent review. Correct-identity/wrong-qname
native evaluator micro gold, lifecycle/concurrency rejection tests, original
field parity and independent product review are not run. Public DEV, scorer,
frozen gold, ranking and budgets are unchanged. V19 and parent items remain open.
