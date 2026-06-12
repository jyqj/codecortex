# 并发与一致性模型

> 范围：跨 crate 的锁、缓存失效与生命周期协议——谁持有什么锁、以什么顺序、
> 读者在什么窗口里看到什么。面向需要改动构建/查询并发行为，或排查死锁、
> 过期读、缓存不失效问题的开发者。管线视角见 [INDEXING.md](INDEXING.md)；
> epoch 的持久化语义见 [STORAGE.md](STORAGE.md#epoch-双时钟)。

## 锁清单

| 锁 | 位置 | 保护对象 |
|---|---|---|
| `Arc<RwLock<CodeIndex>>` | `handlers/mod.rs` | 每项目的引擎状态（`index_db`、`SearchEngine`、项目路径）；读工具拿读锁，构建/换项目拿写锁 |
| per-project build gate：`Arc<Mutex<()>>` | `engine.rs`（`CodeIndex::empty()` 创建） | 串行化该项目的所有 prepare+commit 构建对：MCP `index()`、watcher tick、连接时自动索引 |
| `IndexDb` 写连接 `Mutex` | cc-db | 单写者；多语句事务只经 `UnitOfWork` |
| `ProjectSession`：`tokio::sync::Mutex<LruCache>` | `project_session.rs` | 项目路径 → `CodeIndex` 实例的 16 槽 LRU |
| GraphReadModel 进程级缓存：`OnceLock` + `Mutex<LruCache>` | `graph_read_model/cache.rs` | 跨项目共享的图读缓存槽（默认 16 槽，`CODECORTEX_GRAPH_CACHE_SIZE`） |

## 锁序规则

build gate 与 `CodeIndex` RwLock 之间的顺序是硬规则（`engine.rs` 注释即
契约）：

1. **gate 先于 RwLock**：构建流程先拿 gate，再做任何 RwLock 获取；
2. **持 RwLock 时禁止阻塞等 gate**（会死锁）；`&mut self` 构建入口只
   `try_lock`，失败作为 "busy" 错误浮出，不等待。

gate 在 `CodeIndex::empty()` 创建一次，**close()/reopen() 不touch 它**——
所以序列化能力在空闲驱逐后存活。

gate 生效的前提是每个项目全进程只有一个 `CodeIndex` 实例：
`ProjectSession` 以归一化项目路径（`fs::canonicalize`，失败回退原路径）
为键缓存实例，`index_for_project_path` 与 `set_active_project` 都复用缓存。

### 毒锁恢复

`handlers/mod.rs`：`lock_index` / `lock_index_write` 在 RwLock 中毒时恢复
而不是传播 panic；写锁恢复路径额外调用
`invalidate_search_cache_after_poison()`，宁可丢缓存也不让可能写了一半的
状态被缓存放大。全局 `POISON_RECOVERED` 原子标志记录发生过恢复。

## 构建路径：三段提交的锁窗口

`run_split_build`（`handlers/core.rs`）在**全程持有 build gate** 的前提下
执行三个阶段（机制细节见
[INDEXING.md](INDEXING.md#三段式提交staged-commit)）：

| 阶段 | 锁 | 工作 |
|---|---|---|
| 1 `commit_write` | 写锁（短） | generation guard + 索引内容写入 |
| 2 `compute_postprocess` | **无锁** | postprocess/analysis 计算，经读池读阶段 1 的快照 |
| 3 `apply_postprocess` | 写锁（短） | 小事务 apply 类型化 delta |

- prepare（scan→parse→resolve→压缩）本就在锁外。
- `StalePreparedBuild`（阶段 1 或 3 发现 `index_epoch` 不匹配）触发整体
  重跑 prepare+commit，**最多一次**。同进程内不会发生（gate 串行化了
  构建）；这是给跨进程写者的防线。
- **最终一致窗口**：阶段 1 之后、阶段 3 之前，读者看得到新索引内容但
  后处理产物（合成边、社区、test edges、共变/infra/ADR）还是旧的。每次
  阶段 3 apply 推进 `index_epoch`，epoch 键控缓存随之收敛。这是 ADR-0002
  里明确接受的取舍——换来的是构建期间读工具不再被 300–800ms 的锁内计算
  阻塞。

## FileWatcher acquire-before-drain

`watcher.rs` + `project_session.rs::run_watcher_tick`。`notify` 驱动的
watcher 带自适应去抖、突发退避、gitignore 过滤和 git 脏态轮询兜底。
由 `.codecortex.json` 的 `auto_index.enabled`（默认 `true`）控制，连接时
随项目发现启动，`index()` 切换项目路径时重启。

关键参数（`watcher.rs` 常量）：

| 参数 | 值 |
|---|---|
| 去抖基础 / 增量 / 上限 | 500ms / +100ms 每 500 文件 / 3000ms |
| 突发阈值 / 窗口 / 突发期去抖上限 | 1 秒内 20 事件 / 去抖翻倍 / 5000ms |
| git 脏态轮询 | 每 30 秒 `git status --porcelain`，回填不在 pending 里的遗漏变更 |

**轮询循环按 acquire-before-drain 实现无损**：一个 tick 先抢构建席位——
先 CAS `auto_indexing` 标志，再 `try_lock` build gate——两者都成功才调用
`drain_pending` 消费事件批。gate 忙则事件留在队列里（`has_pending` 只
窥视不消费），下一个 tick 再索引，**不会丢**。

## 会话生命周期

- **空闲驱逐**：每 30 秒检查一次，MCP 无活动超过
  `auto_index.idle_timeout_secs`（默认 60）即对活动项目调用 `close()`——
  释放 DB 句柄、清空 `index_db`/`engine`，保留 `project_path` 与 build
  gate。下一次调用经 `reopen_active_index_if_closed()` 透明重开（读锁
  探测 `is_closed`，需要时升级写锁走 `set_project` 重初始化）。
- **PPID watchdog**（Unix）：每 `CODECORTEX_PPID_POLL_MS`（默认 5000，
  0 关闭）轮询父进程；PPID 变化或变为 1 即触发优雅关停——MCP 客户端死掉
  时服务器不会变成孤儿进程。

## Epoch 失效协议

读路径缓存不靠手工失效，靠版本向量比对：

- cc-db 持久化两个时钟 `index_epoch` / `evidence_epoch`
  （[STORAGE.md](STORAGE.md#epoch-双时钟)），`generation()` 单条 SELECT
  读出一致快照。
- GraphReadModel 的进程级缓存以 `GraphReadGeneration` 为键：
  `(db_identity, index_epoch, evidence_epoch)`，其中 `db_identity` 是
  `IndexDb` 实例的进程内唯一 ID——同一路径重开数据库也会得到新身份，
  防止跨实例串缓存。
- 每个缓存槽声明自己的 `EpochSensitivity`：

  | 档位 | 语义 | 典型槽 |
  |---|---|---|
  | `IndexOnly` | 证据时钟归零参与键比对——evidence-only 推进**不**驱逐 | 语义边投影、导入邻接、社区、死代码 |
  | `IndexAndEvidence` | 任一时钟推进即驱逐 | HTTP/异步桥接边、含桥接的邻接 |

  失效策略因此是槽声明的一部分，而不是调用点纪律。
- cc-search 的结果缓存同理（键里直接含 epoch，见
  [SEARCH.md](SEARCH.md#缓存)）；降级结果（`read_errors` 非空）不缓存。
- 缓存留在 cc-server 而不下沉（cc-db 只拥有持久化 epoch 向量与类型化
  查询）是 [ADR-0001](../adr/0001-cypher-traversal-lazy-bfs-fast-path.md)
  的明确决定。

## 读路径的失败语义

图读路径上的 DB 读失败**不再**静默降级为空结果（旧行为是
`unwrap_or_default`）：失败被记录到 `GraphExplainCollector`，作为
`read_errors`（上限 8 条，溢出计入 `read_errors_dropped`）出现在工具响应
的 `graph_explain` 信封里。调用方能区分"真没有"和"没读出来"。
