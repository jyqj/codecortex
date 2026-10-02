# P6-013 实施记录：worker 资源与连续编辑合并

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-013 节（合并/admission/有界关闭/任何时刻 local 查询可用）、P6-007 节
  （`crates/cc-semantic/src/queue.rs` LeaseGuard 草案）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（P6-013 行约束；
  否决方向"不引入独立队列服务、常驻进程或网络端点"）、02-CONTRACTS C11
  （"网络调用前释放 RwLock…；关闭时有界等待并留下可恢复 outbox"）、批次 1/2/3
  交付（P6-005 唯一索引 + P6-006 supersede-then-insert、P6-007 偏差 5
  "queue.rs 归 P6-013" + 偏差 8 "IndexDb 写门面随 P6-013 首个生产调用方落"、
  P6-011 publish CAS 与 Publisher、P6-009 FakeProvider）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新测试文件；
  `crates/cc-semantic` 1 个新模块 + lib.rs 注册/边界文档更新 + 1 个新测试文件。
  **schema v22 零变更**；P6-005/006/007/008/009/010/011 交付物**只调用未改动**
  （其文件零触碰）；`tasks.json` status 未改；未 git commit；组合根接线未做
  （cc-server 零改动）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_queue.rs` | 新增（138 行） | P6-007 偏差 8 兑现：IndexDb 写门面。`queue_txn`(:32，IMMEDIATE 短事务包装) + `claim_semantic`(:62，解析 active space；未配置/空队返回 None) + `renew_semantic_lease`(:82) + `retry_semantic_task`(:105) + `reclaim_expired_semantic`(:132)。每个都是独立 IMMEDIATE 短事务，Auxiliary 零 bump；无 ack 门面（ack 属 publish CAS 事务，见 §5 偏差 4） |
| `crates/cc-db/src/lib.rs:51` | 修改 | `pub mod semantic_queue;`（一行注册） |
| `crates/cc-db/tests/semantic_queue.rs` | 新增（207 行） | 2 个门面集成测试（§4） |
| `crates/cc-semantic/src/queue.rs` | 新增（404 行） | worker 原语全量（§2）：`WorkerLimits`(:76，validated :95，renew_period :134) + `LeaseGuard`(:152，`claim` :163，`renew` :183) + `TaskExit`(:191) + `BatchReport`(:205) + `drain_pending`(:243，liveness 门 :269) + `EmbedHandler`(:335，`handle` :355) |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod queue;`(:43) + 模块边界文档补 P6-013 段（显式 drain、无常驻进程） |
| `crates/cc-semantic/tests/queue_worker.rs` | 新增（646 行） | 8 个端到端集成测试（§4） |

## 2. LeaseGuard / claim 循环 API（定案签名）

```rust
// crates/cc-semantic/src/queue.rs
pub struct WorkerLimits {                       // :76 admission/资源预算
    pub max_batch: usize,      // 单次 drain 的 claim 上限（有界执行器容量）
    pub lease_secs: f64,       // 对接 P6-007 lease 到期
    pub backoff_secs: f64,     // fenced retry 回退
    pub max_attempts: u32,     // 预算耗尽 → 终态 failed
}
pub fn validated(max_batch, lease_secs, backoff_secs, max_attempts) -> CcResult<Self>; // :95
pub fn renew_period(&self) -> f64;              // :134 = lease_secs/3（简报周期）

pub struct LeaseGuard<'a> { /* db: &'a IndexDb, task: ClaimedTask, lease_secs: f64 */ } // :152
impl LeaseGuard<'_> {
    pub fn claim(db, owner, lease_secs) -> CcResult<Option<Self>>;  // :163 受许构造器
    pub fn task(&self) -> &ClaimedTask;                             // :171
    pub fn renew(&self) -> CcResult<bool>;   // :183 心跳 + liveness 门（token fencing）
    // Drop 策略：**显式 no-op**——不 ack、不 retry、零 DB I/O（见 §3）
}

pub enum TaskExit { Disposed, NeedsRetry { reason: String } }        // :191
pub struct BatchReport { claimed, completed, retried, lease_lost }   // :205

pub fn drain_pending(db: &IndexDb, owner: &str, limits: &WorkerLimits,
    handler: &mut dyn FnMut(&LeaseGuard<'_>) -> CcResult<TaskExit>)
    -> CcResult<BatchReport>;                    // :243 显式单批 drain

pub struct EmbedHandler<'a> { /* publisher, provider, resolve_input 注入 */ } // :335
impl EmbedHandler<'_> {
    pub fn handle(&self, guard: &LeaseGuard<'_>) -> CcResult<TaskExit>; // :355
}

// crates/cc-db/src/semantic_queue.rs（写门面，每个 = 独立 IMMEDIATE 短事务）
impl IndexDb {
    pub fn claim_semantic(&self, owner, lease_secs) -> CcResult<Option<ClaimedTask>>;  // :62
    pub fn renew_semantic_lease(&self, task_id, token, lease_secs) -> CcResult<bool>;  // :82
    pub fn retry_semantic_task(&self, task_id, token, err, backoff_secs, max_attempts)
        -> CcResult<bool>;                                                             // :105
    pub fn reclaim_expired_semantic(&self) -> CcResult<usize>;                          // :132
}
```

`drain_pending` 单任务编排水管：reclaim 过期 lease（不让崩溃 worker 的残任务卡死）
→ `claim_semantic` → **renew（工作前心跳 = liveness 门，:269）** → `handler` →
`Disposed`（完成）／`NeedsRetry` 或 `Err`（fenced retry，backoff + 预算耗尽死信；
对丢失 lease 的 retry 天然 no-op，只计 `lease_lost`）。全循环 Auxiliary，唯一可能
bump `semantic_epoch` 的是 handler 内部的 publish CAS（P6-011 既有语义）。

## 3. 合并语义定案（`queue.rs:24` 模块文档原文）

1. **DB 层（P6-005/006 既有交付，本模块只依赖）**：部分唯一索引
   `semantic_outbox_live_per_doc` 保证同 `(doc_key, space_id)` 至多一个活跃
   （pending/claimed）任务，且每次源码事务写先 supersede 该 doc 的活跃任务再插入
   最新 `doc_version` 的新 pending 任务——同一文档三连编辑在 worker 看到之前已合并
   为单个活跃任务，worker 对最终版本只做**一次** provider 做功，绝非每次编辑一次。
2. **claimed 任务被 supersede**：写落在 claimed 任务上时将其翻为终态 `superseded`。
   在途 worker 的 publish 被 P6-011 CAS fence 拒绝（该行不再在所呈 token 下处于
   claimed → `LeaseLost`），CAS 内的 fenced retry 对 superseded 行天然 no-op——旧
   attempt 永不能复活或 ack。浪费上限 = 一次已开始的 embed；新版本任务服务下一次
   claim。
3. **renew-before-work 即 liveness 门**：每次 drain 在调 handler 前先续 lease；
   `Ok(false)` = 任务已被他人回收/supersede/完成——该 attempt 跳过，零 provider
   调用、零写。
4. **不做时间窗 debounce**：写路径绝不因合并目的被阻塞或延迟（简报：上限由 worker
   消费速率兜底 + `available_at` backoff）；队列深度由消费速度约束有界，
   `max_batch` 约束单次 drain 的 claim。

**Drop 处置策略**（`LeaseGuard` 模块文档 `queue.rs:152` 起，明文）：丢弃未处置的
guard **不 ack、不 retry、零数据库 I/O**。任务保持 claimed 直到 lease 过期、由
`reclaim_expired_semantic` 归还 pending（不消耗 attempt 预算）。这是 C11"关闭时
有界等待并留下可恢复 outbox"的结构性兑现：panic/取消的 worker 把任务留给 lease
恢复路径，既不烧 attempt 预算也不以静默 retry 掩盖失败，且析构函数不可能吞掉
数据库错误。

**无隐式常驻进程（ADR 红线）**：本模块零线程/定时器/守护进程；`drain_pending` 是
显式、调用方驱动的单批 drain，是否再 drain、何时、从何处完全由组合根决定。

## 4. 测试清单（10 个新增，全绿）

**`crates/cc-semantic/tests/queue_worker.rs`（8 个）**

1. `drop_of_undisposed_lease_leaves_the_claim_recoverable`(:251)——drop 后任务仍
   claimed、token 不变、attempt=1、无 last_error（零写入）；过期回收回 pending 后
   下一次 drain 收敛为发布，provider 恰 1 次。
2. `drain_publishes_end_to_end_through_the_fake_provider`(:308)——FakeProvider →
   cache.put → 读回校验 → 五 fence CAS → manifest 两行 v1、epoch `None→Some(2)`
   （恰两次可见变化）、index/evidence/incarnation 全静；真实读路径 exact 检索
   d1 自匹配居首（score≈1.0，f32 余弦往返容差 1e-9）。
3. `three_rapid_edits_coalesce_into_one_task_and_one_embed`(:388)——v1/v2/v3 三连
   写后活跃任务恰 1 条且为 v3；单次 drain `claimed=1, completed=1`、
   **provider.call_count()==1**、manifest 只落 v3（简报口径"三连编辑最终只一发
   embed"的实测证据）。
4. `superseded_mid_flight_claim_never_publishes_the_old_version`(:429)——resolver
   在 v1 任务在途（已过 renew 门）时执行 v2 写（supersede claimed 任务）；同一次
   drain 收敛：v1 旧 attempt 被 CAS fence（superseded 行 + fenced retry no-op）、
   superseded 计数恰 1、manifest 只见 v2、epoch 恰 1（v1 从未可见）、provider 2 次
   （= 浪费上限 1 + 服务 1）；队列清空。
5. `expired_and_reclaimed_lease_cannot_renew_but_the_successor_publishes`(:500)——
   0.05s lease 过期 + 第三方回收后，旧 guard `renew()==false`（过期 worker 失声）；
   继任 drain 发布成功、provider 恰 1 次（无重复 embed）。
6. `one_drain_claims_at_most_max_batch`(:535)——3 任务 `max_batch=2`：首 drain 恰
   claim 2（余 1 pending，消费有界）；次 drain 收尾；epoch `Some(2)→Some(3)`。
7. `retry_budget_exhaustion_dead_letters_without_further_claims`(:571)——不可解析
   输入两次耗尽预算 → 终态 failed、死信不再可 claim（第三次 drain `claimed=0`）、
   provider.call_count()==0（坏输入永不进 provider）。
8. `worker_limits_reject_structurally_meaningless_bounds`(:629)——0 max_batch /
   非正 lease / 负 backoff / 0 max_attempts 全拒；`renew_period()==lease_secs/3`。

**`crates/cc-db/tests/semantic_queue.rs`（2 个）**

1. `facade_claim_renew_and_retry_keep_the_fenced_semantics`(:106)——未配置语义 =
   干净 None（默认路径零付费）；空队/未来任务 = None；claim（token/owner/attempt+1）、
   持有者 renew 真、伪造 token renew 假零写、retry 回 pending + backoff + last_error
   持久化、旧 token 再 retry 假；全程三钟全静（`ReadGeneration` 整体相等断言）。
2. `facade_reclaim_returns_only_expired_leases_to_pending`(:180)——未过期 reclaim 0；
   过期 reclaim 1（不耗 attempt）；继任 claim 得新 token。

## 5. 偏差清单（与简报草案 / tasks.json scope）

1. **模块落 `queue.rs` 而非 `worker.rs`+`admission.rs`**：tasks.json P6-013 scope
   与简报 P6-013 节写 `worker.rs`/`admission.rs`，但任务指令与 P6-007 偏差 5 明文
   指定 `crates/cc-semantic/src/queue.rs`（LeaseGuard 兑现）；admission 预算
   （`WorkerLimits`：max_batch/lease/backoff/attempts）并入同文件——简报明文
   "写路径只 supersede+insert，上限由 worker 消费速率兜底"，即无写路径闸门，
   独立 `admission.rs` 只会是个空壳。
2. **`LeaseGuard` 持 `&IndexDb` 借用而非草案的 `Arc<IndexDb>`**：无线程/无
   自驱循环（ADR 红线）下借用已覆盖全部用法，避免为 RAII 引入引用计数；附
   `LeaseGuard::claim` 受许构造器使 drain 内外的 claim 配对同源。
3. **公平化 `ORDER BY` 未做可注入参数**：claim SQL 的 `task_id` FIFO（P6-007
   既有交付，红线不可改）即当前公平口径；简报明文"公平化作为 ORDER BY 可注入
   参数留评审"——该参数属 claim SQL 本体改动，归后续轮与 P6-015 扫描一并评审。
4. **写门面无 ack 方法**：ack 是 publish CAS 事务的组成部分（P6-011：fence 2 +
   fenced ack 原子）；独立 ack 门面会让 worker 绕过 artifact 校验推进任务。
   revoke 任务（P6-017 空间撤销）的直 ack 消费面由 P6-017 作为第二生产调用方
   落。`EmbedHandler` 对 `op=Revoke` 显式返回 NeedsRetry（不静默 ack）。
5. **`EmbedHandler` 的输入字节解析注入而非内建**：`resolve_input` 闭包由组合根
   提供，queue.rs 不耦合 `record_json` schema（与 P6-011 CAS 的
   `RecordInputPeek` 同一解耦原则）；真实解析器（读 `document_manifest
   .record_json` 的 `input.text`）随组合根接线轮落。
6. **provider 批大小 = 每 attempt 单输入**：`max_batch` 是 claim/admission 上限
   （简报口径），非 provider 输入批上限；跨任务输入合批（同 input_digest 复用）
   是 cache 层已有能力，未在 worker 层再造。
7. **`drain_pending` 每次迭代先 `reclaim_expired_semantic`**：简报将周期扫描归
   P6-015；本处为**机会性** reclaim（单条 Auxiliary UPDATE，无界扫描语义同
   P6-007 交付），让崩溃 worker 的残任务不必等下一个 P6-015 周期。P6-015 的
   有界化若给 reclaim 加 limit 参数，此调用点随之收窄。
8. **测试 resolver 经 `DocumentInput::from_bytes` 构造**（digest 表驱动）：
   `InputDigest` 构造面在 P6-003 已密封（`new` 为 crate 内部），测试按密封契约
   从真实输入字节出发展示。

## 6. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-db --locked --offline
→ 22 个 "test result: ok."，0 failed。关键计数：
  cc-db lib 170 passed (+1 ignored)；semantic_queue 2（新增门面套件）；
  semantic_lease 9、semantic_outbox 17、semantic_publish 11、semantic_schema 6、
  semantic_coverage 3 等既有套件零回归；
  cc-semantic lib 54；queue_worker 8（新增端到端套件）；
  publish_cas 4、artifact_cache 17、manifest_exact_integration 4 全绿。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.09s（零 error）

cargo clippy -p cc-db --locked --offline            → 零 warning（cc-db）
cargo clippy -p cc-semantic --locked --offline      → 3 个 warning，全部位于
  crates/cc-semantic/src/cache.rs:102/:126/:127（P6-008 交付文件，与 P6-011
  记录在案的同一批既有项），本轮红线未触碰
rustfmt --check（本轮新增 4 文件）                    → 干净
```

## 7. 未做与剩余风险

- **组合根接线仍不做**（红线）：`drain_pending`/`EmbedHandler` 无任何 cc-server
  调用点；真实 input-bytes 解析器、drain 触发时机（写后/查询前/后台）归接线轮。
- **公平批次（ORDER BY 注入）与全局 pending 水位观测**未实现（偏差 3）；当前
  队列深度有界性依赖消费速率 + backoff，无显式水位告警——capability 侧
  `SemanticCoverage`（P6-012）已透出 pending/failed 计数，可作观测面。
- **在途 supersede 的浪费上限 = 一次已开始 embed**（§3.2 定案）：renew 门只能
  消掉"尚未开始做功"的窗口；provider 调用开始后的 supersede 由 publish CAS
  fence 兜底，不再重试省钱。
- **`reclaim_expired_on` 仍无行数上限**（P6-007 既有边界）：本任务的机会性调用
  未改变其量级风险（表量级 = 活跃任务量）；有界化归 P6-015。
- V14/V20 正式验证矩阵证据（`artifacts/benchmarks/<run-id>/`）不推断，归验收轮；
  本记录 §4 的测试证据为工程级。50k 大量保存的压测口径（V20）归验证轮。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
