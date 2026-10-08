# 原始上下文证据索引

日志源固定于 final SHA；下列代码块每行前的数字是解压后原日志行号，文字未改写。压缩/解压 SHA256 与段落定位见 evidence-index.json。不是本任务新测试。

## all-features-complete-no-fail-fast

源：[原始 gzip](https://github.com/jyqj/codecortex/blob/6db4d396e5d994388ada1e97c3d28947ffdb9c81/artifacts/checkpoints/index-fix-integration-20261003/final-v24/all-features-complete-no-fail-fast.log.gz)；raw SHA256 `d56f732dab6004694650aeee140ed32cda80d705e64dc0db934b2fbbbe81e117`。

原日志 L425–L433：

```text
425: test benchmark::ablation::tests::profile_features_jobs_binding_drift_is_rejected ... ok
426: test benchmark::ablation::tests::rustflags_drift_is_rejected_by_semantic_projection ... ok
427: test benchmark::ablation::tests::shared_target_dir_across_cells_is_rejected ... ok
428: test benchmark::ablation::tests::unknown_build_option_key_fails_closed ... ok
429: test benchmark::sampler::process_probe_tests::cpu_tick_conversion_is_checked_and_preserves_units ... ok
430: test benchmark::ablation::tests::consistently_missing_required_semantic_key_is_rejected ... ok
431: test benchmark::ablation::tests::unknown_projection_kind_or_version_is_rejected ... ok
432: test benchmark::sampler::process_probe_tests::linux::invalid_units_and_rss_overflow_are_unavailable_cpu_overflow_stays_raw ... ok
433: test benchmark::sampler::process_probe_tests::linux::proc_stat_handles_parentheses_whitespace_and_documented_units ... ok
```

原日志 L454–L511：

```text
454: test tests::benchmark_real_workspace ... ignored, benchmarks the real workspace copy; run explicitly
455: test tests::corpus_load_new_format ... ok
456: test benchmark::sampler::process_probe_tests::unresponsive_optional_probe_cannot_block_or_accumulate_workers ... ok
457: test tests::incremental_latency_summary_computation ... ok
458: test tests::report_summary_computation ... ok
459: test benchmark::sampler::process_probe_tests::linux::live_child_snapshot_is_attributed_monotonic_and_disappears ... ok
460: test tests::integration_fixtures_and_corpus ... ok
461: test tests::benchmark_fixture ... FAILED
462:
463: failures:
464:
465: ---- tests::benchmark_fixture stdout ----
466: # Benchmark Results
467:
468: Generated: 2026-10-03T05:59:50.832376749+00:00
469: Dataset: fixture
470: Files: 18
471:
472: ## Per-Tool Latency
473:
474: Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).
475:
476: Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.
477:
478: | Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
479: |------|-------|----------|----------|----------|----------|----------|------------|
480: | adr | 1 | 9.79ms | 9.79ms | 1.75ms | 1.75ms | 1.75ms | 11 B |
481: | architecture | 6 | 3.75ms | 19.66ms | 3.87ms | 22.92ms | 22.92ms | 5.3 KB |
482: | context | 8 | 201.30ms | 285.11ms | 129.07ms | 256.36ms | 256.36ms | 15.4 KB |
483: | explore | 5 | 7.83ms | 10.93ms | 4.11ms | 9.15ms | 9.15ms | 2.2 KB |
484: | files | 2 | 1.40ms | 2.23ms | 373µs | 2.33ms | 2.33ms | 1.2 KB |
485: | graph_query | 9 | 3.38ms | 13.18ms | 2.28ms | 7.20ms | 7.20ms | 311 B |
486: | impact | 7 | 4.23ms | 29.58ms | 3.92ms | 12.34ms | 12.34ms | 1.6 KB |
487: | index | 1 | 702.27ms | 702.27ms | 533.62ms | 533.62ms | 533.62ms | 2.5 KB |
488: | ingest_traces | 1 | 2.51ms | 2.51ms | 1.25ms | 1.25ms | 1.25ms | 157 B |
489: | node | 6 | 6.13ms | 19.75ms | 2.46ms | 4.57ms | 4.57ms | 886 B |
490: | relations | 8 | 3.85ms | 18.97ms | 2.23ms | 3.22ms | 3.22ms | 811 B |
491: | search | 28 | 6.99ms | 211.41ms | 2.42ms | 95.02ms | 147.00ms | 4.0 KB |
492: | status | 1 | 12.14ms | 12.14ms | 10.52ms | 10.52ms | 10.52ms | 13.9 KB |
493: | trace | 11 | 4.59ms | 10.02ms | 1.98ms | 3.81ms | 3.81ms | 1.2 KB |
494:
495: ## Summary
496:
497: - Total cases: 94
498: - All tools under 500ms warm p95: NO
499:
500:
501: thread 'tests::benchmark_fixture' (95849) panicked at crates/cc-eval/src/lib.rs:915:13:
502: perf regression: tool 'index' warm p95 = 533.62ms (limit 500ms)
503: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
504:
505:
506: failures:
507:     tests::benchmark_fixture
508:
509: test result: FAILED. 38 passed; 1 failed; 5 ignored; 0 measured; 0 filtered out; finished in 16.24s
510:
511: error: test failed, to rerun pass `-p cc-eval --lib`
```

原日志 L1195–L1215：

```text
1195:      Running tests/semantic_lifecycle.rs (/workspace/index-fix-target-v24/debug/deps/semantic_lifecycle-5af432fe168f2d97)
1196:
1197: running 2 tests
1198: test partial_backfill_allows_real_local_query_mutation_delete_and_cancel ... ok
1199: test partial_backfill_allows_model_switch_and_new_space_progress ... FAILED
1200:
1201: failures:
1202:
1203: ---- partial_backfill_allows_model_switch_and_new_space_progress stdout ----
1204:
1205: thread 'partial_backfill_allows_model_switch_and_new_space_progress' (112136) panicked at crates/cc-eval/tests/semantic_lifecycle.rs:297:19:
1206: replacement backfill must finish and old worker physically exit: Elapsed(())
1207: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
1208:
1209:
1210: failures:
1211:     partial_backfill_allows_model_switch_and_new_space_progress
1212:
1213: test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.28s
1214:
1215: error: test failed, to rerun pass `-p cc-eval --test semantic_lifecycle`
```

原日志 L3245–L3261：

```text
3245: failures:
3246:
3247: ---- semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts stdout ----
3248:
3249: thread 'semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts' (118128) panicked at crates/cc-server/src/semantic_runtime.rs:792:9:
3250: assertion `left == right` failed: coverage=SemanticCoverage { eligible: 1100, published: 0, uncovered: 1100, failed: 0, stale: 0, reason: None }; errors=cache put failed: IO error: Read-only file system (os error 30); worker=None
3251:   left: 1100
3252:  right: 0
3253: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
3254:
3255:
3256: failures:
3257:     semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts
3258:
3259: test result: FAILED. 327 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 14.27s
3260:
3261: error: test failed, to rerun pass `-p cc-server --lib`
```

原日志 L3746–L3767：

```text
3746:      Running tests/p7_runtime_independent_review.rs (/workspace/index-fix-target-v24/debug/deps/p7_runtime_independent_review-158a04a8710afada)
3747:
3748: running 3 tests
3749: test close_after_response_before_publish_still_fences_old_worker ... FAILED
3750: test retired_worker_cannot_clear_reopened_degradation_projection ... ok
3751: test closing_one_inflight_call_does_not_charge_unstarted_documents ... ok
3752:
3753: failures:
3754:
3755: ---- close_after_response_before_publish_still_fences_old_worker stdout ----
3756:
3757: thread 'close_after_response_before_publish_still_fences_old_worker' (119976) panicked at crates/cc-server/tests/p7_runtime_independent_review.rs:268:6:
3758: durable artifact proves provider returned past its close check: Elapsed(())
3759: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
3760:
3761:
3762: failures:
3763:     close_after_response_before_publish_still_fences_old_worker
3764:
3765: test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.24s
3766:
3767: error: test failed, to rerun pass `-p cc-server --test p7_runtime_independent_review`
```

原日志 L4283–L4287：

```text
4283: error: 4 targets failed:
4284:     `-p cc-eval --lib`
4285:     `-p cc-eval --test semantic_lifecycle`
4286:     `-p cc-server --lib`
4287:     `-p cc-server --test p7_runtime_independent_review`
```

## fixture-isolated

源：[原始 gzip](https://github.com/jyqj/codecortex/blob/6db4d396e5d994388ada1e97c3d28947ffdb9c81/artifacts/checkpoints/index-fix-integration-20261003/final-v24/fixture-isolated.log.gz)；raw SHA256 `b480a7ae0674d8f15a82179988eef220f2e2a4bd142c9f0b89b886a1d5bc8afe`。

原日志 L1–L42：

```text
1:     Finished `test` profile [unoptimized] target(s) in 0.54s
2:      Running unittests src/lib.rs (/workspace/index-fix-target-v24/debug/deps/cc_eval-2e9c6535ec970510)
3:
4: running 1 test
5: # Benchmark Results
6:
7: Generated: 2026-10-03T06:06:46.063822582+00:00
8: Dataset: fixture
9: Files: 18
10:
11: ## Per-Tool Latency
12:
13: Methodology: cold = first call of a fresh MCP session per iteration (new IndexDb identity → cold graph adjacency + SQLite page caches, empty search LRUs; OS file cache retained). warm = repeated identical calls in one shared session after 1 discarded warmup (cache-hit path).
14:
15: Per case: cold = 1 fresh-session call, warm = best of 2 measured calls; percentiles aggregate across cases per tool.
16:
17: | Tool | Cases | cold p50 | cold max | warm p50 | warm p95 | warm max | Avg Output |
18: |------|-------|----------|----------|----------|----------|----------|------------|
19: | adr | 1 | 1.66ms | 1.66ms | 780µs | 780µs | 780µs | 11 B |
20: | architecture | 6 | 3.58ms | 9.65ms | 2.16ms | 6.55ms | 6.55ms | 5.3 KB |
21: | context | 8 | 113.47ms | 141.70ms | 71.81ms | 104.91ms | 104.91ms | 15.3 KB |
22: | explore | 5 | 3.95ms | 5.33ms | 3.13ms | 3.39ms | 3.39ms | 2.2 KB |
23: | files | 2 | 1.39ms | 3.52ms | 720µs | 1.56ms | 1.56ms | 1.2 KB |
24: | graph_query | 9 | 1.93ms | 5.63ms | 1.48ms | 4.44ms | 4.44ms | 311 B |
25: | impact | 7 | 3.52ms | 11.08ms | 2.81ms | 9.10ms | 9.10ms | 1.6 KB |
26: | index | 1 | 407.82ms | 407.82ms | 314.93ms | 314.93ms | 314.93ms | 2.5 KB |
27: | ingest_traces | 1 | 1.97ms | 1.97ms | 1.32ms | 1.32ms | 1.32ms | 157 B |
28: | node | 6 | 2.37ms | 3.19ms | 1.76ms | 2.64ms | 2.64ms | 886 B |
29: | relations | 8 | 1.98ms | 4.60ms | 1.60ms | 2.11ms | 2.11ms | 811 B |
30: | search | 28 | 2.57ms | 115.18ms | 1.69ms | 50.94ms | 74.89ms | 4.0 KB |
31: | status | 1 | 8.54ms | 8.54ms | 6.01ms | 6.01ms | 6.01ms | 13.9 KB |
32: | trace | 11 | 3.23ms | 3.68ms | 1.83ms | 2.75ms | 2.75ms | 1.2 KB |
33:
34: ## Summary
35:
36: - Total cases: 94
37: - All tools under 500ms warm p95: YES
38:
39: test tests::benchmark_fixture ... ok
40:
41: test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out; finished in 8.33s
42:
```

## lifecycle-isolated

源：[原始 gzip](https://github.com/jyqj/codecortex/blob/6db4d396e5d994388ada1e97c3d28947ffdb9c81/artifacts/checkpoints/index-fix-integration-20261003/final-v24/lifecycle-isolated.log.gz)；raw SHA256 `a87c68c06883148a09861dcf591295e2c6a9c08d412850f7d35b9367e1e26b14`。

原日志 L1–L22：

```text
1:    Compiling reqwest v0.12.28
2:    Compiling cc-server v1.0.0 (/workspace/codecortex/crates/cc-server)
3:    Compiling cc-eval v1.0.0 (/workspace/codecortex/crates/cc-eval)
4:     Finished `test` profile [unoptimized] target(s) in 14.62s
5:      Running tests/semantic_lifecycle.rs (/workspace/index-fix-target-v24/debug/deps/semantic_lifecycle-b098cfbe2728d0f7)
6:
7: running 2 tests
8: test partial_backfill_allows_real_local_query_mutation_delete_and_cancel ... ok
9:
10: thread 'partial_backfill_allows_model_switch_and_new_space_progress' (91804) panicked at crates/cc-eval/tests/semantic_lifecycle.rs:297:19:
11: replacement backfill must finish and old worker physically exit: Elapsed(())
12: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
13: test partial_backfill_allows_model_switch_and_new_space_progress ... FAILED
14:
15: failures:
16:
17: failures:
18:     partial_backfill_allows_model_switch_and_new_space_progress
19:
20: test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.19s
21:
22: error: test failed, to rerun pass `-p cc-eval --test semantic_lifecycle`
```

## base101-lifecycle

源：[原始 gzip](https://github.com/jyqj/codecortex/blob/6db4d396e5d994388ada1e97c3d28947ffdb9c81/artifacts/checkpoints/index-fix-integration-20261003/final-v24/base101-lifecycle.log.gz)；raw SHA256 `2213e11cd164d6e1aa1d6703fa6b556973152b27ae028dfb8aeeac2f4d09bca5`。

原日志 L239–L258：

```text
239:    Compiling cc-eval v1.0.0 (/workspace/index-fix-old/crates/cc-eval)
240:     Finished `test` profile [unoptimized] target(s) in 1m 35s
241:      Running tests/semantic_lifecycle.rs (/workspace/index-fix-target-base101-features/debug/deps/semantic_lifecycle-5af432fe168f2d97)
242:
243: running 2 tests
244: test partial_backfill_allows_real_local_query_mutation_delete_and_cancel ... ok
245:
246: thread 'partial_backfill_allows_model_switch_and_new_space_progress' (106051) panicked at crates/cc-eval/tests/semantic_lifecycle.rs:297:19:
247: replacement backfill must finish and old worker physically exit: Elapsed(())
248: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
249: test partial_backfill_allows_model_switch_and_new_space_progress ... FAILED
250:
251: failures:
252:
253: failures:
254:     partial_backfill_allows_model_switch_and_new_space_progress
255:
256: test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.34s
257:
258: error: test failed, to rerun pass `-p cc-eval --test semantic_lifecycle`
```

## all-features-first-run

源：[原始 gzip](https://github.com/jyqj/codecortex/blob/6db4d396e5d994388ada1e97c3d28947ffdb9c81/artifacts/checkpoints/index-fix-integration-20261003/final-v24/all-features-first-run.log.gz)；raw SHA256 `ff59e4f7961f4717587a30ae55d0ca7744c9e066fd9397cef2e269fa9e3a4b65`。

原日志 L1325–L1345：

```text
1325:      Running tests/semantic_lifecycle.rs (/workspace/index-fix-target-v24/debug/deps/semantic_lifecycle-5af432fe168f2d97)
1326:
1327: running 2 tests
1328: test partial_backfill_allows_real_local_query_mutation_delete_and_cancel ... ok
1329: test partial_backfill_allows_model_switch_and_new_space_progress ... FAILED
1330:
1331: failures:
1332:
1333: ---- partial_backfill_allows_model_switch_and_new_space_progress stdout ----
1334:
1335: thread 'partial_backfill_allows_model_switch_and_new_space_progress' (89093) panicked at crates/cc-eval/tests/semantic_lifecycle.rs:297:19:
1336: replacement backfill must finish and old worker physically exit: Elapsed(())
1337: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
1338:
1339:
1340: failures:
1341:     partial_backfill_allows_model_switch_and_new_space_progress
1342:
1343: test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.27s
1344:
1345: error: test failed, to rerun pass `-p cc-eval --test semantic_lifecycle`
```


## all-features 测试目标身份

原日志 L415–L419 / L2906–L2910：

```text
415:      Running unittests src/lib.rs (/workspace/index-fix-target-v24/debug/deps/cc_eval-2e9c6535ec970510)
416:
417: running 44 tests
418: test benchmark::ablation::tests::binary_and_compiler_drift_cannot_be_compared ... ok
419: test benchmark::ablation::tests::identical_options_except_target_dir_pass_with_positional_metrics ... ok
2906:      Running unittests src/lib.rs (/workspace/index-fix-target-v24/debug/deps/cc_server-0c12bb93deec97e6)
2907:
2908: running 329 tests
2909: test capability_status::tests::attached_port_with_degradation_reports_degraded_state_and_reasons ... ok
2910: test capability_status::tests::old_active_space_cannot_report_configured_new_model_ready ... ok
```
