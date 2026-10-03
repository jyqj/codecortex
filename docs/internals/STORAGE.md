# 存储层（cc-db）

> 范围：`crates/cc-db` —— SQLite 索引存储的连接模型、写隔离、epoch 失效协议、
> 表结构与重建协议。面向需要改动持久化层或排查缓存失效问题的开发者。
> 上层视角见 [ARCHITECTURE.md](../ARCHITECTURE.md)；并发全景见
> [CONCURRENCY.md](CONCURRENCY.md)。

**权威状态全部存于单一数据库文件 `index.sqlite3`**：索引内容、
incarnation/generation、语义 manifest 可见集合（`semantic_manifest`）、
desired outbox 与 claim/lease 队列（`semantic_outbox`）、active space
指针（`semantic_spaces`）。没有第二个权威数据库、没有会话存储、没有遥测
落盘（见 [DESIGN.md](../../DESIGN.md) 的非目标清单）。

**显式例外（唯一）**：可选语义功能的派生 artifact cache 存于库外本地目录
（内容寻址、可逐条校验、可整目录丢弃重建，namespace 跨项目默认隔离）。
它不是权威状态——损坏只降级不污染，删除只损失已付费的嵌入产物，不损失
任何可从源码/主库重建的事实。该边界与 durability 顺序的正式决策见
[ADR-0003](../adr/0003-semantic-persistence-single-db-boundary.md)（本节
无限定"单一数据库"表述由该 ADR 限定修订）；cache 的目录布局、namespace
键与降级语义见本文[语义持久化](#语义持久化p6schema-v22)一节与
[CONFIGURATION.md](../CONFIGURATION.md#语义缓存与降级p6可选)。

当前 schema **22**：在 v21 基础上追加 P6 语义持久化三表
（`semantic_manifest` / `semantic_outbox` / `semantic_spaces`，见下文）
及 7 个索引；历史沿革（P2-C resolution_frontier、P3 模块证据与 Go 包集合、
P4 chunk policy/document manifest、P4-D 质量成本口径）见
[DOCUMENTS.md](DOCUMENTS.md) 与 [SOURCE_CHUNKS.md](SOURCE_CHUNKS.md)。
数据库不持久化 tree-sitter tree 或通用 AST，边界只以 chunk/source proof
和派生事实存在。v21 及更早缓存须隔离重建。欠账及其确认与文件/关系批次在
同一 IMMEDIATE 事务发布，并由 index epoch 围栏保护。不存在独立的
frontier setter。升级/回滚路径见本文 [Schema 版本策略](#schema-版本策略)
与 [INCREMENTAL_RECOVERY.md](INCREMENTAL_RECOVERY.md)。

完整重建的 WAL/SHM 文件名通过向完整数据库路径追加 `-wal` / `-shm` 得到，不替换扩展名；否则自定义文件名可能残留旧 WAL 并污染新库。相关回归与成本口径见 [INCREMENTAL_VERIFICATION.md](INCREMENTAL_VERIFICATION.md)。

项目配置输入确认在事实/frontier 成功发布后进行；失败可重复失效，不宣称与所有后处理同事务。提交前复核捕获配置，详见 [PROJECT_MODEL.md](PROJECT_MODEL.md)。

## 连接模型

`IndexDb`（`index_db.rs`）持有：

- **r2d2 读连接池**：默认 4 个读连接（`open_with_read_pool_size` 可指定，
  钳制到 1–64；`indexing.db_read_pool_size` 为 `null` 时按仓库规模档位推导
  4–12）。`min_idle = min(pool, 2)`，空闲连接 300 秒回收。
- **专用写连接**：单条、`Mutex` 保护。多语句写事务只能通过 `UnitOfWork`
  进入（见下），不存在裸写连接的公开路径。
- **语句缓存**：读写连接均为 64 槽（rusqlite 默认 16 槽会被热路径 ~25+ 条
  常驻 `prepare_cached` 语句、批写路径 ~20+ 条轮转语句打穿，导致逐行重
  prepare —— 这是 10k 规模写阶段优化中实测过的退化点）。
- **seed 符号快照缓存**（`seed_symbol_cache.rs`）：resolver seed 符号
  快照的跨构建缓存，挂在 `IndexDb` 句柄上（唯一跨构建存活的宿主）。以
  写时维护的 `symbols_seed` 聚合（见 `signature_agg.rs`）为有效性
  token——token 相等即种子行多重集相等，命中省去增量构建逐次全量重载
  symbols 的 O(repo) 成本；任何改动种子列的写（进程内或跨进程）都会
  移动 token，下次读取 miss 重载。
- **resolver 目录停靠槽**（`resolver_catalog_slot`）：同一宿主上还有一个
  类型擦除槽（`Box<dyn Any + Send>`），cc-index 把构建完成的
  `SymbolCatalog` 停靠在这里跨构建复用（cc-db 不依赖其具体类型；有效性
  同样由 `symbols_seed` token 证明，`write_incremental_batch` 为此把事务
  内观察到的 token 前后值作为 `SeedTokenSpan` 返回给上层折叠）。见
  [INDEXING.md](INDEXING.md#符号目录跨构建缓存catalog-cache)。
- **file-state 快照缓存**（`file_state_cache.rs`）：scan/diff 用的
  `files` 表元数据（hash/mtime/size）跨构建缓存，同一宿主、同一模式——
  以写时维护的 `files_state` 聚合为有效性 token，命中省去每次增量构建
  全表重载 `get_file_state` 的 O(repo) 成本；增量批提交后原位应用差分。

连接初始化 PRAGMA：

- **写连接**：`journal_mode=WAL`、`synchronous=NORMAL`、`foreign_keys=ON`、
  `busy_timeout=5000`、`cache_size=-65536`（64 MB）、
  `mmap_size=536870912`（512 MB）。cache/mmap 强化是多百 MB 库随机索引页
  命中的关键（50k 5% 批量写 8.66s→5.07s，commit `976a626`）。
- **读池连接**：以上除 cache/mmap 外，额外加 `query_only=ON`——写隔离从
  "只有 `WriteOps` 暴露写方法"的类型纪律升级为机制保证：经池化连接的
  任何 INSERT/UPDATE/DELETE 直接报 `SQLITE_READONLY`，不可能绕过 epoch
  推进让 epoch 键控缓存读到陈旧数据。

### 按能力切分的方法面

`retrieval-capabilities-v2` 明确采用 `consistency=point_in_time`：完整 `generation` 由 `generation_scope=observed_database_snapshot` 标识为观察 generation。indexed_files/symbols、freshness、semantic_active_space 与覆盖/待处理计数来自同一短只读事务；普通并发发布可以返回完整旧或新观察，不能旧 root 配新 coverage。ready 仅表示该观察时点的覆盖，下一条 query 仍执行自己的严格代际 fence。`identity_validation` 为 `checked_at_observation_boundary` 或 `not_observed`，检测到替库/身份不确定时保守不可用；`service_state_scope=process_observed_separately` 区分运行态，worker failed/degraded 仍优先覆盖 semantic ready。当前 Linux 的 SQLite VFS 身份检查已进入本次验证；其他平台未测，不支持 HAS_MOVED 的 VFS 上 capability 诊断受限，但不据此判定普通 query 不可用。v1 到 v2 没有长期兼容分支，工具名称与输入 schema 不变。详见 [ADR-0004](../adr/0004-capability-status-point-in-time.md)。

公开方法面按能力切成三个零成本借用视图（与 cc-server `CodeIndex` 同一模式）；
生命周期（`open` / `open_with_read_pool_size`）留在 `IndexDb` 本体上：

| 视图 | 入口 | 内容 |
|---|---|---|
| `ReadOps` | `.reads()` | 类型化符号/文件读取、`generation`、`stats`、`get_metadata`、`get_file_state`、`public_surfaces`、`surface_dependents`、`resolution_manifests`、`resolution_dependents` 与按包键读取的贡献。不向上层代理裸 `read_conn`；邻接读取保留批量接口。图 SQL 数量依请求范围、空集合和批大小而变，不能固定宣称整次搜索只有三条 SQL |
| `RetrievalReadModel` | `.retrieval()` | FTS5/trigram **检索**查询（file-summary bm25、path/symbol token 批量命中、chunk 批量取、symbol seed/uid 查找）——cc-search 的 preselect 与 graph lane 消费。SQL 直接拥有在 `impl RetrievalReadModel`，不经 `impl IndexDb` 转发 |
| `GraphReads` | `.graph_reads()` / `GraphReads::new` | 任务形态的**图读**查询（邻接/impact/dead-code、imports/communities、HTTP/async 桥、infra 绑定）——cc-server 的 graph/impact/exploration 工具消费。15 个核心 SQL 直接拥有在 `impl GraphReads`（`self.db.read_conn` / `self.db.query_json`），不经 `impl IndexDb` 转发；另有 8 个仍借用 `IndexDb` 通用方法（`call_uid_edges_lite` 等在 `index_db_graph.rs`）。`ReadOps` 的图读 delegate 改转发到 `.graph_reads()`，保留通用读入口 |
| `WriteOps` | `.writes()` | 所有推进 epoch 的变更：批量写、边/证据写入、`set_metadata`、`begin_unit_of_work`。这是写方法的唯一公开路径（编译期写隔离） |
| `MaintenanceOps` | `.admin()` | 重建协议（`rebuild_with_temp_db` / `rebuild_with_direct_writer`，及拆半程的 `build_temp_db_staging` + `swap_rebuild_staging`）、`checkpoint_wal*`、`instance_id` |

### UnitOfWork：多语句写的唯一缝

`unit_of_work.rs`。`UnitOfWork` 在整个生命周期内持有写连接，运行一个
`IMMEDIATE` 事务，只暴露类型化写方法（从不交出裸连接）：

- `commit()` 时**恰好一次**推进 `index_epoch`（等价于
  `commit_with(EffectSet::of(Index))`）；语义写路径经
  `commit_with(effects: EffectSet)` 声明 `Semantic`/`Auxiliary` 效应，
  规则见上文 [Epoch 协议](#epoch-协议三钟与四效应)；
- 未 commit 即 drop 时自动回滚；
- 需要新的写操作时，在 `UnitOfWork` 上加类型化方法，而不是绕过它。

调用方通过 `IndexDb::writes().begin_unit_of_work()` 获取。参考用法：
dispatch synthesis 的 apply 阶段（`cc-index/src/synthesis_pipeline.rs`）。

## Epoch 协议：三钟与四效应

持久化时钟（存于 `metadata` 表）各自独立推进，是全部下游缓存失效的依据。
P6 引入 `semantic_epoch` 后协议从"双时钟"扩展为**三钟 + 四类 typed write
effects**（`epoch_rules.rs` 的 `WriteEffect`/`EffectSet`，`commit_with` 为
提交缝，`index_db_write_batch.rs` 的批量写路径同语义）：

| 时钟 | 推进时机 | 失效语义 |
|---|---|---|
| `index_epoch` | 声明 `Index` 效应的提交各恰一次：增量/批量写、后处理产物写回、全量重建 finalize | 索引内容变了，索引派生缓存全部失效 |
| `evidence_epoch` | 仅声明 `Evidence` 效应的提交：运行时证据写入与 `boost_http_edge_confidence` | 证据持续到达，不得驱逐 index-only 缓存槽 |
| `semantic_epoch` | 仅声明 `Semantic` 效应的提交：**语义状态实际变化**——manifest 可见集合变化（发布 CAS、删除撤销、revoke 消费）或 outbox 期望集合重入队（P6-006 口径：非零 outbox 统计即语义状态变化） | 语义可见集合/期望状态变了；检索内容的完整查询缓存不因 Auxiliary 重试冲刷 |

四效应的提交级规则（`epoch_rules.rs`）：

- `Index`/`Evidence`：commit 必 bump 对应钟，恰一次（既有规则）。
- `Semantic`：bump `semantic_epoch` 恰一次，且仅当提交者声明语义状态变化
  （可见集合 diff / outbox 变化的判定与声明责任在写路径与发布编排方，
  cc-db 是声明式机制：不声明 `Semantic` 的提交绝不移动该钟）。
- `Auxiliary`（claim/renew/heartbeat/retry/reclaim 等队列簿记）：**不推进
  任何钟**——队列可靠性数据不是检索内容（ADR-0003 边界修订在 epoch 协议
  上的直接体现）；空 `EffectSet` 与 `Auxiliary` epoch 等价。
- `semantic_epoch` 键**缺失 = 语义未就绪**：strict 读 `ReadGeneration::
  semantic_epoch` 返回 `None`，绝不折叠为 0；首次 bump 恰写入 `1`。

组合效应 `{Index, Semantic}` = 单事务内两钟各 bump 恰一次（互不共享计数）。
`epoch_rules.rs` 声明完整的表 → 时钟映射，审计测试逐一核对每个写方法：

| 表 | 时钟 | 理由 |
|---|---|---|
| files, public_surfaces, resolution_manifests, resolution_dependencies, symbols, imports, call_edges, symbol_refs, semantic_edges, dispatch_sites, data_flow_edges, literal_index, chunks | `index_epoch` | 文件批写入的索引内容 |
| routes, http_call_edges, test_edges, co_change_edges, communities, frameworks, infra_nodes, infra_edges | `index_epoch` | 解析/后处理产物，被 context/graph 输出当作索引内容消费 |
| adr | `index_epoch` | ADR 在 context 输出中以 index_epoch 为键出现 |
| runtime_evidence | `evidence_epoch` | 持续摄入，不能驱逐 index-only 缓存槽 |
| semantic_manifest, semantic_outbox, semantic_spaces | `semantic_epoch`（声明式） | 语义可见集合与期望队列状态；状态机簿记（claim/lease/retry）走 `Auxiliary` 零 bump |

唯一例外：`boost_http_edge_confidence` 只改 `http_call_edges.confidence`
一列，推进的是 `evidence_epoch` —— 它是证据驱动的置信度提升，不是索引变更。
下游缓存槽通过 `EpochSensitivity` 声明自己对哪个时钟敏感
（[`graph_read_model/cache.rs`](../../crates/cc-server/src/graph_read_model/cache.rs)，
详见 [CONCURRENCY.md](CONCURRENCY.md#epoch-失效协议)）。

`generation()` 用单条 SELECT 同时读出全部时钟，返回一致的
`ReadGeneration` 快照（含 `semantic_epoch: Option<u64>`），避免读到撕裂的
版本向量。语义诊断读（覆盖率等）在同一池化连接上做 before/after 代比对，
前后不等即重试，绝不返回混代计数（`semantic_coverage.rs`）。

### P4-D 文档存储成本口径

P4-D的release收据分别记录`chunks`/`document_manifest`行数、原始source span字节、模型输入字节、manifest JSON字节、数据库文件大小及最大块。它们是机制成本，不是SQLite页级归因或长期空间放大率；阶段RSS还包含测试进程、解析器和连接缓存，不能从单次差值推断某一张表的独占内存。

## 表结构

基表（schema v22，`index_v1.sql`；PublicSurface 与 ResolutionManifest 格式版本各自独立为 1）：

| 组 | 表 | 内容 |
|---|---|---|
| 元数据 | `metadata` | KV：epoch、版本、各 pass 的输入签名等 |
| 文件与内容 | `files`, `chunks` | 文件元数据（路径/语言/哈希/摘要）；检索分块（zstd 压缩正文） |
| 公共接口证据 | `public_surfaces` | 与文件同事务的版本化声明、Known/Unknown 状态及规范指纹；删除级联，详见 [PUBLIC_SURFACE.md](PUBLIC_SURFACE.md) |
| 解析结果与依赖 | `resolution_manifests`, `resolution_dependencies` | 有界结果、歧义、能力状态和正/负查找键；同文件事务与删除级联，详见 [RESOLUTION_DEPENDENCIES.md](RESOLUTION_DEPENDENCIES.md) |
| 符号与引用 | `symbols`, `symbol_refs`, `imports` | 符号定义；引用位置（含跨文件目标 UID）；导入声明 |
| 调用与关系 | `call_edges`, `semantic_edges`, `data_flow_edges`, `dispatch_sites` | 调用图（含合成边）；继承/实现等语义关系；数据流（type_ref / env_access / param_pass / return_flow）；动态派发点 |
| Web 与服务 | `routes`, `http_call_edges`, `infra_nodes`, `infra_edges` | HTTP 路由；出站 HTTP/异步调用；基础设施节点与连边 |
| 分析产物 | `communities`, `frameworks`, `co_change_edges`, `test_edges` | Louvain 社区；框架检测（repo 级 + file 级）；git 共变；测试关联 |
| 其他 | `literal_index`, `runtime_evidence`, `adr` | 字面量索引；OTLP 运行时证据；架构决策记录 |
| 语义持久化（P6） | `semantic_manifest`, `semantic_outbox`, `semantic_spaces` | 可见集合（发布 CAS 的产物）；desired 任务队列（claim/lease fencing 内联列）；编码空间生命周期（backfilling/active/revoked）。字段与不变式见[语义持久化](#语义持久化p6schema-v22) |

`metadata` 表中的固定键：`index_epoch`、`evidence_epoch`、
`semantic_epoch`（P6，键缺失 = 语义未就绪，见上文 Epoch 协议）、
`last_indexed_at`、`index_version`、`graph_sig_aggregates`（图签名聚合
基线：逐组 `(count, 行哈希和)` 的序列化快照，由写路径在同事务内维护，
是 postprocess 签名门与 seed 缓存 token 的输入，见 `signature_agg.rs`），
以及各后处理 pass 的输入签名键（如 config-linker 的 `last_config_sig`，
见 [INDEXING.md](INDEXING.md#写入阶段)）。

### FTS5 虚拟表：双维护模型

5 张 FTS5 虚拟表，rowid 与基表 rowid 对齐，但维护方式分两类：

| FTS 表 | 索引列 | tokenizer | 维护方式 |
|---|---|---|---|
| `symbols_fts` | name | trigram | **触发器**（INSERT/DELETE/UPDATE OF name） |
| `file_paths_fts` | file_path | trigram | **触发器**（INSERT/DELETE/UPDATE OF file_path） |
| `chunks_fts` | breadcrumb, symbol_name, text | unicode61 | **应用层** |
| `files_fts` | summary, content_excerpt | unicode61 | **应用层** |
| `literal_fts` | literal, literal_kind | unicode61 | **应用层** |

- 两张 trigram 镜像服务于文件预选中的子串符号查找和路径 token 查找，由
  触发器与基表同步——**任何写路径都不得直接写它们**。
- 三张应用层维护的表由 `delete_file_data` 与共享插入助手负责：单行写入用
  `last_insert_rowid()` 对齐 rowid，批量写入用
  `INSERT INTO x_fts(rowid, …) SELECT rowid, … FROM base WHERE file_path IN (…)`；
  删除走 rowid 对齐的批量 `DELETE … WHERE rowid IN (SELECT …)`，避免
  FTS 全表扫描（这是 10k 写阶段优化的关键一步）。
- **重建协议必须经由这些共享助手写数据**，否则应用层维护的 FTS 表会失去同步。

### REGEXP UDF

标量 UDF `REGEXP(pattern, text)` 支撑 Cypher 的 `=~`。编译后的正则作为
SQLite auxiliary data 缓存：常量模式每条语句编译一次，而不是每行一次。

## 全量重建协议

两种重建策略是同一个 `run_rebuild_protocol` 之上的薄构建适配器：

1. 快照一个 epoch 下限（floor）；
2. 在临时文件中构建替换数据库——`rebuild_with_temp_db` 走常规批写，
   `rebuild_with_direct_writer` 走实验性的直写器（绕过 SQL 解析，
   `indexing.use_direct_writer` 开启）；
3. 在写锁下：把 generation 终结为 `max(floor, live) + 1` → 原子 rename
   临时文件 → 主文件 → 重开写连接、重建读池、checkpoint WAL。

`max(floor, live) + 1` 保证重建期间并发落地的增量写不会让 epoch 倒退，
下游 epoch 键控缓存不会读到"回到过去"的版本号。

协议本身由两个可独立调用的半程组成：`build_temp_db_staging`（步骤
1–2，产出 staging 文件并返回 epoch floor）与 `swap_rebuild_staging`
（步骤 3）。cc-index 的全量构建在 prepare 阶段（不持索引写锁）就完成
staging 写入，commit 阶段只做换库——`PreparedBuild` 因此无需把全部
write_units/chunk blob 驻留内存到 commit（见 INDEXING.md）。staging
写入只触碰临时文件，不违反"prepare 不写索引"的锁契约；换库前
`swap_rebuild_staging` 校验 staging 文件仍存在。

### 批量重建的 PRAGMA 切换

进入重建：`synchronous=OFF`、`temp_store=MEMORY`、`cache_size=-64000`
（64 MB）、`mmap_size=268435456`（256 MB）；完成后恢复
`synchronous=NORMAL`、`temp_store=DEFAULT`、`cache_size=-2000`。
`synchronous=OFF` 只在临时文件构建期间使用——崩溃最多丢掉尚未 rename
的临时库，主库不受影响。

## WAL 管理

- 全量重建做 `wal_checkpoint(TRUNCATE)`，折叠 WAL 回主文件；
- 长期只跑增量的会话通过 `checkpoint_wal_if_large` 在 WAL 超过 16 MB 时
  checkpoint。

## 语义持久化（P6，schema v22）

本节是 P6 语义底座的存储侧事实记录，边界权威为
[ADR-0003](../adr/0003-semantic-persistence-single-db-boundary.md)。实现
分居两 crate：SQL 与权威状态在 cc-db（`semantic_*.rs`），cache 与编排
原语在 cc-semantic（`cache.rs`/`queue.rs`/`publish.rs`/`reconcile.rs`/
`recovery.rs`/`gc.rs`/`space_switch.rs`/`degrade.rs`）。组合根接线
（worker drain 触发时机、reconcile/recovery 调度、降级快照转写）尚未
落地——库层协议已全部交付并有测试固化，未实现的行为本文不写。

### 权威侧三表与状态机

- `semantic_manifest`（**可见集合**）：`doc_key` 主键、FK 指向
  `document_manifest(doc_key) ON DELETE CASCADE`（删基行即撤可见条目），
  另含 `doc_version/file_path/encoding_key/input_digest/space_id/
  artifact_ref/published_at/published_incarnation`。唯一写入路径是发布
  CAS（见下）；`artifact_ref` 指向派生 cache 对象，GC mark 索引
  `semantic_manifest_artifact` 预留于此。
- `semantic_outbox`（**desired 任务队列**）：`op ∈ {'embed','revoke'}`
  （revoke 仅由显式空间切换产生）、
  `state ∈ {'pending','claimed','done','failed','superseded'}` 走封闭
  迁移表（`semantic_outbox.rs` `OutboxState::can_transition_to`，非法
  迁移类型化拒绝）；claim/lease 状态内联三列
  `lease_token/lease_expires_at/claim_owner`；`attempt_count/last_error`
  构成持久审计轨。部分唯一索引
  `semantic_outbox_live_per_doc(doc_key,space_id) WHERE state IN
  ('pending','claimed')` 是**合并语义的 DB 层保证**：同 (doc,space) 至多
  一个活跃任务——连续编辑在 worker 看到之前已合并为单个最新版本任务。
- `semantic_spaces`（**空间生命周期**）：`space_id` 主键、冻结
  `spec_json`、`state ∈ {'backfilling','active','revoked'}`。active
  读口径 = 单行 `WHERE state='active'`，出现多行即 fail-stop。

### at-least-once 与 fencing（明确不是 exactly-once）

- **原子入队**：文档批写/删除与 outbox supersede-then-insert 在同一个
  `IMMEDIATE` 事务（`supersede_and_enqueue_on`，
  `semantic_outbox.rs:239`）——删除只撤 manifest、supersede 活跃任务，
  **永不产生 embed 任务**；事务回滚零残留。无 active space 时整段零 SQL，
  默认路径零行为变化。
- **claim 是单语句 CAS**（`claim_next_on`，`semantic_outbox.rs:496`）：
  候选挑选、state 翻转、lease 列写入、attempt 递增在一条
  `UPDATE … RETURNING` 内，两进程竞争恰一者胜出。**每 attempt 新随机
  token**（`randomblob(16)`）；renew/ack/retry 全部只认 token
  （`WHERE task_id=? AND lease_token=? AND state='claimed'`，
  `claim_owner` 仅诊断）；rowcount=0 一律 `Ok(false)` 零写入——过期
  worker 的迟到结果被结构性拒绝。lease 过期由 `reclaim_expired_on`
  （:653）归还 pending（不消耗 attempt 预算）。
- **重复 ack 被吸收（Q4 口径）**：ack 只发生在发布 CAS 事务内
  （`ack_done_on`，:602）；等值内容重复发布判定
  `visible_set_changed=false`——不写 manifest、照常 ack、不 bump
  `semantic_epoch`。
- **费用上界不是零重复**：attempt 预算耗尽 → 终态 `failed` 死信
  （`retry_on`，:621）；cache 损坏触发的补嵌受进程生命周期 re-embed
  预算约束（见降级小节）。本系统**不宣称跨两库/网络 exactly-once 或
  零重复收费**——重复做功靠合并 supersede、fencing 丢弃慢结果、预算
  死信三道机制限制上界，这是 ADR-0003 与本任务验收的明确口径。

### 发布 CAS 与 durability 顺序

`cc-semantic/src/publish.rs:98` 的 `publish_embedding` 三步强制
**artifact durable 在先、manifest CAS 在后**：

1. `cache.put`（同目录临时名 → fsync → 原子 rename）；
2. `cache.get` 读回必须 `Hit` 且 ref 与 put 返回一致（checksum + 四元组
   寻址验证链全过），否则 `ArtifactNotVerified`，数据库零接触；
3. `IndexDb::publish_semantic`（`semantic_publish.rs:355`，内部
   `publish_and_ack_on` :179）：**五重 fence** 全过才写 manifest 并
   fenced ack——① incarnation（strict `ReadGeneration`，绝不走 legacy
   双钟）、② lease token、③ doc_version（慢旧结果挂不上新版本）、
   ④ input digest、⑤ space 必须是唯一 active 行。任一不过 = 零写入
   返回拒绝（`PublishRejection`，:68）并在同事务落 fenced retry。

两存储间**不存在原子提交**；可能的 crash 残态与恢复见下节。

### 恢复步骤与 crash 点

恢复编排是显式调用方驱动的有界扫描（`recover_scan`，
`cc-semantic/src/recovery.rs:209`）：**fence 先行**（权威路径上新开
只读连接读 incarnation，`semantic_rebuild.rs` `generation_at_path`——
持旧 inode 的幽灵进程在任何事务前即被 `Fenced` 零写入）→ 一次有界
过期回收（≤ `scan_batch`）→ 一轮有界重放（cache 命中的任务走完整五重
fence CAS 重发布，**零新 provider 调用**；miss/corrupt 交还 pending，
worker 是唯一付费方）→ 死信只清点**绝不复活**。`converged` 判定供调用
方循环驱动；无常驻进程、零线程零定时器。

| crash 点 | 残态 | 恢复路径 |
|---|---|---|
| artifact put 中 | cache 半文件 / `*.tmp-*` 残迹 | 读侧表现为 `Miss`/`Corrupt`（meta 后写即完备性标记）→ 交还 worker 重嵌；GC 超期后清扫 temp 与半文件 |
| put 后、CAS 前 | artifact 已 durable、manifest 无行、任务 claimed（lease 过期） | reclaim 回 pending → recover 重放：cache 命中即直接发布，付费产物复用、零重复嵌入 |
| CAS 提交后 | 已原子（manifest 写与 ack 同事务，"CAS 后 ack 前"残态不可达） | 等值重复发布被 Q4 吸收：零可见变化、零双 bump |
| 换库 rename 中/后 | 旧进程仍持旧 inode 连接（读到的永远是旧 incarnation） | incarnation freshness fence：publish/claim/recover 全部在任何事务前 `Fenced` 零写入，权威库零污染 |

### cache 布局、namespace 与降级语义

- **布局**（`crates/cc-semantic/src/cache.rs`）：
  `<root>/namespace-<ns>/<space>/<input>/<spec>.bin` + `.meta.json`。
  寻址四元组 `(namespace, space, input, spec)` 即路径；payload checksum
  **不进路径**，是 meta 的独立字段，读时 `blake3(.bin)` 逐字节比对——
  namespace 与 input/spec/space/checksum 分开验证（ADR 要求）。
- **namespace 键**：`namespace_key`（cache.rs:96）=
  `blake3("cc-semantic.cache-namespace.v1", 项目身份)` 的 64 位 hex，
  跨项目默认隔离、绝对路径不落目录名、**不绑 incarnation**——索引重建
  （换库换 incarnation）解析出同一 namespace，付费向量经
  `(space, input, spec)` 全链校验后直接复用。项目身份字符串由组合根
  提供（当前工作区唯一既有来源是规范项目路径；接线轮接线）。
- **根目录解析**：`resolve_cache_root`（cache.rs:128）——环境变量
  `CODECORTEX_SEMANTIC_CACHE_ROOT`（cache.rs:72）优先（空白视为未设）
  → macOS `~/Library/Caches/codecortex/semantic` → Linux
  `$XDG_CACHE_HOME|~/.cache/codecortex/semantic`。put/get 路径不读任何
  环境变量；`open` 零文件系统副作用，首个 `put` 才惰性建目录——默认
  构建/启动不产生 cache 目录。
- **降级（缺失/损坏）**：检索侧 `Miss`/`Corrupt` 一律跳过候选、不报错、
  绝不缓存为完整结果——本地 lexical/graph 不受影响（"本地继续"）；纯
  Miss（冷缓存）**永不降级**。`Corrupt` 的隔离搬移（双半 rename 进
  `<root>/quarantine/` + 诊断 sidecar，`quarantine_object`，
  `degrade.rs:119`）由检测点显式触发；损坏可见性判据 =
  `corrupt_events > 0 || re-embed 预算耗尽` → capability status 的
  `semantic_state: "degraded"` + `degraded_reason`（
  `capability_status.rs:100`，槽已交付，组合根转写归接线轮）。补嵌经
  `BudgetedProvider`（degrade.rs:347）整批准入，拒绝发生在调用内层
  provider 之前，原因持久化进 outbox `last_error`，attempt 预算耗尽
  死信——不静默无界重费。

### GC 与空间切换

- **GC 与发布共享同步点**：对象可删需同时满足①新鲜宽限
  （meta `created_at` 距今 ≥ `DEFAULT_MIN_RETENTION_SECS`（3600s，
  `gc.rs:93`）——artifact-before-manifest 窗口的机械闭合）、②未被任何
  `semantic_manifest.artifact_ref` 引用（**全 space 含非 active**，
  checksum 不可还原时降级 `(space, input)` 保守对）、③无
  pending/claimed 任务持有该 input。删除决定基于
  `IndexDb::semantic_gc_mark`（`semantic_gc_reads.rs:161`）的**一次短读
  快照**（晚于快照的 publish 由宽限覆盖），"manifest 引用刚被 GC 删除
  产物"的竞态不可达。sweep = unlink + 空目录修剪，零 DB 写、零钟动；
  `quarantine/` 结构性不可触及。全程有界（keyset 候选分页 + 批帽），
  `run_gc_pass`（gc.rs:531）显式驱动。
- **空间切换三段**（`semantic_space_switch.rs`）：注册 `backfilling` +
  回填入队 → `switch_active_space_on`（:308）单 `IMMEDIATE` 事务五步
  （旧 active→revoked、旧空间 live 任务 supersede、目标→active、按旧
  空间 manifest 行生产 revoke 任务、追加 metadata 审计键
  `semantic_space_switch_log`）→ revoke 消费（`consume_revoke_on`：删
  **自己空间** manifest 行 + fenced ack 同事务，删行才 bump；
  `drain_space_revocations` 显式驱动）。不同空间分数永不混排由读取层
  结构性保证（claim/scan 均固定 space_id）。回滚 = 再次切换指回旧空间
  （`revoked→active` 边）+ reconcile：desired 重导 → cache 校验命中
  复用、缺失才回 worker 付费回填。

## Schema 版本策略

`user_version` pragma 记录 schema 版本（当前 **v22**，
`CURRENT_SCHEMA_VERSION` 在 `index_migrate.rs:35`）。打开库时按存储版本分派：

- **相邻加法迁移（v21 → v22）**：v22 相对 v21 的 delta 全部是
  `CREATE TABLE/INDEX ... IF NOT EXISTS`（P6 语义三表 + 7 索引），因此 v21
  库原位重放全量 schema 即完成迁移——只补缺失对象，既有行、
  `index_incarnation` 与 epoch 向量逐字节保留（`ADDITIVE_MIGRATION_FROM =
  21`，`index_migrate.rs:42`；返回 `SchemaStatus::Migrated { from }`）。
  该常量仅在 delta 纯加法时允许推进；任何非加法 bump 必须回退到
  rebuild-on-mismatch（常量 doc 注明，防误用）。迁移幂等：迁移后重开返回
  `UpToDate`，对 v22 库不执行任何 SQL。新表初始为空，语义功能未接线时
  保持空表即为默认行为（无 active space = 语义零参与）。
- **其余一切存储版本（<21、>22）→ rebuild-on-mismatch**：就地清空
  （`writable_schema` 重置）后按当前 schema 重建。源码派生事实可以重建，
  但持久资产/运行时证据仍须遵守已有导出恢复流程。测试升级或回退应使用
  隔离缓存，不以清空开发者日常索引作为验证。
- **降级语义**：旧二进制打开 v22 库 → 版本守卫报 `Mismatch` → 走该二进制
  的重建协议（即"降级 = 重建"）。派生 artifact cache 与 incarnation 无关
  （namespace 键不含 incarnation，见下文），完整校验通过即可被旧/新任意
  版本复用，不因降级重复付费；主库侧语义三表在重建后从空起点开始，
  语义状态经换库 reconcile 协议恢复（见下文[恢复步骤](#恢复步骤与-crash-点)）。

全量 staging 与增量写入 route node 复用同一 `insert_route_nodes_on` 助手，`edge_id=route_id` 均非空，避免 TEXT PRIMARY KEY 的空值或两种写法造成重复与 A/B 差异。
