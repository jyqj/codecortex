# P5-C 实施：三个任务收口，预算和最终来源守卫未完成

> 历史阶段记录：本文件保留当时的子集验收、阻塞和失败，不覆盖原证据。P5-014/015后续完成情况与当前入口见[P5-C-COMPLETION.md](P5-C-COMPLETION.md)、[P5-C-GATE.json](P5-C-GATE.json)及[08-HANDOFF.md](08-HANDOFF.md)。

日期：2026-09-29。P5-011～013完成本地声明子集；P5-014 in_progress，P5-015 blocked。P5-C整批、G5/M2与发行未完成。

当前HEAD `4514630dcd26481cf6dbc2aff38824ed71ef06da`；冻结源码 `0a1a1a2fbe857303304a75d0bc1797c0169c2e2d2e0d93b0a464ff0539062585`，613文件、6562515字节。未提交、推送、PR、合并或日常索引操作。

## 实际完成

**P5-011**：持久化incarnation与ReadGeneration已落地，正常重开保持、staging换库更新；缓存与读前后核验及可选语义依据纳入代际；非查询心跳不改变读代际。

**P5-012**：独立selector接入原有bounded rerank window，保留最高排名锚点并按intent补测试/接口facet，输出仍按原排名且不改score trace。

**P5-013**：按文件及source snapshot计算byte区间并集；只由实际选中的来源抑制重复，跨文件或版本的相同文本不合并。输入/选中冗余与名额省略分别可复算。

## 未完成与回退

P5-014已实现完整JSON预算、机器正文或明确引用二选一，以及独立lane_receipts投影和public-v6解释。单元正负测试通过不代表生产就绪：默认接线在固定题库中导致R07/R09/R10/R11/R12首位答案消失，源码集Top-1由1.0降至0.642857。没有改gold、评分或把引用算作源码命中。保留失败pair-v2，回退默认接线后pair-v3-rollback重新306次对照恢复零排序退化。两项完整预算集成断言继续保留但明确pending，不计入通过。

P5-015写入被OpenAI工具调用安全检查拦截，整组事务未执行；evidence_hydrator.rs不存在，evidence.rs及document_store.rs与P5-B相同。随后预算案例大小诊断读取也被拦截，没有改用其他工具或路径重试。工具内部原因未知，不能归因于已证实的源码权限或Runner故障。

当前生产预算沿用P5-B，不能声明所有JSON/metadata都受新packer约束。新增选择诊断导致context_graph_enriched旧集成断言失败，已补兼容投影：压缩selection说明而不挤掉图/lane诊断，保留原断言和失败final-subset-v1。prototype只保留为后续材料；来源校验仍为既有SourceVerifier，最终统一Hydrator未完成。

JSON往返测试发现浮点解析不稳定后，serde_json启用float_roundtrip；分数公式不变。该特性与原型测试一起纳入当前冻结构建。

## 冻结验证

|工具链|Workspace/doctest|HTTP|专项|取消协议|真实stdio|watcher|
|---|---|---|---|---|---|---|
|stable|1755 passed / 57 ignored|243 passed / 50 ignored|60 passed / 3 ignored|1 passed / 0 ignored|23 passed / 0 ignored|17 passed / 0 ignored|
|1.95.0|1755 passed / 57 ignored|243 passed / 50 ignored|60 passed / 3 ignored|1 passed / 0 ignored|23 passed / 0 ignored|17 passed / 0 ignored|

38条最终命令与日志SHA核对。所有测试组内部失败为0；忽略项不算通过。源码成员、每文件字节、归档与不可变测试二进制均一致。

固定51题、每题三次、两个版本306请求：逐题Top-1/nDCG无负差分，invalid source hit为0，回放一致；各数据集原有状态和门禁失败计数保持。原source42 Partial、intents12 Partial和S11仍保留，因此完整检索门禁未通过。

## 证据与后续

证据目录 `artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2`；源码归档SHA `8aef5362fc3c2502d95a69976041cf9f9224d708da44508c6d46a39888af705c`。

权威状态113done/1in_progress/1blocked/77todo，P5为13/20，下步P5-014。014必须恢复默认接线前解决真实题库退化，再完成015统一来源守卫，不能从三个子项接受推定整批通过。未认证范围包括100k、公共holdout、真实provider、跨平台、峰值RSS、尾延迟及发行。
