# 第 28 轮：诊断源码已准入，原任务仍未关闭

本轮原始 TODO 新增完成 **0 项**。原任务表共 **192 项，163 项完成，29 项未完成**；用户要求至少新增完成 10 项的目标尚未达到。第 29 轮继续实际 CI、诊断观测和原研究的工件验收。原定义、依赖、旧子门及预算没有修改。

## 已完成的具体工作

新增 opt-in `p8-db-lock-observation` 功能，记录实际 DB writer mutex、pool read lock 和连接 checkout 的等待与结果，并区分 workload 和 observer。原 `09-BENCHMARK.md` 第 9 节已要求 C1/4/8/16 混合负载的 DB lock wait；这次实现补足该现有观测缺口。

产品提交为 [48be21414e3e04cb542615157eef25fbffae4a17](https://github.com/jyqj/codecortex/commit/48be21414e3e04cb542615157eef25fbffae4a17)，独立审查提交为 [57c5c4bf9d7bf9732df584d9893e4ced43dc4c72](https://github.com/jyqj/codecortex/commit/57c5c4bf9d7bf9732df584d9893e4ced43dc4c72)，守卫提交为 [18255c53b7fa153bb71c96f57f1b97ca139e427f](https://github.com/jyqj/codecortex/commit/18255c53b7fa153bb71c96f57f1b97ca139e427f)。

实际 Git 复核确认：

- 产品层仅 15 条变更路径：9 条产品、5 条验证、1 条文档。
- 审查层仅增加 69 份证据文件。
- 守卫层仅修改原 2 个守卫文件，Python 守卫除 4 个固定身份赋值外逐字不变。
- 完整产品域为 1,094 个输入，完整验证域为 142 个输入；历史产品差异为 60 个输入。
- 根审查实际重读 86 个新增/变更 Git blob，并验证既有历史文件。独立审查的读取范围见其单独报告，不把两者范围混写。

原 v15 命令于 2026-10-09 10:39:50–10:43:30 UTC 实际执行，耗时 220.634 秒，退出 0，stderr 为空，验证前后源码身份和输入摘要相同。原命令及结果见 [source-admission.json](source-admission.json)、[原 stdout](source-admission.stdout.json)、[根组成证明](root-composition-review.json) 和 [独立 LG 审查](independent-LG-review.json)。

第一次工作树准备因 `--no-checkout` 后尚需显式 `read-tree` 而停止，完整准备失败记录保留在归档中。之后只把固定 Git 树写入工作区，再执行原 v15；没有改源码或守卫来使验收通过。

## 诊断 CI 的实际状态

正常创建一次专用分支 `task/p8-db-lock-observation-a217-20261009`，读回 ref 与 LG 相同。[工作流 37919399759](https://github.com/jyqj/codecortex/actions/runs/37919399759) 于 10:44:02 UTC 由 `create` 触发，attempt 1。初始 admission/control job 113783411293 为 queued；本记录没有把 Rust 格式、编译或新 native 观测标为已通过。

既有工作流先执行原源码守卫、Rust 格式检查、default-off status 控制、真实锁获取控制和 Python 控制；通过后才执行 C1/4/8/16、每组 900 个 offered operations 的原 mixed 负载。所有成功和失败的原始工件均保留。

诊断功能默认关闭。新观测自身有开销；没有声明二进制、内存布局或计时逐位相同。原 C 的 9-observer 工件与新诊断的 10-observer 工件分别验收。端点 lifetime max 按其原意标记，interval max 不伪造；host mutex 等待也不当作 SQLite busy 时间。

## 原 C 版本已接收的执行证据

以下记录只属于原 C `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`。原主 CI 的实际 checkout 是 `54031d548d4199c84bc8ff5ea056adb26470f930`，其树与 C 相同；它不是后续 PR 合并提交的执行信用。

| 项目 | 本记录中的实际验收范围 |
| --- | --- |
| 主 CI、MSRV、安全检查 | 原终态日志已复核；新增 16 个控制全部实际通过。Rust 汇总为 419 个结果组、3,320 passed、0 failed、132 ignored，这是调用汇总，不是唯一测试数。 |
| P7 closeout / mechanism / semantic | 已完成部分的原日志已复核；最后 offline default 在 10:41:36 UTC 仍在运行。mechanism 的质量门仍为 not_evaluated。 |
| 生命周期 | 原工件 11609403178 接受：1,230 个样本、1,200 次实际查询、431 个关闭会话、1,261 条资源采样。 |
| 平台冷构建 | 已接收 4/8 个原 Mac cell；4 个 Linux cell 和最终 collector 仍待完成。没有以缓存测试替代 cold build，也没有把单元矩阵描述为全 workspace 测试。 |
| 失败退出门禁 | 6 个原故障 case 和 6 次独立 CLI 重放通过；failed/inconclusive/invalid 的非零退出与原始失败工件保留。 |
| 原运行时矩阵 | backfill 自 10:41:56 UTC、soak 自 10:42:03 UTC 开始；10:43 UTC 的四 mixed 单元与 recovery 仍 queued。 |

对应独立报告直接保存在本目录；完整原始日志、CLI stdout/stderr、原结果和官方工件元数据位于下述归档。单项门禁接受不解除其原 TODO 的前置依赖。

## 规模研究保持来源独立

截至各自固定快照：

| 固定来源 | 接受分片 | 接受样本 | 当前范围 |
| --- | ---: | ---: | --- |
| 原 C 3ffcefc3 | 3/150 | 32/1500 | 本轮接收 1k、5k、10k 首片；50k、100k 首片原 native 仍运行。 |
| 历史 275e8799（#173） | 4/150 | 41/1500 | 历史研究；不转给 C。 |
| 历史 c8be5afa（#178） | 4/150 | 41/1500 | 本轮新接收历史 50k 工件 11610565832；不转给 C。 |

[C 的三个原分片索引](C-scale-admitted-subset-003.json) 绑定原 ZIP、官方 size/SHA、原 b107 helper、完整源码、binary、15 表 parity 与各次实际 CLI。五份 capacity receipt 只说明容量上下文，样本信用为零，combine 尚未调用。

[原自动执行图](C-scale-original-execution-graph.json) 确认：同一工作流的五个首片全部成功后，原有依赖会自动展开 5 个尺度 × shard 1..29 的 145 个任务，max-parallel 10；之后原 aggregate 强制完整 150 片、1,500 样本及统一来源。原 N=30、native 五小时、job 350 分钟、preflight 5/measure 10 及所有原预算不变。没有补发 dispatch、重复标签或另造一次研究。

## 证据包及复核方法

[round28-public-evidence.tar.gz](round28-public-evidence.tar.gz) 保存 **146 份公开原始证据文件**，未压缩内容共 **8,713,133 字节**；容器 **1,449,625 字节**。

- 容器 SHA-256：`f56f52e37382b04fb990798c5e81a5f737b40b40fbb626e3c60687d51ce68c06`
- 容器 Git blob：`b562ece595677cbe773e474952615185a1fa9906`
- [inventory.json](inventory.json) 给出每个成员的来源、目标路径、原 mode、字节数、SHA-256 与 Git OID。
- [archive-readback.json](archive-readback.json) 记录完整读回、逐成员字节和 mode 一致，以及原文件未变。
- 32.7 MB 的 retained lifecycle replay input 以单独无损 tar.gz 收纳，附原输入和完整还原验证摘要。
- 私有转移记录、文件标识、签名地址和原始 GitHub ZIP 没有加入公开包。原 ZIP 的正式元数据与独立验收摘要仍保留在报告中；这里不声称公开交付了全部 ZIP。

## PR 与第 29 轮

#180 被外部更新至 `4652cad11dde4b41126544a38fddf25eb2fb7474`。根节点实际核对到相对 C 只增加 20 个历史工件并修改 7 份文档；没有在此声称该更新由本代理执行，或把旧 C 执行归属到新提交。原 C 规模研究持续保持 C 身份，诊断功能走专用分支。#174/#176 的外部关闭也没有计作本代理合并或 TODO 完成。

第 29 轮将接收实际诊断 CI 及其原并发工件，继续原 C 的完整规模和剩余运行时/平台验收，再按原依赖闭合目标十项。构建、测试、提交、审查报告与证据文件均不充作原始 TODO 完成数。

