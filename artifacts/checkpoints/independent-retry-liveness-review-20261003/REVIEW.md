# PR122 固定 head 独立 retry liveness 限定复审

结论：**限定范围通过**。`cec4ad8ef47c6c3fe86cae6a35e357935b94a0ec` 在全新独立 oracle 的 18 个 runtime 场景中均完成其他 ready 文档，仅保留失败项的正常退避。7 个测试全通过，Cargo exit 0。旧源码在完全相同的 acceptance oracle 下 4 passed / 3 failed，Cargo exit 101；18 场景中 11 个违反 liveness。没有把“观察到旧缺陷”的 exit 0 当作修复通过。

仅审查 `7bc4935aeb4ff650618e0341e5f42b0e665418c6` 的 queue NeedsRetry 修复。PR121 共享 gate 独立于此任务，没有整合其源码或测试，也不声称组合验收。固定 head 的 git pull ref 经 `ls-remote` 确认为上述 SHA；不跟随之后的变更。

## 独立方法与固定身份

- 旧基线 `8c7c764ac17c5725c08ab2c22a4c2aea87ca35a2`（生产修复的父提交）。它与 PR120 记录的 `5cce6eb3af90b79c786d30f349ca6a674dfcd4a1` 的 crates / Cargo.toml / Cargo.lock 字节相同；空 diff 保存在 `old-production-equivalence.txt`。
- 从两份固定源码导出 Cargo / crates 到本 checkpoint 自有 `source/old`、`source/fixed`；各用独立 `target/old`、`target/fixed`。所有生产字节与各自固定提交相同，仅在 semantic_runtime.rs 尾部追加独立 test module，并加入本次自写的 `oracle.rs`。逐文件核对结果在 `summary.json`，原始与复制源码、binary、oracle SHA256 在各 `*-identity.json`。
- 同一 oracle SHA256：`02df053524d83057f6ec4659e586663a357a4378727a5e7a1d8412d62b0290fc`。
- fixed binary SHA256：`a42102e685f8027de99411bbe410a05d01905990cc937ffa1fa8bf55477db81d`；old binary：`69a93a542317573cd48fc8112093465583f214da40dc55eb556122466e9d8c9e`。同名 binary 位于不同 target，其路径及 features 均在 identity 中绑定，重新核对 hash 一致。
- 直接使用 `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/{cargo,rustc,rustdoc,rustfmt}`，rustc `1.95.0 (59807616e 2026-04-14)`；features 为 `semantic-http`（含 `semantic`），test unoptimized + debuginfo。没有 rustup 代理、工具链安装、HOME/RUSTUP_HOME 或权限设置修改。
- 使用真实 CodeIndex 和真实文档 manifest、typed space bootstrap / current-version enqueue、真实 SemanticRuntime.schedule / run_round_with 和 publish 链路。provider 为全新自有 in-process 实现，普通一次 ServerError 后其余成功；不发 HTTP。目标按真实 outbox task_id 排序的 digest 选择，不依赖并行 handler 进入时序。
- fixture 显式使用 `/tmp/independent-retry-liveness-review-20261003/{old,fixed}/fixtures`；cache 显式位于本 checkpoint 的 `cache/{old,fixed}`。不使用默认 cache。没有 SQL 造文档或重置 attempts / available_at；fixture 用正常 full build 后的 space bootstrap 和 typed missing enqueue 准备队列。runtime 自己完成游标扫描、认领、退避、调度和发布。
- fixed source tree 无 AGENTS.md / .agents/skills / SKILL.md；初始 `/workspace/.agents` 与 `.codex` 为空。没有可应用的仓库技能指令。

## 结果

| 范围 | 修复版 | 旧版 |
| --- | --- | --- |
| width 0 / 2 × 文档 4 / 12 / 33 × 失败项首 / 中 / 尾（18 场景） | 全部 done=N-1 / backoff=1 / ready=0 / provider_calls=N | 11 场景 ready 滞留，7 场景因失败位置或批次边界未触发滞留 |
| 33 文档单轮，width 0 / 2，失败项位置 0 / 8 / 15 / 32（8 场景） | 全部 calls=attempts=16，more=true，无遗留 claimed | 首/中失败可提前结束；例如 width0 首失败 calls=1 more=false，width2 首失败 calls=2 more=false |
| 18 次退避前普通 reschedule | 失败 digest 仅调用 1 次；该 row 的 attempts、deadline、token、error 均不变 | 相同退避检查通过；reschedule 可推进此前滞留的 ready，但不提前重试失败项 |
| runtime.close：两项真实 provider 在途，成功 / ServerError 两种返回 | 物理返回前 running=true / pin=1；完成 join 后 pin=0，无 claimed，无发布，已开始 attempts 保留为2，close 后 schedule=false | 同样通过 |
| 直接 queue NeedsRetry 与 lifecycle.close 交错（width0/2） | 停止继续认领，已 claim 返回 pending、token 清除，无 backoff；started attempts 保留 | 同样通过 |
| unstarted Cancelled（width0/2） | 认领至多1/2项，attempt退款为0，无 claimed；wrong/stale token renew、stale handback 返回false | 同样通过 |
| 普通未处理 handler Err 与受控 companion 在途 | 返回错误前等待物理 companion join；返回后无 claimed | 同样通过 |

原始 PR120 的 12 文档反例日志、identity 原样保留：一次普通 ServerError 后 width2 `calls=2 / ready=10 / unstarted=10`，idle 时 running=false / pins=0 / requested=false。原日志测试 exit0 是“旧缺陷被观测”的成功，**不是 acceptance**。本次独立 matrix 的 width2 / 12 文档 / 首项失败观测到更早的停止：`done=0 / backoff=1 / ready=11 / calls=1`；并行 companion 是否及时开始受调度影响，不将 ready=10 作为唯一复现 oracle。修复版相同场景为 `done=11 / backoff=1 / ready=0 / calls=12`。width0 相同场景旧版也 `ready=11 / calls=1`，修复版为 `ready=0 / calls=12`。

`fresh_minimal_12_document_reproduction` 是最小 runtime 实验：12 个真实索引文档、首项一次 ServerError、一次 schedule 到 idle。fixed 对 width0 和 width2 均通过；old 在 width0 的同一 assert 失败，width2 独立覆盖见完整 matrix。单独重放结果与 binary 绑定于 `*-minimal-receipt.json`，日志为 `*-minimal.log`。

## 复现

在仓库根目录：

```sh
python artifacts/checkpoints/independent-retry-liveness-review-20261003/replay.py fixed
python artifacts/checkpoints/independent-retry-liveness-review-20261003/replay.py old
```

第二条应返回 101，表示旧版不满足同一修复 acceptance。`replay.py` 使用 direct compiler、workspace cargo cache、`--locked --offline`、独立 target、显式 fixture/cache；只运行 `fresh_retry_oracle::` 过滤组。已有 binary 可通过 `minimal.py fixed` / `minimal.py old` 重放最小实验，脚本先校验 binary 和 oracle hash，old 必须非零。source / target / cache 为本 checkpoint 的可重建忽略文件，不提交 binary。

## 保留的失败与未运行

首次 offline 构建因既有 cache 缺 reqwest 索引失败，未运行测试；记录在 `fixed-initial-offline-*`。随后普通 Cargo 下载缺少的 crate 到 `/workspace/.cargo`，没有安装或更改工具链；最终两个成功编译的 acceptance 构建均 locked/offline。原锁文件已逐字节核对。

初稿 oracle 有 usize/u64 与 queue width 类型错误，保留初稿和两版 `*-initial-harness-*`。早期 fixture 未完成 full build 后的 active-space bootstrap，队列为0，不能归因于生产 liveness；相关 `*-ignored-fixture-*`、`*-no-enqueue-*`、`*-inactive-space-*` 日志和 identities 均保留。最初怀疑父目录 ignore，移到自有 /tmp 后问题仍在；typed desired-page 断言证明真实文档已生成，随后确认 full build 清掉 active space，使用正常 bootstrap 修正。全部早期失败不计入修复通过。

明确未运行：`semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts`、宽泛 `semantic_runtime::tests`、其他作者测试、真实外部 provider、heldout、100k、GC/WAL crash/kill/fault、PR121 shared gate 组合检查。没有重试任何已被拒绝的目标。没有独立穷举 incarnation / 所有 publication CAS fence 或未来退避到期自动唤醒；上述 token 结论仅来自 distinct completed tokens、错误/过期 token renew 与 handback 检查。正常取消、close 的禁止晚发布与物理 join 在本次受控交错中已验证。

修复仅恢复正常有限 drain 对 ready 的推进；`claimed==16` 的 runtime 续批条件未变。未来 backoff 自动唤醒仍不是本次验收项。没有发现本范围内的新生产反例。

## 交付边界

仅修改本 checkpoint，不改生产、CI、中央 TODO。用户已授权新分支 commit / push / draft PR；本次分支 `review/independent-retry-liveness-20261003`。

初始 GitHub PR 查询在一个 shell invocation 内对 PR122、PR120 均返回 GraphQL `Forbidden`（`github-forbidden.log`）。观察到拒绝后停止 GitHub API 对应动作；没有换身份、connector 或路线重试，因此 **draft PR 创建阻塞，未创建**。Git fetch / ls-remote 是独立已成功的 git 操作；交付分支的 commit / push / remote SHA 结果记录在 `delivery-receipt.json`。未 merge / force-push / deploy。
