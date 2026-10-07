# Optional-work packing before hit metadata projection

## Result and fixed source

This checkpoint preserves the original CI failure, an unchanged-source reproduction, explicitly separate temporary instrumentation, the minimal production repair, and its authorized validation results. The original BM25 public oracle now passes unchanged. The original packing boundary also retains its exact two-hit omission assertion. Broader local default validation is **not all green**: four failures are retained below, with their actual observed limits.

The repaired full product source is commit `65eb87d70bd7bfd10d251b5d1cbb25850958196b`, tree `661734886f514b31b14ae7601410079fbf58c559`, based on PR143 head `7ee6ed0584356c6f5491452b41033724aaa296a9`. Its complete `crates/`, `Cargo.toml`, and `Cargo.lock` manifest contains 768 inputs. `repaired-source-sha256.json` has SHA256 `487c8d0ec38378c8974725c43fd3f30946ba5bf4ed335aec947990df1125a9fe`; the same manifest is bound before and after the full validation. Only these two source paths differ from the base:

- `crates/cc-search/src/selection/budget.rs`
- `crates/cc-eval/tests/packing_validation_work.rs`

The original P7 oracle file SHA256 remains `5408b09c0723181a4f3d325376a2410f4acbcdb5a65085f83a457a995bf7df9e`. The original packing boundary SHA256 remains `552066132a2ab67ba1e9d260fa8a483085756749e57623513bac366c38382942`. No Cargo, old oracle, workflow, source registry, task state, or prior checkpoint was modified.

## Original failure and executable provenance

GitHub Actions run [37629938552](https://github.com/jyqj/codecortex/actions/runs/37629938552), check job `112821255708`, failed `public_bm25_contribution_matches_independent_sql_order_and_is_nonzero` in `cc-server --test p7_v05_v11_independent_review`. Original line 441 unwraps `stage_a_layer_scores.as_array()`: the hit survived, but the array was missing. The complete target reported 5 passed / 1 failed.

`remote-ci-original.log` is the original decoded 426,890-byte log, including BOM, ANSI sequences and newlines; its SHA256 is `b5dac8828ce00611c65e66d8719f94acb5e275d2799840ecbfa319081fabb889`. The local unchanged-source reproduction also returned exit 101 with the same 5/1 result. `baseline-command.json` records the command, environment, full source map and both executable hashes.

The oracle launches `env!(CARGO_BIN_EXE_codecortex)`. Cargo's executable records identify the actual current-worktree `cc-server` package and show `fresh=false` for both original stdio and test binaries. The original stdio SHA256 is `f135bb283effb33481da58efb4006f45566bc71525d29b02547ea2ef2b320480`; the repaired default binary is `0b16decc7d8b60ef282e52c17a3ab6d7c3fafe449665babc969340725a7260b5`. The final `semantic-http` command separately records all six real executable artifacts (stdio plus five tests), their features, package source paths, `fresh=false` and SHA256. Its stdio SHA256 is `f8b42077e99efa137686e7923832ff0ef92255a41fe8448b82fece04edb4ee1e`.

## Measured mechanism

Temporary instrumentation copied the test without changing the original oracle and observed cloned values at existing packing stages. Its 769-input source map, copied diagnostic source and binary SHA are explicitly separate in `instrumentation/`; these are diagnostic evidence only. The temporary integration test and instrumentation were removed before repaired validation, restoring the product to 768 inputs.

| Instrumented stage | Actual bytes | Trial without complete `validation_work`, with omission marker | Fixed cap |
| --- | ---: | ---: | ---: |
| First entry | 18,630 | 18,188 | 16,000 |
| After duplicate rendering | 16,020 | 15,578 | 16,000 |
| After hit metadata projection | 11,163 | 10,721 | 16,000 |
| Dispatch entry | 11,362 | 10,920 | 16,000 |

At the second stage, both real `stage_a_layer_scores` arrays still exist. The strong and weak FTS-summary contributions are `2.111459058335428` and `2.0816024387832215`. Projecting metadata removes them and makes the response fit, so the previous late body-loop probe never has an opportunity to preserve them. The independent SQL oracle and repaired public contributions are retained in the raw trace and summarized in `diagnosis.json`; this is a contribution-order check, not a final reranking or semantic-quality claim.

The repair extracts the existing rollback-safe whole-receipt omission trial into one helper and also calls it after duplicate rendering yields, immediately before lane/hit metadata projection. The original late body-loop call remains. A successful trial measures the complete output including `details_omitted=true`. A failed trial restores the entire receipt and the marker's original value or absence. Initial envelope and previous-packing validation still run before the new branch. No special score allowlist, changed cap, fabricated zero work, or reconstruction of already deleted metadata is involved.

## Narrow checks and broader validation

The repaired source passed the unchanged original P7 target (6/0), unchanged original packing boundary (3/0), two existing packing controls plus the new early-stage control (3/0), and workspace format check. The new control starts from a real indexed context with full lanes and hit metadata, lets duplicate rendering yield first, applies exact synthetic pressure at the unchanged 16,000-byte cap, and asserts preservation of complete hits/retrieval, source proof, whole-wire accounting, prior Partial state and idempotence. Existing controls preserve exact receipt/marker restoration on unsuccessful trials and ensure omission cannot revive on later expansion.

All following commands used Rust 1.95.0, `--offline --locked -j2`, debug info disabled, incremental compilation disabled, and an exclusive target directory. `continued-validation.json` records every command, target result and original log hash; `validation-summary.json` provides the compact interpretation.

| Authorized continuation command | Target summaries | Passed | Failed | Ignored | Exit | Seconds |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| workspace-no-fail-fast | 166 | 2247 | 4 | 63 | 101 | 242.285 |
| semantic-lib | 1 | 232 | 0 | 0 | 0 | 34.884 |
| semantic-normal-paths | 11 | 83 | 0 | 1 | 0 | 15.710 |
| semantic-http-p7 | 5 | 13 | 0 | 0 | 0 | 105.116 |

The workspace command is the existing default CI scope with `--no-fail-fast` added so the known local sampler failure does not suppress the remaining targets. The semantic library and 11 normal targets are exactly the default CI list. The five explicitly authorized HTTP targets are `p7_v05_all_lane_scope`, `p7_v05_scope_public`, `p7_v16_exact_oracle`, `p7_v16_oracle_independent_review`, and `p7_acceptance_matrix`; all 13 tests pass. No ignored, historical process-fault/GC-WAL, or `semantic_runtime` targets were enabled.

The 39-test default subset consisting of original P7 6, original boundary 3, packing controls 3, validation-work 5, Python capture 11, and Python revalidation 11 passes inside the complete run. These are subset counts, not added tests. The 2,575 successful executions in the four continuation commands are not a unique-logical-case count: feature configurations may rerun same-named cases. The earlier failed default attempt (336 passed / 1 failed / 8 ignored), isolated sampler diagnosis and narrow 12-pass run are preserved separately and are not added to continuation totals.

## Four retained default failures

1. **Live-child process sampling:** the unchanged sampler test fails both within the default run and in isolation at `sampler.rs:536`, reporting no live snapshot. Its READY child executes, but the returned PID resolves in `/proc` to `caas-prefix-tra`, with 71 RSS pages, rather than the intended 16 MiB allocation/CPU child. Raw observations and executable SHA are in `sampler-proc-observation.json`. This establishes that local PID attribution cannot be certified; the exact internal Rust `None` branch was not instrumented.
2. **Original index performance threshold:** continuation reports warm p95 `818.34ms` against the original `500ms` limit. The identical source passed this test in the earlier default attempt. The threshold, fixture and failing result remain unchanged.
3. **Commit-storm timing:** the unchanged `engine_cache` fixture returns generation epoch 0, then observes epoch 1 after writer join. Its first writer commit is gated by an attempt-start counter but is not acknowledged before the work's 60ms sleep ends. A delayed writer can therefore miss the intended overlap. This explanation is an inference from source and output; no thread trace was collected and the failure is not converted into a pass.
4. **Unix socket setup:** the unchanged path-guard fixture's `UnixListener::bind` returns `EPERM` before the reader-under-test assertion. No runtime access controls were changed.

These findings do not certify the full default gate. They do not change the old CI evidence, source guards or published source-review pins. The independent code review is separately owned by the reviewer and does not retroactively certify these local full-run failures.

## Evidence storage

Large raw observations, diagnostic source copies and command/CI logs are stored losslessly in `raw-observations.tar.gz` with their original checkpoint-relative member paths. Small receipts and complete source manifests remain directly readable. `checkpoint-files.json` indexes original byte lengths and SHA256 for every evidence file. `STORAGE.md` explains the deterministic archive and the read-only `verify_storage.py` check. The verifier streams members without filesystem extraction or payload execution; storage success means byte integrity only.
