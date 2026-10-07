# 最终任务台账独立复核

审查对象固定为 `2d060834de39cf19db67428ec4a2a84050f72b6d`；原定义锚定 `6d02d77f018a5965a6f289b0b43558ed4b9f8322`。结论为 **accepted_scoped：台账、去重和有限交付范围一致**。机器记录见 [final-ledger-review.json](final-ledger-review.json)。

## 计数与原定义

192 个任务的 ID、顺序、定义值和 JSON 类型保持不变。任务只变更 `status`、`evidence`、`implementation_notes`；计划只观察到执行说明、实施日期和下一任务的进度变更。当前 `tasks.json` 摘要为 `d69e664aa76119272fb7ff3937dcd6e3dd6e75fc07b55c4f4093922cfc731e9d`。

| 固定轮次 | 实际状态变更的原 ID | 本轮推进 | 累计推进 | 本批剩余 | 全库未 done |
|---|---|---:|---:|---:|---:|
| 合流基线 `154eb4e2` | 无 | 0 | 0 | 10 | 40 |
| 第一轮 `255d7e2d` | P8-002、004、012、016 | 4 | 4 | 6 | 40 |
| 第二轮 `8a7e5788` | P8-003、011、014、015 | 4 | 8 | 2 | 40 |
| 第三轮 `2d060834` | P8-017、020 | 2 | 10 | 0 | 40 |

每一行都与该轮真实 Git 任务快照的状态差异核对，未仅依赖进度表声明。最终为 **152 done + 22 in_progress + 17 todo + 1 blocked = 192**。新十项均保持 `in_progress`，本批正式关闭完整任务数为 **0**。主线最初的 42 项未验收降到 40 项，来自已接受的 #144 的 P7-011、P7-012。

上一批固定 `ae906513` 的十项为 P8-001、005、006、007、008、009、010、013、018、019，与本批交集为空。P8-012 在第三轮补充的一格冷构建没有再次计为一个 TODO。

`05-TODO.md`、仓库 `README.md`、重构总览 `README.md`、`08-HANDOFF.md` 四个派生视图均与固定任务源的内存渲染逐字节一致；该交叉检查由 platform reviewer 完成，corpus reviewer 另行核对四个固定文件及任务摘要。

## 证据与验收边界

- **语料与边界：**301 native / 256 compat 属于固定公开 DEV 审计，正式 600 条多语言与独立 clean holdout 仍开放；原始两条 `review_required` 没有抹掉。
- **兼容：**两轮公开控制实际 774 个请求，首次错误 CLI profile 的 exit 2 保留；原 compare 仍 exit 1，compat 为 inconclusive，native p95 比值 1.2127714 超过原 1.2 门槛。cc-switch / Flask 原外部目标仍 not_run。
- **恢复：**原首次 1 pass / 2 fail / 4 not_run 保留在原始归档。后续旧候选和当前候选各为 3 pass / 4 not_run，不能扩展为完整 P8-011。
- **平台与回滚：**旧八格 not_run 记录保留；后补冷构建为 Linux / Rust 1.95 / default / dev，一格通过、七格未执行。其 785 输入与旧产品候选 78ae 相同。有限回滚保留原来较早的两包身份，不冒充相邻正式版本认证。
- **评审与 semantic：**18 项 judge 控制及 24 项 semantic 控制通过不等于模型执行或语义发布通过。三种 semantic 材料场景均 blocked / exit 1，模型和网络调用为 0。90 个相应证据文件的摘要与大小核对一致。
- **清理：**nearest-rank 共享限定于 `benchmark::statistics` 的 `distribution`、`quantile_interval`；均值 bootstrap 和其他旧百分位、branch/schema、wire 清理的开放范围保持不变。
- **G8：**原 f99 状态回执保留；后续独立执行 `dd5be5a0` 的最终状态回执绑定 2d060834，仍 exit 1、local / semantic blocked、40 项未 done。13 条原记录的字节及观察结果与旧运行相同，20 个新运行文件摘要核对一致。紧凑 stdout 与缩进 receipt 的 JSON 值相同，二者原字节分别保留。

197 项 P8 Python 控制的原日志已读取：**197 passed，18.158 秒**；每个测试名称均匹配固定源码中的方法。此次复核没有重跑 Rust、完整旧 source suite 或产品演练，也没有将测试方法数换算为 TODO 数。

旧产品执行继续绑定 `78ae91ee…`。后来的 `65dd3293…` 只改变 worker 测试一个文件，其余 784 项输入不变；该源授权另见 [v11 独立审查](ci/v11-guard-review.json)。本台账复核不替代完整任务验收、G8 批准或最终远端 CI。
