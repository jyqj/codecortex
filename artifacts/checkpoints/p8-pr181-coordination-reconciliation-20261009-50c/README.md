# 第 8 轮：保全并行 PR 整合，并更正运行时要求的解释

**192 项原任务、163 done、29 项剩余；第1–8轮均为0项新增完全完成。至少完成10项的目标尚未达到。** 原任务定义、状态、依赖和验收要求均未更改。

## 双方代码与历史全部保留

发布前，#181 的远端头已由另一处协作从旧 A 前进为 `4866ec5558f1cedd34c7dc54231af65b77f706ec`。它的三提交链为产品 `fced7366cee6aa727aa10d775539c3ef9df5c8d2`、审查 `d9581c4c9fa0c46cdd792cffb1a19689af946c6d`、绑定 `4866ec5558f1cedd34c7dc54231af65b77f706ec`。当前证据只能确认关联 GitHub 账号，不能确定具体会话或精确 push 时间，详见 `head4866-coordination-review.json`。

本目录首次新增的合并提交以我方 A3 `c5d3c6163e73484d710f686e3a0e7446117e12e1` 和远端 `4866ec5558f1cedd34c7dc54231af65b77f706ec` 为两个父提交。两方全部共同非 guard 路径逐 mode/type/blob 相同，包含产品、验收输入、任务账本和 main fd9 的两份文档。共同的21,912个 artifact 条目均完全相同；我方独有41个文件、外部独有6个文件全部保留。本轮只追加本目录，并为 P8-007/P8-010 追加解释更正和重生成原进度视图。

两套 registry 的1,091产品输入、139验收输入、46个BASE delta完全相同，差异仅为产品/审查身份与审查摘要；verifier完整文件仅四个身份常量不同。整合选择我方已实际验证的 G3 `97497ba782cb21e20ea06d3e9dd6c085f7f64bdc` 成套绑定，保留外部 P/R/G 祖先和其全部六份独立审查原文。BASE、VERSION、完整验证逻辑、历史证明、CI及所有产品代码保持不变。

`external-source-equivalence.json` 保存完整逐路径比较与原件摘要。`A3-final-scope-independent-review.json` 是新增本轮更正之前、固定A3的独立范围审查，不冒充此次合并头的新审查。

## 明确纠正此前过严的解释

第6轮报告及第7轮 README 将原09-BENCHMARK §9进一步解释为必须新增完整生产后端逐操作queue/service和全锁点纯wait归因，表述过严。原条款规定观测主题，没有指定采集层级、逐RPC IDs、跨crate遥测架构，或每项诊断必须额外重复一小时。**撤回由此前解释产生的新增强制埋点/新研究要求。** 原文和原验收要求保持原样。

G275自己的fresh-release backfill原件已有三seed、quiet/held、C1/4/8/16共768请求、每seed四个实际held provider worker，以及`single_read_pool_checkout_us`、`read_statement_us`、`writer_acquire_and_rollback_us`三个探针。最后一项包含真实BEGIN IMMEDIATE/ROLLBACK的SQL和rollback成本，不能重标为纯锁等待。已有mixed/soak客户端时序、status采样和这些同源观测继续保留真实字段与原子验收边界。C8有其自己的观测来源，不能把两者样本合并。

ExecutionPool内部queue/service和更细的纯等待分解仍未直接测得；它们是可以继续完善的诊断能力，不能仅凭现有原文升级成新增硬门。详见 `runtime-attribution-reconciliation.json`，其中保留旧报告摘要、原条款、G275自身原summary核对以及C8证据读取层级。旧raw、旧报告和旧源码位置建议均未删除或改写；P8-007/P8-010现以追加说明明确纠正解释。

## 实际执行保留各自身份

- P3 `034982202bdccc90b4938c6a6d6a56a36d4678af` 的fmt、严格Clippy、cc-index 594个通过、统计2+5个通过、corpus 1个通过，原收据在[第7轮源码归档](../p8-empty-input-main-f7-integration-20261009-50c/README.md)。
- G3原v15 CLI实际exit0、175.317秒、原3600秒边界、前后源码一致，原件在[第7轮绑定归档](../p8-empty-input-main-f7-binding-20261009-50c/README.md)。该归档中的“所在提交”及PR状态描述以其首次新增A3和明确时间快照为准，不表示当前合并头的新执行。
- 本次仅运行原plan `--write`、无参数plan及historical三条命令，全部exit0。它们实际运行于HEAD=G3、带本轮prospective任务说明修改的工作树，完整stdout/stderr及收据在`round8-plan-checks/`。没有把这些命令标为后续合并提交上的新执行。
- 旧P2全workspace/sampler exit101、旧G native181和旧GitHub CI等结果全部保留原身份。这次没有新跑Rust、native suite或任何规模研究。

## 原任务仍未完成的原因

完整同源150分片/1,500 phase samples规模与增量/fanout研究尚未闭合，P8-005/P8-006及下游原硬依赖保持开放；各自既有子验收不等于原任务完整关闭。原统计要求、其他完整验收、原来源和失败记录继续保留。本轮解释更正不是运行或验收新的样本，也没有关闭任何原TODO。

没有手工派发/取消/重跑其他owner研究，也没有付费provider操作。PR更新采用保全双方祖先、带最新expected-head的普通快进。当前头CI需看其实际GitHub状态，不能借用旧头成功。

`archive-manifest.json` 覆盖本目录除自身外的全部文件；旧归档全部保留。
