# P5-A 实施与验收：版本化候选、五通道与缓存一致性

日期：2026-09-29。P5-001～005 已在本地 macOS arm64 声明范围完成。P5-B/C/D、G5/M2 与发行认证未完成。

Git HEAD：`4514630dcd26481cf6dbc2aff38824ed71ef06da`；实际接受工作树：`cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a`。本轮未提交、推送、创建 PR 或合并。

## 1. 实际能力与集成边界

统一 CandidateRef/LaneOutcome，携带 DocKey/DocVersion、原始跨度、lane rank/native score、scoring spec、状态、耗时和覆盖度。exact_symbol/path/lexical/grep/graph 固定注册顺序，rank-only RRF 按文档版本去重，拒绝非有限数、重复票和版本冲突；旧 wire 分数槽和14工具输入保持兼容。

精确符号通道不受软预选限制，按实际保存的 name/qname/字面签名和字节定义位置映射原文，不合成缺失的方法签名或将同名集合冒充唯一解析。路径通道在上限前应用 HardScope，短词和文档类型有独立回归，空文件不消费有效文档预算。

## 2. 本轮闭合的真实问题

缓存正文仅作为解码提示：必须与当前 SQL 行的 source proof 一致才复用。旧缓存不符时读取该行存储正文，并继续执行来源及 document manifest 校验；损坏存储不会因此被放过。成本计数按实际 cache hit/storage read 记录。

此前的并发红测在缓存修复后进一步暴露候选与最终 hydration 跨代。三个公开搜索入口现在共用读前/读后 index/evidence epoch 检查，最多3次尝试。跨代结果/错误丢弃，稳定代真实错误立即返回；耗尽返回类型化 RetrievalChanged，MCP 的 data.retryable=true。结果缓存只在检查通过后按接受的 epoch 写入，命中也经过检查。该机制不承诺文件系统快照或 rebuild incarnation。

并发测试不再假定持续写入时每个调用必然成功：测试客户端仅对 RetrievalChanged 在看门狗内重试，其他错误仍失败；保留范围/成本/静默后最新正文断言，并新增每个返回片段的 source proof 和候选文档一致性核对。3次上限、稳定损坏不重试和 MCP 错误映射另有确定性回归。

前次已落地的 LaneOutcome 原因/coverage 校验、grep执行与budget共用谓词、空HardScope零FTS短路、public-v5 lane-only失败解释以及旧夹具文档身份迁移在当前全仓验证中一并验收。最终检查发现旧DB测试要求任意 CACHED TEXT 覆盖存储正文；已明确更新为无source proof必须回源，并验证cache_hits=0/storage_reads=2。该旧断言与新来源契约冲突，原失败保存在final-v1，未通过放松生产校验修绿。P5-A-PROGRESS.md 是历史中断说明，不是本次 Gate。

## 3. 冻结源码验证

覆盖 592 文件、6401462 字节；37 条完成命令的日志哈希均匹配，源码归档全部逐文件一致。

| 工具链 | Workspace / doctest | HTTP feature | P5-A + scorer专项 | 真实 MCP | watcher |
|---|---|---|---|---|---|
| stable | 1715 passed / 53 ignored | 220 passed / 46 ignored | 38 passed / 1 ignored | 22 passed / 0 ignored | 17 passed / 0 ignored |
| 1.95.0 | 1715 passed / 53 ignored | 220 passed / 46 ignored | 38 passed / 1 ignored | 22 passed / 0 ignored | 17 passed / 0 ignored |

两工具链严格Clippy、格式、模块与源码架构守卫通过。表中每组内部失败均为0，ignored不算通过；真实stdio单独显式执行。

## 4. 固定题库对照与成本

P4-D 对照程序由已接受的586文件源码归档重新构建，不宣称它与历史二进制逐字节相同。新旧程序使用同一 public-v5 runner、同一51题、每题3次，总计306请求；逐题Top-1/nDCG无负差分、invalid hit为0，raw回放保持一致。S11已知无答案失败原样保留；未修改gold或评分公式。

| 数据集 | 版本 | Top-1 | nDCG@10 | 请求数 |
|---|---|---:|---:|---:|
| source | p4d | 1.000000 | 1.000000 | 42 |
| source | p5a | 1.000000 | 1.000000 | 42 |
| smoke | p4d | 0.700000 | 0.700000 | 33 |
| smoke | p5a | 0.700000 | 0.700000 | 33 |
| exact | p4d | 1.000000 | 1.000000 | 24 |
| exact | p5a | 1.000000 | 1.000000 | 24 |
| intents | p4d | 0.625000 | 0.656250 | 54 |
| intents | p5a | 0.625000 | 0.656250 | 54 |

Release成本实验为32/256文件、3种查询、exact/path开关对照、5次重复，共60次重新计算。原始样本全部保留于 observations/release-cost/p5a-cost.json，分组中位数见cost-summary.json。它只度量局部机制，不是加速倍数、100k、峰值RSS或尾延迟认证。

## 5. 证据与后续

证据目录：`artifacts/benchmarks/p5a-20260929-cache-fix/final-v2`。
源码归档 SHA-256：`35f834e8720cbd44ac1a22446c0f58e6dbbc76b5b0502e61f52a8f9d6354e922`。

任务状态源tasks.json更新为105done/87todo，P5完成5/20，下一入口P5-006（P5-B）。后续复用当前候选/错误/有限重试契约，实施QueryPolicy、无锁QueryHandle、有界执行器、总deadline/取消、fake semantic port；不重造本地检索算法，不提前声称provider或G5完成。

已有巨大未提交工作树及旧失败证据保留。所有本次测试仅操作隔离临时索引，未清理或重建日常索引。

## 6. 未通过的完整性门禁必须保留

本批接受的是实现/契约及排序、来源回归，不是完整检索认证。37条驱动命令完成并符合其预先允许的退出码，不代表所有benchmark gate为绿色。P5-A source的42/42请求、intents的12/54请求被public-v5标为Partial，完整性gate仍为gate_failed；S11在新旧smoke中各失败3次。

对规范raw目录的只读检索核对到source的75条非空原因：candidate_limit=42、graph_expansion_limit=15、graph_source_unmapped=15、path_token_limit=3；这些原因可在同一请求重叠。intents的12条均为graph_expansion_limit，对应I03/I07/I11/I15。两组未检索到error/timeout/unavailable/cancelled通道状态。

旧二进制未提供这些lane receipts，不能从缺失诊断推定其等价完整。新诊断将这些限制显式揭示；不提高预算凑绿、不改gold、不压掉Partial。QueryPolicy的lane需求/子预算与最终G5质量完整性校准必须接续处理。原gate.json不改写，详见completeness-review.json。

最后一条默认features的baseline-contract测试替换了共享target中的cc-eval可执行文件。二进制审计发现后按原eval-http特性复建，SHA256与实际配对runner完全相同；源码未改变，37条原命令及raw记录不改写，另存runner-restoration.json和日志。后续脚本应将对照runner放在不可变路径，避免混淆两个编译特性。
