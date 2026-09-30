# 检索引擎（cc-search）

> 范围：`crates/cc-search` —— 排序式本地检索的通道（lane）、文件预选
> （preselect）、RRF 融合与重排、多级缓存，以及 Cypher 子集引擎的实现
> 视角。面向需要调整排序行为或新增检索通道的开发者。Cypher 的查询语法与
> 工具契约见 [CYPHER.md](../CYPHER.md)；可调权重见
> [CONFIGURATION.md](../CONFIGURATION.md)。

设计前提：**确定性、离线、纯词法/结构信号**。没有外部模型依赖，没有网络
调用；在索引静止且读取成功时，同一索引、配置和查询保持确定性。并发写入时不将分阶段读取冒充单一事务快照。

## 图连接度不是独立相关性（P3-B 回归修正）

Rust 内联模块提取扩大了真实符号覆盖，旧“按已加文件先验的 rerank 前缀选图种子，再加连接度”会放大测试辅助函数等弱文本匹配。现在图种子先按 exact-target、直接 lexical/grep 倒数排名、融合分数和稳定身份选择，仍遵守既有 max_resolve/图预算与硬范围；没有直接证据的 graph-only 候选不继续自我强化。图连接度分数仍保留原值用于解释，但实际加分为 `graph_score * graph_rerank_weight * max(lexical_score, grep_score)`，直接证据钳制到 [0,1]、非有限值记0，exact-target 支持为1。并不把不同通道的原始BM25/字符串命中数相加。

缓存策略版本已变更；score_trace 记录实际入账分数。测试证明纯文件先验不能买入种子优先权，零直接证据不获得连接度加分，exact身份不丢失。旧固定图分数测试保留原始基线与BM25代数，并增加可手算的直接证据系数差值；保留真正发生图排序翻转的断言，不重录不透明输出分数。原始R04退化、R08提升的抵消现象保留于P3-B初次配对，不能据平均分不变声称逐题无退化。最终配对新增逐题负delta检查；这些开发题不是独立holdout，收益不外推到全仓自然语言质量。

## 一次搜索的流程

```
查询
  │
  ├─ HardScope：调用方约束与 DSL 约束取交集；空集合直接结束
  ├─ SoftHints / preselect：给候选文件打分，不限制其他合法文件召回
  │
  ├─ 检索通道（lanes，并发执行、确定性融合序）：
  │     exact_symbol（精确 name/qname/signature）
  │     path（精确规范路径 + 路径 token）
  │     lexical（FTS5 over chunks）
  │     grep（regex/子串 over chunks 正文）
  │     graph（种子符号 + 调用图 1 跳扩展）
  │
  ├─ 身份物化：chunk locator → DocKey/DocVersion + source span
  ├─ RRF 融合：按文档版本去重，score = Σ lane_weight × 1/(rrf_k + rank)
  │
  ├─ 重排（rerank_window 内）：文件路径 / breadcrumb / 时近性 /
  │     图连通度 / 查询重叠 等加成
  │
  └─ （默认入口）图富化：callers/callees/tests 摘要附着到命中上
        → ContextEnvelope
```

## 硬范围与软线索（P1-A / P1-B）

`cc-model::retrieval::{HardScope, SoftHints}` 区分两类输入，`scope.rs` 统一应用 DSL。调用方的 `path_prefix`、`languages`、`file_paths` 与全部重复 `path:` / `lang:` 约束取交集，不采用后写覆盖；矛盾约束得到空集合。未知或空的 DSL 语言、空 `path:` 显式报参数错误。只有过滤器而无检索词时，不执行空串 grep；`name:foo` 使用 foo 作为检索词。

`None` 表示没有该维度限制，`Some([])` 表示明确空集合。P1-B 将路径规则改为大小写敏感的完整路径/目录后代关系：`src/api` 不再匹配 `src/apix`。相对路径统一分隔符并消除 `.` 与重复分隔符，拒绝 `..`、绝对/驱动器路径和控制字符；原生非UTF8、反斜杠/冒号文件名因不可无歧义表示而不纳入索引。SQL 使用二进制等值或目录区间，过滤发生在候选 LIMIT 前。图种子、邻居、显式范围下的补充边证据也在限额前筛选；最终 hydration 仍二次守卫。图连通度保持项目级先验，HardScope 不是跨租户访问控制。磁盘源码入口核对精确文件名和符号链接归属，不宣称具备抵抗并发替换的原子文件系统沙箱。

预选、recent/pinned/overlay 不缩小 HardScope。`overlay_files` 目前只是路径提示，并不代表未保存文本已入库。缓存对每个可选硬字段编码有无标记；软提示列表因 rank-decay 保留顺序，避免空硬范围误用空软提示的缓存。

## 缓存与范围解释（P1-C）

两条结果缓存都纳入完整 SearchConfig、RankingConfig、检索政策版本和仓库规模档位；图缓存另有全部图限额与 token budget。配置由每个引擎不可变持有；显式重载项目会建立新引擎，不等同于已经实现配置文件热监听。硬集合保留 None/Some(empty) 区别，软提示保持顺序；等价路径拼写允许保守缓存未命中，不为提高命中率错误合并请求。

`evidence_summary.retrieval.scope` 输出生效的硬约束、显式文件数、软提示数量、候选/解压预算和排序政策，不列出被排除的索引路径或原始软提示列表。长前缀解释有截断标志；`empty` 只表示约束交集为空，不是全仓文件数。`evidence_summary.retrieval.lanes` 逐 lane 输出 `disabled/not_configured/complete/partial/timeout/unavailable/error/cancelled`、耗时、候选数、HardScope coverage 与版本化候选身份；因此“未运行”和“运行后零命中”不再共用空列表表达。输出总预算触发截断时可省略候选数组，但保留 lane 状态、候选数和 coverage。`score_trace` 回放数值重排分，`stage_a_layer_scores` 是其中 stage-a 项的明细，不能重复相加。最终顺序先 exact-target 身份层，再分数，再稳定 ID。

缓存响应内的 grep 计数描述生成该结果时的工作，不代表缓存命中时又扫描了一遍。确定性的扫描预算截断可随完整 key 缓存；预过滤读错误不进入结果缓存。`search_with_diagnostics()` 仍是绕过结果缓存的显式新查询，不伪装成缓存命中性能。

## 请求成本账单与并发边界（P1-D）

`evidence_summary.retrieval.cost`（schema_version=1）报告词法 FTS 与候选正文批取的 SQL 工作；`grep.stages[]` 分别报告 soft/prefilter/fallback。SQL 的 rows 是传给 Rust 的行数，包含重复行与仅元数据的 cap 探测；vm_steps/fullscan_steps/sorts 是 SQLite 外层语句计数，不包含全部 FTS 内部工作、存储 I/O，也不是耗时或硬上限。缓存语句每次执行前重置计数；负值或累计溢出为 null，旧产品缺少账单为 unavailable，不填零。SQLite 计数器超出其有符号 32 位有效范围时没有可依赖的数值保证，不以此认证超大工作量。

正文账单区分 storage_reads、zstd_decodes、legacy_auto_reads 与 cache_hits；utf8_bytes 是已经取得的正文字节数，不能当作磁盘读取量或峰值内存。grep_scan_cap 控制正文读取次数（plain 也占用），不是 SQL 总成本上限。账单明确未覆盖预选、精确查找、图扩展/富化 SQL，不能把这些部分遗漏包装成全查询资源账单。预过滤失败时仅保留成功阶段账单，失败标志仍可见。

文本缓存按查询开始捕获的 `(index_epoch, chunk_id)` 隔离。旧查询迟到的回填不能覆盖新代正文；图富化复用同一个 SearchPlan 的硬范围，不重新解析第二份约束。必要的 SQL 限额前过滤与最终正文守卫继续保留。读池为 1 时有并发查询/写入、连接占用释放和 SQL 错误恢复回归；`stats()` 的元数据读取复用已有连接，避免持有唯一连接时再次 checkout 自锁。P5-A 已增加三次 epoch 核验；P5-B 的查询期限和有界调度见 [QUERY_EXECUTION.md](QUERY_EXECUTION.md)。这些机制不等于严格 SQLite/文件系统快照。P5-C 已新增持久化 incarnation、集合选择、默认完整 JSON 预算与最终 EvidenceHydrator，见 [EVIDENCE_ASSEMBLY.md](EVIDENCE_ASSEMBLY.md)。冻结验收范围以 tasks.json/Gate 为准。

### 可执行请求示例

下列示例由 `p1d_docs` 通过真实 MCP 验证。前两个请求只允许 `src/` 的结果；第三个的两条路径约束互相矛盾，应返回空命中。`symbol` 仍只使用显式 path_prefix，不解释 hybrid DSL。

<!-- p1-search-contract:start -->
```json
[
  {"query":"needle lang:python path:src","mode":"hybrid","path_prefix":"src","top_k":5,"pinned_files":["excluded/private.py"],"file_preselect_limit":1},
  {"query":"needle","mode":"symbol","path_prefix":"src","exact":true,"top_k":5},
  {"query":"needle path:excluded","mode":"hybrid","path_prefix":"src","top_k":5}
]
```
<!-- p1-search-contract:end -->

## 检索通道（RetrievalLane）

`lanes.rs` 是引擎与检索策略之间的单一注册缝：每条通道实现
`RetrievalLane`，注册在 `default_lanes()`。P5-A 的权威顺序为：

| 通道 | 数据源 | 候选上限 | RRF 权重 |
|---|---|---|---|
| exact_symbol | `symbols` + `chunks`，精确 name/qname/signature | `search.exact_symbol_top_k`（24） | `search.exact_symbol_weight`（1.1） |
| path | 规范路径精确匹配 + `file_paths_fts/files` token | `search.path_top_k`（24） | `search.path_weight`（1.0） |
| lexical | `chunks_fts`（FTS5） | `search.lexical_top_k`（24） | `search.lexical_weight`（1.1） |
| grep | chunks 正文（zstd 解压后 regex/子串匹配） | `search.grep_top_k`（12） | `search.grep_weight`（0.8） |
| graph | 种子符号 + 调用边 1 跳扩展 | `search.graph_top_k`（12） | `search.graph_weight`（0.6；0 关闭） |

exact/path 是独立召回通道，不消费 preselect 白名单，也不把 soft hint 变成
HardScope；它们与其他 lane 一样在 SQL `LIMIT` 前应用完整 path/language/file
硬约束。exact symbol 有歧义时保留全部有界同名候选，不把第一行冒充唯一解析。
旧 `chunk_id` 仅作为 hydrate/wire locator，融合身份是 `(DocKey, DocVersion)`。

精确符号先匹配名称/限定名/字面签名，再用定义的行与字节列定位原始 byte span。映射只按需解压该行最早的一个前缀 chunk，复用同一连接与行起点缓存，不重建 AST、不读文件系统；同一行不同类的同名方法不能串绑。候选预算在有效坐标映射与去重后消费。该前缀解码属于 exact 通道成本，尚未计入 legacy hydration SQL 账单。截断后的同名集合不是唯一性证明。签名检索只匹配 parser 实际保存的非空字面签名，不合成缺失的签名，不宣称编译器级重载解析；例如现有 JS/TS 方法的 signature 为 None，可按 name/qname 找回，但不能假装其完整签名已被索引。

路径 token 最多8个，每 token 的文件候选预算为 max(4×lane_limit,32)，额外探测一行检查截断；没有 chunk 的文件不消费候选预算。多 token 查询只将至少三个 Unicode 标量的词作为模糊路径线索，单 token 及精确路径仍支持短名字。该规则在工作预算之前应用，被忽略的自然语言短词不被伪报成截断。

启用的本地通道在进程共享的固定4线程 Rayon 池中并发执行；仅一条启用时内联运行，不再每请求新开 scoped 线程。每条通道自行短暂取得读池连接，结果仍按注册顺序收集。MCP 请求准入与可选 async 端口使用独立有界额度，见 [QUERY_EXECUTION.md](QUERY_EXECUTION.md)。
结果按注册序收集，再通过一次不解压正文的批量查询物化 `CandidateRef`：文档
版本、原始 source span、lane 内 rank、原始诊断分和 scoring spec。缺失或损坏
manifest/source mirror、lane 重复候选、HardScope 越界都 fail closed。graph 读错
会成为显式 `error` lane，而不是伪装成 complete-empty。`RetrievalLane` 要求 `Sync`。

grep 的执行由 `grep.rs` 负责，共用一个解压预算和去重集合。先扫描软候选与 HardScope 的交集（最多512个软候选文件，最多 ceil(cap/2) 次解压）；再以 FTS 短语预过滤使用剩余预算的一部分；最后在完整 HardScope 内回退，补充 tokenizer 看不到的中缝子串。未花完的预算留给下一段，预过滤失败也不会重置已经花掉的预算。软候选内部采用稳定数据库探测序，不承诺逐个遵循提示列表顺序。

`scan_grep_stage` 在解压之前检查重复ID和 cap，允许只读下一行元数据判断游标是否真正耗尽；不会解压第 cap+1 块。只缓存已命中的正文。结果包含 `scanned/scan_cap`、各段扫描量、跳过的重复数和原因。`complete` 只表示 grep 遍历完当前硬范围；`limited` 表示达到候选数量上限，并非完整枚举；`partial` 表示预算不足或预过滤错误。它们都不是对整个搜索系统语义完整性的承诺。

MCP 的 `evidence_summary.retrieval.grep` 与图结果缓存保留该状态，即使命中列表为空也不丢失；预算不足的空结果摘要明确标注 incomplete。旧 Rust `search()` 保留仅返回命中的 API，新 `search_with_diagnostics()` 显式返回状态且不使用结果缓存。

graph 通道的种子打分与衰减由 `RankingConfig` 控制
（`graph_seed_exact_score` / `graph_seed_fuzzy_score` /
`graph_neighbor_decay`）。最多5个查询 token、每 token 10个种子、合计20个种子，每个种子每个方向10个邻居；每层独立额外探测一项确认截断。即使最终候选不足 top_k，内部截断仍标 `partial/graph_expansion_limit`；无法将已选符号映射到当前 chunk 时标 `partial/graph_source_unmapped`，不伪装成 complete。保留既有图算法与留存顺序，错误状态为 `error/graph_read_error`，不进入结果缓存。

grep 的执行判定和 `scope.budget.grep_enabled` 共用同一谓词：请求启用、查询非空且 HardScope 非空。沿用原行为，`grep_weight=0` 只去掉 RRF 权重，不代表未扫描；显式空范围下五条通道均报告 `disabled`；保留原来的零 FTS 查询与零正文读取短路，而不是执行带恒假 WHERE 的无意义 SQL。`complete` 不得同时携带截断原因；`partial/timeout/unavailable/error/cancelled` 必须有明确原因，避免矛盾或不可解释的状态通过协议验证。

Lane `elapsed_us` 仅统计该次原始通道执行，不是身份物化/最终 hydration/输出装配的总延迟。确定性的预算 partial 可缓存，但保持原状态；error/timeout/unavailable/cancelled 和预过滤故障不缓存。P5-B 已接入 query deadline、协作取消与有界 MCP 查询执行；单次不可中断 SQL 不伪称立刻终止。P5-C 已补充持久化 incarnation 与最终来源守卫；完整生命周期、规模与 G5 整体认证仍由后续验收承担。

P5-A 的正文缓存只作为解码提示：与当前行 source proof 匹配才复用，不匹配则读取同一 SQL 行的存储正文，随后仍强制验证 source proof 与 document manifest。缓存失配不再冒充数据库损坏，真正损坏的数据不被忽略。`search`、`search_with_diagnostics`、`search_with_graph_context` 在每次尝试前后比较现有 index/evidence epoch，最多三次；跨代结果和错误均丢弃，稳定代的真实错误立即返回。持续写入时返回 `RetrievalChanged`，MCP 标 `data.retryable=true`，不会返回伪空结果。结果缓存只在通过读前/读后检查后按接受的 epoch 写入；缓存命中也经过检查。P5-C 的读前后检查进一步纳入持久化 incarnation 和可选 semantic epoch；不承诺文件系统原子快照。P5-B 的总查询期限贯穿这些尝试。成本与 lane receipts 描述最后接受的尝试，端到端耗时包含重试，不应当作所有尝试累计 SQL 计数。

评测适配器 `cc-eval-public-v5` 对新 lane receipts 执行完整版本/状态/覆盖/候选验证，拒绝重复 lane；任一 partial/error/timeout/unavailable/cancelled 将测量标为 Partial，空结果不得计作正确无答案。正常非空候选仍按原评分函数计分。旧二进制没有 lanes 字段时兼容旧响应；历史 v4 记录不改写，须用原适配器回放。

P5-C 的 context 路径从相同检索算法取得有界 rerank window，通过统一 EvidenceHydrator 后进行 facet/区间选择。最终按整个结果对象预算装配，正文优先，剩余空间可放明确文档/文件引用；引用不作为正文命中评分。早期默认接线的五题退化及回退保留为历史证据；当前源码修正正文/引用优先级并恢复默认接线。public-v7 支持严格 lane_receipts 和 v2 packing 状态，新增真实预算省略仍为 Partial，不隐去完整性失败；历史回放使用对应版本 runner。详见 [EVIDENCE_ASSEMBLY.md](EVIDENCE_ASSEMBLY.md)。

## 文件预选（PreselectLayer）

`preselect.rs`。chunk 级检索之前先对文件打分作为软提示，不写回硬范围。8 个已注册的层
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

FTS5 原始 BM25 越负越相关。文件摘要贡献使用 `base + x/(1+x)`，其中 `x=max(-raw_bm25,0)`；权重配置不变，原始分数仍可从 DB 读取诊断。非有限分数不提供额外贡献。实际 SQLite→preselect→MCP 的排序方向由 `benchmark_defects` / `p1a_retrieval` 红绿测试锁定。


**独立层并发**：不读先前层得分的层（`reads_prior_scores() == false`，
今天是层 1–6）按注册表中的连续段并发执行（DB 绑定的 FTS summary 与
token search 各自取读池连接），命中按注册序合并——得分、理由、逐层
明细与串行逐字节一致。读分层（fallback 门、graph-neighbor 以前层结果
为种子）仍在合并后串行执行。新增层默认 `true`（安全侧）。

`PreselectResult` 携带逐层得分明细（`layer_scores`），并以
`preselect:<layer>:+<score>` 的形式进入命中理由——任何文件为什么被预选
进来是可审计的。

## 精确定位保护（P1-B / P5-A）

精确符号与完整路径已从 lexical 内部保留项迁移成独立 lane。最终排序先区分
`exact-target` 身份层，再按原有数值重排分和稳定 locator 排序，因此整体结果
不保证数值分数跨身份层单调。主查询是自然语言句子时不会自动当成精确标识符；
有歧义的同名定义仍可同时返回。路径规范化继续拒绝绝对路径、`..`、驱动器与
控制字符；exact/path 都不能绕过 HardScope。

## RRF 与重排

- **RRF**（Reciprocal Rank Fusion）：`lane_weight / (rrf_k + rank)` 加权求和，
  `search.rrf_k` 默认 50。各 lane 的 BM25、字符串匹配率等原始量纲只作诊断，
  从不直接相加。候选先按 `(DocKey, DocVersion)` 去重；同一文档版本映射到多个
  locator、重复 lane 身份、NaN/Inf/负权重或不可回放账单均 fail closed。
- **融合账单**：每个候选保留有序 `(lane_id, contribution)`，总分必须按位精确
  回放；该账单随后作为 `rrf:<lane>` 项进入公开 `score_trace`。
- **重排**：前 `search.rerank_window`（40）个融合候选进入重排，叠加
  `RankingConfig` 的各项加成（符号精确匹配、路径前缀、文档文件、
  working-set/recent/pinned/overlay、preselect 分映射、DSL `name:`
  过滤命中、图连通度 `graph_rerank_weight`、查询 token 重叠
  `overlap_weight`）。每一项都是独立可调、可置零的。

## 缓存

`engine.rs` 维护三级 LRU，按相应 epoch 键控。内容写入推进 index_epoch，运行时证据写入推进 evidence_epoch；纯辅助状态并不等同于内容变更。无需调用方手工失效钩子：

| 缓存 | 键 | 容量环境变量（默认） |
|---|---|---|
| 核心结果缓存 | `(index_epoch, query_hash)` | `CODECORTEX_SEARCH_RESULT_CACHE_SIZE`（32） |
| 图感知结果缓存 | `(index_epoch, evidence_epoch, 请求哈希, GraphEnrichLimits, token 预算, 排序指纹)` | `CODECORTEX_GRAPH_SEARCH_CACHE_SIZE`（32） |
| chunk 正文缓存 | `(index_epoch, chunk_id)` | `CODECORTEX_SEARCH_CHUNK_CACHE_SIZE`（512） |

chunk 正文缓存只在批取与 grep **命中**路径写入——grep 扫描过但未命中的
chunk 刻意不进缓存，防一次冷扫描刷穿 LRU（见上文 grep 通道）。

- 核心结果缓存存的是最终不可变命中列表 `Arc<[SearchHit]>`——
  `SearchEngine::search()` 直接返回这个 Arc，缓存命中是一次指针克隆，
  没有逐命中的文本拷贝。
- 图感知缓存服务于 `search_with_graph_context`（agent 默认入口
  `search_in_context` 的底层）。键覆盖**两个** epoch：运行时证据摄入会
  改变富化节点里内嵌的边置信度，所以 `evidence_epoch` 必须参与；排序指纹
  覆盖完整 `SearchConfig`、`RankingConfig`、政策版本和仓库档位；显式配置重载创建新引擎。
- **降级结果不缓存**：`graph_explain.read_errors` 非空的结果不进缓存，
  瞬时 DB 故障不会被同一 epoch 对服务到天荒地老。

图富化的基础邻接读取使用批量接口（`caller_rows_by_uids` 等），显式硬范围还会核验调用点及目标定义；不能把全流程描述为固定 3 条 SQL。见 [STORAGE.md](STORAGE.md#按能力切分的方法面)。

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
