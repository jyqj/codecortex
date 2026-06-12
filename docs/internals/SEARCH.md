# 检索引擎（cc-search）

> 范围：`crates/cc-search` —— 排序式本地检索的通道（lane）、文件预选
> （preselect）、RRF 融合与重排、多级缓存，以及 Cypher 子集引擎的实现
> 视角。面向需要调整排序行为或新增检索通道的开发者。Cypher 的查询语法与
> 工具契约见 [CYPHER.md](../CYPHER.md)；可调权重见
> [CONFIGURATION.md](../CONFIGURATION.md)。

设计前提：**确定性、离线、纯词法/结构信号**。没有外部模型依赖，没有网络
调用；同一索引 + 同一配置 + 同一查询必然得到同一结果。

## 一次搜索的流程

```
查询
  │
  ├─ 文件预选（preselect）：给候选文件打分，收窄 chunk 级检索范围
  │
  ├─ 检索通道（lanes，确定性顺序）：
  │     lexical（FTS5 over chunks）
  │     grep（regex/子串 over symbols）
  │     graph（种子符号 + 调用图 1 跳扩展）
  │
  ├─ RRF 融合：score = Σ lane_weight × 1/(rrf_k + rank)
  │
  ├─ 重排（rerank_window 内）：文件路径 / breadcrumb / 时近性 /
  │     图连通度 / 查询重叠 等加成
  │
  └─ （默认入口）图富化：callers/callees/tests 摘要附着到命中上
        → ContextEnvelope
```

## 检索通道（RetrievalLane）

`lanes.rs` 是引擎与检索策略之间的缝：每条通道实现 `RetrievalLane`
trait，注册在 `default_lanes()`。今天有三条适配器：

| 通道 | 数据源 | 候选上限 | RRF 权重 |
|---|---|---|---|
| lexical | `chunks_fts`（FTS5） | `search.lexical_top_k`（24） | `search.lexical_weight`（1.1） |
| grep | symbols 表的 regex/子串扫描 | `search.grep_top_k`（12） | `search.grep_weight`（0.8） |
| graph | 种子符号 + 调用边 1 跳扩展 | `search.graph_top_k`（12） | `search.graph_weight`（0.6；0 关闭） |

注册顺序即确定性的 RRF 融合顺序。新增一条通道只需实现 trait 并追加到
`default_lanes()`，不需要改 `plan.rs` / `engine.rs`。

graph 通道的种子打分与衰减由 `RankingConfig` 控制
（`graph_seed_exact_score` / `graph_seed_fuzzy_score` /
`graph_neighbor_decay`）。

## 文件预选（PreselectLayer）

`preselect.rs`。chunk 级检索之前先对文件打分收窄范围。8 个已注册的层
适配器（与 lane 同样的缝隙风格，注册在 `default_preselect_layers()`，
顺序即执行顺序）：

1. working set（调用方 `boost_files`）
2. recent（`recent_files`）
3. pinned（`pinned_files`）
4. overlay（脏缓冲区 `overlay_files`）
5. FTS summary（`files_fts` 的 bm25）
6. symbol/path tokens（`symbols_fts` / `file_paths_fts` 两张 trigram 镜像
   支撑的子串符号与路径 token 查找）
7. `FallbackLayer`（门控：仅当前面的主层一分未得时点火，给最近索引的
   文件兜底分）
8. graph-neighbor 扩展（以前面所有层的结果为种子做 1 跳调用图邻居）

四个上下文层的打分模型是 `max(floor, scale / rank)`；全部常量在
`RankingConfig`（`preselect_*` 字段，见
[CONFIGURATION.md](../CONFIGURATION.md#ranking)）。显式限定文件
（`file_paths`）拿短路分 `preselect_explicit_scope_score`（10.0）。

`PreselectResult` 携带逐层得分明细（`layer_scores`），并以
`preselect:<layer>:+<score>` 的形式进入命中理由——任何文件为什么被预选
进来是可审计的。

## RRF 与重排

- **RRF**（Reciprocal Rank Fusion）：`1 / (rrf_k + rank)` 加权求和，
  `search.rrf_k` 默认 50，值越大排名差异越平。
- **重排**：前 `search.rerank_window`（40）个融合候选进入重排，叠加
  `RankingConfig` 的各项加成（符号精确匹配、路径前缀、文档文件、
  working-set/recent/pinned/overlay、preselect 分映射、DSL `name:`
  过滤命中、图连通度 `graph_rerank_weight`、查询 token 重叠
  `overlap_weight`）。每一项都是独立可调、可置零的。

## 缓存

`engine.rs` 维护三级 LRU，全部 epoch 键控、随写自动失效（每次 cc-db 写
事务都推进持久化 epoch，无需手工失效钩子）：

| 缓存 | 键 | 容量环境变量（默认） |
|---|---|---|
| 核心结果缓存 | `(index_epoch, query_hash)` | `CODECORTEX_SEARCH_RESULT_CACHE_SIZE`（32） |
| 图感知结果缓存 | `(index_epoch, evidence_epoch, 请求哈希, GraphEnrichLimits, token 预算, 排序指纹)` | `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`（32） |
| chunk 正文缓存 | chunk id | `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`（512） |

- 核心结果缓存存的是最终不可变命中列表 `Arc<[SearchHit]>`——
  `SearchEngine::search()` 直接返回这个 Arc，缓存命中是一次指针克隆，
  没有逐命中的文本拷贝。
- 图感知缓存服务于 `search_with_graph_context`（agent 默认入口
  `search_in_context` 的底层）。键覆盖**两个** epoch：运行时证据摄入会
  改变富化节点里内嵌的边置信度，所以 `evidence_epoch` 必须参与；排序指纹
  覆盖完整 `RankingConfig` 加 `graph_weight`/`graph_top_k`，配置变更即
  失效。
- **降级结果不缓存**：`graph_explain.read_errors` 非空的结果不进缓存，
  瞬时 DB 故障不会被同一 epoch 对服务到天荒地老。

图富化对 cc-db 的邻接读取全部走批量接口（`caller_rows_by_uids` 等），
一次搜索 3 条查询，与命中数无关（见
[STORAGE.md](STORAGE.md#按能力切分的方法面)）。

## Cypher 子集引擎

`cypher/`。只读查询引擎：MATCH / OPTIONAL MATCH / WHERE / RETURN /
ORDER BY / LIMIT / UNION，编译到 SQLite SQL 执行。语法面与有意为之的
限制见 [CYPHER.md](../CYPHER.md)。

实现要点：

- `=~` 由 cc-db 注册的 `REGEXP` UDF 支撑（常量模式每语句编译一次）；
- 变长 `CALLS` 遍历有惰性 BFS fast path（`cypher/fast_path.rs`，
  [ADR-0001](../adr/0001-cypher-traversal-lazy-bfs-fast-path.md)）：
  零预热、逐点 `call_edges_from_uid_lite` 点查 + 查询内 memo，LIMIT-50
  形态比递归 CTE 快 30–250 倍；与 CTE 的逐行等价性由测试锁定。
- 资格门由 `FastPathConfig` 声明（合格边 kind 引用图目录的
  `tool_graph_subsets::CYPHER_FAST_PATH`，目录与门不可能漂移）；不合格
  即回落 CTE，原因作为类型化 `FastPathIneligibility` 浮出为
  `graph_query` 响应的 `fast_path` 元数据。
- `CODECORTEX_CYPHER_FAST_PATH=0` 全局关闭 fast path（用于对照或排查）。

## 扩展点

- **新增检索通道**：实现 `RetrievalLane`，追加到 `default_lanes()`。
  顺序即融合顺序。
- **新增预选层**：实现 `PreselectLayer`，追加到
  `default_preselect_layers()`。顺序即执行顺序——fallback 门读取更早层的
  得分，graph-neighbor 以它之前的一切为种子。
