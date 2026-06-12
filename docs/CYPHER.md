# Cypher 子集

`graph_query` 在 CodeCortex 的 SQLite 图 schema 之上支持一个刻意精简的、
只读的类 Cypher 子集。引擎实现视角见
[internals/SEARCH.md](internals/SEARCH.md#cypher-子集引擎)。

## 支持的子句

- `MATCH`，带节点 label 与关系类型。
- `OPTIONAL MATCH`，单跳可选关系，包括锚定的两子句形式
  `MATCH (f:Label) OPTIONAL MATCH (f)-[:R]->(g)`——无目标匹配时保留源
  节点（目标列为 NULL）。
- `WHERE`：`=`、`<>`、比较运算、`AND`、`OR`、`CONTAINS`、`STARTS WITH`、
  `ENDS WITH`，以及 `=~` 正则（经 Rust `regex` crate 的 SQLite REGEXP）。
- 变长关系：`*`、`*N`、`*1..N`、`*..N`。
- `RETURN`、`AS` 别名、`DISTINCT`。
- 聚合：`COUNT`、`SUM`、`AVG`、`MIN`、`MAX`、`COLLECT`，含已实现的
  `DISTINCT` 变体。
- `ORDER BY` 与 `LIMIT`。
- `UNION` / `UNION ALL`（各分支返回列数相同时）。

## 支持的 label 与关系

权威的运行时清单用 `status(aspect="schema")` 查。常见 label：`File`、
`Function`、`Class`、`Method`、`Route`、`Package`。常见关系：`CALLS`、
`DEFINES`、`DEFINES_METHOD`、`CONTAINS_FILE`、`CONTAINS_MODULE` 及 HTTP
路由/调用边。

## 正则（`=~`）

`=~` 用 Rust `regex` crate 编译模式，经自定义 SQLite `REGEXP` 函数执行。
[`regex::Regex`](https://docs.rs/regex/latest/regex/) 接受的语法都可用：
字符类、并联、锚点、量词等。

非法模式产生显式 SQL 错误，不会静默返回 false。

## fast path 元数据（fast_path）

变长 `CALLS` 遍历可由惰性 BFS fast path 服务，替代递归 SQL CTE
（ADR-0001）；两条路径结果完全一致。`graph_query` 信封通过只增的
`fast_path` 字段报告本次由哪个引擎执行：

- 合格遍历由惰性 BFS 服务：`"fast_path": { "used": true }`。
- 回落到 SQL CTE 的变长查询：
  `"fast_path": { "used": false, "reason": "<token>" }`，`reason` 是
  稳定的、快照锁定的 token，点名未通过的资格检查，如
  `no_where_clause`、`edge_kind_not_eligible(IMPORTS)`、
  `return_not_simple_property`、`limit_too_large(5000>1000)`。
- 环境开关禁用：
  `"fast_path": { "used": false, "reason": "disabled(CODECORTEX_CYPHER_FAST_PATH=0)" }`。
- 从不走 fast path 的查询形态（单节点、单跳、`OPTIONAL MATCH`、`UNION`）
  整体省略该字段——缺席意味着"不是变长遍历"，而不是"回落了"。

reason token 只是建议性的：它解释延迟，并提示如何把查询改造成 fast path
形态（单 `MATCH`、单个变长 `CALLS` 段、`name`/`symbol_uid` 字符串等值
钉住源点、简单属性 `RETURN`、`LIMIT <= 1000`）。它从不影响结果内容。
fast path 的合格边 kind 集合就是图目录里的共享声明
`tool_graph_subsets::CYPHER_FAST_PATH`（仅 `CALLS`——唯一有惰性邻接源的
kind），门与目录不可能漂移。

## 有意为之的限制

- 只读：没有 `CREATE`、`MERGE`、`DELETE`、`SET`、`WITH`、`UNWIND`。
- `LIMIT` 省略时默认 50。`graph_query` 返回信封
  `{ results, row_count, truncated, truncated_reason, limit_applied }`：
  默认 limit 可能裁掉行时置 `truncated: true` 且
  `truncated_reason: "default_limit"`（服务端条目预算截断时为
  `"output_budget"`），调用方能区分完整结果集与被钳制的结果集。截断时
  响应还携带只增的 `graph_explain` 信封（同样的
  `truncated`/`truncated_reason` 加工具声明的边 kinds）；干净的运行省略
  它（见 [MCP_TOOLS.md](MCP_TOOLS.md#图可解释性graph_explain)）。
- `OPTIONAL MATCH` 支持单跳可选关系——独立使用，或作为第二子句锚定在
  共享源变量的前置单节点 `MATCH` 上。多个 optional 子句的链不支持。
- 变长路径（`*1..N`）钳制到最多 32 跳，只支持 `CALLS`、`DEFINES`、
  `DEFINES_METHOD`、`CONTAINS_FILE`、`CONTAINS_MODULE` 边类型。不同边
  类型的多跳链不支持。
- 变长遍历是**可达性**语义：返回跳数范围内可达的节点集合（去重），
  不枚举不同路径。路径多重性不保留——对变长匹配做 `COUNT(*)` 这类聚合
  数的是可达节点，不是路径。
- 节点 label、关系类型及其属性的权威清单用 `status(aspect="schema")`。
