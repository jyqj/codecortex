# P5-C 开发过程与未完成事项

> 历史阶段记录：本文件保留当时的子集验收、阻塞和失败，不覆盖原证据。P5-014/015后续完成情况与当前入口见[P5-C-COMPLETION.md](P5-C-COMPLETION.md)、[P5-C-GATE.json](P5-C-GATE.json)及[08-HANDOFF.md](08-HANDOFF.md)。

> 最新状态：113done、1in_progress、1blocked、77todo；P5为13/20，下一P5-014，P5-C整批未完成。子集验收见[P5-C-IMPLEMENTATION.md](P5-C-IMPLEMENTATION.md)与[P5-C-PARTIAL-GATE.json](P5-C-PARTIAL-GATE.json)；以下开发过程不覆盖最新Gate的范围。


2026-09-29。当前110 done、4 in_progress、1 blocked、77 todo；P5-C未完成，不能进入P5-D或声明G5/M2。

## 最新接续状态（覆盖下文开发过程的当前状态）

P5-011～013已通过开发期验证，正在final-subset-v2进行冻结整体验证，尚未增加done数量。当前613文件摘要`0a1a1a2fbe857303304a75d0bc1797c0169c2e2d2e0d93b0a464ff0539062585`。

P5-014默认接线已回退：固定题库pair-v2有五道源码题Top-1从1变0，且新增意图Partial；诊断读取被工具安全检查拦截。回退后pair-v3-rollback的306请求恢复零排序退化与零新增gate。BudgetPacker原型和失败证据保留，两项默认接口预算断言明确pending，不计通过。

全仓final-subset-v1另发现selection说明挤掉旧graph_enrichment诊断，已补兼容投影并保留原断言。20项输出测试与原94用例集成入口复验通过，再冻结v2。该补丁不启用BudgetPacker；P5-015最终来源写入仍未落地。下文保留此前步骤和失败，不能将它们读作新的接受状态。

## 已有修改

P5-011：新增持久化index_incarnation与ReadGeneration；正常重开保持身份，完成的staging重建更新身份。结果缓存和可选语义响应核对incarnation/epoch；语义epoch未实现时保持None，不伪装成就绪的0。

P5-012/013：使用原有bounded rerank候选窗口，不重算相关性分数；保留排名锚点、按任务选择测试/接口facet。重叠按文件和source snapshot的byte区间并集计算，只有实际选中证据才能抑制另一个候选。输入与选中集合统计分开。

P5-014：完整响应JSON与metadata、收据均进入预算。首先压缩重复展示和可省略说明，然后完整源码或明确引用二选一；没有生成新源码切片，不声称引用就是完整源码。适配器v6识别明确省略候选详情的lane_receipts及packing partial。来源检查仍沿用现有实现。

## 开发证据与未完成事项

证据位于artifacts/benchmarks/p5c-20260929-evidence-assembly/。入场核对P5-B的603文件没有漂移，并保存源码及任务快照。generation-v1基础测试为DB146 passed/1 ignored、search275 passed。focused-v1的并发3、P5A边界16、P5B执行10、新选择7通过；这些不是预算接线后的最新全仓证据。

focused-v2保留预算三项失败：小正文被诊断挤掉，以及JSON往返不一致。正在修复优先级、保留原测试并复验；此前结果不得表述为当前完整通过。

## 明确阻塞

P5-015的EvidenceHydrator及evidence.rs/document_store.rs/lib.rs配套编辑被OpenAI工具调用安全检查拦截。读回确认事务整体未执行，evidence_hydrator.rs不存在；没有经其他工具/路径重试。现有SourceVerifier保持原状，最终来源强校验缺口不能宣称已经解决。

P5-011～014完成状态须以随后冻结的测试和报告为准。P5-015仍blocked，既有Partial/S11、100k、跨平台、真实provider与G5/M2仍未完成。HEAD保持4514630；未提交、推送、PR、合并或清理日常索引。
