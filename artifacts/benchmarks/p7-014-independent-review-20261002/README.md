# P7-014 独立生命周期审查：三个未修复反例

最终固定生产基线 **`993a64ecd7a454b41bcab816c81d58cbc987bae2`**，来自 `codex/cloud-p7-recovery-integration`；已包含 `afc663b` reopen/attempt 去重修复。最初审查的 `2b8eb20c410ae9c3154bfba64e0bd82c98340fe0` 结果单独保存在 `baseline-2b8eb20/`；最终新 SHA 的实际重跑日志/观察在 `final-993a64e/`，两包均再次复现以下三个反例。本检查点只增加 `crates/cc-server/tests/p7_runtime_independent_review.rs` 和本目录，不修改生产、Cargo、旧测试或共享清单。

## 发现与精确交错

### 1. 关闭后仍可发布：post-response 检查与 CAS 之间存在窗口（高影响）

`semantic_runtime.rs:349` 在 provider 返回后检查 closed，随后 queue handler 在 `queue.rs:409` 调用 publisher；`publish.rs:110` 写 artifact，`publish.rs:163` 执行 DB CAS。CAS 的 incarnation/doc/space/lease fence 不包含 runtime close 身份。

独立测试用真实 CodeIndex、单文档、受控合成 provider 和 SQLite writer transaction：provider 在途时取得 `BEGIN IMMEDIATE`；释放 provider，等待 cache read-back 为 Hit，证明已通过 post-response 检查且 artifact durable；此时发布因 writer 阻塞，关闭项目，再释放 writer。实际 **published 0 → 1**，项目已关闭，最终 pin=0。未更改 outbox/manifest/task identity，writer 仅制造合法本地竞争。见 `final-993a64e/semantic/close-publish-window.json`。

修复边界应覆盖最终 manifest CAS，而不只增加另一个非原子的 closed 检查；close 与 publication 需有可线性化的退休 fence/受控提交入口。由主 owner 决定 runtime/queue/publisher/DB 的最小实现边界。本检查点不实现它。

### 2. close 消耗未开始文档的 retry budget（高影响）

`semantic_runtime.rs:293` 关闭后 resolver 返回 None；`queue.rs` 仍继续 claim 当前批次，并将缺少输入作为 NeedsRetry 交给 fenced retry。没有停止批次或不消耗次数的取消处置。

130 个真实合成 Rust 文档，第一页排队 64 个，第一笔 provider 调用受控阻塞；关闭后释放。实际只有 **1 次 provider 调用，但 16 个任务消耗 attempt_count**，其中 **15 个**错误为 `embed input unavailable for task`，未开始 provider 工作。published=0，pin=0。见 `final-993a64e/semantic/cancel-budget.json`。重复 close/replace 可耗尽这些文档的持久预算，形成不必要 terminal failure。

修复应在每次 claim/处置边界尊重取消，并保存未开始工作的次数；不能把关闭导致的 resolver None 当普通永久输入失败。已在途调用的取消计数政策可由主 owner 决定，测试只要求未开始文档不被收费/消耗次数。

### 3. retired worker 清除 reopened degradation 投影（高影响）

worker failure status 已绑定各 runtime 的 Arc，但 `semantic_wiring.rs:580` 无条件将旧 subsystem 的 ledger snapshot 写入共享 QueryServices，绕过 runtime 所有权。

旧 provider 受控阻塞；真实 CodeIndex.close/reopen；通过新 ledger 的公开 note_corrupt 注入合成 corruption event，再由真实 replacement worker 投影。新状态确实为 **degraded**；旧 worker 退出后实际改为 **backfilling**，而新 ledger 仍 degraded。没有直接设置 QueryServices 状态，也没有伪造 provider/recall 结果。见 `final-993a64e/semantic/retired-projection.json`。这是 ledger 边界故障注入，不声称执行了 cache 文件损坏发现流程。

修复应使 degradation 写入同样具有 runtime/incarnation 所有权；简单延迟/重复读 closed 无法排除 close/replacement 与写入之间的 race。

## 验证结果（故意保留正确断言的失败回归）

- 最终 `993a64e` 的 `semantic`：0 passed / **3 failed** / 0 ignored；`semantic-http` 同样 0/3/0。旧 `2b8eb20` 的两包和重复运行单独保留，不充当最终基线证据。每个测试直接断言正确行为，未 ignore、反向断言错误行为或放宽旧测试。
- targeted clippy `--features semantic-http --test p7_runtime_independent_review -- -D warnings`、独占文件 rustfmt、最终 baseline diff check 通过。
- 对 130 文档的关闭测试额外实际确认：仅有的 read-pool connection、writer transaction 和 CodeIndex write lock 在 provider 等待中可用；所有旧 worker 最终释放 pin。发布窗口用例验证的是上述特定关闭交错，不替代全生命周期资源验收。
- 静态审查：keyset cursor 按实际行推进，page=64、claim=16、finite job=64 rounds；enqueue 短事务保留同版本 pending/claimed/failed 次数；factory resolve 和本轮 provider drop 都在 spawn_blocking 内；全局 job semaphore=2，permit/pin 保留至阻塞工作退出。上述静态观察不是独立全量通过证明。

## 重放及集成

```sh
CODECORTEX_SEMANTIC_CACHE_ROOT=/tmp/p7-review-cache \
P7_REVIEW_EVIDENCE=/tmp/p7-review-evidence \
cargo test -p cc-server --features semantic \
  --test p7_runtime_independent_review --locked --offline -- --nocapture
```

将 feature 改为 `semantic-http` 可复核同一 runtime 路径；测试仍只使用进程内 fake provider，没有发出 HTTP。证据 receipt 记录工具链、精确生产文件/测试 hash、实际命令和退出码。不要将测试 checkpoint 作为绿色验收合入；主 owner 先修复，然后在其修复 SHA 重跑本 target。现有完整旧矩阵、130/1100 全量完成/reopen 组合、完整 pending/claimed/failed 重开矩阵、真实 HTTP 构造/销毁/取消边界本轮未重复执行；不会声称其独立通过。

清单建议：P7-014 继续 in_progress，登记以上三个阻塞反例及本目录；不要因旧局部回归通过而标 done。所有 fixtures 合成，不调用真实/付费 provider，不外传源码；保持 D1/D2。没有当前基线缺失 reopen 修复的问题。
