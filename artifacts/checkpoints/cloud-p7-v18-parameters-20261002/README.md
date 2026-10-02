# P7-014 / V18 frozen public-parameter subitem

Fixed product baseline: `codex/cloud-p7-lifecycle-fences@7481731429118f45b6144518e8e9cfad3e02d7b4` (PR27). Test source commit: `30eb750dd4172be67e20ab61bf2ba74f5cfd0a8b`. Sole new test file: `crates/cc-server/tests/p7_v18_parameter_contract.rs`. This block does not change production, old acceptance/fault tests, Cargo/CI, lifecycle or shared tasks/TODO. D1/D2 unchanged. Each actual product child has an isolated synthetic Python project and semantic cache root.

Authority read: `tasks.json` P7-014 (in_progress; unconfigured/disabled/backfilling/failed/ready must reflect reality); `06-VALIDATION.md` V18 (14 tools preserved, old mode unchanged, new parameter schema/sanitize/dispatch/status chain, default no key/network); `MCP_TOOLS.md` public `retrieval_strategy` contract and distinct SDK schema-vs-sanitize error forms; production `SearchParams`, `ContextParams`, `StatusParams`, query policy and capability status projections. The fault owner covers failed/degraded/local-auto-explicit result combinations separately; no fault matrix is added here.

All successful calls use the **built product binary over real JSON-RPC stdio**. The tests read actual `tools/list` schema, send public arguments through the real SDK/schema deserializer and sanitizer, and assert production query policy/lane receipts and status outputs. No manually constructed internal receipt or injected query outcome stands in for this chain.

Coverage:

- Actual 14-name tool inventory, search/context optional nullable strategy fields, required query/task, unknown-field rejection, advertised hybrid/top_k defaults; omitted search uses local project default and corresponding status.
- Invalid string strategies/mode/symbol+auto/semantic/status aspect produce sanitizer JSON-RPC `-32602`. Non-string strategies/top_k/status aspect, misspelled/unknown fields and provider endpoint argument injection produce SDK `CallToolResult.is_error=true`, no success structure, deserialization diagnostic. These are intentionally different documented representations. Rejected parameters leave actual generation and empty index unchanged.
- Symbol mode retains the original root array for omitted/null/local, even with non-local project default. top_k=0 is sanitized to 1 and the expected real `needle` symbol remains.
- Search and context omitted/null inherit project auto; explicit local/auto overrides survive dispatch and appear in actual requested/effective policy. Explicit context uses unified retrieval instead of silently taking the legacy direct-symbol shortcut. Per-request overrides do not mutate project-default status.
- Enabled but not network-opted-in project: actual wired query policy admits semantic/auto/local and inherited/null requests, with executed semantic lane receipt only for non-local policies. Since no worker has activated a space, honest public status is `port_attached_unverified` / disabled / `semantic_no_active_space`; queries/status create no provider contact or fake ready state.
- semantic-http healthy **loopback only** worker: first HTTP response is held, actual status is backfilling/partial with pending work; after release it becomes ready, published=desired>0, pending=failed=0. Search/context strategies still propagate. status(all) and status(capabilities) agree on configuration/publication fields. query_coverage stays not_measured/per_query_not_global. The mock binds 127.0.0.1, validates peer/path/model/encoding, returns declared-model two-dimensional synthetic vectors and uses only a public dummy authorization marker; no real credential/reference/source endpoint is read.

Canonical evidence: `matrix/summary.json`, per-run `receipt.json`, individual `raw/*.json` public records and `audit.json`. Independent hash verification covered every per-run raw/log digest. Twenty fresh-suite runs pass: default 5 × 4, semantic 5 × 5, semantic-http 10 × 6 = **105 passed / 0 failed / 0 ignored**. Forty actual schema records, 240 SDK schema-error records, 200 sanitizer RPC errors, 721 successful public calls and ten loopback summaries are retained. Ten loopback runs made 20 synthetic embedding HTTP calls, zero query HTTP calls, zero real provider calls. Strict `cargo clippy -p cc-server --test p7_v18_parameter_contract --features semantic-http --no-deps --locked --offline -- -D warnings`, per-file rustfmt and source whitespace checks pass. Old matrices were not executed as new coverage.

**No nonempty semantic public-query success is claimed.** At the frozen baseline, the query path intentionally does not encode a cache miss inline. The 160 actual `query_vector_not_encoded` lane observations are the correct current default, not an original-default contract violation. Published document readiness does not prove a particular query's coverage. A new separately opted-in query encoding feature is being implemented by the designated owners; its wiring and nonempty public result are **pending acceptance**. Existing `network_opt_in` is not expanded here. No new field name is guessed. After the owner fixes the interface, add default-false/invalid-value/gate-chain tests in this owned new file against the new exact combination SHA.

Preliminary scaffolding failures are retained under `preliminary/`, and earlier uncommitted positive debug traces under `http/` and `final-http/` are not counted in the canonical twenty-run acceptance. Corrections came from the actual current definitions: index requires public path, SDK deserialization is a tool error, enabled/no-network has no active space yet, output packing may project verbose policy obligations away (executed lane receipts remain), and mock HTTP envelope must match the adapter's documented model/encoding contract. These were test-fixture/expectation corrections; no production fix was made or concealed.

Checklist recommendation: **V18 existing public parameter propagation subitem passed at frozen SHA**, including enabled configuration and honest backfill/publication status. Keep formal P7-014 in_progress pending its full dependency set, independent review, separate fault contract evidence and the new controlled query-encoding acceptance. Do not mark complete semantic retrieval, real provider quality, V19, or the whole P7-014 done.

Reproduce from repository root at the exact test source commit (with the workspace Rust env):

```sh
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_BUILD_JOBS=5
export CARGO_INCREMENTAL=0
cargo test -p cc-server --test p7_v18_parameter_contract --locked --offline -- --nocapture
cargo test -p cc-server --test p7_v18_parameter_contract --features semantic --locked --offline -- --nocapture
cargo test -p cc-server --test p7_v18_parameter_contract --features semantic-http --locked --offline -- --nocapture
cargo clippy -p cc-server --test p7_v18_parameter_contract --features semantic-http --no-deps --locked --offline -- -D warnings
rustfmt --check --edition 2021 crates/cc-server/tests/p7_v18_parameter_contract.rs
```

Set `P7_V18_EVIDENCE_DIR` to a new output directory to retain separate JSON records. Feature runs must be sequential because Cargo's product binary output path is shared. `verify.py` reproduces the twenty-run matrix only in a fresh evidence directory: it refuses an existing matrix and never refreshes or replaces old raw evidence. Product/test/source binary SHA256 identities and exact commands are bound in each receipt. This is contract testing, not semantic quality or performance benchmarking.
