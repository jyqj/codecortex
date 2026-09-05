# Non-embedding code-index correctness round 1

## Provenance and scope

Measured production baseline: `4514630dcd26481cf6dbc2aff38824ed71ef06da`.
Measured candidate: `dc6b3b306b4bad87f53e1c59cf66e489cd87c9b8`.

[Validation and paired run](https://github.com/jyqj/codecortex/actions/runs/33982629712)
completed successfully. Its development job explicitly reports the candidate commit
it formatted, committed, and tested; the workflow trigger SHA is its preceding
patch-staging commit, not the tested implementation SHA.

[Raw observations, replayed reports, paired comparison, and test logs](https://github.com/jyqj/codecortex/actions/runs/33982629712/artifacts/9974258365)
were uploaded as `quality-development-33982629712` with 14-day retention. The raw
JSONL headers contain full configuration, source/manifest blob hashes, Rust version,
platform and working-tree provenance. This Markdown retains the measured summary;
the CI artifact is not permanent archival storage.

Both variants used the same new evaluator and authored `code-index-regression-v1`
manifest: **2 fixture repositories, 9 files, 10 tasks (7 positive, 3 negative)**.
Each task ran 3 cold-session and 3 warm-cache observations. Repetitions are not
independent tasks or repositories. Baseline instrumentation copied only the eval
module/examples/manifest, leaving production source unchanged. Separate Cargo target
directories prevent cross-checkout artifact reuse. Both measured variants used the
same debug profile and `RUSTFLAGS=-D warnings -A dead_code`; the allowance accommodates
historical unused code, not candidate acceptance (candidate clippy/tests separately
passed with all warnings denied).

## Paired task-quality observations

The following means weight the 7 positive tasks equally; each task has the same
number of observations. Negative tasks are evaluated separately.

| Metric | Baseline | Candidate |
|---|---:|---:|
| Recall@5 | 0.642857 | 1.000000 |
| MRR | 0.714286 | 0.904762 |
| Novelty nDCG@5 (`maximum-new-label-gain-per-hit-v1`) | 0.659021 | 0.928571 |
| Declared evidence-group sufficiency | 0.571429 | 1.000000 |
| Correct abstention on 3 negative tasks | 1.000000 | 1.000000 |
| Tool/schema errors | 0 | 0 |
| Failing task scenarios | 3 | 0 |

Baseline failures were `preselect-rescue`, `graph-rescue`, and `split-symbol-graph`,
each failing all six observations. The baseline runner's exit status 1 represents
these quality-gate failures, not a build or startup failure.

The separate comparison script reports **repository-macro differences**, which
have a different weighting: Recall@5 +0.312500, RR +0.166667, novelty nDCG@5
+0.235857. No inferential confidence interval is reported for authored regression
fixtures. These results validate the targeted fixes, not unseen-repository
semantic retrieval or agent task completion.

## Measured latency tradeoff

Debug profile, small fixtures, hosted Ubuntu 24.04 x86-64, rustc 1.98.1. Session
initialization is excluded; OS cache is retained. All individual measured requests
are included, not best-of-two minima.

| Mode / statistic | Baseline | Candidate |
|---|---:|---:|
| Cold-session p50 | 7.758 ms | 8.534 ms |
| Cold-session p95 | 12.466 ms | 12.854 ms |
| Warm-cache p50 | 1.070 ms | 1.598 ms |
| Warm-cache p95 | 2.743 ms | 5.133 ms |

Each mode has 30 observations. Retrieval rescue can return additional source and
graph evidence, so this is not a byte-identical-output microbenchmark. The measured
latency increased; no performance-speedup claim follows from this round. Larger
repositories and workloads remain necessary to characterize the new scope and
conservative-invalidation costs.

## Mechanical validation

At the measured candidate:

- `cargo fmt --all` and `git diff --check` succeeded before publication.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `cargo test --workspace --all-targets --locked`: **1326 passed, 0 failed,
  16 ignored**, summed over all test binaries (not the obsolete documentation total).
- Dedicated TypeScript/Python/Rust incremental oracle: **3/3 passed**, each retaining
  a single live session through signature edit and deletion and comparing against
  independent full builds. Initial resolved-call assertions prevent vacuous success.
- Existing MCP corpus integration passed; new quality gates passed all 60 measured
  observations; raw replay matched the original JSON report byte-for-byte.
- Python paired-comparison protocol tests: **7/7 passed**.

Final PR documentation and CI integration may have a later commit SHA. This report
is pinned to the measured implementation rather than relabeling historical results
with an unmeasured SHA. PR CI validates the final merge candidate separately.

See [Benchmark v2](../BENCHMARK_V2.md) for contracts and limitations. This round adds
no embedding model, vector database, remote code transmission, or dependencies.
