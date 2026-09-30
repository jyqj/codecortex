# P5-C 后续进展：两项实现已接入，最终验收尚未完成

> 后续状态（2026-09-30）：P5-014／015 已通过当前冻结源码验收并标为 done。当前为 115 done / 77 todo，下一项 P5-016。见 [P5-C-COMPLETION.md](P5-C-COMPLETION.md)、[P5-C-GATE.json](P5-C-GATE.json) 与 [08-HANDOFF.md](08-HANDOFF.md)。以下正文保留上轮未验收状态及失败证据，不代表当前状态。

日期：2026-09-29。本页接续 P5-C-IMPLEMENTATION/PARTIAL-GATE；不是新的完成 Gate。当前 P5-014 的默认预算接线与 P5-015 的 EvidenceHydrator 均已落地，但最终冻结验证仍失败/受阻，完成任务数保持 113，不进入 P5-D。

**任务状态同步限制：**本轮后续 tasks.json 状态同步调用也被工具检查拦截，未执行。权威任务表仍为此前已写入的 **113 done / 2 in_progress / 77 todo**，P5-014/015 均未标完成。blocked-checkpoint/progress-notes.json 是原拟写入的说明，不是已生效任务状态；本文件单独保存实际源码进展，不替代 tasks.json。

## 1. 相比上一轮的实质变化

P5-014 已恢复默认同步/异步 code_index_context 的结构化预算。先保留完整正文，再用剩余空间放文档引用；完整引用放不下可退为带原文件 digest 和召回诊断的文件级引用。引用没有源码正文或行号声明，不计为源码命中。超大首项无预算豁免，最终 freshness、metadata 和预算收据都纳入紧凑 JSON 对象字节数。其他旧工具形状与 JSON-RPC 帧不在这项对象预算声明中。

旧五题退化来自低排名引用挤掉高排名正文，已修复。开发期 51 题/306 请求对照恢复逐题 Top-1/nDCG 零负差分、无效源码命中为零；这不是最后一次编辑后的最终认证。原 Partial/S11 保留，当时意图集另有 18 次真实预算省略 Partial，不能写成完整检索门禁全绿。

P5-015 新增 evidence_hydrator.rs，并接入实际 engine/query_handle/handler 组合根。它在选择和预算之前批量核对完整 document record、当前 chunk 映射、规范路径、存储语言、HardScope、原字节跨度、snapshot identity 和显示行号。暖缓存也必须重新验证完整 record_json；源码不符时报错，磁盘变动/删除/读取限额是带原因的明确省略。每文件只构建一次身份和行索引。

最终装配与异步后置 metadata 均核对持久化 ReadGeneration，复用原 deadline 和有限重试。图节点只作关系描述，不能附未经核验的源码切片声明。原 symbol_extract.rs 只提取查询名称，不读正文，因此不为匹配规划路径重复实现名称提取器。

## 2. 已实际完成的开发期复验

`development/assembly-v6.log`：34 项新旧专项通过（P5-B 执行 10、预算 9、Hydrator 7、选择 8）；两项真实 MCP 测试需显式运行，不计作通过。

`development/legacy-94-v5.log`：原 94 用例全部恢复。Flask 用例依赖的是有界文件召回理由中的符号提示，现保留为诊断，不冒充准确函数正文。给引用预留固定份额曾挤掉图调用方正文，已撤回；当前采用正文优先、剩余空间引用。原断言没有删除。

`development/pair-v1/summary.json`：51 题/306 请求开发期对照恢复旧五题的源码排名，其他排名无下降，源码无 invalid hit；新增预算 Partial 单独保留。后续引用诊断和测试修正仍需要最终配对重跑。

## 3. 冻结验证期间发现的问题

- `final-v1`：旧架构脚本要求 server 中出现 SourceVerifier 类名。已改为检查 Hydrator → selector → packer → generation finish 的实际顺序及复用依赖，旧失败不覆盖。
- `final-v2`：该冻结版本 stable 全仓为 1766 passed / 0 failed / 57 ignored；HTTP 的旧 debug index 计时为 590.49 毫秒，超过原 500 毫秒门限。未修改阈值，同源码单次隔离复验为 275.72 毫秒。后续计时测试组串行执行，不把隔离结果作为共享负载尾延迟认证。
- `final-v3`：真实 MCP 并发用例直接 unwrap 了已明确标为 retryable 的代际冲突。测试客户端现仅对 -32603、代际冲突消息和 retryable=true 同时成立的响应进行最多 64 次、10 秒内的重试。其他错误仍失败，所有范围、显式省略和写入静止后的最新正文断言保留，并增加源码证明检查。迁移后的 4 项常规测试及 1 项真实 MCP 测试通过，见 development/mcp-conflict-regular.log 和 mcp-conflict-real.log。
- `final-v4`：资源回收测试等待 CPU permit 归零后立即检查另一份 admission permit，两个独立计数之间存在短暂观察窗口。现改为等待两者都归零，原 2 秒看门狗、超时结果与拒绝迟到发布断言不变。该最后的测试修正已写入，但尚未执行复验。

因此不能将不同源码时点的局部通过拼接成当前全仓通过。当前最低 Rust 版本、完整 HTTP/MCP、release 成本和最终配对尚未完成新的接受性验收；新增 p5c_cost 测试已准备，但不报告尚未得到的性能数据。

## 4. 工具阻塞与当前快照

继续执行格式、资源回收测试、构建和完整真实 MCP 预检的工具调用被安全检查拦截。随后核对 `development/preflight-final` 不存在，确认预检没有启动；另一次汇总读取也被拦截。未通过其他调用重试被拦截的操作，内部原因不可见。与上一轮不同，Hydrator 源码写入本轮成功，当前阻塞属于后续验证。

证据根目录：`artifacts/benchmarks/p5c-20260929-completion/`。`final-v1`～`final-v4` 保留各自失败，不修改为通过。

当前未验收快照：616 个文件、6,605,476 字节，摘要：

```text
471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473
```

归档与记录：`blocked-checkpoint/source-manifest.json`、`blocked-checkpoint/unaccepted-source.tar.gz`、`blocked-checkpoint/blocked-validation.json`。相对 final-v4 的唯一新增文件差异是 execution.rs 的测试等待条件；不是新的生产执行器行为。

Git HEAD 仍为 `4514630dcd26481cf6dbc2aff38824ed71ef06da`，没有提交、推送、PR 或合并，没有操作日常索引。不存在已接受的新 P5-C 完成 Gate；准备好的验收生成脚本没有执行完成状态写入。

## 5. 后续入口

继续 P5-C，不进入 P5-D。先复验最后的资源回收测试修正，再完成当前源码的双工具链、真实 MCP、完整对象预算、来源证明与固定题库验收；保留新旧 Partial/S11 和所有失败。只有真实验证完成，才将 P5-014/015 标为 done。G5/M2、100k、公开 holdout、跨平台、真实 provider/vector 与发行仍未认证。
