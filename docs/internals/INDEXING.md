# 索引管线（cc-index）

> 范围：`crates/cc-index` —— 从文件扫描到分析产物落库的完整阶段管线、增量
> 正确性机制（脏闭包、签名门）、三段式提交。面向需要改动索引行为或排查
> 增量正确性问题的开发者。存储细节见 [STORAGE.md](STORAGE.md)；锁与一致性
> 窗口见 [CONCURRENCY.md](CONCURRENCY.md)。

## 阶段总览

索引是一条阶段管线（阶段头在 `indexer.rs` 与 `indexer_phases/` 目录——
按阶段拆为 `mod` / `resolve` / `write` / `config_link` / `snapshot` /
`postprocess` / `analysis` / `dirty` 8 个文件；顺序不变式在
`build_plan.rs`）：

```
scan/diff → parse → dirty closure → framework enrichment → resolve
        → write → postprocess → analysis
```

全量与增量构建共享同一套编排（`build_plan.rs`）：`prepare` 半程只读
（scan → parse → resolve → chunk 压缩 → 快照），产出自有的
`PreparedBuild`；commit 半程消费它。两种模式因此不可能漂移。

每个阶段的耗时记录在 `IndexReport.phase_timing`
（`scan_diff_ms` / `parse_ms` / `resolve_ms` / `write_ms` /
`postprocess_ms` / `analysis_ms`），MCP `index()` 响应原样携带。

## scan/diff

- gitignore 感知的文件发现（`ignore` crate），叠加 `.codecortex.json` 的
  `indexing.ignore` 与 `indexing.include`（include 是**扩展**而非收窄：
  已知语言文件总是被索引，include 只救援匹配 glob 的未知语言文件）。
- 变更检测走 **mtime+size 快路径 + 哈希确认**：mtime+size 都没变的文件跳过
  哈希；`CODECORTEX_STRICT_HASH=1` 禁用快路径，每个文件都做 blake3。
- 超过 `indexing.max_file_bytes`（默认 512000）的文件跳过。

## parse

- `rayon` 并行解析，受 `memory_budget.rs` 的自适应内存预算约束：RSS 上限为
  系统内存 × `indexing.memory_budget_fraction`（默认 0.5，钳制 0.1–0.95）。
- `indexing.max_concurrent_parse` / `CODECORTEX_MAX_CONCURRENT_PARSE`
  可显式压并发；`indexing.parse_timeout_micros` 可设单文件解析超时。
- 解析产物（符号、各类边、dispatch sites）的提取能力按语言分层，见
  [LANGUAGES.md](../LANGUAGES.md)。

## dirty closure（脏闭包）

`dirty_closure.rs`。增量构建的跨文件正确性来自一个不动点循环：

1. 对每个重解析的文件计算**导出指纹**（export fingerprint）；
2. 指纹变化的文件，其导入者被提升为 DirtyResolveOnly（重做解析后的
   resolve，不重新 parse）；
3. 循环直至收敛，受两个上限约束：文件预算
   `indexing.dirty_propagation_max_files`（默认 200）与轮次上限
   `DIRTY_CLOSURE_MAX_ROUNDS = 16`。

重新加载的边数据经过按类别的 dirty-reload 策略
（`dirty_reload_policy.rs`），决定已存储的目标 UID 是清除、重生成还是保留。

阶段如何结束由 `DirtyClosureResult::status()` 分类，作为
`dirty_propagation` 字段进入 `IndexReport`（全量构建省略）：

| 状态 | 含义 | 后果 |
|---|---|---|
| `normal` | 不动点收敛 | 无 |
| `partial_closure` | 第 1 轮之后命中轮次上限或预算 | 保留了闭包的完整轮前缀，更深的传递引用可能过期 |
| `budget_exceeded` | 第 1 轮的直接导入者就超出预算 | 什么都没重解析，跨文件引用可能过期，**建议全量重建** |
| `disabled` | 配置关闭（`indexing.dirty_propagation=false`） | 跨文件引用不维护 |

## enrichment 与 resolve

顺序：框架 resolver 富化（`framework_resolvers/`）→ 符号目录 / 类型目录 /
语义边解析（`resolver/`、`type_catalog.rs`）→ 跨文件框架解析。

### 名字解析阶梯

跨文件名字解析走一条声明式阶梯（`RESOLVE_LADDER`，
`resolver/resolve_core.rs`），自上而下首个命中即停：

```
self-member → scope → same-file → imports → suffix → global-unique
    → call-site signals（先参数个数，再 receiver；无元数据候选作为
      通配符存活）→ import-distance
```

- 每个结果的 `winning_step` 决定持久化在边/引用上的
  `resolution_strategy`（如 `fuzzy_arg_count`、`...:upgraded_from=...`）；
- `candidate_count` 进入置信度惩罚但不持久化；
- 解析器目录的 `resolve_name` 有 LRU 缓存
  （`CODECORTEX_RESOLVER_CACHE_SIZE`，默认 8192）。
- **候选上限**（`CODECORTEX_RESOLVER_MAX_POOL`，默认 256）：当某名字被超过上限个符号共享时，
  global-unique / fuzzy 阶与 `find_best` 兜底直接判不可解，不再构建/扫描该候选桶。解析器
  会把函数局部变量（`left`、`value`、`label` 等）也并入全局 `by_name`，在大仓库里这类名字
  的桶规模随文件数线性增长，逐引用扫桶 + 逐候选 `is_import_reachable` 即冷建 resolve 阶段的
  O(N²) 主因（10k→50k 实测 resolve 由近线性退化为平方）。上限把这类名义不可消歧的引用早停为
  未解析——既消除 O(N²)（16k 冷建 resolve 29.1s→1.4s），又提升精度（避免解析到他文件的随机
  同名局部变量）。`find_best` 的同文件优先则改走 `by_file_name`/`by_file_qname` 嵌套索引做
  O(1) 命中，suffix 阶用 `by_qname_leaf`（叶段索引）替代整表扫描。

### 符号目录跨构建缓存（catalog cache）

增量 resolve 历史上每次构建都从全部持久化符号重建 `SymbolCatalog`
（9 张查找表 + `TypeCatalog`）——即使单文件批也要付 O(仓库符号数) 的地板
（50k/278k 符号 ~0.8s）。`resolver/catalog_cache.rs` 把这层地板消掉：

- **宿主与有效性**：构建完成的目录停靠在 `IndexDb` 句柄的类型擦除槽上
  （与 seed 快照缓存同宿主同理由——只有句柄跨构建存活），以
  `symbols_seed` 聚合 token 为唯一有效性证明：取用时对当前持久化 token
  校验，折叠存回时携带写事务内读出的 post token
  （`write_incremental_batch` 返回 `SeedTokenSpan`）。任何批外符号写
  （全量重建、config-link 符号写、跨进程写）都移动 token → 下次取用
  miss → 冷加载，不存在过期复用。
- **取用（4a）**：命中后按文件删除被排除条目（批文件 + 被删文件；
  `SymbolCatalog::remove_files`，逐 distinct 键批量 retain，联动
  `TypeCatalog` 的按文件删除），条目槽位打墓碑、永不复用（幸存索引保持
  有效），然后照常叠加本批全量符号。`TypeCatalog` 的三张类型表为此改为
  按 `(file, value)` 存多值贡献（读取"最后存活者"），删除一个文件不再抹掉
  其他文件的同键贡献；变量类型赋值（type_assigns）保持 build-local，
  复用时清空重喂。
- **折叠（写后）**：批文件的构建期条目整体替换为**最终写入单元**的行
  （4d 富化后的版本），按 SQL 执行序（normal→dirty）做
  `INSERT OR REPLACE` 同语义的 id/uid last-wins 去重，seed 投影
  （`scope_id = None`）后存回；`live == token.count` 兜底校验。全量构建
  清槽；纯删除批直接在停靠目录上折叠删除。
- **发散契约**：复用目录与新载目录是同一条目**多重集**但桶内**顺序**
  不同（新载按 `(file_path, start_line)`），等分候选的 tie-break 可能选出
  不同的（同样合法、已受置信度惩罚的）赢家。墓碑超过存活数（含 4096
  绝对下限）时折叠拒绝停靠，下次构建重建即压实。
- **容量**：与 seed 缓存共用 `CODECORTEX_SEED_CACHE_MAX_SYMBOLS`
  （默认 500k，`0` 同时禁两层）。

配套修复：dirty-reload 对 call edges / symbol refs 的清除补齐
`target_symbol_id` / `target_file_path` / 置信度与策略字段——4c 的重解析
跳过门是 `target_symbol_id.is_some()`，旧行为只清 UID 不清 id，脏重载边
永远不会被重新解析，写回后留下悬空目标（uid 为 NULL 但 id 指向已改变的
符号）。

### 路由 handler 解析的来源记录

`resolver/route_resolve.rs` 在路由记录上留下自己的层级来源：

| `resolution_strategy` | 置信度 |
|---|---|
| `route_dotted` | 0.85 |
| `route_ladder:<阶梯策略名>` | 阶梯各策略自身的置信度 |
| `route_global` | 0.5 |
| （框架 resolver 解析，如 NestJS、ASP.NET） | NULL（由 resolver 自管） |

## 写入阶段

增量原子批写或全量重建（协议见 [STORAGE.md](STORAGE.md#全量重建协议)）。
两个写路径上的设计点：

- **chunk 压缩前置**：chunk 正文在 `prepare`（无锁阶段）zstd 压缩，作为
  `PreparedBuild` 的边车（`chunk_blobs`）携带——写事务只绑定预先算好的
  blob。事务内压缩回退仍保留，兜底边车缺失的 chunk。
- **config-linker 签名门**：昂贵的扫描半程（`scan_config_tokens`：项目
  遍历 + 分词）在配置文件集签名（路径 + mtime + size，持久化为
  `last_config_sig`）未变时跳过——缓存的原始 token 直接对当前符号/文件
  目录重新解析（`resolve_config_links`）；签名未变**且**本批没写任何东西
  时整个 pass 跳过；缓存 token 集为空时解析半程也短路。

### 写阶段性能注记

10k 合成基准上历经三轮优化（详见 [BENCHMARK.md](../BENCHMARK.md)），
留下的结构性约束值得知晓：

- FTS5 删除必须 rowid 对齐批量化（见 STORAGE.md），禁止逐文件 DELETE；
- 层级边只为本批文件重生成，不做全量重算；
- 框架检测不再因"变更文件 > 20"回退全仓扫描（file 级信号是文件的纯函数；
  repo 级 manifest pass 本来就每次构建重跑）；
- 写连接语句缓存 64 槽（默认 16 槽会被批写路径打穿，逐行重 prepare）；
- **签名门输入是持久化行哈希聚合**（cc-db `signature_agg.rs`，metadata
  键 `graph_sig_aggregates`：逐组 `(count, 行哈希和)` 的可交换聚合）：
  dispatch/interface/community 门不再每轮全表扫描四张表。代价是维护
  契约——凡写 `symbols` / `call_edges` / `semantic_edges` /
  `dispatch_sites` 的**生产路径必须在同事务维护聚合**（文件域写者按
  path 差分，全量重建末尾重算基线）；raw-SQL 绕过维护会让聚合过期、
  腐蚀 gate 决策（无基线的库回退全表扫描，永远不会得到错误值）；
- **community 门在聚合空间投影决策**：staged 合成动作对聚合做投影
  （`community_signature_projected`，不载边），只有判为 RUN 才真正载入
  边表跑 Louvain；
- **resolver seed 有跨构建缓存**（cc-db `seed_symbol_cache.rs`）：seed
  符号快照挂在 `IndexDb` 句柄上跨构建复用，以 `symbols_seed` 聚合为
  token 校验，miss 即回退全量重载。其上还有一层**符号目录缓存**
  （cc-index `resolver/catalog_cache.rs`，见
  [符号目录跨构建缓存](#符号目录跨构建缓存catalog-cache)）：命中时连
  seed 物化都省掉，seed 缓存只服务目录缓存 miss 时的快速重建。
- **FK CASCADE 子表显式批量删除**（cc-db
  `delete_files_data_chunk_keep_test_edges`）：`DELETE FROM files` 不依赖
  ON DELETE CASCADE 逐父行触发子表删除，而是先按 `file_path` 批量 DELETE
  各 CASCADE 子表（`call_edges`/`symbol_refs`/`chunks`/`imports`/
  `literal_index`/`symbols`；routes 在前序循环里删），再删 files。SQLite 的
  CASCADE 对每个父行触发一次子表 DELETE（父行数 × 子表数次内部语句 + FK
  检查），在大库上比"每子表一次批量 `DELETE … WHERE file_path IN`"慢
  2–5 倍（50k 5% 批量 `db_replace_delete` ~24s → ~10s；索引维护工作量两者
  相同，省下的是逐父行语句/FK 开销）。维护契约：该列表须覆盖所有
  `REFERENCES files(file_path) ON DELETE CASCADE` 的表，schema 测试兜底。

## postprocess（写后处理）

在写入**之后**运行，读已提交的索引、写回产物：test edges → dispatch
synthesis → Louvain 社区检测。

### PassGate：声明式跳过

每个 pass 的跳过逻辑声明为一个 `PassGate` 适配器（`pass_gate.rs`）：
`DbSignatureGate`（DB 内容签名）、`FileSignatureGate`（文件集签名）、
`StringCacheGate`（HEAD 字符串）、`PairGate`（把两个门耦合成一轮）。
未变的输入通过同一个缝隙跳过，而不是各处手写检查。

门把**决策**与**记账**分离：在无锁的 compute 阶段决定 skip/run，发出一个
`DeferredSignatureRecord` 随 pass 的类型化 delta 一起走，apply 阶段在对应
delta 真正落库后才持久化签名——保证"签名已记录但产物没写入"的窗口不存在。

`PassGate` 只覆盖 postprocess/analysis（compute→apply 二元决策 + 延迟记账）。
写阶段的 **config-linker 签名门**是唯一不套 `PassGate` 的跳过门：它是三方决策
（整跳 / 复用缓存 token / 重扫）、立即记录、带 raw-token 缓存，与延迟记账模型
不匹配。它与 `FileSignatureGate` 共享的是"algo 版本化的 u64 签名比较"这一**模式**
（见 [写入阶段](#写入阶段) 与 `config_link.rs::build_config_link_units_gated`
的文档），而非 trait。

### BuildExplain：构建侧决策信封

postprocess/analysis 各门的决策（`synthesis_round` / `community` /
`git_cochange` / `infra` 的 run/skip + 原因）与降级信号
（`community_edge_cap_exceeded`、`cochange_unavailable`）收集进一个
`BuildExplain` 信封（`cc-model/src/build_explain.rs`，与读侧
[`GraphExplain`](../ARCHITECTURE.md#图可解释性graphexplain) 对偶），挂在
`IndexReport.build_explain`（空则序列化省略）。它是读侧 `GraphExplain` 的
构建侧对应物——回答"为什么这次合成/社区/git 共变被跳过或降级"。`compute` 阶段
经 `BuildExplainCollector` 增量收集，`apply` 阶段盖章进 `IndexReport`；门决策
同时仍发 `tracing`，信封是追加而非替代。**不重复** `dirty_propagation` 与
`phase_timing`（已在 `IndexReport`）。config-linker 门的三方决策（整跳 / 复用
缓存 token / 重扫）也纳入：collector 创建提前到 `commit_write`，穿越 `WrittenBuild`
到 `compute_postprocess`，与 postprocess/analysis 决策汇合。



### dispatch synthesis（派发合成）

为动态派发合成调用边：事件 emitter → handler、JSX/Vue 组件渲染、
state-setter 重渲染链、字段反向 observer、接口派发。

- 每个 pass 在 `dispatch_synthesis/mod.rs` 恰好声明一次为
  `SynthesisPassSpec`：id、签名门、它拥有的合成调用 kind 与语义边前缀、
  compute 函数。`registry()` 按执行顺序列出（接口派发最后跑）；合成关闭时
  的清理集合从这些声明派生，不手工重复。
- compute/apply 分离（`synthesis_pipeline.rs`）：每个 pass 通过读池对已
  提交快照计算 `EdgeDelta`（不占写锁）；所有 delta 在单个 `UnitOfWork`
  里原子 apply。
- 跨 pass 叠加层（`PassContext::prior_deltas`）只覆盖 CALL 边；消费语义边
  的 pass 读已提交状态。
- 事件合成受 `indexing.event_fanout_cap`（默认 6，先按 receiver/同文件
  收窄）与 `indexing.event_denylist` 约束。

## analysis（分析）

同一 `PassGate` 注册表驱动：

- **git 共变**：`HEAD` 未变则跳过（`StringCacheGate`）；
- **基础设施 pass**：以 infra 候选文件集的路径+mtime+size 签名为门；
- **ADR 索引**。

## 三段式提交（staged commit）

commit 半程拆成三个锁域（`build_plan.rs` 的模块文档即契约；决策记录见
[ADR-0002](../adr/0002-staged-commit-postprocess-out-of-write-lock.md)）：

1. **`commit_write`** —— generation guard + `phase_write`，在调用方写锁内；
2. **`compute_postprocess`** —— postprocess/analysis 的**计算**，不持任何
   锁：签名扫描、合成 pass、Louvain、git/infra/ADR 分析通过读池读刚提交的
   快照，产出类型化 delta（`PostprocessPlan` / `AnalysisPlan`）；
3. **`apply_postprocess`** —— 短写锁内以小事务 apply delta，产出
   `IndexReport`。

**新鲜度守卫**：`PreparedBuild` 记录 prepare 开始时的 `index_epoch`；
阶段 1 重读并在不匹配时拒绝写入，阶段 3 在 apply 前复查写后 epoch——任一
不匹配都浮出为 `CcError::StalePreparedBuild`，cc-server 的共享构建驱动
（`run_split_build`，`handlers/core.rs`）整体重跑 prepare+commit 一次。

阶段 2 的正确性（write 与 apply 之间没有并发构建改库）依赖调用方在三个
阶段全程持有 per-project build gate；epoch 复查针对的是跨进程写者。

**接受的最终一致窗口**：读者可能短暂看到阶段 1 的索引内容，而其后处理
产物（合成边、社区、test edges、共变/infra/ADR）尚未刷新——每次阶段 3
apply 都推进 `index_epoch`，epoch 键控缓存随 delta 落地而收敛。

捆绑式 `commit`（及调用它的 `&mut self` 构建）内联组合同样的三个阶段
函数，每个阶段只有一份实现。

## 扩展点

本 crate 的可扩展缝隙（完整目录见
[ARCHITECTURE.md](../ARCHITECTURE.md#扩展点)）：

- 框架路由 resolver：`FrameworkResolver` → `default_registry()`；
- 框架检测信号：`FrameworkSignalSpec` → `signal_registry()`；
- 合成边 pass：`SynthesisPassSpec` → `registry()`；
- 后处理跳过门：`PassGate` → `indexer_phases/postprocess.rs` 与
  `indexer_phases/analysis.rs` 的 compute 阶段。
