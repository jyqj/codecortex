## Current normal CI — 2026-10-09

The current head **`7da43a80bc00698101de5619dd486a3393a667a3`** passed [the original main check, job 113933295910](https://github.com/jyqj/codecortex/actions/runs/37963897699/job/113933295910) at **17:35:45 UTC**. The complete original log was retained and independently reviewed. Its actual PR test-merge tree exactly equals the reviewed head tree `c97fe8b57612aac6fa8eae3a7ed2b6c0fc659f1a`.

Formatting, Clippy, all-target compilation, the authorized default and subsequent regressions, and the original source/task guards passed. All six added controls actually passed once: complete public/rebuild database values; trigger order and rollback; lazy empty/missing-FTS/source-error behavior; plain/compressed chunks and short sidecars; binding cleanup after success/failure; and schema reset/reprepare. The subsequent official job results now show all three normal CI jobs successful: security completed at 18:21:34 UTC and MSRV at 18:22:38 UTC. Their newly completed logs have not yet received the separate full-log review; the detailed control claims above are supported by the retained main-check log.

**Default adoption remains on hold.** The original 80 cost records belong to `87c2274e497c3d0c9d40de4b538794318672e028` and run 37960889762. The small cases show substantial order dependence, and larger cases show only modest local differences. The ignored cost probe stayed ignored in this normal regression check. No new speed or end-to-end rebuild claim is made.

The original fmt failure and the later local guard wrapper's index-stat metadata mismatch remain preserved. The original local v15 CLI itself returned 0; the wrapper returned 1 for that metadata drift, and the original task-plan passed on its first separate execution. The new normal CI does not rewrite either earlier record.

Task ledger remains **192 total / 163 done / 29 remaining / 0 newly completed**.

<details>
<summary>Original implementation, cost observations and formatter admission</summary>

## 当前状态（2026-10-09，第39轮）

**源码格式修复已正常快进到 `7da43a80bc00698101de5619dd486a3393a667a3`；默认性能集成暂缓。**

原始成本实验 [run 37960889762 / attempt 1](https://github.com/jyqj/codecortex/actions/runs/37960889762) 已在原始 `87c2274e497c3d0c9d40de4b538794318672e028` 成功执行。完整原始 artifact `11632400464`（136,957 bytes，SHA256 `d8ca0b56c96e8bda9ad108216e352a30edb78696853e25619563539cd6c81196`）和完整 job 日志已保留。[完整原件、恢复工具、原日志与80轮统计](https://github.com/jyqj/codecortex/tree/24e85e6ed5eaccc82b52e02e05953d20b1eea65b/artifacts/benchmarks/original-custody-chunk87c-37960889762-a217-20261009) 已保留。原始流/观察器/全部1096个源输入逐项一致；实际唯一 cc-db release libtest、4×20完整交替配对、1 passed / 0 failed；各配对原始/新 typed-row摘要与base/FTS计数一致。

| 案例 | 配对 retained/original 耗时比中位数 | retained 更快的配对 |
|---|---:|---:|
| single_plain | 1.073403 | 9/20 |
| short_plain_6 | 0.930771 | 10/20 |
| mixed_32 | 0.988064 | 14/20 |
| compressed_64 | 0.985219 | 17/20 |

小案例明显受执行顺序影响；大案例在这一次进程内仅有约1%–1.5%的局部改善。这些结果不足以支持默认启用或宣称完整rebuild提速。保留全部80个原始轮次，不剔除前轮、不加新的性能阈值、不重跑原实验。该有限观察不计入原TODO或规模样本。

当前修复链为 `87c → P107f4d → Rbb6fe1 → G7da43`：只应用原fmt失败实际给出的四处排版修改（两个文件，其中一处移除可选尾逗号），不修改工作流、生产语义、测试断言或成本协议。当前head的普通CI重新排队；原始六个普通Rust controls不能从那个单独ignored probe获得信用。

原v15仅执行一次，实际exit0；外层收据因评审期间git status刷新index stat元数据而exit1，原失败完整保留。全部源码字节、mode、HEAD、24,193项staged mode/OID均未变；独立原task-plan随后首次执行exit0。没有重跑v15或把外层失败改记成功。

**原始成本身份仍是87c/run37960889762/attempt1；未再次添加成本标签。当前163完成、29剩余，本PR新增原始TODO完成数0。**

<details>
<summary>原始创建与协议说明（保留历史）</summary>

## Purpose and behavior

Full rebuilds currently reacquire the same cached chunk INSERT statements for every row. This candidate retains the base-table and FTS statements for one rebuild-owned file, preparing each lazily when its first row reaches it.

The sequence remains source JSON / compression selection → base INSERT → last_insert_rowid → FTS INSERT for every chunk. Each execution clears bindings on success and failure; handles are released at the original file boundary or error. The ordinary public path, statement-cache capacity, transaction scope, SQLite pragmas and row values retain their original behavior. No chunk batching or FTS deferral is added.

## Fixed source and independent review

The PR uses the fixed M7F review base `base/p8-M7F-a217-20261009` / `fffd950d5b34b0180f308db1428a57d5bf358bf1`. The later #180 crash-ready fix remains a separate change.

| Role | Fixed commit |
| --- | --- |
| Product: two Rust paths and one cost workflow | [de37a7b4](https://github.com/jyqj/codecortex/commit/de37a7b412784bd4defff03acafae9bfc4f573a8) |
| Independent review: 17 retained records | [7d555379](https://github.com/jyqj/codecortex/commit/7d555379c7c5fd7fd89e9432fb9acb056dda597f) |
| Guard: original registry and four bindings | [87c2274e](https://github.com/jyqj/codecortex/commit/87c2274e497c3d0c9d40de4b538794318672e028) |

G tree is `3399514bf070cbdc4af9e6737c0171bde7d508ba`. [The fixed reviews](https://github.com/jyqj/codecortex/tree/7d555379c7c5fd7fd89e9432fb9acb056dda597f/artifacts/checkpoints/p8-rebuild-chunk-source-a217-20261009) bind 1,096 product inputs, 72 original-BASE differences and 143 validation inputs. Source and workflow authors were cross-reviewed; the original verifier algorithm is unchanged apart from its four identity constants.

## Actual validation

The original v15 CLI actually exited 0 once on G (285.92844661 s), followed by the original task-plan CLI exit 0. All 1,877 materialized inputs, modes, HEAD and index remained unchanged. The final handoff SHA256 is `612741f2f9ba96e304db40bf2f1d8fa09df3837151cf1021be27827a8000f63b`.

Twenty-two workflow/output protocol controls passed in Python. Six new ordinary Rust controls cover plain/compressed/source/short-sidecar rows, lazy/error prefixes, constraint/trigger/rollback order, binding cleanup, schema changes, and complete public-versus-rebuild database values. Their actual Rust execution, compilation and rustfmt are pending in ordinary CI; static review is not recorded as a runtime pass.

## Independent finite cost observation

The distinct `p8-rebuild-chunk-cost-run` label starts one 90-minute-bounded observer, including release compilation. It uses the exact ignored `cc-db` / `cc_db` test with `features=[]`, one actual Cargo-produced executable and a real one-test result. Four cases (1, 6, 32 and 64 chunks) run 20 alternating paired rounds each. All 80 slots, four base/FTS counts, exact typed-row digests, original nanosecond strings, source/toolchain identities and complete original streams must be retained.

The timing excludes begin/readback/rollback and includes the unchanged compression choice and both inserts. There is no speed threshold, and finite mechanism results will not be counted as full-build improvement or original scale acceptance. This Draft PR starts ordinary CI; the cost label will be applied once after creation. No native cost result is yet claimed.

Original ledger: **192 total / 163 done / 29 remaining / 0 newly completed**. Original study identities, populations, budgets, task definitions and dependencies remain unchanged.

</details>

</details>
