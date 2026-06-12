# 存储层（cc-db）

> 范围：`crates/cc-db` —— SQLite 索引存储的连接模型、写隔离、epoch 失效协议、
> 表结构与重建协议。面向需要改动持久化层或排查缓存失效问题的开发者。
> 上层视角见 [ARCHITECTURE.md](../ARCHITECTURE.md)；并发全景见
> [CONCURRENCY.md](CONCURRENCY.md)。

所有状态存于单一数据库文件 `index.sqlite3`。没有第二个数据库、没有会话存储、
没有遥测落盘（见 [DESIGN.md](../../DESIGN.md) 的非目标清单）。

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

连接初始化 PRAGMA：`journal_mode=WAL`、`synchronous=NORMAL`、
`foreign_keys=ON`、`busy_timeout=5000`。

### 按能力切分的方法面

公开方法面按能力切成三个零成本借用视图（与 cc-server `CodeIndex` 同一模式）；
生命周期（`open` / `open_with_read_pool_size`）留在 `IndexDb` 本体上：

| 视图 | 入口 | 内容 |
|---|---|---|
| `ReadOps` | `.reads()` | 全部查询：符号/文件/图/检索读取、`generation`、`stats`、`get_metadata`、`get_file_state`、`read_conn`。多符号邻接读取有批量变体（`caller_rows_by_uids` / `callee_rows_by_uids` / `symbol_degree_details_batch`）——cc-search 的图消费方（enrichment、preselect、graph lane）全部走批量接口，一次搜索的图富化只发 3 条查询，而不是每个符号 3 条 |
| `WriteOps` | `.writes()` | 所有推进 epoch 的变更：批量写、边/证据写入、`set_metadata`、`begin_unit_of_work`。这是写方法的唯一公开路径（编译期写隔离） |
| `MaintenanceOps` | `.admin()` | 重建协议（`rebuild_with_temp_db` / `rebuild_with_direct_writer`）、`checkpoint_wal*`、`instance_id` |

### UnitOfWork：多语句写的唯一缝

`unit_of_work.rs`。`UnitOfWork` 在整个生命周期内持有写连接，运行一个
`IMMEDIATE` 事务，只暴露类型化写方法（从不交出裸连接）：

- `commit()` 时**恰好一次**推进 `index_epoch`；
- 未 commit 即 drop 时自动回滚；
- 需要新的写操作时，在 `UnitOfWork` 上加类型化方法，而不是绕过它。

调用方通过 `IndexDb::writes().begin_unit_of_work()` 获取。参考用法：
dispatch synthesis 的 apply 阶段（`cc-index/src/synthesis_pipeline.rs`）。

## Epoch 双时钟

两个持久化计数器（存于 `metadata` 表）各自独立推进，是全部下游缓存
失效的依据：

| 时钟 | 推进时机 | 失效语义 |
|---|---|---|
| `index_epoch` | 每次 `UnitOfWork` commit 恰好一次；批量写、后处理产物写回、全量重建 | 索引内容变了，索引派生缓存全部失效 |
| `evidence_epoch` | 仅运行时证据写入（`upsert_runtime_evidence`、`link_evidence_to_edge` 等）与 `boost_http_edge_confidence` | 证据持续到达，不得驱逐 index-only 缓存槽 |

`epoch_rules.rs` 声明完整的表 → 时钟映射，审计测试逐一核对每个写方法：

| 表 | 时钟 | 理由 |
|---|---|---|
| files, symbols, imports, call_edges, symbol_refs, semantic_edges, dispatch_sites, data_flow_edges, literal_index, chunks | `index_epoch` | 文件批写入的索引内容 |
| routes, http_call_edges, test_edges, co_change_edges, communities, frameworks, infra_nodes, infra_edges | `index_epoch` | 解析/后处理产物，被 context/graph 输出当作索引内容消费 |
| adr | `index_epoch` | ADR 在 context 输出中以 index_epoch 为键出现 |
| runtime_evidence | `evidence_epoch` | 持续摄入，不能驱逐 index-only 缓存槽 |

唯一例外：`boost_http_edge_confidence` 只改 `http_call_edges.confidence`
一列，推进的是 `evidence_epoch` —— 它是证据驱动的置信度提升，不是索引变更。
下游缓存槽通过 `EpochSensitivity` 声明自己对哪个时钟敏感
（[`graph_read_model/cache.rs`](../../crates/cc-server/src/graph_read_model/cache.rs)，
详见 [CONCURRENCY.md](CONCURRENCY.md#epoch-失效协议)）。

`generation()` 用单条 SELECT 同时读出两个 epoch，返回一致的
`IndexGeneration` 快照，避免读到撕裂的版本向量。

## 表结构

21 张基表（schema v5，`index_v1.sql`）：

| 组 | 表 | 内容 |
|---|---|---|
| 元数据 | `metadata` | KV：epoch、版本、各 pass 的输入签名等 |
| 文件与内容 | `files`, `chunks` | 文件元数据（路径/语言/哈希/摘要）；检索分块（zstd 压缩正文） |
| 符号与引用 | `symbols`, `symbol_refs`, `imports` | 符号定义；引用位置（含跨文件目标 UID）；导入声明 |
| 调用与关系 | `call_edges`, `semantic_edges`, `data_flow_edges`, `dispatch_sites` | 调用图（含合成边）；继承/实现等语义关系；数据流（type_ref / env_access / param_pass / return_flow）；动态派发点 |
| Web 与服务 | `routes`, `http_call_edges`, `infra_nodes`, `infra_edges` | HTTP 路由；出站 HTTP/异步调用；基础设施节点与连边 |
| 分析产物 | `communities`, `frameworks`, `co_change_edges`, `test_edges` | Louvain 社区；框架检测（repo 级 + file 级）；git 共变；测试关联 |
| 其他 | `literal_index`, `runtime_evidence`, `adr` | 字面量索引；OTLP 运行时证据；架构决策记录 |

`metadata` 表中的固定键：`index_epoch`、`evidence_epoch`、
`last_indexed_at`、`index_version`，以及各后处理 pass 的输入签名键
（如 config-linker 的 `last_config_sig`，见
[INDEXING.md](INDEXING.md#写入阶段)）。

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

## Schema 版本策略

`user_version` pragma 记录 schema 版本（当前 v5，
`CURRENT_SCHEMA_VERSION` 在 `index_migrate.rs`）。磁盘索引版本不匹配时的
策略是 **rebuild-on-mismatch**：就地清空（`writable_schema` 重置）后按当前
schema 重建，不做向后迁移。索引是缓存而非数据源，重建总是安全的。
