# 第 7 轮：G3 实际绑定验证及主分支文档合并

原账本仍为 **192 项、163 done、29 项剩余；本轮和本会话新增完全完成 0 项**。用户要求的至少 10 项完全完成尚未达到。本目录封存实际执行及审查结果，不将测试、修复、源码审查或 PR 管理折算为原 TODO 完成。

## 不可变身份与实际执行

| 身份 | 提交 |
|---|---|
| 实际工程测试的组合产品 P3 | `034982202bdccc90b4938c6a6d6a56a36d4678af` |
| 独立源码审查及 32 个新增归档文件 R3 | `299d2eaca3bb525f309929b74342a65a7cdae9c0` |
| 实际运行原 v15 CLI 的绑定 G3 | `97497ba782cb21e20ea06d3e9dd6c085f7f64bdc` |
| 合并前 main，仅新增 #183 两处文档 | `fd9ca5db6485b575872c722995c451db926d05c9` |

本目录所在的后续发布提交以 G3 和上述 main 为父提交，只添加本目录，并保留 main 的 `docs/ARCHITECTURE.md` 与 `docs/MCP_TOOLS.md` 两份原文。产品、验收输入、registry、verifier 及历史证据均保持 G3 的逐字内容。执行结果的身份仍为 G3，没有改标为后续发布提交的新执行。

`G3-v15-cli/receipt.json` 和原日志记录了原始命令：

```
python3 -B scripts/verify_reviewed_source_v15.py --source-version p8-completion-source-20261009-v15
```

实际解释器绝对路径、环境与命令参数见原收据及 `run_G3_v15_cli.py`。原 **3,600 秒**上限保持不变，执行于 **2026-10-09 10:04:40–10:07:35 UTC** 完成，**exit 0、175.317 秒**。执行前后 HEAD、tree、registry、verifier 一致，工作树没有 tracked 变化。日志 SHA-256 为 `d49ba935481c7b3e756da63e1b8ae247036aebcb147ff34043449cca662f9d62`。

这次只执行原 v15 CLI。P3 的六条工程命令以及各自原收据在[上一阶段](../p8-empty-input-main-f7-integration-20261009-50c/README.md)：fmt、严格 workspace/all-target Clippy、594 个 cc-index 测试、2 个 legacy-ns 测试、5 个统计二进制测试和 1 个 corpus 测试通过。旧 P2 的 workspace 失败、旧 G 的 181 个 native 测试和旧 GitHub CI 保留各自身份，没有标为 P3/G3 的新执行。

## 独立复核

`G3-independent-binding-review.json` 直接核验实际 P3/R3/G3 不可变对象，结论 `accepted_scoped_binding`，无绑定范围阻塞：R3 恰好新增 32 文件，31 个 manifest 成员逐字及 SHA-256 全匹配；P3 原 21,912 个 artifact 条目没有改删。G3 恰好修改两个当前 guard 文件，verifier 逐字只变化四个身份常量。独立重算的 1,091 个产品输入、139 个验收输入、46 个 BASE delta 与实际 canonical/registry 全相等。BASE、VERSION、历史证明、冻结文件、验证逻辑和 CI 均保持原约束。

`G3-cli-independent-review.json` 独立复核本次原 CLI 收据和日志；它不声称执行新的项目命令。`G3-tree-proof.json` 保存 G3 原建树证明。

`final-coordination-status.json` 保存 **10:04:10–10:05:39 UTC** 的带时间快照：#175/#179 已合并；#183 仅改两份文档，12 个非 docs 根对象逐 OID 与 f7 相同。快照中的 #181 仍是旧 A 头、Draft/dirty；这是发布前状态，不应描述为本目录所在后续头的状态。

## 尚未完成的原验收

原 P8-005/P8-006 仍需要同一研究来源的完整 150 分片、1,500 phase samples。上述快照中 G275 的 100k preflight 仍运行，Gc8 的 50k/100k 仍运行，两个研究的后续 145 个测量 jobs 均未创建。状态会继续变化，本目录没有声称终态接受。

原 benchmark 第 9 节要求的完整后端 queue/service、DB lock wait、worker contention 归因仍未由现有数据提供。#175 的纳秒统计字段不能补出未采集的产品计时。八个下游任务已有的 G275 子验收保留该来源，不认证本轮组合源码。

没有更改原任务定义、状态、依赖、BASE 或验收标准，没有授权付费 provider，也没有手工派发规模研究、添加 scale label 或取消/重跑其他 owner 的运行。七轮计数均保持 29 项剩余、0 项新增完全完成。

`archive-manifest.json` 覆盖本目录除自身外的全部实际文件，不改写上一阶段或任何旧归档。
