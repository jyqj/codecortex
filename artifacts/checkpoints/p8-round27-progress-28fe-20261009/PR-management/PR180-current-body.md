## 当前整合解决的问题

全量快照与 oracle 路径存在逐行派发、重复容器构建，以及没有输入时仍构建全量查找表的开销。本次在已审 #180 基础上精确加入 #181 的两处准备逻辑，保留 FTS、依赖批写、resolver 与统计实现；源码变更尚不代表测得提速。

## 当前固定版本

| 对象 | 固定提交 |
| --- | --- |
| 产品 P | `b13ba8c1b738bc9c24b81cad1e88711fc9734283` |
| 独立审查 R | `2ccf89dd6c6f558bdfa5d8ddfa7ddf8958c7aa89` |
| 最终验证绑定 C | `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` |

C tree 为 `0e655b4258afd9ca040dd7c1ace868bf1c322a30`。完整域为 **1,092 个产品输入 / 52 个历史 BASE 差异 / 139 个验证输入**。两个独立重建得到完全相同的摘要映射；原 192 条任务定义及历史 acceptance subgates 保持逐字相同。

[本次完整独立审查与原始证据](https://github.com/jyqj/codecortex/tree/3ffcefc3b28ee1a4ed80caecebd7208a45c3e302/artifacts/checkpoints/p8-pr180-integration-a217-20261009)。P 相对上一 E6 仅修改五个路径；R 仅新增 69 个审查/证据文件；C 仅更新原 registry 和原 guard 的四个固定引用。

## 此轮行为变化

- **空配置 token：** 完整扫描、签名计算与 typed config 过滤照常执行；没有剩余 token 时省去 symbol/file catalog 复制和查找表构建。完整重建、旧 config-link 替换、时间戳、缓存/签名、generation 与 typed module resolution 保持原路径。
- **无 interface dispatch 前提：** 保留 CALL 解码、已有 CALL overlay、全部 symbol 解码和有条件的 implements 解码，只在前提为空时省去后续查找表。旧合成边清理、真实 CALL、错误顺序、fanout cap 和正常分派均保留。两份新增测试文件共十项控制与模块一起导入。
- **保留 FTS 重建：** snapshot 仅选择 #181 的两处准备逻辑，完整保留 `write_file_data_for_rebuild`；没有整份覆盖 donor 的旧 writer。E6 的九条已审 DB/oracle/ready 路径及其原边界逐字不变。
- **恢复原调度：** `measure.max-parallel` 从 E6 的 20 恢复为原来的 **10**。预检仍为 5，五个 rep0 全通过才进入其余 145 片；N30、150 片/1500 样本、5h native/350min job、全部输出/dirty/resume 预算、seed 与 15 表 parity 不变。

## 实际验证与尚待结果

**本地原版 v15 已实际通过**：2026-10-09 09:02:55 UTC，精确 C3ff 的命令 `python3 -B scripts/verify_reviewed_source_v15.py --source-version p8-completion-source-20261009-v15` 退出 0，验证 1,092 个产品输入并实际执行原 v14 历史证明。首次执行因 sparse checkout 缺少历史保护文件退出 1；随后只从固定 Git 对象补齐 186 个缺失原文件，全部 236 个保护输入相同，原验证程序和源码没有改动，首次失败日志保留。

[原版来源验证的完整执行收据（首次缺文件失败、精确补齐及实际通过）](https://github.com/jyqj/codecortex/tree/c6a5d443738bde8574a5c81c091391294b26e340/artifacts/checkpoints/p8-round25-a217-20261009)。这些补充记录单独保存，当前候选固定为 C3ff；后续只处理实际验证揭示的故障。

**当前 C 的 Rust 格式、编译、完整回归、MSRV、security 和各原工作流仍待实际 CI 结果。** 原普通 workspace 命令覆盖六项 DB/oracle 和十项 interface/config 新控制；原子 ready 的实际回归属于原 P7 closeout，不能用普通 CI 编译代替。没有在本地冒称运行 Rust。

保留 #181 原 workspace `101`（cc-eval 59 passed / 1 failed / 5 ignored，`sampler.rs:830:17: live child snapshot unavailable`）。C3ff 的 sampler 仍为原 blob `3808397a…`，本次没有修复或解释该失败。原 Mac fresh-helper 250ms 失败、旧规模失败及其他 unknown 均保留原身份；prepared-helper 边界和别的 CI 成功不会改写它们。

#179 C4 的原主 CI 已成功，四个 resolver 控制实际执行；原完整日志和终态独审已保存。#173 原 8 份 runtime/lifecycle/recovery ZIP、8-cell cold/stdio 与 gates 的限定验收也已保存。所有这些结果均保留原 source/run/attempt，**不给 C3ff 记实测通过**。

## 原研究与任务关账

- [#173 原研究 37896198208](https://github.com/jyqj/codecortex/actions/runs/37896198208)：固定 `275e8799…`，当前独立接受 **4/150 片、41/1500 样本**，100k 原预检仍在运行。
- [#178 原研究 37902429727](https://github.com/jyqj/codecortex/actions/runs/37902429727)：固定 `c8be5afa…`，原 helper 独立接受 **3/150 片、32/1500 样本**；50k/100k 原预检仍在运行。它于 08:01:31 UTC 已由外部提交，本次没有另开重复矩阵。

两次研究完全分开，不能合并样本或为当前 C3ff 改写来源。原 ZIP 独立保留；公开 runtime 审计包 v2 完整保留原测量审查材料，明确排除私有传输 capture，不声称包内包含全部 ZIP。

任务总账为 **163/192 已完成、29 剩余，本轮新增完成 0**。仍以原 P8-005 至 P8-013 和 P8-016 共十项为目标，逐项满足原验收及硬依赖后才修改状态。此 PR 不授予 G8、live-provider、质量或发布批准；完成本次实际 CI/整合后，再据精确来源处理被整合的原 PR。

<details>
<summary>上一固定 E6 的原描述（历史记录，保留其当时来源与状态）</summary>

## 为什么需要这份 PR

原规模执行中，全量快照写入和全表 oracle 对照占用了大量时间；相关路径仍有逐行 SQL 派发和重复容器分配。与此同时，原 P7 closeout 日志暴露了测试辅助程序过早发布空 PID 文件的竞态。本 PR 将已分别审查的改动精确组合，保留原语义、失败判定与研究协议。

## 当前固定版本

| 对象 | 提交 |
| --- | --- |
| 产品 P | `86a6e8eb88ddc97ee50a534452839bea25d92d16` |
| 独立审查 R | `cc73a151f7c1a47a3b05c3e49140d2e90771df46` |
| 原 v15 引用登记 G | `e6ab28bc5956e225e2015a57f601bc4d867d3732` |

G tree：`3d01f935c76a6c4da4ca4f1348804888fe08d45e`。完整清单为 **1090 产品输入 / 49 个历史 BASE 差异 / 139 验证输入**。P 保留 main `bd469335…` 的源码及两份最新平台范围说明；R 只增加 12 个审查及原始证据文件；G 只更新原 registry 和 guard 的四个固定引用，两个文件的原 `100644` 模式、guard 逻辑和 FROZEN 均保留。

[当前固定组合及独立审查](https://github.com/jyqj/codecortex/tree/e6ab28bc5956e225e2015a57f601bc4d867d3732/artifacts/checkpoints/p8-fts-resolver-ready-integration-28fe-20261009)；[前一轮 snapshot/oracle/统计整合原件](https://github.com/jyqj/codecortex/tree/e6ab28bc5956e225e2015a57f601bc4d867d3732/artifacts/checkpoints/p8-snapshot-oracle-integration-28fe-20261009)。

## 行为变化

- **全量快照依赖写入：** 对原 snapshot seam 中已经正规化的 resolution dependencies 使用有界批写，保留原校验、manifest、DELETE/INSERT 顺序、64 行 / 64 KiB 及 64/8/1 分层。普通增量仍走原路径。
- **FTS 重建窗口：** 纳入 #178 的五路径实现。仅完整重建 opt-in 在最多 256 个文件的窗口内，按实际存活 rowid 镜像 files/literal FTS；保留冲突幸存者、schema/overflow 回退、其他 FTS 触发器以及错误向上交给 staging owner 的行为。两个重叠文件经过精确组合和另一位 reviewer 的逆向字节核对，保留本 PR 的依赖批写。256 文件上限不代表全部 literal 或 RSS 上限，未声明 `journal_mode=OFF` 的物理回滚。
- **Oracle 与统计：** 复用每表投影容器，保留全部原 SQL 值转换、严格 UTF-8、JSON、排序和 15 表对照规则；纳入 #175 五个路径，由原 Rust owner 计算纳秒统计，Python 保留原 replay、摘要与原字节验证。
- **Resolver：** 纳入 #179 两个文件，减少临时 export-key HashSet、重复文件桶访问和重复路径距离扫描；仍遍历所有候选、保留全部最佳 ties 并调用原稳定排序。临时 Vec 的容量保留不保证所有输入的 RSS 都更低。
- **PID 文件修复：** 采用 #178 已发布的 `412bfb88…`，在同目录完成 write/sync/close 后 rename 发布 ready。原 15s 上限、10ms 轮询、严格 PID、3 seeds × 5 boundaries、SIGKILL 和恢复断言保持。
- **调度：** 仅将独立 measure 作业上限 10 改为 20；原 5 个 rep0 全通过后才执行其余 145 分片，全部样本、预算与失败条件保留。该调度值不保证实际 runner 配额或提速比例。

#173 reconcile 与 #174 macOS 修正、最新 SDK/平台范围说明均已保留。

## 验证状态及事实边界

源码组合、完整输入映射和实际 P/R/G 安装已经非作者独立复核。当前 G 通过正常 PR synchronize 事件进入原 CI；**当前组合的格式、编译、测试、runtime、平台和规模执行尚无通过结论**。

前一版 `ee467290…` 的 engineering 与原 failure gates 已实际成功，完整日志在本次 R 中保留。它们没有选择六个新增 resolution/oracle 单元控制；因此不将这两项 success 写成六项新控制通过，也不转给当前 G。#175、#176、#178、#179 各自原运行按其实际源码与 checkout 保留。

FTS donor 的九项控制有实际 MATCH、冲突和失败不发布结果，257 files + 257 literals 的镜像语句次数由 514 降至 4；当前组合尚无同环境 A/B 耗时或 RSS 改善证据。原 A23 100k 失联、#178 旧 `9f7…` 的 Empty/101、格式诊断失败、旧 SDK 失败及所有 unknown 均保留原身份。

## 完整研究与 PR 协调

#178 已于 08:01:31 UTC 提交固定 `c8be5afaac568ffd40ef86d3795423c3b73c9f39` 的原完整研究 [37902429727](https://github.com/jyqj/codecortex/actions/runs/37902429727)，早于本轮协调评论。当前先跟进这次既有研究的真实结果，本 PR 暂不重复提交另一套 150 分片矩阵。它的结果仅属于 c8，不作为本 G 的测量结果；本 G 的额外优化保持独立 CI 与证据身份。旧 #173 study、A23 和 G2 也各自保留。

原研究契约保持 **5 容量 × 30 次、150 分片 / 1500 复合样本、15 表 parity、seed 12648430、5h native / 350min job、512 MiB 输出、dirty 200、resume 1024**。原 CI 与研究均需实际通过后才能依各自固定版本验收；没有新增重复研究或串行等待门槛。

原任务仍为 **163/192 done、29 remaining，本次新增完成 0**。目标仍是 P8-005 至 P8-013 以及 P8-016 共十项，只有原验收、硬依赖和实际运行证据齐备后才更新任务状态。

</details>
