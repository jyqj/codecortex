# 架构总览

CodeCortex 是一个纯代码智能引擎：对代码库构建语义索引，通过 MCP 暴露。
没有 UI、没有会话/工作流/记忆系统，磁盘上只有一个数据库
（`index.sqlite3`）。设计哲学与明确的非目标见
[`DESIGN.md`](../DESIGN.md)。

本文是地图。每个子系统的深入文档在 `internals/`：

| 深入文档 | 内容 |
|---|---|
| [internals/STORAGE.md](internals/STORAGE.md) | cc-db：连接模型、UnitOfWork、epoch 双时钟、21 张表、FTS5 双维护、重建协议 |
| [internals/INDEXING.md](internals/INDEXING.md) | cc-index：八阶段管线、脏闭包、解析阶梯、PassGate、dispatch 合成、三段提交 |
| [internals/SEARCH.md](internals/SEARCH.md) | cc-search：检索通道、文件预选、RRF/重排、缓存、Cypher fast path |
| [internals/CONCURRENCY.md](internals/CONCURRENCY.md) | 锁清单与锁序、一致性窗口、watcher、会话生命周期、epoch 失效协议 |

## Crate 布局

7 crate 的 Cargo 工作区，依赖严格单向（无环）。每个 crate 都能独立编译和
测试：

```
cc-model      数据类型、配置、错误定义（serde、thiserror、blake3）
    |
cc-parsers    tree-sitter AST 提取 + 框架检测（仅依赖 cc-model）
cc-db         SQLite 索引存储（r2d2 读池、WAL、FTS5、21 表 + 5 FTS5、schema v6）
    |
cc-index      文件扫描、增量索引、Louvain 社区检测（依赖 cc-db + cc-parsers）
cc-search     排序式本地检索（FTS5 + grep + 预选/RRF）、Cypher 子集引擎（依赖 cc-model + cc-db）
    |
cc-server     MCP 服务器（rmcp）、CLI（clap）、CodeIndex 引擎、ImpactAnalyzer、FileWatcher
    |
cc-eval       检索质量与延迟的评测套件
```

第二层并列：cc-parsers 与 cc-db 都只依赖 cc-model、互不依赖。第三层
并列：cc-index 依赖 cc-db + cc-parsers，cc-search 只依赖 cc-model 与
cc-db（不依赖 cc-parsers/cc-index），二者互不依赖，在 cc-server 汇合。

各 crate 的一句话职责：

| Crate | 职责 |
|---|---|
| cc-model | 共享数据类型（`ContextEnvelope`、`SearchRequest`、`ImpactReport`、`GraphExplain`…）、`ProjectConfig`、`IndexPaths`、图目录（`graph_catalog.rs`） |
| cc-db | 单库持久化与写隔离；epoch 向量的唯一拥有者 |
| cc-parsers | 30 种语言标识符的符号/边提取（10 种全 AST），置信度分层见 [LANGUAGES.md](LANGUAGES.md) |
| cc-index | 把解析产物变成一致的增量索引；后处理与分析 pass |
| cc-search | 确定性本地检索与只读图查询 |
| cc-server | 工具面（14 个 MCP 工具）、项目会话、构建编排、图读模型 |
| cc-eval | 走真实 MCP 线路的评测与基准（见 [TEST_PLAN.md](TEST_PLAN.md)、[BENCHMARK.md](BENCHMARK.md)） |

## 数据流

```
源文件
    |  gitignore 感知扫描，mtime+size 快路径 + 哈希确认
    v
tree-sitter 解析  -->  符号、调用边、导入、test edges、路由、
    |                  数据流边、HTTP 调用边、语义边、dispatch sites
    v
脏闭包 + 框架富化 + 名字解析（符号/类型目录，跨文件 UID 绑定）
    v
SQLite 索引（index.sqlite3）
    |  写入：增量原子批 或 全量重建（temp-db / direct writer）
    |
    |<-- postprocess（写后、读已提交快照、写回）：test edges、
    |    dispatch 合成、Louvain 社区 —— 各自有输入签名门
    |<-- analysis：git 共变、infra pass、ADR 索引
    |
    +--> FTS5 全文检索
    +--> regex 符号 grep
    +--> trigram 文件预选
    |
    v
RRF 融合 + 重排  -->  ContextEnvelope  -->  MCP 工具响应
```

## 关键不变式

改动任何子系统前，先确认没有破坏这几条：

1. **单库**：所有状态在 `index.sqlite3`；索引是缓存，可随时重建
   （schema 不匹配即重建，不做迁移）。
2. **依赖单向**：crate 只向下依赖；图邻接缓存留在 cc-server、不下沉
   （cc-db 只拥有持久化 epoch 向量，ADR-0001 的决定）；所有跨请求
   缓存以 epoch 为键自失效。
3. **写隔离**：多语句写只经 `UnitOfWork`；commit 恰好推进一次
   `index_epoch`；未 commit 即回滚。
4. **双时钟失效**：索引内容走 `index_epoch`，运行时证据走
   `evidence_epoch`；所有缓存以 epoch 为键自失效，没有常规的手工失效
   钩子（唯一声明过的防御性例外：RwLock 毒锁恢复路径的
   `invalidate_search_cache_after_poison`，宁可丢缓存不放大半写状态）。
5. **构建串行**：每项目一个 build gate 串行化所有构建入口；gate 先于
   RwLock，持 RwLock 不等 gate。
6. **prepare 无锁、commit 三段**：读者最多被两段短写锁打断；接受
   后处理产物的最终一致窗口（ADR-0002）。
7. **确定性离线**：解析、FTS5、grep、预选、Louvain 全部本地；检索只用
   词法/结构信号，无外部模型。
8. **读失败可见**：图读路径的 DB 错误进 `graph_explain.read_errors`，
   不静默吞掉。

## 关键组件（cc-server）

- **CodeIndex**（`engine.rs` + `engine_query.rs`）——包装 cc-db +
  cc-index + cc-search。生命周期与共享设施在本体上；查询面按能力分三个
  零成本借用视图：`.search()`（`search_in_context`、`task_symbols`）、
  `.graph()`（`find_symbol`、`graph_query`、callers/callees…）、
  `.impact()`（`detect_impact`、`analyze_impact`、
  `find_impacted_tests`）。每个实例携带一个 per-project build gate
  （见 [CONCURRENCY.md](internals/CONCURRENCY.md#锁序规则)）。
- **ProjectSession**——归一化项目路径 → `CodeIndex` 的 16 槽 LRU；
  空闲 60 秒驱逐、透明重开。
- **GraphReadModel**（`graph_read_model/`）——trace/flow/cycles/impact
  共享的读路径：邻接加载、邻域 BFS、语义边投影、HTTP/异步桥接合成。
  进程级缓存按 `GraphReadGeneration` 键控、按 `EpochSensitivity` 失效。
- **symbol_resolution**（`symbol_resolution.rs`）——`trace_path` /
  `explore_flow` / `type_hierarchy` 共享的符号名 → 候选消歧管线；逐工具
  差异（精确 vs LIKE、文件过滤语义、kind 过滤）固定在 `ResolutionOpts`
  预设（`for_trace` / `for_flow` / `for_type_hierarchy`）里。
- **ImpactAnalyzer**——BFS 反向调用者扩展 + 社区边界检测 + 跨服务 HTTP
  影响 + git 共变。git 集成读 unstaged、staged、untracked 与
  `base...HEAD` 差异。
- **FileWatcher**——自适应去抖 + acquire-before-drain 的无损轮询
  （见 [CONCURRENCY.md](internals/CONCURRENCY.md#filewatcher-acquire-before-drain)）。

## 图可解释性（GraphExplain）

图读工具附着一个共享的、只增不改的信封
（`cc-model/src/graph_explain.rs`），而不是各自发明截断元数据：

- `edge_kinds_used` —— 实际遍历/投影到的边 kind（如 `call` /
  `http_bridge`），按首见顺序去重；
- `declared_edge_kinds` —— 该工具在 `tool_graph_subsets` 里的静态边 kind
  契约（纯契约元数据，不会让空信封变得值得附着）；
- `synthetic_edge_count` / `runtime_evidence_edge_count` —— 遍历到的边里
  多少是合成的（HTTP/异步桥）、多少有运行时验证；
- `truncated` + `truncated_reason` —— **第一个**裁剪结果的原因的稳定
  token（`output_budget`、`default_limit`、`max_depth`、`max_paths`、
  `max_expansions`、`max_nodes`、`max_per_layer`、`result_limit`、
  `bridge_cap`、`db_error:<op>` …）；
- `read_errors`（上限 8，溢出计入 `read_errors_dropped`）—— 被降级为
  部分/空结果而非令调用失败的 DB 读错误。

信封跨 `impact`、`trace`、`graph_query`、`relations`、`type_hierarchy`、
循环依赖分析与搜索图富化（`context`/`search`）附着；空信封序列化为 `{}`
并整体省略。

**构建侧对偶 `BuildExplain`**（`cc-model/src/build_explain.rs`）：把 postprocess/
analysis 的签名门决策（run/skip + 原因）与降级信号收进 `IndexReport.build_explain`
（空则省略）。读侧解释"遍历了什么/为何截断"，构建侧解释"为何合成/社区/git 共变
被跳过或降级"。见 [internals/INDEXING.md](internals/INDEXING.md#buildexplain构建侧决策信封)。

### 工具 → 边 kind 矩阵

`tool_graph_subsets`（`cc-model/src/graph_catalog.rs`）是每个工具面消费
哪些目录边 kind 的唯一声明点，由目录一致性矩阵快照测试机器校验。声明
只关可见性，不改变工具实际遍历什么。

| 工具面 | 边 kinds |
|---|---|
| CYPHER_FAST_PATH | CALLS |
| CYCLES | IMPORTS, CALLS |
| FLOW | CALLS, HANDLES, HTTP_CALLS, ASYNC_CALLS |
| IMPACT | CALLS, TESTS, HANDLES, HTTP_CALLS, ASYNC_CALLS, CO_CHANGE |
| RELATIONS | CALLS, SEMANTIC, REFERENCES |
| SEARCH_ENRICH | CALLS, REFERENCES, TESTS |
| TRACE | CALLS, HANDLES, HTTP_CALLS, ASYNC_CALLS |
| TYPE_HIERARCHY | INHERITS, IMPLEMENTS, DEFINES_METHOD |

cc-search 的 `FastPathConfig::DEFAULT.eligible_edge_kinds` 直接引用
`CYPHER_FAST_PATH`，门与目录不可能漂移（ADR-0001，2026-06-11 更新）。

读侧**虚拟**桥边（`http_bridge` / `async_bridge`）不在 `tool_graph_subsets`
里——它们不持久化，按 `GraphReadGeneration` 即时投影自 `http_call_edges` +
routes。这类虚拟 kind 在 `cc-server/src/graph_read_model/bridge_spec.rs` 的封闭
`bridge_registry()` 声明（`bridges.rs` 经 `dispatch_kind_for` 消费，单一来源），
由一致性测试机器校验，是 catalog 封闭集的平行声明点。

## 置信度分层

| 层 | 分值 | 来源 |
|---|---|---|
| Generic | 0.3 | 正则提取 |
| Heuristic | 0.5 | 带语言感知的模式匹配 |
| TreeSitter | 0.7 | 完整 AST 解析 |
| Semantic | 0.85 | 完整 AST + 更深的文件内语义提取 |
| Verified | 0.95 | 运行时验证（经 `ingest_traces`） |

注：`ingest_traces` 的证据 boost 只做数值置信度提升（每次匹配 +0.15、
封顶 1.0），不会把边迁移到 Verified 层；当前唯一写入 Verified 层的是
目录包含边（`cc-index/src/hierarchy.rs` 的 `ContainsFile`）。

按元素 kind 对层默认值的偏离单源化在
`ParserTier::element_confidence`（`cc-model/src/lib.rs`）；矩阵见
[LANGUAGES.md](LANGUAGES.md)。跨文件解析在 cc-index 单独赋
`resolution_confidence`，是另一个概念。

## 扩展点

下面每条缝都是单一注册点的 trait 或声明式 spec；新增实现不需要改别处。

| 要新增… | 缝（trait/类型） | 注册点 | 参考适配器 |
|---|---|---|---|
| 检索通道 | `RetrievalLane` | `default_lanes()`（[`cc-search/src/lanes.rs`](../crates/cc-search/src/lanes.rs)） | `GraphLane` |
| 预选层 | `PreselectLayer` | `default_preselect_layers()`（[`cc-search/src/preselect.rs`](../crates/cc-search/src/preselect.rs)） | `GraphNeighborLayer` |
| 框架路由 resolver | `FrameworkResolver` | `default_registry()`（[`cc-index/src/framework_resolvers/mod.rs`](../crates/cc-index/src/framework_resolvers/mod.rs)） | [`fastapi.rs`](../crates/cc-index/src/framework_resolvers/fastapi.rs) |
| 框架检测信号 | `FrameworkSignalSpec` | `signal_registry()`（[`cc-index/src/framework_registry/mod.rs`](../crates/cc-index/src/framework_registry/mod.rs)） | [`import_marker.rs`](../crates/cc-index/src/framework_registry/import_marker.rs) |
| 语言（无 tree-sitter 语法） | `LangSpec` | [`cc-parsers/src/lang_spec.rs`](../crates/cc-parsers/src/lang_spec.rs) + `ParserRegistry`（[`cc-parsers/src/lib.rs`](../crates/cc-parsers/src/lib.rs)） | `CSHARP_SPEC` |
| 合成边 pass | `SynthesisPassSpec` | `registry()`（[`cc-index/src/dispatch_synthesis/mod.rs`](../crates/cc-index/src/dispatch_synthesis/mod.rs)） | [`event_emitter.rs`](../crates/cc-index/src/dispatch_synthesis/event_emitter.rs) |
| 后处理跳过门 | `PassGate` | [`cc-index/src/pass_gate.rs`](../crates/cc-index/src/pass_gate.rs)，由 `indexer_phases/` 的 compute 阶段（postprocess/analysis）消费 | `DbSignatureGate` |
| 多语句写 | `UnitOfWork` | [`cc-db/src/unit_of_work.rs`](../crates/cc-db/src/unit_of_work.rs)，经 `IndexDb::writes().begin_unit_of_work()` 进入 | [`synthesis_pipeline.rs`](../crates/cc-index/src/synthesis_pipeline.rs) 的合成 apply |

各缝的注意事项（顺序敏感性、门的记账协议等）见对应 internals 文档的
扩展点小节。
