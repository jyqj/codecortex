# Contributing

## Minimum Rust version

1.88 (2021 edition). This matches the current dependency floor and is enforced by
CI.

## Build

```bash
cargo build
cargo build --release    # optimized binary with thin LTO
```

## Test

```bash
cargo test                 # all crates
cargo test -p cc-model     # single crate
cargo test -p cc-eval      # evaluation suite (fixture + corpus)
```

Each crate compiles and tests in isolation thanks to the strictly downward
dependency graph — `cargo test -p cc-db` and `cargo test -p cc-index` work
without building the full workspace.

See [docs/TEST_PLAN.md](docs/TEST_PLAN.md) for the test layout and eval corpus.

## Lint and format

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Pre-commit check

No pre-commit hooks are configured. Before committing, run:

```bash
cargo fmt --all -- --check \
  && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace \
  && cargo test -p cc-eval -- integration_fixtures_and_corpus
```

For real-workspace performance regression checks:

```bash
CODECORTEX_WRITE_REAL_BENCHMARK=1 \
  cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture
```

See [docs/BENCHMARK.md](docs/BENCHMARK.md) for benchmark details.

## CLI commands

```
codecortex mcp [--project_path PATH]   Start MCP stdio server
codecortex install [--force]           Install MCP config for detected AI agents
codecortex uninstall                   Remove MCP config from all AI agents
```
