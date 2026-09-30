# P0 development benchmark commands

From the repository root, build the development runner separately from the product:

```sh
cargo build -p cc-eval --features eval-http --bin cc-eval --locked
cargo build -p cc-server --bin codecortex --locked
```

The `eval-http` feature only adds the optional OCE HTTP benchmark adapter. It is not a production server dependency and it makes no requests merely by being compiled.

```sh
# Strict, read-only lock validation; repeat --suite for a directory-like suite.
cargo run -p cc-eval --bin cc-eval -- validate \
  --suite crates/cc-eval/benchmarks/manifests/p0-smoke.json \
  --suite crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json

# Real MCP subprocess; use the exact product binary associated with a build receipt.
cargo run -p cc-eval --bin cc-eval -- run --backend mcp-stdio \
  --binary target/debug/codecortex \
  --suite crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json \
  --output artifacts/benchmarks/my-new-run --profile smoke

# No new search calls: verify hashes and recompute metrics/report from retained rows.
cargo run -p cc-eval --bin cc-eval -- replay --run artifacts/benchmarks/my-new-run

# Independent full/incremental diagnostic: a known baseline mismatch returns 1.
cargo run -p cc-eval --bin cc-eval -- mutate \
  --suite crates/cc-eval/benchmarks/manifests/p0-python-api.json \
  --mutation crates/cc-eval/benchmarks/mutations/p0-python-signature.json \
  --output artifacts/benchmarks/my-new-oracle

cargo run -p cc-eval --bin cc-eval -- compare \
  --baseline artifacts/benchmarks/before --candidate artifacts/benchmarks/after \
  --gate crates/cc-eval/benchmarks/manifests/comparison-policy.json \
  --output artifacts/benchmarks/comparison.json
```

All run output directories must be new. In suite mode one invalid corpus does not hide the other outcomes, and the process still exits nonzero. `freeze --suite ...` is an explicit authoring operation only: use it after reviewing intentional source/gold changes, never as a pre-run workaround for a lock failure. The source subset uses a path/content snapshot, not a claim that the working Git repository is clean.

`schema --output <new-dir>` exports Suite, Query and Row JSON Schemas from the same strict Rust types the runner uses. Cross-field validation remains in `validation.rs`/`manifest.rs`.

`import-oce --queries <external.jsonl> --metadata <external.metadata.json> --output <new.jsonl>` accepts user-supplied external data. It checks actual counts and leaves a `.receipt.json` with source/metadata/output digests. It does not bundle or license external data. Compatibility is case-sensitive, slash-normalized Python-style wildcard behavior; overlapping expected patterns are rejected rather than depending on ambiguous match order.

`run --backend oce-http` requires `--features eval-http`, an endpoint and the credential environment variable (`OCE_API_KEY` by default). Loopback HTTP is supported for controlled tests; other endpoints require HTTPS and explicit `--allow-external`. No API secret is written into artifacts. The public adapter does not claim it can set the remote model, internal token budget, or path-prefix filter; those limits must be recorded separately before a fair cross-system comparison. This P0 delivery tests the adapter with a local HTTP stub, not a real model service.

## Evidence and limits

Exit 0 means the baseline measurement completed without integrity/availability failures, not that the engine is a high-quality retrieval system. Exit 1 means a product/gate failure; exit 2 invalid input/protocol/infrastructure; exit 3 explicit cancellation with a partial artifact prefix. Small-sample comparisons are inconclusive and nonzero. The smoke negative case intentionally exposes a current product failure.

Warmup and repetitions come from the locked suite, not hidden command defaults. `--profile performance` additionally requires a release-built eval binary, warmup and at least 200 rows. Product binary provenance must still be checked against its build receipt. Stage-boundary RSS is labelled sampled, not continuous peak RSS; external service memory stays unavailable unless independently measured. P0's source subsets and small mutations are not the P8 six-repository/100k/holdout certification.

For this Mac's SDK mismatch, use `scripts/p0-validation.py --sdkroot <installed-compatible-sdk> --target-dir <separate-target> --evidence-dir <new-output> --product-binary <exact-product>`; the script applies settings only to child processes and does not change the global SDK or Rust default. It records selected toolchain validation, not an unexecuted MSRV test.

## Controlled ablations and language slices (P1-C)

`cc-eval ablate --plan <matrix.json> --output <new-dir>` validates a complete 2^N matrix (1–4 controls) over explicitly built local MCP binaries. Each control declares one exact on/off replacement in a distinct source file. Every variant must differ from the immutable reference snapshot by exactly its declared controls; all other file SHA-256 values, compiler/options and the binary build receipt are checked before and after execution. No production feature flag restores the old scope bug. Local receipts are provenance, not cryptographic source-to-binary attestation.

The matrix binds common dataset manifests and optional `pinned_files` / `file_preselect_limit` per dataset; these request hints are identical across variants and are written in the plan and each run. Gold answers never enter requests. Each cell retains ordinary raw/normalized/scores/gate files. `ablation.json` reports only one-factor edges, per-question deltas and failed gates. A counterfactual built on P1-B is not an exact reproduction of the entire G0 product. Build artifacts are local, not a request to run models or external services. Interrupted matrices mark retained child runs cancelled.

`query-slices.json` is emitted with every run and offline replay. It groups existing per-question means by category and annotation-only `query_language` and `lexical_anchor`; source `language` keeps its original meaning. Translations retain the same query family; repetitions do not become independent quality samples. No-answer outcomes remain in the score/gate files. This adds diagnostic reporting without changing Top-1 or nDCG formulas.

## Optional process-tree sampling

`CODECORTEX_BENCH_PROCESS_PROBE=0` explicitly disables the optional external `ps` probe when host process enumeration is unavailable or stalled. Resource records identify the disabled method and leave process-tree RSS as null, not zero; native current-process RSS remains a separate optional observation. This setting does not skip retrieval, source, contract or budget checks, and is not a memory-performance certification.

With probing enabled, a 250 ms caller budget and one process-local admitted worker isolate even a stalled spawn/reap. A timed-out worker retains its slot until it exits; later samples report unavailable/busy rather than spawning unbounded workers. Native/kernel work is not forcibly terminated by this bound. Record this environment choice in every validation receipt.

## Task maintenance

After recording actual task evidence in `docs/roadmap/code-index-v2/tasks.json`:

```sh
python3 scripts/code_index_plan.py --write
python3 scripts/code_index_plan.py
```

The checker rejects duplicate IDs, cyclic/missing dependencies, unknown verification references, done tasks without evidence and Markdown drift.
