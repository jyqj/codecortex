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
  token 校验，miss 即回退全量重载。

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
