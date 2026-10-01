# 查询执行：策略、拥有资源的句柄、预算与取消

本页描述 P5-B 实际实现。验收状态以 roadmap 的 tasks.json 和对应冻结证据为准，不由文件存在推定。默认无语义 provider、无网络请求，不代表 embedding、向量发布或 G5/M2 已实现。

## 入口与资源所有权

MCP 的 `search` 和 `context` 使用异步编排。先在共享执行器中短锁取得 `QueryHandle`，复制项目路径、规模档、配置以及 DB/SearchEngine/QueryServices 的 Arc；离开锁后再运行检索或等待可选端口。所有原文组装仍使用原有实现，没有复制另一套搜索引擎。

句柄绑定捕获的项目与 DB 实例，不在 await 后重新取“当前项目”作为源码来源。低层纯检索句柄保有资源，可在 CodeIndex 的 idle close 后完成；重开创建新的实例。项目切换不把旧项目的可选端口带到新项目。锁污染恢复会 retire 旧 SearchEngine 并创建新实例，旧句柄不能继续发布缓存。

`context` 的附加符号源码扩展及 `search(mode=symbol)` 仍复用同步图接口，在受限 CPU 阶段持有短读锁，不跨异步端口等待；附加扩展时核对当前实例，不允许把另一项目的正文接到旧结果。若实例已关闭/替换，明确返回可重试 QueryInvalidated。这不是所有图工具的无锁改写，完整生命周期整体验收仍由后续批次承担。

解析新鲜度沿用同一 `attach_observed` 投影；异步路径的查询前/后观测来自捕获的数据库。对象输出保留 `resolution_freshness`，数组仍遵循旧的 incomplete 报错边界，不能因异步重构而静默消失。磁盘原文验证、DocVersion 和三次 index/evidence epoch 核验继续保留；DB 实例 ID 不是持久化 incarnation 或文件系统快照。

## QueryPolicy 与配置

P5-019 开发中的词法规范将 compound identifier 编译为
`(whole_literal OR (all_distinct_components AND))`，不再把任一 camel/snake
碎片独立 OR 成有效代码证据；普通独立词之间仍是 OR。所有用户项 literal
quote，不能注入 FTS 运算符。总量上限 12 atom，优先保留原词，派生 AND 组
不能截断成前缀；不足时退回 whole，并保留 `query_expansion_atom_budget`
的 Partial 诊断。它是词法支持，不冒充 exact-symbol 或语义证明。
QueryPolicy 与检索缓存 policy 版本一并推进；旧结果不能沿用新的解释。
该改动的正式完整质量/成本验收以新的冻结证据为准，不沿用 P5-D 收据。

GraphLane 的当前开发映射由 UID、原始 line/byte-column 与当前 source
snapshot 定位声明文档。20 个 seed/有界双向 1-hop 邻居经分批投影、共享
source-prefix 解码和 byte-span 入场；每个 UID 不单独执行 SQL，途中检查
QueryControl。policy 与缓存域已随此语义推进。`graph_source_mapping`
在压缩后的 policy 也保留：complete 只表示有界已执行图召回的声明映射，
不表示长函数全部 statement/body 文档均返回。正文任务义务仍按实际
byte-span/facet 覆盖审计；source unmapped、邻居/seed/candidate 截断继续
明确 Partial，不能靠一个锚点洗成全正文完整。

## 规范化精确路径域

PathLane scoring spec 为 `canonical-scoped-exact-path-domain-token-fallback-v3`，
QueryPolicy v4 和检索缓存 policy v22 共同失效旧解释。只有整个查询是没有任何
Unicode 空白/反斜杠、至少两段且 `normalize_relative(query)==query` 的路径，
并且同一 HardScope 下有精确 indexed-document 命中时，本次 PathLane 域才是
该文件的真实文档。不会再执行目录名等 generic component 的 token fallback。
这不是删除已经执行的限额，也不扩大检索预算。精确文档超过候选上限仍返回
`candidate_limit` Partial；indexed-current 不等于磁盘已验证或整文件正文完整，
最终 hydrate 的 byte/DocVersion/新鲜度守卫仍执行。

没有精确 scoped 文档、普通 basename/identifier、含额外 prose/空白、`./`、
双斜杠或反斜杠别名，均保持原精确尝试加有界 token fallback；其真实
`path_token_limit`、省略和 Partial 继续公开。语言/file-path/prefix 限制在 SQL
LIMIT 前执行，soft hints 不扩 HardScope。当前 API 没有 doc-kind 过滤参数；
Markdown 等非代码文档应保留实际原件/类型元数据，不借此增加虚构 public 参数。
压缩 policy 保留 `path_source_domain`，不把定位声明或文件等同任务所有 facets。

## 解释性 metadata 的紧凑域标签

完整 policy 的原长定义保留。packing v5 只在严格等于已知长定义时，将下列
解释文改为语义等价、版本化的紧凑标签；未知 caller label 不改写。此投影先于
删除实际正文，全部预算、计数、耗时、generation、source proof、score trace、
原 Partial/省略状态保持原值；不得提高 cap。旧 packing v2/v3/v4 仅兼容读取，
不升级它们的完整性结论。

| 完整字段定义 | 紧凑标签 | 不改变的证明域 |
| --- | --- | --- |
| `canonical_scoped_existing_path_docs; else_bounded_tokens; not_whole_file_coverage` | `canonical_scoped_exact_else_bounded_tokens;not_whole_body:v3` | 同 HardScope 的 canonical existing indexed Doc；否则原有界 token fallback。不是磁盘已验证或整文件正文完整。 |
| `uid_byte_declaration_document; complete_mapping_is_not_whole_symbol_body_coverage` | `uid_byte_decl_docs;not_whole_body:v2` | 当前 snapshot/DocVersion 内按 UID+byte 证明声明文档。不是整个 symbol 正文覆盖。 |

语义/预算规格变化同时使缓存 policy v22 失效；presentation 数字位宽与
incarnation/nonce 的回归属于明确的合成 metadata 压力，不冒实际测量。

## 声明、任务 facet 与输出预算

P5-019 开发版的 selector v2 可从已验证的 `2 * top_k` 候选窗口识别
字面 program cue（保留大小写的 ASCII token/qualified component，支持中文
文句内的 ASCII 标识符）。窗口中仅一个文件有该字面证据不等于全仓唯一。
非首词或 compound cue 优先于句首标题化的 fallback；不是框架词典、
语义推断或 exact-symbol 身份。支持项必须带有效 source proof、包含非空
signature 的 owner，且当前返回声明有 query 的 code-token 支持。

selector 先清除外部 `coverage_priority`/`evidence_priority`，再标记最多
两个 source-support 正文和可信 intent-facet 代表。分数与原有顺序不改，
hard scope 不扩大。packing v4 保 rank one，然后保实际 intent-facet、
source-support，最后 incidental/comment/reference；空间仍不足则明确
Partial/omission。低预算下先把 scope 的解释性长文压成版本化短标签，
所有 scope/budget 数值、ordering、source proof、score trace 与状态保留。
旧 packing v2/v3 仅由评测 decoder 按各自状态读取用于 immutable baseline
对照，不升级旧 Partial；新 source-v2 必须完整重新验收。

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
