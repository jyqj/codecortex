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
  ├─ 文件预选（preselect）：给候选文件打分；仅为 grep 提供有界扫描提示
  │
  ├─ 检索通道（lanes，并发执行、确定性融合序）：
  │     lexical（FTS5 over chunks）
  │     grep（regex/子串 over chunks 正文）
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
| grep | chunks 正文（zstd 解压后 regex/子串匹配） | `search.grep_top_k`（12） | `search.grep_weight`（0.8） |
| graph | 种子符号 + 调用边 1 跳扩展 | `search.graph_top_k`（12） | `search.graph_weight`（0.6；0 关闭） |

注册顺序即确定性的 RRF 融合顺序。新增一条通道只需实现 trait 并追加到
`default_lanes()`，不需要改 `plan.rs` / `engine.rs`。

启用的通道**并发执行**（`run_lanes`，`std::thread::scope`：首条启用
通道跑在调用线程上，其余各开一条 scoped 线程；每条通道在 `run` 内自取
读池连接，副作用限于各自锁后的引擎缓存）。结果按注册序收集，RRF 融合
与 tie-break 看到的序列与旧串行循环逐字节一致；错误语义同样按注册序取
首个错误。`RetrievalLane` 因此要求 `Sync`。

grep 通道的每行匹配都要先 zstd 解压，四项行为约束最坏情况：

- **扫描预算**：单次查询最多解压扫描 `search.grep_scan_cap`（默认
  20000）行 chunk，预算耗尽即截断并返回预算内的命中（tracing 日志
  提示可调大换召回）——否则无命中/罕见词查询会解压全仓 chunk；
- **时近性扫描序**：无 file scope（预选空手且调用方未给 `file_paths`）
  的扫描按 rowid 倒序——chunks 逐次写入只增不改，倒序即最近索引的文件
  优先，预算花在最新代码上（SQLite 倒走 b-tree，无排序步骤）；有 scope
  时保持自然探测序；
- **缓存防刷穿**：仅命中的 chunk 进 chunk_text 缓存（批取阶段恰好要重读
  它们）；扫描过但未命中的行刻意不进缓存，否则一次冷扫描会把整个 LRU
  轮转一遍、驱逐热条目；
- **FTS 预过滤（两段扫描）**：无 scope 的扫描先从 grep 字面量导出
  `chunks_fts` MATCH 短语（`grep_prefilter_phrase`；token 边界匹配是
  FTS 短语命中的超集）拉候选做第 1 段，第 2 段回退全量时近序扫描，
  补 tokenizer 看不见的 token 中缝子串命中（如以 `UserById` 查
  `getUserById`），跳过第 1 段已解压的行；两段命中按 rowid 倒序合并——
  预算够覆盖时结果与单趟扫描完全一致，预算吃紧时预过滤**更早找到更多
  命中**。有 file scope（基数有界）保持单趟。MATCH 报错（tokenizer 拒收
  的短语）静默降级为纯全扫。

graph 通道的种子打分与衰减由 `RankingConfig` 控制
（`graph_seed_exact_score` / `graph_seed_fuzzy_score` /
`graph_neighbor_decay`）。

## 文件预选（PreselectLayer）

**范围契约（ADR-0003）**：用户的 `file_paths` / `path_prefix` / `languages`
是硬范围，lexical 和 graph 通道在该范围内独立召回。Preselect 是软提示，
不再覆盖 `SearchRequest.file_paths`；非 fallback 的提示可收窄 grep 扫描，
但 recency fallback 不隐藏全库字面量。候选仍受各通道数量与扫描预算限制。
BM25 文件预选使用 `base + strength/(1+strength)`，其中
`strength=max(-bm25,0)`；排序方向与 SQLite 负分越小越好的约定一致。

图通道的符号投影独立于 SQL：优先最小完整包含 chunk，长符号则取相交的
多个分片；同分项在截断前按稳定身份打破平分。RRF 同一 lane 内的重复
chunk 只投一次票，但仍保留原始位置成本。验证入口见
[Benchmark v2](../BENCHMARK_V2.md)。


`preselect.rs`。先对文件打分形成排序先验，不将系统猜测写入调用者的硬范围。8 个已注册的层
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

**独立层并发**：不读先前层得分的层（`reads_prior_scores() == false`，
今天是层 1–6）按注册表中的连续段并发执行（DB 绑定的 FTS summary 与
token search 各自取读池连接），命中按注册序合并——得分、理由、逐层
明细与串行逐字节一致。读分层（fallback 门、graph-neighbor 以前层结果
为种子）仍在合并后串行执行。新增层默认 `true`（安全侧）。

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

chunk 正文缓存只在批取与 grep **命中**路径写入——grep 扫描过但未命中的
chunk 刻意不进缓存，防一次冷扫描刷穿 LRU（见上文 grep 通道）。

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

- **新增检索通道**：实现 `RetrievalLane`（需 `Sync`；通道间并发执行），
  追加到 `default_lanes()`。顺序即融合顺序。
- **新增预选层**：实现 `PreselectLayer`，追加到
  `default_preselect_layers()`。顺序即（逻辑）执行顺序——fallback 门读取
  更早层的得分，graph-neighbor 以它之前的一切为种子。不读先前层得分的
  层可覆写 `reads_prior_scores()` 返回 `false` 以加入并发段（默认
  `true`，安全侧）。

## ADR-0004 后的覆盖说明

上文历史 preselect 提示不再裁剪 grep；所有 lane 使用调用者 hard scope。
通道与预选错误统一进入 RetrievalDiagnostics，工作预算耗尽和读取失败的结果
不缓存。可选 stage trace 有 512 候选记录上限，默认关闭。graph_features=false
完整关闭检索图特征，非仅将 graph_weight 置零。Unicode token 去重、代码标识符/
引号优先选择和安全 FTS quoting 代替纯句首截断；排名默认权重保持不变，未声称
已经校准。源码片段的反向符号投影遵循“精确名字→最小容器→歧义拒绝”。
