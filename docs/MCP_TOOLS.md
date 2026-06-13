# MCP 工具参考

14 个工具全部常驻可用——没有激活步骤、没有域系统。本文列出每个工具的
用途、关键参数、**响应形态**与错误路径；权威的运行时口径以
`status(aspect="capabilities")` 为准。术语见 [GLOSSARY.md](GLOSSARY.md)。

## 通用契约

所有工具共享同一套参数与输出契约（实现：`crates/cc-server/src/tools.rs`、
`handlers/output_budget.rs`）：

### 参数校验（sanitize）

每个工具的参数在分发前经过 `sanitize()`：

- **未知参数名直接拒绝**：JSON-RPC `-32602` invalid-params，错误信息原样
  携带 serde 诊断，如
  ``failed to deserialize parameters: unknown field `qurey`, expected one of `query`, `mode`, `top_k`, ...``
  ——拼错的参数立刻失败，而不是静默按默认值运行。（早期版本会静默忽略
  未知参数；依赖旧行为的客户端按报错里的字段名与 `expected one of`
  清单改名或删除即可。）
- **字符串钳制**：查询/意图类参数 UTF-8 安全截断到 4096 字节，路径类到
  1024 字节（永不切在多字节字符中间）。
- **数值钳制**：`top_k` ∈ [1,200]、`limit` ∈ [1,500]、`max_depth` ∈
  [1,15]、`confidence_threshold` ∈ [0,1]、BFS 上限 ∈ [1,5000]。
- **集合上限**：`symbols[]` ≤ 10、文件列表 ≤ 200、`traces[]` ≤ 1000。
- **枚举校验**：非法枚举值返回 `-32602` 并列出合法值。

### 错误包装

- 参数问题 → JSON-RPC error `-32602`（invalid params）；
- 运行期失败 → JSON-RPC error `-32603`（internal error），消息为底层
  错误文本；
- "查无此物"通常**不是错误**：返回合法响应并携带空结果或
  `error` 字段（如 `node` 对不存在符号返回
  `{"query": …, "error": "symbol not found"}`），让 agent 能继续。

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

响应：`IndexReport` 序列化——`files_scanned` / `files_added` /
`files_updated` / `files_removed` / `files_skipped`、`symbols_total`、
`chunks_total`、`parse_errors[]`、`elapsed_ms`、`phase_timing`
（六阶段毫秒数）、`dirty_propagation`（仅增量：`normal` /
`partial_closure` / `budget_exceeded` / `disabled`，语义见
[internals/INDEXING.md](internals/INDEXING.md#dirty-closure脏闭包)）。
`budget_exceeded` 意味着跨文件引用可能过期，建议 `index(full=true)`。

## Discovery

### `search` —— 自然语言或符号名找代码

| 参数 | 说明 |
|---|---|
| `query` | 查询串 |
| `mode` | `hybrid`（默认，FTS5+grep+图融合）/ `symbol`（符号名查找） |
| `top_k`、`intent`、`exact` | 数量、意图（如 `fix`）、精确匹配开关 |
| `boost_files` / `recent_files` / `pinned_files` / `path_prefix` 等 | 上下文加成与范围限定（见 [CONFIGURATION.md](CONFIGURATION.md#ranking)） |

响应：

- `hybrid`：序列化的 `ContextEnvelope` —— `query`、`intent`、`summary`、
  `nodes[]`（每个命中：`title`、`file_path`、`start_line`/`end_line`、
  `score`、`confidence`、`reasons[]`（含 `preselect:<layer>:+<score>`
  等可审计的排序理由）、`metadata`）、`spans[]`、`token_estimate`、
  `evidence_summary`（图富化摘要，可含 `graph_explain`）；
- `symbol`：符号行数组（`name`、`kind`、`file_path`、`start_line`、
  `qname`、`symbol_uid` …），不带信封。

### `context` —— 一次调用拿到任务的完整上下文

| 参数 | 说明 |
|---|---|
| `task` | 任务描述 |
| `max_symbols`、`include_source`、`intent` | 规模与意图控制 |

响应：`ContextEnvelope`——`task`、`intent`、`query`、`summary`、`nodes[]`
（命中符号：`title`、`file_path`、行范围、`score`、`confidence`、`reasons[]`、
`metadata`）、`spans[]`、`token_estimate`、`evidence_summary`，外加
`include_source=true` 时按文件分组的 `symbol_details`。出口 ByteCap。

## Deep dive

### `node` —— 细看单个符号

| 参数 | 说明 |
|---|---|
| `symbol` | 符号名 |
| `include` | `trail`（默认：callers+callees+源码）/ `source` / `outline` / `summary` |

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
| `source_mode` | `none` / `snippet` / `body` / `outline` |
| `max_depth`、`max_snippet_lines` | 深度与片段控制 |

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
| `limit`、`direction` | `direction` 用于 hierarchy：`up` / `down` / `both` |

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
| `granularity`、`confidence_threshold` | 粒度与置信度过滤 |
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
