# 配置

在项目根目录创建 `.codecortex.json` 自定义行为。所有字段都可省略——默认值
适用于大多数项目。

`.codecortex.json` 仅按普通文件读取，上限为 **1 MiB**；符号链接、管道、目录、无效 UTF-8 和超限内容被拒绝并记录告警，沿用默认值再应用已有环境变量覆盖。配置读取失败不是配置成功生效。TS/package/Cargo/Python/Go 模块配置有独立预算和捕获/提交前复核，见 [模块输入防护](internals/MODULE_INPUT_SAFETY.md)。这些路径不执行配置代码、构建脚本或抓取远程依赖。

未知键只在日志告警、不会导致加载失败；历史版本已移除的键（如
`indexing.parallelism`）会提示删除，迁移对照见
[TROUBLESHOOTING.md](TROUBLESHOOTING.md#配置迁移)。

`indexing.dirty_propagation_max_files` 限制每次依赖重解析工作量，不再丢弃超过额度的消费者：余量持久化后可由后续增量、重启或已启用 watcher 继续处理。关闭 dirty propagation 或设置零预算不会假装已收敛，也不会自动忙循环；可以重新启用或显式全量重建。恢复状态与边界见 [增量恢复](internals/INCREMENTAL_RECOVERY.md)。

```json
{
  "indexing": {
    "include": ["**/*.py", "**/*.ts", "**/*.go"],
    "ignore": ["**/generated/**"],
    "max_file_bytes": 512000,
    "chunk_line_budget": 80,
    "chunk_byte_budget": 16384,
    "chunk_char_budget": 16384,
    "chunk_token_budget": 4096,
    "chunk_merge_min_bytes": 256,
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
    "exact_symbol_top_k": 24,
    "path_top_k": 24,
    "grep_top_k": 12,
    "grep_scan_cap": 20000,
    "rrf_k": 50,
    "lexical_weight": 1.1,
    "exact_symbol_weight": 1.1,
    "path_weight": 1.0,
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
| `include` | 27 项默认 glob | **扩展**（而非收窄）索引范围。已知语言的文件总是被索引；`include` 救援匹配这些 glob 的未知语言文件。设值是**替换**默认集而非追加。 |
| `ignore` | 15 项默认排除 glob | 在 gitignore 感知发现之上额外排除的 glob（默认含 `.git/**`、`node_modules/**`、`target/**` 等）。设值是**替换**默认集而非追加。 |
| `max_file_bytes` | `512000` | 超过此大小的文件跳过。 |
| `chunk_line_budget` | `80` | 已接入所有 Registry 解析器与 SFC；每块占用行数，范围 1–10000。 |
| `chunk_byte_budget` | `16384` | 每块原始 UTF-8 字节上限，范围 4–1048576；包含首块、gap 和尾部。 |
| `chunk_char_budget` | `16384` | 每块 Unicode 标量数，范围 2–1048576；不是字形或显示宽度。 |
| `chunk_token_budget` | `4096` | ceil(UTF-8 字节数/4) 的估算上限，范围 1–262144；非模型 tokenizer 实测。 |
| `chunk_merge_min_bytes` | `256` | 同父域且同 owner 的相邻非符号片段，一方小于此值时尝试合并；仍受全部预算约束。0 禁用，最大 1048576。 |
| `parse_timeout_micros` | `null` | 单文件解析超时（微秒）。`null` 不超时。 |
| `db_read_pool_size` | `null` | SQLite 读连接池大小。`null` 按仓库规模档位推导（4–12）。 |
| `dirty_propagation` | `true` | 文件导出面变化时重解析其依赖方。 |
| `dirty_propagation_max_files` | `200` | 一次依赖重解析的文件预算；未完成闭包持久化，后续增量/重启/watcher 可续跑。零预算或关闭传播不等于欠账完成，详见增量恢复文档。 |
| `memory_budget_fraction` | `0.5` | 并行解析的 RSS 上限（系统内存占比，0.1–0.95）。 |
| `max_concurrent_parse` | `null` | 解析线程上限。`null` 用 rayon 默认。 |
| `use_direct_writer` | `false` | 实验性：全量重建时绕过 SQL 解析器的直写器。 |
| `dispatch_synthesis` | `true` | 索引时合成事件 emitter → handler 等派发边。 |
| `event_fanout_cap` | `6` | 单个 emit 点最多匹配的 handler 数（先按 receiver/同文件收窄）。 |
| `event_denylist` | `[]` | 派发合成排除的事件名。空则用内置默认。 |

## search

本地检索统一注册五条通道：exact_symbol、path、lexical、grep、graph。
精确符号和路径召回独立于软文件预选；通道按文档版本去重，仅按排名做 RRF 融合，
再按文件路径 / breadcrumb / 时近性加成重排。机制详见
[internals/SEARCH.md](internals/SEARCH.md)。

| 字段 | 默认 | 含义 |
|------|------|------|
| `lexical_top_k` | `24` | FTS5 词法候选预算；实际取配置值与请求 top_k 的较大者，精确候选已迁往独立通道。 |
| `exact_symbol_top_k` | `24` | 精确 name/qname/字面签名候选预算；按字节定义位置映射文档，不因共享行号或短名称误绑。 |
| `path_top_k` | `24` | 精确文件路径及有界 token 路径候选预算，不区分源码或文档优先。 |
| `exact_symbol_weight` | `1.1` | 精确符号通道 RRF 权重，0 关闭该通道。 |
| `path_weight` | `1.0` | 独立路径通道 RRF 权重，0 关闭该通道。 |
| `grep_top_k` | `12` | 字面子串 grep 候选预算；实际取配置值与请求 top_k 的较大者。 |
| `grep_scan_cap` | `20000` | grep 三阶段合计正文读取上限，plain 与 zstd 均计数，重复 ID 不重复读取；0 仍允许元数据探测。耗尽标 partial，不代表完整无答案；不是 SQL、内存或耗时上限。 |
| `rrf_k` | `50` | RRF 平滑常数 `k`（`1 / (k + rank)`）。越大排名差异越平。 |
| `lexical_weight` | `1.1` | FTS5 全文通道的 RRF 权重。 |
| `grep_weight` | `0.8` | regex 符号 grep 通道的 RRF 权重。 |
| `rerank_window` | `40` | 进入重排的融合候选预算；实际取配置值与请求 top_k 的较大者。 |
| `graph_weight` | `0.6` | 调用图通道（种子符号 + 1 跳调用边扩展）的 RRF 权重。`0.0` 关闭该通道。 |
| `graph_top_k` | `12` | 调用图通道每查询贡献的最大候选数。 |

`file_preselect_limit` 是每次搜索请求的软文件预选数量，不是全局搜索白名单。MCP 的 path_prefix 与 hybrid 查询内 path:/lang: 取交集；提示只改变排名和扫描优先级。Rust API 的 None 与 Some([]) 分别表示不限范围和明确空集合。配置文件本身不会因此获得新的 MCP 参数。

`evidence_summary.retrieval.lanes` 保留各通道状态、候选数、覆盖度、耗时与版本化候选。complete 且 candidate_count=0 表示已执行但无命中，不等于 disabled。候选/扫描/种子/邻居上限可产生 partial，数据库瞬时失败不缓存成成功。多词路径查询忽略少于三个 Unicode 标量的模糊 token，避免自然语言 a/is 等偶然命中文件名；单 token 和完整路径直接查询仍支持短名称。旧 lexical/grep 权重为0时仍可能执行并保留诊断，这与 exact/path/graph 的关闭语义不同。

`evidence_summary.retrieval` 可查看 scope、grep 覆盖状态和分部分成本。缓存中的计数属于生成该结果时的工作；没有清缓存的请求不能把旧计数再次累计成实际扫描量。配置由引擎不可变持有，需显式重载项目生效，不承诺自动监听配置文件。

## 查询资源与能力观测

`status(aspect="capabilities").retrieval` 报告索引可用性、解析欠账、完整 ReadGeneration 和可选端口配置状态；不把局部 ready 当成全局覆盖。`query_pins` 是仍持有资源的查询视图数量，同一视图的工作线程副本共用一次计数，不是请求计数。执行器统计仍为进程共享，不是本项目独占。

空闲清理采用非等待写锁与构建门检查；忙实例留给下一次清理。缓存只强持有 16 个项目，非拥有的弱引用登记使被 LRU 淘汰但仍有在途查询的实例可复用。监听与空闲循环随会话关闭或最后所有者释放而停止；已经运行的同步工作不能被强杀。冷项目初始化不持查询锁，但同会话的首次初始化串行化，不代表所有冷启动都包含在查询 deadline 内。

## query

查询执行策略与期限，省略保持 local，不发送网络请求。`auto` 未配置端口时等价 local；`semantic` 未配置则明确报不可用。P5-B 只提供宿主可注入接口及 fake 测试，没有真实 provider/embedding 配置。

P5-D 支持通过 `search`/`context` 的可选 `retrieval_strategy` 覆盖本次请求的策略；它不会修改项目配置。`search.mode` 仍仅为 hybrid/symbol，symbol 模式不接受 auto/semantic 覆盖。真实 dense 尚未实现，状态始终明确显示 disabled。

| 字段 | 默认 | 含义 |
|---|---|---|
| `strategy` | `local` | `local` / `auto` / `semantic`；不改变 search.mode |
| `deadline_ms` | `30000` | 查询准入起的总预算，包含排队、可选端口与输出组装 |
| `lane_timeout_ms` | `20000` | 本地 lane 子预算，不延长总期限 |
| `semantic_timeout_ms` | `5000` | 可选异步端口子预算，超时诊断保留 |
| `semantic_top_k` | `24` | 提供方候选上限；1–4096 |

时间字段允许1–600000毫秒。CPU执行4/等待32、async执行8/等待32是进程共享边界，不按项目扩张；`status(aspect=index).diagnostics.query_execution` 可观察占用。同步算法不可强制中断时保留执行额度至退出，迟到结果不能写正常缓存。冷项目定位/加载的生命周期阶段不属于上述查询预算；具体边界见 [QUERY_EXECUTION.md](internals/QUERY_EXECUTION.md)。

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
| `preselect_fts_base` | `1.4` | FTS summary 层：`base + x/(1+x)`，`x=max(-raw_bm25,0)`；更负的 BM25 贡献不会更低。 |
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
| `CODECORTEX_RESOLVER_MAX_POOL` | `256` | 名字解析候选上限：同名符号数超过此值时，按名解析（global-unique / fuzzy import-distance 与 `find_best` 兜底）直接判为不可解。被数百符号共享的名字（典型是解析器也并入全局目录的函数局部变量，如 `left`/`value`）无法靠路径启发式消歧，逐引用扫该桶是冷建 resolve 阶段 O(N²) 的主因；上限同时消除该开销并提升精度（这类引用从"跨文件随机同名"变为未解析） |
| `CODECORTEX_SEED_CACHE_MAX_SYMBOLS` | `500000` | seed 符号快照与 resolver 目录两层跨构建缓存共用的符号数上限（`0` 同时禁用两层）；超限的仓库回退每次构建直接重载，见 [internals/INDEXING.md](internals/INDEXING.md#符号目录跨构建缓存catalog-cache) |
| `CODECORTEX_COMMUNITY_MAX_EDGES` | `2000000` | Louvain 社区检测的边数上限；超限时按权重裁剪到上限（保留最重的调用点对）再检测，避免 OOM |
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
| `CODECORTEX_BENCH_FILES` | `10000` | 增量写基准（`incremental_write_bench`）的合成仓文件数 |
| `CODECORTEX_PROFILE_SCALES` | `1000,2000,4000,8000,16000` | 冷建缩放 profiler（`profile_cold_build_scaling`）的规模列表 |
| `CODECORTEX_SOAK_SECS` | `60` | RSS soak（`soak_mcp_session_rss`，ignored）的运行时长（秒） |
| `CODECORTEX_SOAK_FILES` | `1000` | RSS soak 的合成仓文件数 |
