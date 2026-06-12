# 配置

在项目根目录创建 `.codecortex.json` 自定义行为。所有字段都可省略——默认值
适用于大多数项目。

```json
{
  "indexing": {
    "include": ["**/*.py", "**/*.ts", "**/*.go"],
    "ignore": ["**/generated/**"],
    "max_file_bytes": 512000,
    "chunk_line_budget": 80,
    "dirty_propagation": true,
    "dirty_propagation_max_files": 200,
    "memory_budget_fraction": 0.5,
    "max_concurrent_parse": null,
    "use_direct_writer": false,
    "dispatch_synthesis": true,
    "event_fanout_cap": 6,
    "event_denylist": []
  },
  "search": {
    "lexical_top_k": 24,
    "grep_top_k": 12,
    "rrf_k": 50,
    "lexical_weight": 1.1,
    "grep_weight": 0.8,
    "rerank_window": 40,
    "graph_weight": 0.6,
    "graph_top_k": 12
  },
  "ranking": {
    "graph_rerank_weight": 0.3,
    "overlap_weight": 0.35
  },
  "auto_index": {
    "enabled": true,
    "file_limit": 50000,
    "idle_timeout_secs": 60
  }
}
```

## indexing

| 字段 | 默认 | 含义 |
|------|------|------|
| `include` | `[]` | **扩展**（而非收窄）索引范围。已知语言的文件总是被索引；`include` 救援匹配这些 glob 的未知语言文件。 |
| `ignore` | `[]` | 在 gitignore 感知发现之上额外排除的 glob。 |
| `max_file_bytes` | `512000` | 超过此大小的文件跳过。 |
| `chunk_line_budget` | `80` | 符号提取时单个代码 chunk 的最大行数。 |
| `parse_timeout_micros` | `null` | 单文件解析超时（微秒）。`null` 不超时。 |
| `db_read_pool_size` | `null` | SQLite 读连接池大小。`null` 按仓库规模档位推导（4–12）。 |
| `dirty_propagation` | `true` | 文件导出面变化时重解析其依赖方。 |
| `dirty_propagation_max_files` | `200` | 一次脏传播最多提升的文件数。超限第 1 轮降级为 no-op（建议全量重建），后续轮保留部分闭包；结果以 `dirty_propagation` 字段出现在索引报告中。 |
| `memory_budget_fraction` | `0.5` | 并行解析的 RSS 上限（系统内存占比，0.1–0.95）。 |
| `max_concurrent_parse` | `null` | 解析线程上限。`null` 用 rayon 默认。 |
| `use_direct_writer` | `false` | 实验性：全量重建时绕过 SQL 解析器的直写器。 |
| `dispatch_synthesis` | `true` | 索引时合成事件 emitter → handler 等派发边。 |
| `event_fanout_cap` | `6` | 单个 emit 点最多匹配的 handler 数（先按 receiver/同文件收窄）。 |
| `event_denylist` | `[]` | 派发合成排除的事件名。空则用内置默认。 |

## search

本地检索跑三条通道——FTS5 全文、regex 符号 grep、调用图扩展——之上是
trigram 支撑的文件预选。通道结果经 RRF（Reciprocal Rank Fusion）融合，
再按文件路径 / breadcrumb / 时近性加成重排。机制详见
[internals/SEARCH.md](internals/SEARCH.md)。

| 字段 | 默认 | 含义 |
|------|------|------|
| `lexical_top_k` | `24` | FTS5 词法通道每查询的最大候选数。 |
| `grep_top_k` | `12` | regex 符号 grep 通道每查询的最大候选数。 |
| `rrf_k` | `50` | RRF 平滑常数 `k`（`1 / (k + rank)`）。越大排名差异越平。 |
| `lexical_weight` | `1.1` | FTS5 全文通道的 RRF 权重。 |
| `grep_weight` | `0.8` | regex 符号 grep 通道的 RRF 权重。 |
| `rerank_window` | `40` | 进入重排的融合候选数。 |
| `graph_weight` | `0.6` | 调用图通道（种子符号 + 1 跳调用边扩展）的 RRF 权重。`0.0` 关闭该通道。 |
| `graph_top_k` | `12` | 调用图通道每查询贡献的最大候选数。 |

## ranking

搜索结果排序的打分权重，顶层 `"ranking"` 键之下。所有字段可省略——省略的
字段保持内置默认（与历史硬编码行为完全一致）。大多数项目不需要碰这些。

### 命中重排权重

RRF 融合后折入每个命中最终 `rerank_score` 的加成。

| 字段 | 默认 | 含义 |
|------|------|------|
| `graph_rerank_weight` | `0.3` | 图连通度得分对最终 `rerank_score` 的权重（0.0 关闭）。 |
| `overlap_weight` | `0.35` | 查询 token 与文本重叠度加到融合分上的权重。 |
| `symbol_exact_bonus` | `0.18` | 查询 token 与 chunk 符号名精确匹配的加成。 |
| `path_prefix_bonus` | `0.05` | 文件路径匹配请求的路径前缀的加成。 |
| `doc_file_bonus` | `0.08` | 项目文档文件（README、docs/、ADR）的加成。 |
| `working_set_boost` | `0.22` | 调用方 working set（`boost_file_paths`）内文件的加成。 |
| `recent_file_boost` | `0.12` | 最近编辑文件（`recent_file_paths`）的加成。 |
| `pinned_context_boost` | `0.20` | 钉住的上下文文件（`pinned_file_paths`）的加成。 |
| `overlay_neighbor_boost` | `0.10` | overlay/脏缓冲文件（`overlay_file_paths`）的加成。 |
| `stage_a_weight` | `0.04` | 预选（stage-A）文件分映射进重排的乘数。 |
| `stage_a_cap` | `0.25` | 预选文件分对重排贡献的上限。 |
| `dsl_name_bonus` | `0.25` | `name:` DSL 过滤匹配命中符号名的加成。 |

### 预选文件打分

chunk 级检索前的文件预选阶段使用的逐文件分值。四个上下文层对调用方提供的
文件列表打 `max(floor, scale / rank)`。

| 字段 | 默认 | 含义 |
|------|------|------|
| `preselect_working_set_floor` | `2.0` | working-set 层分数下限。 |
| `preselect_working_set_scale` | `5.0` | working-set 层按排名衰减的尺度。 |
| `preselect_recent_floor` | `1.2` | recent 层分数下限。 |
| `preselect_recent_scale` | `3.5` | recent 层衰减尺度。 |
| `preselect_pinned_floor` | `2.2` | pinned 层分数下限。 |
| `preselect_pinned_scale` | `4.0` | pinned 层衰减尺度。 |
| `preselect_overlay_floor` | `1.5` | overlay（脏缓冲）层分数下限。 |
| `preselect_overlay_scale` | `3.0` | overlay 层衰减尺度。 |
| `preselect_fts_base` | `1.4` | FTS summary 层：分数为 `base + 1 / (1 + |bm25|)`。 |
| `preselect_symbol_exact_bonus` | `2.0` | 符号名精确匹配的每 token 加成。 |
| `preselect_symbol_fuzzy_bonus` | `1.2` | 符号名子串匹配的每 token 加成。 |
| `preselect_path_token_bonus` | `1.0` | 路径分量匹配的每 token 加成。 |
| `preselect_graph_neighbor_base` | `0.8` | 1 跳调用图邻居文件的基础分（受 `preselect_graph_accum_cap` 钳制）。 |
| `preselect_graph_edge_increment` | `0.1` | 图邻居基础分之上的每边增量。 |
| `preselect_graph_accum_cap` | `1.2` | 单文件累计图邻居分上限（基础 + 增量）。 |
| `preselect_fallback_score` | `0.2` | 其他层都没命中时给最近索引文件的兜底分。 |
| `preselect_explicit_scope_score` | `10.0` | 显式限定文件（`file_paths`）的短路分。 |

### 图检索通道

调用图检索通道喂给 RRF 的种子与扩展分。

| 字段 | 默认 | 含义 |
|------|------|------|
| `graph_neighbor_decay` | `0.5` | 从种子符号到调用图邻居的每跳分数衰减。 |
| `graph_seed_exact_score` | `1.0` | 符号名精确匹配的种子相关度。 |
| `graph_seed_fuzzy_score` | `0.5` | 符号名子串匹配的种子相关度。 |

## auto_index

| 字段 | 默认 | 含义 |
|------|------|------|
| `enabled` | `true` | 启动 `FileWatcher`，文件变更时增量重索引。 |
| `file_limit` | `50000` | 首次连接自动索引的最大文件数。 |
| `idle_timeout_secs` | `60` | 空闲会话驱逐：MCP 无活动超过此秒数后关闭活动项目的 `CodeIndex`（释放 DB 句柄），下次调用透明重开。与 watcher 去抖无关——watcher 按仓库大小自行计算自适应去抖（500ms 基础 + 每 500 文件 100ms，上限 3000ms）。 |

## 仓库规模档位

CodeCortex 检测项目规模并自动调整输出预算：

| 档位 | 文件数 | token 预算 | 搜索 `top_k` | 最大输出字符 |
|------|--------|-----------|--------------|--------------|
| Tiny | < 500 | 4,000 | 5 | 18,000 |
| Small | 500 – 4,999 | 6,000 | 10 | 24,000 |
| Medium | 5,000 – 24,999 | 8,000 | 15 | 32,000 |
| Large | 25,000+ | 12,000 | 20 | 38,000 |

预算按 handler 缩放（如 Large 档 `files` 最多 10,000 项、`impact` 最多
80 项）。预算如何在工具出口生效见
[MCP_TOOLS.md](MCP_TOOLS.md#输出预算)。

## 环境变量覆盖

环境变量优先于 `.codecortex.json`。

### 索引

| 变量 | 默认 | 作用 |
|------|------|------|
| `CODECORTEX_MEMORY_BUDGET_FRACTION` | `0.5`（配置） | RSS 内存上限占比，钳制 0.1–0.95；覆盖 `indexing.memory_budget_fraction` |
| `CODECORTEX_DIRTY_PROPAGATION` | `true`（配置） | 开/关增量脏传播 |
| `CODECORTEX_DIRTY_PROPAGATION_MAX_FILES` | `200`（配置） | 脏传播最多重载的文件数 |
| `CODECORTEX_MAX_CONCURRENT_PARSE` | 未设（rayon 默认） | 解析工作线程上限 |
| `CODECORTEX_USE_DIRECT_WRITER` | `false`（配置） | 启用实验性直写器 |
| `CODECORTEX_CACHE_DIR` | 未设（仓库内） | 项目索引缓存改存此目录而非 `<project>/.codecortex`；每个项目一个稳定哈希子目录 |
| `CODECORTEX_STRICT_HASH` | 关 | `1`/`true`/`yes` 时增量扫描对每个文件做哈希，不走 mtime+size 快路径 |
| `CODECORTEX_RESOLVER_CACHE_SIZE` | `8192` | 解析器目录 `resolve_name` 的 LRU 容量 |
| `CODECORTEX_COMMUNITY_MAX_EDGES` | `2000000` | Louvain 社区检测的边数上限；超限跳过检测，所有符号归社区 0，避免 OOM |

### 搜索与图缓存

| 变量 | 默认 | 作用 |
|------|------|------|
| `CODECORTEX_SEARCH_RESULT_CACHE_SIZE` | `32` | 核心搜索结果缓存的 LRU 容量，键 `(index_epoch, query_hash)` |
| `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE` | `32` | 图感知搜索结果缓存（`search_with_graph_context`）的 LRU 容量，键含两个 epoch + 查询/limits/预算/排序指纹 |
| `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE` | `512` | 解压 chunk 正文缓存的 LRU 容量 |
| `CODECORTEX_GRAPH_CACHE_SIZE` | `16` | 进程级图邻接缓存（`GraphReadModel`）的项目槽数 |
| `CODECORTEX_BRIDGE_EDGE_LIMIT` | `10000` | 合成跨服务桥接边时加载的 HTTP 调用边/路由节点上限；命中上限记一条截断警告 |
| `CODECORTEX_CYPHER_FAST_PATH` | 启用 | 恰为 `0` 时禁用变长 `CALLS` 遍历的惰性 BFS fast path（ADR-0001）；`graph_query` 响应报告 `fast_path.reason = "disabled(CODECORTEX_CYPHER_FAST_PATH=0)"` |

### 服务器生命周期

| 变量 | 默认 | 作用 |
|------|------|------|
| `CODECORTEX_PPID_POLL_MS` | `5000` | 父进程死亡检测间隔（毫秒）；`0` 关闭 watchdog |

### 评测 / 基准（仅 cc-eval）

| 变量 | 默认 | 作用 |
|------|------|------|
| `CODECORTEX_WRITE_BENCHMARK` | 关 | `1` 时 fixture 与合成规模基准把报告持久化到 `docs/benchmarks/` |
| `CODECORTEX_WRITE_REAL_BENCHMARK` | 关 | `1` 时被 ignore 的真实工作区基准写 `docs/benchmarks/real_workspace_latest.md` |
| `CODECORTEX_BENCH_50K` | 关 | `1` 时启用默认跳过的 50k 文件合成规模基准（`bench_synthetic_50k`） |
