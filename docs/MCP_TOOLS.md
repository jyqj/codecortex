# MCP 工具参考

14 个工具全部常驻可用——没有激活步骤、没有域系统。本文列出每个工具的
用途、关键参数、**响应形态**与错误路径；权威的运行时口径以
`status(aspect="capabilities")` 为准。术语见 [GLOSSARY.md](GLOSSARY.md)。

## 通用契约

所有工具共享同一套参数与输出契约（实现：`crates/cc-server/src/tools.rs`、
`handlers/output_budget.rs`）：

### 增量新鲜度（P2-C）

`index`、索引 `status` 及对象型搜索/关系响应追加 `resolution_freshness`，区分 `ready`、`incomplete`、`changed_during_query`，并提供原因与重试说明。它仅覆盖已观察到的解析失效欠账，不代表语言语义完整、未观察到的磁盘变化或全查询快照隔离。未完成的旧数组型关系响应显式报错，不静默返回旧关系或改变原数组格式；输出截断保留该状态。工具名称及输入 schema 不变。详见 [INCREMENTAL_RECOVERY.md](internals/INCREMENTAL_RECOVERY.md)。

### 检索能力与显式策略（P5-D）

`status(aspect="capabilities").retrieval` 区分 `no_project / closed / empty / available / error`，保留已有布尔字段。`local_state=available` 只表示本地路径可运行，不保证每条查询完整。`semantic_state=not_configured` 表示无端口；宿主注入接口时为 `port_attached_unverified`，不能据此声称真实模型就绪。未接线时 `dense_state=disabled`；已接线且存在 active space 时，只读状态投影报告 `semantic_pending` / `semantic_failed` 与 `dense_published` / `dense_desired`。有 pending 或未发布文档为 `backfilling`，无 pending 且有失败任务为 `failed`；只有覆盖完成时才报告 `ready`，零 eligible 文档仍为 dense `partial`。无 active space 保留 `port_attached_unverified`，不会冒称 ready。该投影仅描述 active space 的数据库发布状态，不证明真实 provider 可用或语义效果；启用 `semantic-http` 构建、配置 enabled 且显式 network opt-in 后，成功索引触发有限 blocking worker job；默认构建/关闭配置不创建语义缓存或发出请求。worker 在网络等待前释放数据库与 CodeIndex 锁；跨页回填在 job 边界交还容量，close 取消并阻止迟到发布。重开保留同一输入任务的重试预算并复用已验证 publication/cache。装配和执行失败分别报告 `semantic_provider_assembly_failed`、`semantic_worker_round_failed`；真实 stdio 的本地合成/loopback 生命周期子项已验证，独立审查和完整 V18 仍待 P7-014 收口。状态查询不调用端口，不启动索引。实际覆盖仍以查询的 lanes/lane_receipts、GraphExplain、source_freshness、selection 与 packing 为准；构建原因沿用原 BuildExplain，不伪造持久化的最后构建报告。

`search`（hybrid 模式）与 `context` 新增可选 `retrieval_strategy=local|auto|semantic`。省略或 null 使用项目 `query.strategy`；local 不调用可选端口，auto 无端口时等价 local，semantic 无端口明确报不可用。显式 context 策略使用统一检索路径，不会被旧的直接符号快捷路径忽略。symbol 模式保留原数组响应，只接受省略/null/local，其他策略报参数错误。没有 provider、模型或任意 HTTP endpoint 新参数。

以下请求由 `p5d_contract` 经真实 stdio 执行：
<!-- p5d-query-contract:start -->
```json
[
  {"tool":"search","args":{"query":"needle","retrieval_strategy":"local","top_k":3}},
  {"tool":"search","args":{"query":"needle","retrieval_strategy":"auto","top_k":3}},
  {"tool":"search","args":{"query":"needle","mode":"symbol","retrieval_strategy":"local"}},
  {"tool":"context","args":{"task":"needle","retrieval_strategy":"auto"}}
]
```
<!-- p5d-query-contract:end -->

`code_index_context` 采用完整对象预算，包含最终新鲜度及元数据；超过预算时保留完整正文或明确引用，不返回截断 JSON 前缀。引用不作为源码命中。其他旧工具形状继续使用下文原出口策略。持有查询视图期间空闲清理不关闭实例；LRU 淘汰后同项目仍复用在途实例及构建门，释放最后视图后才可回收。参见 [运行时生命周期](internals/QUERY_LIFECYCLE.md)。

### 参数校验（sanitize）

每个工具的参数在分发前经过 `sanitize()`：

- **未知参数名直接拒绝**：schema 反序列化失败（未知字段、类型不匹配、
  缺必填字段）经 rmcp 以**工具级错误结果**返回（`CallToolResult.is_error
  = true`，文本原样携带 serde 诊断，如
  ``failed to deserialize parameters: unknown field `qurey`, expected one of `query`, `mode`, `top_k`, ...``）
  ——拼错的参数立刻失败，而不是静默按默认值运行。（早期版本会静默忽略
  未知参数；依赖旧行为的客户端按报错里的字段名与 `expected one of`
  清单改名或删除即可。）由 stdio E2E 与分发缝测试锁定。
- **字符串钳制**：查询/意图类参数 UTF-8 安全截断到 4096 字节，路径类到
  1024 字节（永不切在多字节字符中间）。
- **数值钳制**：`top_k` ∈ [1,200]、`limit` ∈ [1,500]、`max_depth` ∈
  [1,15]、`confidence_threshold` ∈ [0,1]、BFS 上限 ∈ [1,5000]。
- **集合上限**：`symbols[]` ≤ 10、文件列表 ≤ 200、`traces[]` ≤ 1000。
- **枚举校验**：非法枚举值返回 `-32602` 并列出合法值。

### 通用参数：`project_path`

除 `index`（用 `path` 设定活动项目）外，所有工具都接受可选的
`project_path`：给定时该次调用针对指定项目执行（命中项目会话的 16 槽
LRU，必要时透明重开），不改变活动项目；省略时作用于当前活动项目。
下文各工具的参数表不再重复列出。

### 错误包装

Handler 全链路使用类型化错误（`cc_model::CcError`），仅在 MCP 出口
（`mcp.rs` 的 `handler_error_data`）统一映射 JSON-RPC 错误码：

- 参数问题 → JSON-RPC error `-32602`（invalid params）。包括 schema 层
  `sanitize()` 失败，以及 handler 层的参数校验
  （`CcError::InvalidParams`：缺必填参数、未知 action、路径越界等）；
- 运行期失败 → JSON-RPC error `-32603`（internal error），消息为底层
  错误文本；
P5-A 的本地检索会核对每次尝试前后的 index/evidence epoch，最多三次；持续变化时 `CcError::RetrievalChanged` 通过同一出口返回 `-32603` 和 `data.retryable=true`。缓存正文不匹配会重新读取当前行，损坏的源码或文档清单仍是非可重试错误；这不是文件系统快照；P5-B 在该重试窗口外增加共享总期限及协作取消。

P5-B 的 search/context 已接入 MCP `notifications/cancelled`。QueryTimedOut、QueryBusy、QueryInvalidated 返回可重试错误；QueryCancelled 不自动重试。可选语义子期限失败会保留 timeout/error/unavailable lane 并在父预算允许时返回本地结果。输出继续保留 resolution_freshness；工具数量和输入字段不变，默认不调用端口。完整执行边界见 [QUERY_EXECUTION.md](internals/QUERY_EXECUTION.md)。

- **瞬态可重试错误**额外携带 `data: {"retryable": true}`：
  `CcError::BuildBusy`（"index build already in progress"，构建门被占）
  与 `CcError::StalePreparedBuild`（跨进程写者抢先提交）。客户端可据此
  程序化区分"稍后重试"与永久失败；其余错误不带 `data`；
- "查无此物"通常**不是错误**：返回合法响应并携带空结果或
  `error` 字段（如 `node` 对不存在符号返回
  `{"query": …, "error": "symbol not found"}`），让 agent 能继续。
  例外是显式定位类查询（如 `relations(kind="hierarchy")` 对不存在的
  类型）：以 `CcError::NotFound` 报 `-32603`，消息以 "no symbol named …"
  说明未命中原因。

### 输出预算

成功结果统一经出口侧 `finalize()` 应用预算（按仓库规模档位取值，见
[CONFIGURATION.md](CONFIGURATION.md#仓库规模档位)）：

| 出口策略 | 工具 | 行为 |
|---|---|---|
| ByteCap | `context`、`node`、`relations`、`impact`、`architecture` | 序列化 JSON 超过档位 `max_output_chars` 时整体替换为截断信封：`{"_truncated": true, "_original_chars", "_max_chars", "partial"}`（`partial` 是 UTF-8 安全的前缀预览） |
| ItemCap | `files`（仅 `list`） | 顶层数组截到档位 `max_items`，末尾追加 `{"_truncated": true, "_total", "_shown"}` 标记 |
| Passthrough | 其余 8 个 | 出口不截断——工具在 handler 内部用语义化预算自我约束（如 trace 的 snippet 字符预算、graph_query 的行数信封） |

### 图可解释性（graph_explain）

图读工具在有事可报时附着只增不改的 `graph_explain` 信封：`impact`
（含 `scope="circular"`）、`trace`、`relations`（含 `kind="hierarchy"`）、
`graph_query`，以及 `context`/`search` 响应里的图富化摘要。字段：
`edge_kinds_used`、`declared_edge_kinds`、`synthetic_edge_count` /
`runtime_evidence_edge_count`、`truncated` + 稳定的 `truncated_reason`
token、`read_errors`（上限 8）。干净且未截断的运行整体省略该字段。
逐工具边 kind 矩阵见
[ARCHITECTURE.md](ARCHITECTURE.md#工具--边-kind-矩阵)。

## Setup

### `status` —— 查询前先看索引健康度

| 参数 | 说明 |
|---|---|
| `aspect` | `index`（默认统计）/ `capabilities` / `schema` / `all` |

响应（按 `aspect`）：

- `index`：`project_path`、`indexed_files` / `indexed_symbols` /
  `indexed_chunks` / `indexed_call_edges` 等计数、`diagnostics`、
  `runtime_evidence`（有证据时）；
- `capabilities`：`has_index`、`has_project`、`capabilities.{search,graph,impact}`；
- `schema`：`node_kinds[]`（每项 `{kind, count}`）、`edge_counts`（表名→行数）、
  `relationship_patterns[]`（`{from, edge, to, table, description}`）、
  `edge_properties`、`example_queries[]`、`next_tool_hints`——写 Cypher 前先看这个；
- `all`：以上合并为 `{index, capabilities, schema, diagnostics, runtime_evidence?}`。

### `index` —— 指向项目并构建/更新索引

| 参数 | 说明 |
|---|---|
| `path` | 项目路径 |
| `full` | `true` 强制全量重建（默认 `false` 增量） |
| `changed_paths` | 可选：已知变更的相对路径集，增量构建走事件域扫描（只 stat/哈希这些路径，见 [internals/INDEXING.md](internals/INDEXING.md#scandiff)）。与 `full=true` 互斥；两集合合计超过 10,000 条时忽略并退回全树扫描 |
| `removed_paths` | 可选：已知删除的相对路径集，语义同上 |

响应：`IndexReport` 序列化——`files_scanned` / `files_added` /
`files_updated` / `files_removed` / `files_skipped`、`symbols_total`、
`chunks_total`、`parse_errors[]`、`elapsed_ms`、`phase_timing`
（六阶段毫秒数）、`dirty_propagation`（仅增量：`normal` /
`partial_closure` / `budget_exceeded` / `disabled`，语义见
[internals/INDEXING.md](internals/INDEXING.md#dirty-closure脏闭包)）。
`budget_exceeded` 表示仍有依赖工作：继续增量可按持久 frontier 续跑，显式全量也可恢复；以 `resolution_freshness` 判定，不把未完成状态写成正常。

P2-A 追加 `IndexReport.public_surface_coverage`：`parsed_files`、`known`、`known_empty`、`unknown`、`unknown_reasons`。只统计这次实际解析的文件，no-op 为零，不代表全仓所有事实的新鲜度。无新增工具或输入字段；详见 [internals/PUBLIC_SURFACE.md](internals/PUBLIC_SURFACE.md)。

P3-A 追加 `IndexReport.project_model`，包含配置输入摘要、目录规模、配置发现读取/解析缓存命中、根路径探测、模式与诊断。发现读取数不含提交前复核读取；模块 resolved/unresolved/unsupported 计数与符号统计分开。无新工具或输入字段，详见 [PROJECT_MODEL.md](internals/PROJECT_MODEL.md)。

P3-B扩展既有project_model报告，补充Rust源码声明读取/缓存命中及跨语言模块诊断；导入语法、配置条件与解析置信度分开。无新增工具或输入字段，默认无网络；见 [MODULE_RESOLUTION.md](internals/MODULE_RESOLUTION.md)。

P3-C沿用14个工具输入，project_model追加Go紧凑声明读取/缓存命中，resolution_coverage追加external/ambiguous/unknown及packages_resolved计数。包结果不伪造代表文件；Go imports.resolved_path为NULL时需查看模块包集合证据，实际调用/引用仍指向具体源码符号。见 [GO_MODULES.md](internals/GO_MODULES.md)。

P4-A 的 indexed search hit 新增可选 `metadata.source_evidence`：原始输入摘要、编码、字节长度、半开 `span`、切片摘要和边界依据。DB hydration 会核验切片与正文一致；这些是索引时快照坐标，不代表查询时磁盘已最新。旧或合成结果可能没有此字段，不得据此伪造 proof。工具数量与输入参数不变。详见 [SOURCE_CHUNKS.md](internals/SOURCE_CHUNKS.md)。

P4-B 的 `IndexReport.chunk_policy` 报告本次构建使用的行、字节、Unicode 标量与估算 token 限额及合并阈值。每次构建重新捕获 indexing 配置；同一 MCP 进程也能使用新的切块预算。失败文件保留旧 stamp，报告不保证所有文件已采用新规则。`metadata.source_evidence` 仍是原始源码；单独的 embedding 文本渲染 API 不新增工具或发起模型调用。见 [CHUNK_POLICY.md](internals/CHUNK_POLICY.md)。

P4-C增加可选`metadata.document`和`source_freshness`，公开hybrid结果每次核验磁盘（包含缓存命中），变更/删除/受限文件省略并标partial。源码展开使用同一核验入口；不拿旧位置解释新正文。`IndexReport.document_changes`仅统计本次投影文件，非全仓总量。14工具输入保持不变，详情见[DOCUMENTS.md](internals/DOCUMENTS.md)。

## Discovery

### `search` —— 自然语言或符号名找代码

| 参数 | 说明 |
|---|---|
| `query` | 查询串 |
| `mode` | `hybrid`（默认，FTS5+grep+图融合）/ `symbol`（符号名查找） |
| `retrieval_strategy` | 可选 local/auto/semantic，省略使用项目配置；symbol 仅接受省略/null/local |
| `top_k`、`intent`、`exact` | 数量、意图（如 `fix`）、精确匹配开关 |
| `boost_files` / `recent_files` / `pinned_files` / `overlay_files` / `path_prefix` | 前四项是软路径提示；path_prefix 是硬范围。overlay_files 不代表未保存正文已入库，见 [CONFIGURATION.md](CONFIGURATION.md#ranking)。 |
| `conversation_queries` / `file_preselect_limit` | 会话内先前查询（时近性加成）与文件预选数量上限 |

响应：

- `hybrid`：序列化的 `ContextEnvelope` —— `query`、`intent`、`summary`、
  `nodes[]`（每个命中：`title`、`file_path`、`start_line`/`end_line`、
  `score`、`confidence`、`reasons[]`（含 `preselect:<layer>:+<score>`
  等可审计的排序理由）、`metadata`）、`spans[]`、`token_estimate`、
  `evidence_summary`（图富化摘要，可含 `graph_explain`）；
  完整源码以 `machine_pack.hits` 为准；预算压缩时 nodes/spans 可省略，引用不含正文且不算命中；
- `symbol`：符号行数组（`name`、`kind`、`file_path`、`start_line`、
  `qname`、`symbol_uid` …），不带信封。

`hybrid` 范围规则（P1-A）：`path_prefix` 与 query 中的全部 `path:` 取交集；多个 `lang:` 同样取交集，互相矛盾时返回空命中。未知/空语言和空路径 DSL 返回明确参数错误。预选与 boost/recent/pinned/overlay 仅影响排名，不是搜索白名单。图补充证据也遵守范围。Rust 公共 `SearchRequest` 的 `file_paths`/`languages` 已贯通上下文查询，其中 `Some([])` 明确表示无可搜索内容；本批没有在 MCP 上新增同名参数。`symbol` 模式没有因此获得新的 DSL 能力。

P1-B 路径补充：`path_prefix` 为大小写敏感的完整路径或目录后代，`src/api` 不匹配 `src/apix`。`symbol` 模式同样在 LIMIT 前应用显式 `path_prefix`，但仍不解析新 DSL。hybrid 响应新增 `evidence_summary.retrieval.grep`：complete、limited（候选上限）、partial（扫描预算/错误）、扫描计数与原因；空列表不等于完整无答案。精确符号或完整文件路径候选带 `exact-target` 理由，优先于非精确提示匹配；不新增工具或参数。

P1-C 增量解释：`evidence_summary.retrieval.scope` 区分 hard / soft / budget / ordering；不会枚举被排除路径，缓存中的扫描计数属于原始计算。工具名与旧 hybrid/symbol 参数保持不变。当前 SDK 的未知字段反序列化拒绝是 `CallToolResult.is_error=true`；sanitize 对非法模式返回 JSON-RPC `-32602`，两种错误不要混为一种。十四工具的旧有效请求、未知字段和已有两种搜索模式有真实 stdio 回归。

P1-D 成本解释：`evidence_summary.retrieval.cost`（version 1）包含词法 FTS、候选批取的外层 SQL 计数、文本缓存/存储读取、候选数量；`grep.stages` 保留三个阶段的计数。未覆盖预选/图查询/FTS 内部 SQL，不是全部 I/O 或资源上限。缺失成本是 unavailable，缓存响应的成本属于原始计算。结构化请求示例与具体计数边界见 [检索引擎](internals/SEARCH.md#可执行请求示例)。没有新增工具、搜索模式或输入参数。

### `context` —— 一次调用拿到任务的完整上下文

| 参数 | 说明 |
|---|---|
| `task` | 任务描述 |
| `retrieval_strategy` | 可选 local/auto/semantic；显式指定走统一检索与证据装配 |
| `max_symbols`、`include_source`、`intent` | 规模与意图控制 |

响应有两种兼容形态：省略策略的本地直接符号路径可返回 `matched_symbols`、关系及可选 `symbol_details`；统一检索路径返回 `ContextEnvelope`，其 `machine_pack.hits` 为带来源证明的完整正文，`nodes/spans` 可因预算省略重复展示，`machine_pack.references` 仅作引用。`packing.partial` 和来源/通道不完整状态始终可见。`code_index_context` 使用完整对象预算，其他旧形态沿用出口 ByteCap。

## Deep dive

### `node` —— 细看单个符号

| 参数 | 说明 |
|---|---|
| `symbol` | 符号名 |
| `include` | `trail`（默认：callers+callees+源码）/ `source` / `outline` / `summary`（索引期启发式生成的文件摘要，非模型产物） |

响应（`trail`）：`source`（`file_path`、行范围、正文）、`callers[]`、
`callees[]`。符号不存在 → `{"query", "error": "symbol not found"}`；
多候选歧义 → `{"query", "candidates": [...]}`。

### `explore` —— 批量看多个符号，或追数据流

| 参数 | 说明 |
|---|---|
| `symbols[]` | 最多 10 个 |
| `mode` | `symbols`（默认）/ `flow` |
| symbols 模式 | `include_source`、`outline`、`max_callers`、`max_callees`、`max_source_per_file` |
| flow 模式 | `max_depth`、`max_paths`、`exact`、`file_path`、`max_candidates` |

响应：`symbols` 模式 → `files[]`（按文件分组，每符号含
source/callers/callees）、`total_symbols`、`truncated`；`flow` 模式 →
`paths[]`（节点+边）、`start_symbols`、`end_symbols`、`total_paths`、
`truncated`。

### `trace` —— 两个符号间的调用路径

| 参数 | 说明 |
|---|---|
| `from`、`to` | 端点符号 |
| `from_uid`、`to_uid` | 端点 UID（多候选歧义时按 `disambiguation[]` 提示精确指定） |
| `source_mode` | `none` / `snippet` / `body` / `outline` |
| `max_depth`、`max_snippet_lines`、`include_source` | 深度与片段控制（`include_source` 是 `source_mode=snippet` 的旧式开关） |

响应：`TracePathResult`——`paths[]`（每条是一个**名称路径数组**
`Vec<String>`）、`nodes[]`（`TraceNode`：`uid`/`name`/`kind`/`file_path`/
行范围/`signature`?/`snippet`?/`outgoing_calls`?，正文按 `source_mode`）、
`edges[]`（`TraceEdge`）、`path_count`；`from`/`to` 匹配多符号时带
`disambiguation[]`（用 `from_uid`/`to_uid` 消歧），无路径时带 `diagnostic`
提示。可含 `graph_explain`（HTTP/异步桥被遍历时 `synthetic_edge_count` 非零）。

## Analysis

### `relations` —— 定向查 callers/callees/引用/类型层级

| 参数 | 说明 |
|---|---|
| `symbol` | 符号名 |
| `kind` | `callers` / `callees` / `both`（默认）/ `refs` / `hierarchy` |
| `limit`、`direction` | `direction` 用于 hierarchy：`up` / `down` / `both`（`ancestors` / `descendants` 是 up/down 的别名） |

响应：`callers`/`callees` → `CallEdgeLite` 数组（`file_path`、`line`、
`caller_symbol`?/`callee_symbol`、`caller_symbol_uid`?/`callee_symbol_uid`、
`resolution_kind`、`confidence`、`dispatch_kind`、`synthesized_by`?）；
`refs` → 引用位置数组；
`hierarchy` → 祖先/后代数组（`relation_type`: supertype/subtype）。
出口 ByteCap；可含 `graph_explain`。

### `impact` —— 改动前看爆炸半径

| 参数 | 说明 |
|---|---|
| `scope` | `changes` / `tests` / `dead_code` / `circular` / `dependents` |
| `files`、`base_branch` | 显式文件集或 git 基线（默认读工作区 diff） |
| `granularity`、`confidence_threshold` | `granularity` 仅 `circular` 消费：`file`（默认）/ `package` / `community`；置信度过滤 |
| `limit` | 返回符号数上限（受档位 `max_items` 再钳制；也是 dead_code / circular 的结果上限） |
| `file_path` | `dead_code` 的范围过滤；`dependents` 的必填目标文件 |
| `max_nodes` / `max_per_layer` | changes-scope 的 BFS 上限 |

响应（按 `scope`）：

- `changes`：`ImpactReport`——`changed_files[]`、`impacted_symbols[]`、
  `suggested_tests[]`、`boundary_crossings[]`、`risk_summary`（含
  `total_impacted`）、`confidence_weighted_risk`、`cross_service_impacts[]`、
  `historical_impacts[]`、`truncated`、`returned_symbol_count`、
  `total_impacted_discovered`（BFS 被钳制时的下界）；置信度过滤是输入侧
  `confidence_threshold` 静默应用，结果不单独标记；
- `tests`：`impacted_tests[]`、`test_count`；
- `dead_code`：`dead_code[]`（含 `reason`）、`count`、`total_found`、
  `truncated`、`scan_limit`；
- `circular`：`cycles[]`（节点环 + `cycle_length`）、`count`；
- `dependents`：`file_path`、`dependents[]`、`count`（必须给
  `file_path=[一个文件]`）。

出口 ByteCap；可含 `graph_explain`。

### `architecture` —— 高层项目结构

| 参数 | 说明 |
|---|---|
| `aspect` | `overview` / `communities` / `frameworks` / `routes` / `services` / `async` / `boundaries` / `env` / `unresolved` |
| `filter`、`limit` | 按名过滤与数量 |

响应随 `aspect`：`overview` → `packages` / `languages` / `entry_points`；
`communities` → 社区列表（内部/边界边计数）；`routes` →
`route_handlers[]`（方法、路径、handler、框架）；`env` → `env_vars[]`
（键、使用计数、文件）；`unresolved` → 未解析引用列表；等等。
出口 ByteCap。

## Utilities

### `files` —— 列文件或读代码区间

| 参数 | 说明 |
|---|---|
| `action` | `list` / `region` / `expand` |
| `path`、`start_line`、`end_line`、`context_lines` | region/expand 用 |

响应：`list` → 文件数组（`file_path`、`language`、`size`、`parser_tier`、
`indexed_at`；出口 ItemCap）；
`region` → `{file_path, start_line, end_line, content, symbols[]}`；
`expand` → 扩展到符号边界后的同形结构。

### `graph_query` —— Cypher 子集查询

| 参数 | 说明 |
|---|---|
| `query` | Cypher 字符串（语法见 [CYPHER.md](CYPHER.md)） |

响应信封：`{results[], row_count, truncated, truncated_reason?,
limit_applied?, fast_path?, graph_explain?}`。`truncated_reason` 区分
`default_limit`（默认 LIMIT 50 可能裁了行）与 `output_budget`；
`fast_path` 仅变长遍历出现（见
[CYPHER.md](CYPHER.md#fast-path-元数据fast_path)）。
非法 Cypher → 错误（`-32603`，带解析诊断）。

### `ingest_traces` —— 用 OTLP 运行时痕迹验证 HTTP 边

| 参数 | 说明 |
|---|---|
| `traces[]` | 每条：`service_name`、`method`、`path`、`status_code`（≤1000 条/次） |

响应：`{accepted, matched_to_edges, routes_matched, ambiguous,
unmatched, spans_processed, total_submitted, write_errors}`。每次匹配给边的数值
置信度 +0.15（封顶 1.0，不改变解析层级），只推进 `evidence_epoch`
（见 [internals/STORAGE.md](internals/STORAGE.md#epoch-双时钟)）。

### `adr` —— 架构决策记录管理

| 参数 | 说明 |
|---|---|
| `action` | `list` / `get` / `store` / `delete` |
| `adr_id`、`title`、`status`、`context`、`decision` | store/get/delete 用 |

响应：`list` → `{adrs[]}`；`get` → 单条记录或 `error`；`store` →
`{stored: adr_id}`；`delete` → `{deleted, adr_id}`。ADR 是仓库元数据
（存于索引库 `adr` 表），不是 agent 记忆。

## 推荐使用路径

典型 agent 工作流：

```
index(path) -> status() -> context(task) -> explore(symbols) -> trace(from, to) -> graph_query(cypher)
```

1. **新任务先 `context`**。一次调用返回最相关符号、关系与源码；优先于
   手工 search + node 链。
2. **多符号用 `explore` 而不是循环 `node`**。3 个以上符号时一次
   `explore(symbols)` 按文件分组全部返回；`mode="flow"` 发现符号间
   数据/控制流路径。
3. **完整理解流程用 `trace(source_mode="body")`**。每一跳带完整函数体与
   出向调用——一次调用看懂 A 如何到达 B。
4. **改代码前用 `impact`**。`scope="changes"` 看当前 diff 的爆炸半径；
   `scope="tests"` 找受影响测试。
5. **定向查询用 `relations`**。只要某符号的 callers 或 callees 时比
   `explore` 更轻；`kind="hierarchy"` 看类型继承树。
6. **结构化工具不够再上 `graph_query`**。先 `status(aspect="schema")`
   发现节点/边类型，再写 Cypher。

## 反模式

- 有 `search()` 就别 grep/find——它是带排序的 FTS5 + grep + 预选融合。
- 要上下文别串 `search` + `node`——`context(task)` 一个往返。
- 别对一堆符号循环 `node()`——一次 `explore(symbols)` 全拿。
- 深度理解别用 `trace(include_source=true)`——用
  `trace(source_mode="body")` 拿完整函数体。
- 编辑后别手工重索引——文件变更自动检测并增量重索引
  （`.codecortex.json` 的 `auto_index.enabled`）。

## CLI 命令

```
codecortex mcp [--project-path PATH]   启动 MCP stdio 服务器
codecortex install [--force]           为检测到的 AI agent 安装 MCP 配置
codecortex uninstall                   从所有 AI agent 移除 MCP 配置
```
