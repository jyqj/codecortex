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

## 语义缓存与降级（P6，可选）

语义持久化是可选功能（`semantic` feature + 组合根接线），默认构建不含
`cc-semantic`、不读配置、不创建任何文件。以下事实来自已交付的库层实现
（`crates/cc-semantic/src/cache.rs`、`degrade.rs`；存储侧总记录见
[STORAGE.md](internals/STORAGE.md#语义持久化p6schema-v22)）；预算/租约参数
由调用方按库层 API 传入，组合根接线归接线轮。

### `semantic` 配置节（P7-002，声明面）

P7-002 起 `.codecortex.json` 新增首个语义配置节 `semantic`（声明模型能力，
`crates/cc-model/src/config.rs` 的 `SemanticProviderConfig`）。默认
`enabled: false`（C14：默认无网络）——关闭时其余字段**可解释但不生效**，
不解析 provider、不建任何网络路径。

| 字段 | 默认 | 含义 |
|------|------|------|
| `enabled` | `false` | 语义 provider 总开关；`false` 时整节惰性。 |
| `model_id` | `""` | 嵌入模型全名，构成冻结 `VectorSpace` 身份的一部分；`enabled` 时必填。 |
| `dimensions` | — | 声明的输出维度；`enabled` 时必填，须与模型真实输出一致（探针验证）。 |
| `metric` | `"cosine"` | 距离度量；冻结 spec v1 只承认 `cosine`，其余值拒启。 |
| `dimensions_mode` | `"configurable"` | `"configurable"`：端点接受请求体显式 `dimensions` 字段；`"fixed"`：模型单一原生维度、不得发送该字段。供应商不支持 `dimensions` 时走此配置路径，不伪成功。 |
| `endpoint` | `""` | OpenAI 兼容端点 base URL（如 `https://host/v1`），非凭据。 |
| `api_key_ref` | — | API key 的**外部引用名**（如 `env:MY_KEY`）；密钥本体永不进入配置，解析归凭据政策层（P7-007）。 |
| `max_input_tokens` | — | 声明的模型单输入 token 上限；`enabled` 时必填，文档/查询 spec 的 `max_tokens` 超过它即配置错误拒启。 |
| `max_batch_items` | — | 声明的端点批次上限；`enabled` 时必填，探针按此值全量探测（协议 v1 上限 4096）。 |
| `encoding_formats` | `["float"]` | 声明支持的 `encoding_format`；必须含 `float`（适配器唯一发送格式）。 |
| `supports_instruction` | `false` | 模型是否支持 instruction 前缀；spec 声明 instruction 而此处为 `false` 即配置错误拒启。 |
| `max_concurrent` | `0` | 进程级共享 provider 并发上限（P7-005）。`0` = 不限流（默认，保守值）：组合根单例 `ProviderGate` 处于 permissive 模式，准入永不等待；`≥1` 即硬信号量上限。 |
| `max_concurrent_per_project` | `0` | 单项目在共享 gate 中的并发份额；`0` = 不限（默认）。设置时必须**严格小于** `max_concurrent`，否则单个项目可占满全部许可、饿死其他项目（V20 验收红线），配置错误拒启。 |
| `acquire_timeout_ms` | `30000` | 调用方获取并发许可的默认等待预算（毫秒）。只约束**准入等待**，不约束进行中的 provider 调用，也不跨任何 DB 事务（C11）；等待超时返回显式超时，由调用方走 fenced retry。收到 429（带 `Retry-After`）时 gate 进入全局暂停、准入快速失败直至冷却到期。 |
| `retry_max_attempts` | `0` | 调用层有界重试（P7-006）：**一次 outbox attempt 内**重试装饰器发起的 provider 总尝试次数。`0` = 调用层重试关闭（默认，保守值）：每次 provider 调用只试一次，任何失败直接交还 fenced retry；`≥2` 启用有界重试（指数退避 + 确定性抖动 + deadline/费用封顶，仅重试 429/5xx/timeout 可重试类；auth/永久错误不重试）。该预算独立于 outbox attempt 预算，不消耗 DB attempt。 |
| `retry_base_backoff_ms` | `500` | 重试指数退避基值（毫秒）：第 `n` 次重试等待 `min(base×2ⁿ, retry_max_backoff_ms)`，再被确定性抖动最多缩 25%。 |
| `retry_max_backoff_ms` | `8000` | 指数退避上限（毫秒），必须 ≥ `retry_base_backoff_ms`，否则配置错误拒启。 |
| `retry_total_deadline_ms` | `30000` | **单次**重试序列的墙钟总预算（毫秒）：一次等待若会越过它则不再等待、立即以最后一个错误返回。 |
| `retry_respect_retry_after` | `true` | 429 的 `Retry-After` 时长是否覆盖该次退避计算（默认尊重）。共享 gate 仍会收到 429 上报并进入全局冷却；若等待后 gate 仍处暂停，重试循环立即停止（不泊线程对抗冷却，交还 fenced retry）。 |
| `retry_max_cost_units` | — | 单次重试序列的费用封顶（抽象费用单位；P7-008 收据层接入前占位费率为每次尝试 1 单位）。缺省 = 无独立费用帽，仅受次数/deadline 约束。 |
| `breaker_failure_threshold` | `5` | 进程级 provider 断路器（P7-006）的**连续**可重试失败（5xx/timeout）阈值，成功清零；`0` = 显式关闭断路器。默认开启（保守值）：断路器只对持续失败生效，不影响任何成功路径。429、输入非法、取消不计入。 |
| `breaker_open_ms` | `30000` | 断路器开路窗长（毫秒）：开路期间快速失败、不触 provider；窗口流逝后惰性进入半开、每次只放行一个探测调用（成功闭合、失败重开）。`AuthError` 立即开路且窗长 ×10。断路器是组合根单例（与 `ProviderGate` 同点装配），无线程/定时器，时钟注入。 |
| `network_opt_in` | `false` | **显式网络 opt-in**（P7-007 外发政策）。默认 `false` = 默认无网络：未开启时组合根拒绝装配任何 provider transport（`cc-semantic::policy::gate_transport_assembly` 与适配器构造器双重强制，配置错误拒启）。`enabled: true` 单独不足以放开网络——外发必须由本键独立、显式声明。 |
| `allow_http` | `false` | 是否允许明文 `http://` 端点（P7-007）。默认 `false` = 仅 https；明文端点被外发政策拒绝（配置错误），设置 `true` 才放行。生产部署建议保持关闭。 |

### 代码外发与凭据政策（P7-007，执行机制腿）

口径：**默认无网络 + 显式 opt-in**。本节声明政策的机制保证与数据流向；
机制实现见 `crates/cc-semantic/src/policy.rs`（`EgressPolicy`、
`gate_transport_assembly`、`GuardedTransport`、`audit_egress`、
`resolve_key_reference`、`redact_for_log`）。

**默认无网络**：`semantic.enabled` 与 `semantic.network_opt_in` 双默认
`false`。默认状态下不存在任何网络代码路径（transport 为 `None`，适配器
fail-closed disabled），不解析密钥、不发起连接；未 opt-in 时装配
transport 是配置错误拒启，而不是静默联网。

**数据流向声明（外发面逐项清单）**：每一次 `/embeddings` 请求**只会**
发送以下内容，`policy::audit_egress` 机制化审计请求体键集与头部集合
"恰好等于声明面"（多余字段/缺失字段/多余头部都判违规并点名）：

1. **input 文本 bytes**——批内文本（渲染后的文档分块或查询文本；它们
   **可能包含源码片段**，这是嵌入调用的声明内容本身）；
2. **model 名**——冻结 `space.model_id()`；
3. **`encoding_format: "float"`**——唯一承认的编码格式；
4. **`dimensions` 参数**——**仅探针请求**、且仅 `dimensions_mode =
   "configurable"` 时（适配器 embed 请求永不携带）。

头部恰好为 `Content-Type`、`Authorization`（凭据唯一通道）、`Accept`。
除上述声明外**零外发**：无遥测、无额外字段、无 tracing 日志（适配层与
政策层零日志调用）。

**凭据机制**：配置只存外部引用 `api_key_ref`（`env:NAME` 或 `file:PATH`
两种形式，其余形式——包括内联明文——显式拒绝）。密钥在调用点解析为
内存中的 `EmbeddingApiKey`：`Debug` 固定输出 `[REDACTED]`、无 `Display`、
只经 `Authorization: Bearer` 头离开进程；解析失败只点名引用名/配置键，
永不回显密钥值；错误诊断若意外嵌入密钥，`redact_for_log` 将
`Bearer <token>` 脱敏后再进日志。密钥零落配置、日志、错误、缓存与磁盘
（能力缓存只存结构判定，不含凭据）；内存生命周期尽量短——无 static、
无缓存、无持久化，组合根装配后即可丢弃。**如实声明**：显式
zero-on-drop 内存擦除本轮未实现（避免为离线闭包新增依赖），属已知边界
而非已完成保证。

**传输安全机制**：默认仅 https（`allow_http: false` 拒绝明文端点）；
per-request 超时强制（零超时在政策守卫 `GuardedTransport` 处被拒）；
**重定向永不跟随**——适配层把任何 3xx 判为不可重试的 `InvalidInput`
（机制测试断言 seam 恰好收到一次请求、认证头绝不重发），且
`EmbeddingHttpTransport` 契约条款绑定一切实现：不得自动跟随重定向，
底层客户端无法关闭 auto-follow 时至少必须在任何重定向跳前剥离
`Authorization` 头并仍透出最终 3xx 状态。任何注入的 transport 在适配器
构造时被 `GuardedTransport` 包装，scheme 准入与超时强制不依赖实现方自觉。

**blocked 声明（双轨口径，D1/D2 2026-10-02）**：live 真实外发（生产
transport、真实端点验证）归 P7-018，本轮 conditional blocked；敏感文件
外发分类矩阵同因未开放，无配置面。本轮政策只声明上述机制保证，**不**
宣称任何审计认证、合规认证或对第三方 provider 行为的保证。

声明能力与冻结编码面（`VectorSpace`/`DocumentEncodingSpec`/
`QueryEncodingSpec`）的一致性校验与探针协议见 `crates/cc-semantic/src/capability.rs`：
任何"支持差异"（维度/度量/上限/instruction/编码格式不匹配）都是
`CcError::Config` 拒启，不是静默默认；探针（mock transport 全链验证）只有
成功判定入进程内能力缓存（按 `SpaceDigest` 键），失败永不入缓存。

### cache 根目录解析

`resolve_cache_root`（cache.rs:128）按序取第一个可用项：

1. 环境变量 `CODECORTEX_SEMANTIC_CACHE_ROOT`（空白值视为未设）——
   部署/测试钉根的首选方式；
2. macOS：`~/Library/Caches/codecortex/semantic`；
3. Linux：`$XDG_CACHE_HOME/codecortex/semantic`，未设 XDG 时
   `~/.cache/codecortex/semantic`。

解析不出默认根（无 `HOME`）时调用方必须显式传根。`put`/`get` 路径
不读任何环境变量；`open` 零文件系统副作用，首个 `put` 才惰性建目录
——默认构建/启动不产生 cache 目录。

### namespace 键与跨项目隔离

namespace = `blake3("cc-semantic.cache-namespace.v1", 项目身份)` 的
64 位 hex（`namespace_key`，cache.rs:96），是 cache 目录的首层分隔：
`<root>/namespace-<ns>/<space>/<input>/<spec>.bin`。跨项目默认隔离、
跨克隆（同项目身份）共享；**不绑 incarnation**——索引重建换库后解析
同一 namespace，付费向量经 `(space, input, spec)` 全链校验直接复用。
项目身份字符串由组合根提供（当前来源与项目索引缓存 `CODECORTEX_CACHE_DIR`
的规范路径同源；接线轮接线）。

### 降级语义与 re-embed 预算

- 纯缓存 Miss（冷缓存）是常态，**永不降级**：dense lane 只缺席该文档，
  lexical/graph 本地检索不受影响。
- 损坏（Corrupt）：检索跳过候选不报错；显式检测点将坏对象双半隔离进
  `<root>/quarantine/` 并留诊断 sidecar（`degrade.rs:119`），可见性
  判据 `corrupt_events > 0 || 预算耗尽` → capability status
  `semantic_state: "degraded"` + `degraded_reason`（透出槽已交付
  `capability_status.rs:100`，组合根转写归接线轮）。
- **re-embed 预算**：只对"已付费产物损坏后的补嵌"计费准入（首嵌不入
  预算）；`BudgetedProvider`（`degrade.rs:347`）整批裁决，超限拒绝发生
  在调用内层 provider 之前，原因持久化进 outbox `last_error`，attempt
  预算耗尽终态 `failed` 死信——不静默无界重费。预算值当前为进程内
  调用方参数（进程生命周期，重启清零；outbox 行计数为持久审计轨），
  尚无配置文件键。
- GC 宽限（`min_retention_secs`，默认 3600s）同理为调用方参数
  （`gc.rs:93`），防"刚发布产物被 GC 删除"；接线轮应将其纳入配置面并
  保证非零下限。

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

### 语义缓存（P6，可选，`semantic` 接线后生效）

| 变量 | 默认 | 作用 |
|------|------|------|
| `CODECORTEX_SEMANTIC_CACHE_ROOT` | 未设（平台默认） | 派生 artifact cache 根目录覆盖，优先于平台默认（macOS `~/Library/Caches/codecortex/semantic`、Linux `$XDG_CACHE_HOME\|~/.cache/codecortex/semantic`）；空白值视为未设。cache 机制已交付库层，组合根接线归接线轮，详见上文[语义缓存与降级](#语义缓存与降级p6可选) |

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
