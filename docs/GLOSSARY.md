# 术语表

CodeCortex 文档与代码共用的术语。按主题分组，括号内是代码中的标识符。

## 索引与存储

- **索引（index）**：`index.sqlite3` 中的全部派生数据。它是缓存而非数据
  源——任何时候都可以从源码安全重建。
- **chunk**：检索的基本单元；按 `chunk_line_budget`（默认 80 行）切分的
  代码片段，正文 zstd 压缩存储，FTS5 索引其文本。
- **symbol UID（`symbol_uid`）**：符号的全局稳定标识，跨文件边
  （call_edges、symbol_refs 等）用它绑定目标。
- **导出指纹（export fingerprint）**：文件对外可见面（导出符号集）的
  哈希；指纹变化才触发脏闭包传播。
- **epoch / 双时钟**：两个持久化版本计数器。`index_epoch` 随每次索引
  内容提交 +1；`evidence_epoch` 只随运行时证据写入 +1。所有缓存以 epoch
  为键自失效。见 [internals/STORAGE.md](internals/STORAGE.md#epoch-双时钟)。
- **generation**：某一时刻 `(index_epoch, evidence_epoch)` 的一致快照
  （`IndexGeneration`）；cc-server 侧加上 `db_identity` 构成
  `GraphReadGeneration`。
- **UnitOfWork**：多语句写事务的唯一入口；持有写连接全程，commit 恰好
  推进一次 `index_epoch`，drop 未提交即回滚。
- **rebuild-on-mismatch**：schema 版本（`user_version`，当前 v6）不匹配
  时直接重建索引，不做迁移。

## 索引管线

- **prepare / commit**：构建的两个半程。prepare 只读（扫描→解析→解析→
  压缩→快照），产出 `PreparedBuild`；commit 消费它落库。
- **三段提交（staged commit）**：commit 拆为 `commit_write`（写锁）→
  `compute_postprocess`（无锁计算）→ `apply_postprocess`（短写锁
  apply）。见 [ADR-0002](adr/0002-staged-commit-postprocess-out-of-write-lock.md)。
- **脏闭包（dirty closure）**：增量构建时，把导出面变化文件的导入者
  提升为重解析的不动点循环；结束方式分类为
  `normal` / `partial_closure` / `budget_exceeded` / `disabled`。
- **pass**：postprocess/analysis 阶段的一个独立工作单元（test edges、
  各 dispatch 合成、Louvain、git 共变、infra、ADR）。dispatch 合成 pass
  声明为 `SynthesisPassSpec`（`dispatch_synthesis/mod.rs` 的 `registry()`）。
- **PassGate（门）**：pass 的声明式跳过条件（输入签名未变即跳过）；
  trait `PassGate`（[`pass_gate.rs`](../crates/cc-index/src/pass_gate.rs)），
  参考实现 `DbSignatureGate`；决策在 compute 阶段，签名记账
  （`DeferredSignatureRecord`）推迟到 apply 落库之后。
- **dispatch synthesis（派发合成）**：为动态派发（事件、JSX/Vue 渲染、
  接口派发等）合成调用边的后处理 pass 族，每个声明为
  `SynthesisPassSpec`。
- **解析阶梯（RESOLVE_LADDER）**：跨文件名字解析的有序策略列表；命中的
  阶梯步骤名持久化为边上的 `resolution_strategy`。
- **catalog cache（符号目录跨构建缓存）**：构建完成的 `SymbolCatalog`
  停靠在 `IndexDb` 句柄的类型擦除槽上跨构建复用，以 `symbols_seed`
  聚合 token 证明有效性；取用时按文件删除被排除条目，写后折叠存回。
  消掉增量 resolve 的 O(仓库符号数) 重建地板。见
  [internals/INDEXING.md](internals/INDEXING.md#符号目录跨构建缓存catalog-cache)。
- **build gate**：每项目一个的 `Mutex<()>`，串行化所有构建入口；锁序
  规则见 [internals/CONCURRENCY.md](internals/CONCURRENCY.md#锁序规则)。

## 检索

- **lane（检索通道）**：一种检索策略适配器（`RetrievalLane`）：lexical
  （FTS5）、grep、graph 三条，结果进 RRF 融合。
- **preselect（文件预选）/ layer（预选层）**：chunk 级检索前的文件打分
  阶段；8 个 `PreselectLayer` 适配器按注册顺序执行，逐层得分进入命中
  理由（`preselect:<layer>:+<score>`）。
- **RRF（Reciprocal Rank Fusion）**：`Σ weight × 1/(k + rank)` 的多通道
  排名融合。
- **rerank（重排）**：RRF 之后对窗口内候选叠加路径/时近/图连通度等加成。
- **ContextEnvelope**：`search`（hybrid）与 `context` 的响应载体：
  `nodes[]` + `spans[]` + 排序理由 + token 预算估计。
- **意图（intent）**：查询的任务意图（如 `fix`），影响排序侧重。

## 图与工具面

- **图目录（graph catalog / `tool_graph_subsets`）**：每个工具面消费
  哪些边 kind 的唯一声明点，测试机器校验。
- **GraphExplain**：图读工具共享的只增信封：实际遍历的边 kind、合成边/
  证据边计数、`truncated_reason`、`read_errors`。
- **合成边（synthetic edge）**：非源码直接声明、由派发合成或桥接产生的
  边（如 `http_bridge`）。
- **框架检测信号层（`framework_registry`）**：framework 管线第一段
  （`FrameworkSignalSpec → framework_key`）。把索引数据（imports / file_path /
  route_edges / symbol patterns / 包清单）折算成多信号加权分数，输出
  `framework_key` + confidence 并持久化，回答「仓库/文件用了哪个框架」。
  taxonomy 单一声明源在 `cc_model::framework_taxonomy`。
- **框架路由解析层（`framework_resolvers`）**：framework 管线第二段
  （`framework_key → 路由边`）。`FrameworkResolver` trait 消费检测层产出的
  framework_key，把框架特有语义（路由声明、装饰器、JSX 组件等）解析成
  `RouteEdgeRecord` 并尽量绑定 handler 的 symbol UID，是框架路由边的主要来源。
- **运行时证据（runtime evidence）**：经 `ingest_traces` 摄入的 OTLP
  痕迹；每次匹配给 HTTP 边的数值置信度 +0.15（封顶 1.0），不改变
  解析层级（`parser_tier`）。
- **fast path**：Cypher 变长 `CALLS` 遍历的惰性 BFS 快路径
  （[ADR-0001](adr/0001-cypher-traversal-lazy-bfs-fast-path.md)）；
  不合格时回落递归 CTE，原因见响应的 `fast_path.reason`。
- **社区（community）**：Louvain 检测出的符号聚类，作为架构边界与
  影响分析的输入。

## 服务与生命周期

- **仓库规模档位（repo size tier）**：按文件数分 Tiny/Small/Medium/Large
  四档，决定输出预算与默认 `top_k`（见
  [CONFIGURATION.md](CONFIGURATION.md#仓库规模档位)）。
- **输出预算（output budget）**：工具响应的出口侧限额；三种策略
  （ByteCap / ItemCap / Passthrough）见
  [MCP_TOOLS.md](MCP_TOOLS.md#输出预算)。
- **空闲驱逐（idle eviction）**：MCP 无活动超时后关闭会话持有的全部
  项目实例（active + LRU 缓存）的 DB 句柄，下次调用透明重开。
- **acquire-before-drain**：watcher 先抢到构建席位再消费事件队列的
  无损协议。
- **置信度分层（confidence tier）**：提取来源的可信度档
  （Generic 0.3 → Verified 0.95），与解析期的
  `resolution_confidence` 是两个独立概念。

## 版本命名空间

仓库里有四套互相独立、各管一事的版本号，彼此不联动：

| 版本号 | 当前值 | 含义 | 位置 |
|---|---|---|---|
| Cargo workspace 版本 | `1.0.0` | 发布的 crate 版本 | 根 `Cargo.toml` `workspace.package` |
| 设计文档修订号 | `2.4` | `DESIGN.md` 自身的修订版本 | `DESIGN.md` 头部 |
| schema 版本 | `v6` | SQLite 表结构版本（`user_version`），不匹配即重建 | `cc-db/src/index_migrate.rs` `CURRENT_SCHEMA_VERSION` |
| `index_version` | `1.0.0` | 写进 `metadata` 表的索引产物语义版本 | `indexer_phases/write.rs` |

## 哈希分工

两个哈希库并存是分工而非重复：**blake3** 负责语义/ID 域（`StableId`、
导出指纹、合成边 ID），**sha2 (SHA-256)** 负责文件内容哈希（scan/diff
的变更确认、config-linker 的内容签名），与外部工具的 digest 习惯一致；
`signature_agg` 的行哈希聚合另用 std `DefaultHasher`（进程内比较，不需
密码学强度）。
