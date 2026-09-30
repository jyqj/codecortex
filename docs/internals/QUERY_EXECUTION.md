# 查询执行：策略、拥有资源的句柄、预算与取消

本页描述 P5-B 实际实现。验收状态以 roadmap 的 tasks.json 和对应冻结证据为准，不由文件存在推定。默认无语义 provider、无网络请求，不代表 embedding、向量发布或 G5/M2 已实现。

## 入口与资源所有权

MCP 的 `search` 和 `context` 使用异步编排。先在共享执行器中短锁取得 `QueryHandle`，复制项目路径、规模档、配置以及 DB/SearchEngine/QueryServices 的 Arc；离开锁后再运行检索或等待可选端口。所有原文组装仍使用原有实现，没有复制另一套搜索引擎。

句柄绑定捕获的项目与 DB 实例，不在 await 后重新取“当前项目”作为源码来源。低层纯检索句柄保有资源，可在 CodeIndex 的 idle close 后完成；重开创建新的实例。项目切换不把旧项目的可选端口带到新项目。锁污染恢复会 retire 旧 SearchEngine 并创建新实例，旧句柄不能继续发布缓存。

`context` 的附加符号源码扩展及 `search(mode=symbol)` 仍复用同步图接口，在受限 CPU 阶段持有短读锁，不跨异步端口等待；附加扩展时核对当前实例，不允许把另一项目的正文接到旧结果。若实例已关闭/替换，明确返回可重试 QueryInvalidated。这不是所有图工具的无锁改写，完整生命周期整体验收仍由后续批次承担。

解析新鲜度沿用同一 `attach_observed` 投影；异步路径的查询前/后观测来自捕获的数据库。对象输出保留 `resolution_freshness`，数组仍遵循旧的 incomplete 报错边界，不能因异步重构而静默消失。磁盘原文验证、DocVersion 和三次 index/evidence epoch 核验继续保留；DB 实例 ID 不是持久化 incarnation 或文件系统快照。

## QueryPolicy 与配置

`.codecortex.json` 可增加顶层 `query`：

```json
{
  "query": {
    "strategy": "local",
    "deadline_ms": 30000,
    "lane_timeout_ms": 20000,
    "semantic_timeout_ms": 5000,
    "semantic_top_k": 24
  }
}
```

时间字段允许 1–600000 毫秒，semantic_top_k 允许 1–4096；语义无效预算会报配置错误。项目配置保持显式重新加载语义，不承诺热监听。工具名称、14 个 MCP 输入 schema 及 hybrid/symbol 分流不变；P5-B 没有新增任意 URL、凭证或网络配置。

`local` 不调用端口，即便宿主注入了端口；`auto` 在未配置时等价 local 并解释 not_configured；显式 `semantic` 没有端口则返回 SemanticUnavailable，不伪装成已执行。当前 semantic 表示在本地召回之外启用可选召回，不删除本地故障回退。已知符号直接查找不需要语义召回；context 的语义策略用于其文本检索回退。

Policy 固定 strategy、intent、子预算与候选上限，并将完整配置/意图纳入本地缓存域。locate 的 exact/path 标记 identity，trace/fix/refactor/test 的 graph 标记 structural，lexical/grep 标记 source；这些是可解释的召回职责，不是抹掉 partial 的豁免。结果的 `evidence_summary.retrieval.policy` 暴露实际策略。

弱排名不是不存在证明。S11 和既有预算/图映射 Partial 仍保持可见，本批没有改题目、评分器或根据测试 ID 特判无答案；完整 selector/预算分配与 G5 校准仍在后续任务。

## 两层有界执行

生产 QueryServices 使用进程共享的额度，而不是每个项目再开一套线程：

| 层 | 执行上限 | 等待队列上限 | 满载行为 |
|---|---:|---:|---|
| CPU/SQLite/输出组装 | 4 | 32 | QueryBusy，可重试 |
| 可选 async 端口 | 8 | 32 | semantic unavailable/capacity，保留诊断 |
| 本地召回 lane 工作池 | 固定 4 个 Rayon 线程 | 由已准入的 CPU 请求约束 | 固定注册顺序收集 |

不同阶段独立计数；不能把这些数字称为整个进程、索引构建或所有 MCP 方法的总线程上限。index/status 不排进检索 CPU 队列。直接使用同步 SearchEngine 的库调用者仍负责自己的请求准入；其本地 lane 使用共享固定工作池。

先 try-acquire 总准入额度，再等待执行额度，避免把无限请求先送到 spawn_blocking。CPU 闭包拥有两份 permit，直到真实退出才释放；丢弃 JoinHandle 不等于阻塞线程已停止。等待中的请求取消会释放准入额度；已执行的任务取消后仍占用额度，因此不会通过反复取消制造无限后台阻塞工作。统计是即时观测，不是原子全进程快照。

## Deadline 与取消

总时钟在查询准入/句柄捕获前建立，取得项目配置后从原起点收紧，排队等待、可选端口、重试与输出组装共用该时钟。首次项目定位/加载发生在已有 ProjectSession 路由中，属于此前生命周期阶段；不声称该冷启动阶段已被查询预算完全覆盖。

每个本地 lane 和语义端口有不超过剩余总预算的子期限。本地同步算法在入口/出口检查；grep 在批次及行回调检查，源码组装逐项检查。单次 SQLite 调用或不可中断的同步工作不被伪称为即时终止，额度保留至退出，过期结果拒绝发布。子端口超时可降级为 timeout lane，父请求尚有预算时继续本地检索。

rmcp 的 notifications/cancelled 通过 RequestContext.ct 接入 search/context，Future 被丢弃会触发共享取消控制。等待者注册会在 Future Drop 时撤销；没有额外常驻轮询或每查询后台任务。取消和缓存插入使用同一短临界区线性化，普通结果缓存、正文提示缓存都拒绝取消后开始的插入；缓存命中同样检查期限和句柄状态。

QueryTimedOut、QueryBusy、QueryInvalidated 可重试；QueryCancelled 不自动重试；稳定的来源/文档损坏仍是错误，不能变成空成功。

## 可选 SemanticRecall 接口

接口只依赖 cc-model 中的 Future、HardScope、版本化 LaneOutcome 和取消控制。请求包括 query、规范硬范围、候选上限、策略指纹及捕获的 index/evidence generation。提供方必须使用非阻塞异步实现；本批不保证强制终止违反该接口约定、在 poll 内无限阻塞的第三方代码。

显式空 HardScope 下不调用可选端口，记录 disabled 而不扩大范围。返回值逐项验证 schema、状态、排名、数量、有限分数、重复文档及 generation；再从当前 DB 核对文档版本、原始跨度与硬范围。外部返回的正文不参与原文组装，也不能将相似结果升级为 exact identity。权重由接收方固定为 1，提供方不能自行抬高融合权重。来源验收后才进入现有 rank-only RRF 和 hydration。

超时、拥塞和 provider 错误是可见的 lane 状态；构造 Future 或 poll 的 panic 同样转为错误诊断。含语义结果的请求暂不进入普通完整结果缓存，避免在 semantic epoch 尚未实现时缓存错误坐标空间或故障。fake 成功只证明接口/编排，不证明真实模型、provider、召回质量或向量持久化。

## 裁剪后的诊断

输出超过原有预算时，完整 lane candidates 仍可省略但状态/覆盖度保留。新增策略说明过大时先只保留策略与预算的紧凑投影并标 details_omitted；空间仍不足时标 policy_omitted，而不是让冗长策略说明挤掉原本可保留的 graph_enrichment。未裁剪响应仍返回完整 policy。这是既有输出限额的兼容修复，不等于 P5-C 的完整结构化 BudgetPacker。

## P5-C 接续

ReadGeneration 已在 P5-C 中加入持久化 incarnation 和可选 semantic_epoch，SemanticRequest 的 RecallGeneration 现在使用该读模型；Rust 调用者应从数据库取得完整读依据，不再手写只有两个 epoch 的值。纯本地默认仍不调用网络。集合选择、默认完整 JSON 预算和最终 EvidenceHydrator 见 [EVIDENCE_ASSEMBLY.md](EVIDENCE_ASSEMBLY.md)。最终上下文增加受原 deadline 约束的装配前后 generation 检查，不能把每文件磁盘观察当作原子快照。

## 验证入口

`p5b_execution.rs` 使用真实 parser、SQLite、单读连接及 fake 端口，覆盖 scope/version、慢端口、取消/Drop、超时、panic、旧句柄和公开新鲜度。`mcp_query_tests.rs` 通过真实 MCP 双工传输发送取消通知；p5b_execution 的 ignored 测试另启动实际 codecortex stdio 子进程。`execution.rs` 的测试区分排队取消与运行任务的额度；`p5b_cost.rs` 是 release 受控并发观测，不是尾延迟或 100k 认证。
