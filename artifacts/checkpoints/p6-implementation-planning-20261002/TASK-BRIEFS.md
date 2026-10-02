# P6-002~020 逐任务设计简报

> 每节含：目标与约束（ADR 锚点）、模块归属、接口/DDL 草案、验收对照、风险。
> 所有签名均为**草案**，实施时需按现有代码风格评审（不猜 API：现有代码引用均带文件:行号）。
> 通用约定：错误走 `cc_model::CcError`（`crates/cc-db/src/unit_of_work.rs:38` 的用法）；
> SQL 全部留在 cc-db 内（`unit_of_work.rs` 模块注释"never the raw rusqlite::Connection"）。

---

## P6-002 新增可选 cc-semantic 骨架

**ADR 约束**（约束表第 138 行）：只依赖 `cc-model`/`cc-db`；feature + 组合根延迟初始化；
默认编译/启动不生成空 cache 目录。

**模块归属**：新 workspace member `crates/cc-semantic`；根 `Cargo.toml` members
（Cargo.toml:3-11）追加；`crates/cc-server/Cargo.toml` 增加

```toml
[features]
semantic = ["dep:cc-semantic"]
[dependencies]
cc-semantic = { path = "../cc-semantic", optional = true }
```

**接口草案**：

```rust
// crates/cc-semantic/src/lib.rs
pub mod spec;      // P6-003 落位
pub mod queue;     // P6-007 落位
pub mod cache;     // P6-008 落位
pub mod providers; // P6-009 落位（mod providers { pub mod fake; }）
pub mod vector;    // P6-010 落位（mod vector { pub mod exact; }）

/// 组合根延迟初始化入口：feature 开启且配置存在时才装配。
pub struct SemanticHandle { /* 后续批次填充 */ }
pub fn try_init(config: &cc_model::config::ProjectConfig) -> Option<SemanticHandle>;
```

组合根挂接点已存在：`crates/cc-server/src/service_factory.rs:24` 的
`semantic: RwLock<Option<Arc<dyn SemanticRecall>>>` 与 `set_semantic`
（service_factory.rs:58）、`crates/cc-server/src/engine.rs:310`
`set_semantic_recall`。本任务**只落骨架**，不实现 recall。

**验收对照**：默认编译不拉网络模型实现（cc-semantic 不在默认依赖图，V21 默认/semantic
两包）；不生成空缓存目录（`try_init` 与任何 cache 路径在 P6-008 前不存在；lib.rs 不含
任何 mkdir/文件系统副作用）；`capability_status.rs:27` 现有
`semantic_state: "port_attached_unverified"|"not_configured"` 行为不变（V18）。

**风险**：workspace member 一旦加入，`cargo test --workspace` 即编译 cc-semantic——
"默认编译不含语义"的正确表述是**默认 feature/默认包**不含，测试工作区全量编译可接受
（与 V21"默认/semantic 两包"口径对齐，见 `OPEN-QUESTIONS.md` Q3 的验证命令裁决）。

---

## P6-003 冻结编码空间与输入规范

**ADR 约束**（第 139 行）：`VectorSpace/DocumentEncoding/QueryEncoding` 三分与完整
digest 是 cache namespace 验证的前提；同维度不同模型不可混用。

**模块归属**：`crates/cc-semantic/src/spec.rs`（主体）+ `crates/cc-model/src/identity.rs`
（digest 原语复用：`bytes_hash`/`hash`，identity.rs:7-14）。

**接口草案**：

```rust
// crates/cc-semantic/src/spec.rs
/// 冻结的向量空间身份：model_id 进 digest，维度不构成身份。
pub struct VectorSpace {
    pub model_id: String,        // provider+model 全名
    pub dimension: u32,
    pub distance: DistanceMetric, // 先只支持 Cosine
    pub spec_version: u32,
}
pub struct DocumentEncodingSpec { pub space: VectorSpace, pub instruction: Option<String>,
                                  pub max_tokens: u32, pub tokenizer: String }
pub struct QueryEncodingSpec   { pub space: VectorSpace, pub instruction: Option<String>,
                                  pub max_tokens: u32, pub tokenizer: String }

impl VectorSpace    { pub fn digest(&self) -> SpaceDigest; }   // blake3(canonical json)
impl DocumentEncodingSpec { pub fn digest(&self) -> DocSpecDigest; }
impl QueryEncodingSpec    { pub fn digest(&self) -> QuerySpecDigest; }
```

三分语义：query-only 变化（instruction/参数）只改 `QuerySpecDigest`，不触碰
`DocSpecDigest` → 文档不需重嵌（验收 2）；模型/维度/距离变化改 `SpaceDigest` →
整个空间切换（P6-017 的前提），同维度不同 model_id 必然不同 `SpaceDigest`
（验收 1，"同维度不同模型不能混用"）。

与现有 identity 的衔接：`DocumentRef.encoding_key` 目前是
`hash(("document-encoding-v1", encoding_spec, input_hash))`（identity.rs:103-108）。
P6-003 不改该公式（避免 v21 文档身份漂移），`SpaceDigest`/`DocSpecDigest` 作为**新增
独立维度**进入 outbox/manifest/cache key；是否把 space_id 并入 encoding_key 推迟到
P6-005 DDL 评审（见 `OPEN-QUESTIONS.md` Q4）。

**验收对照**：V10（文档身份与投影：模板/spec 变化语义不变）、V16（不同空间拒混的
第一层：digest 比较单测）。

**风险**：spec 序列化必须 canonical（固定字段序、无 HashMap——照 C03
`02-CONTRACTS.md:21`"序列化固定排序、长度分隔和版本"），用 struct 序列化而非 map。

---

## P6-004 扩展类型化 write effects

**ADR 约束**（第 140 行）：`Index/Evidence/Semantic/Auxiliary` 封闭枚举即"epoch 边界"
的机制化：Auxiliary 永不刷 `index_epoch`/检索缓存；`Index`/`Evidence` 保持"commit 必
bump"现行规则；`Semantic` 按语义 epoch 规则推进。

**模块归属**：`crates/cc-db/src/epoch_rules.rs`（枚举与审计）、
`crates/cc-db/src/unit_of_work.rs`（commit 类型化）。

**现状锚点**：

- 现有双钟：`EpochClock { Index, Evidence }`（epoch_rules.rs:21-29）；表→钟声明
  `EPOCH_RULES`（epoch_rules.rs:40-166）+ 审计测试
  `declared_clock_matches_observed_bump_for_every_table`（epoch_rules.rs:240-344）。
- `UnitOfWork::commit()` 硬编码 `IndexDb::bump_index_epoch_on(&self.conn)`
  （unit_of_work.rs:66-73）。
- `semantic_epoch` 读取侧已就绪：`read_generation.rs:52` 已 SELECT
  `'semantic_epoch'`，缺失→`None`（read_generation.rs:70,80）；
  `cc_model::generation::ReadGeneration.semantic_epoch: Option<u64>`
  （cc-model/src/generation.rs:12）。**尚无任何写入侧 bump semantic_epoch**。

**接口草案**：

```rust
// epoch_rules.rs
/// 封闭的写效应类别。扩展 ADR-0003 的 epoch 边界机制化。
pub enum WriteEffect {
    Index,      // 现行：commit 必 bump index_epoch
    Evidence,   // 现行：commit 必 bump evidence_epoch
    Semantic,   // 仅可见集合变化：bump semantic_epoch（None→1 起）
    Auxiliary,  // heartbeat/renew/retry/claim：不推进任何 epoch
}
/// 一笔提交可携带的效应集合（P6-006 源码事务 = Index+Semantic 组合）。
pub struct EffectSet(u8); // bit0..3 对应四类；From<WriteEffect>；合并 Or
```

```rust
// unit_of_work.rs
impl UnitOfWork<'_> {
    /// 现有 commit() 语义完全保留 = commit_with(EffectSet::of(Index))。
    pub fn commit(self) -> CcResult<()> { self.commit_with(EffectSet::index()) }
    pub fn commit_with(self, effects: EffectSet) -> CcResult<()> {
        // effects.contains(Index)   → bump_index_epoch_on（现有 fn）
        // effects.contains(Evidence)→ bump_evidence_epoch_on
        // effects.contains(Semantic)→ bump_semantic_epoch_on（本任务新增）
        // Auxiliary → 无操作；空 EffectSet 只对纯 Auxiliary UoW 合法
    }
}
```

claim/renew/heartbeat 等短事务不需要完整 `UnitOfWork`，直接用
`IMMEDIATE` 短事务 + `bump_*_on` 跳过（模式照 unit_of_work.rs:55-63）；是否引入
轻量 `AuxTx` 封装由实施评审定，最小改动是 cc-db 内部私有 helper。

**验收对照**（V13）：heartbeat 不刷 index——新增审计测试：Auxiliary commit 后
`read_generation()` 三元组（index/evidence/semantic）逐项不变；commit/rollback 恰好
推进预期 epoch——扩展 `assert_bumps`（epoch_rules.rs:208-234）支持四效应；rollback
不推进（现有 `drop_without_commit_rolls_back...` 模式，unit_of_work.rs:191-214）。
默认旧行为保留：现有全部测试不改断言即绿。

**风险**：`bump_semantic_epoch_on` 首次写会把 `semantic_epoch` 从"键缺失(None)"变为
"键存在(≥1)"——下游任何把 None 当 0 的读路径都是 bug（REQUIREMENTS hard_risks 末条
"None semantic epoch not ready0"）；在 cc-db 内加守卫测试：strict 读路径下
`None != Some(0)` 语义由类型保证，但 cc-search/cc-server 任何 `unwrap_or(0)` 都要在
本任务 grep 排查。

---

## P6-005 新表与 schema 初始化（深化）

**ADR 约束**（第 141 行）：document/manifest/outbox 表与索引全部进主库并**按发布节点
合并 schema 版本**；新旧 DB 有明确重建路径，FTS 旧数据不半升级。

**模块归属**：`crates/cc-db/src/sql/index_v1.sql`（FULL_SCHEMA_SQL，
index_migrate.rs:31）+ `crates/cc-db/src/index_migrate.rs`
（`CURRENT_SCHEMA_VERSION` 21→22，index_migrate.rs:29）。
写/读方法落 `crates/cc-db/src/semantic_outbox.rs`（006/007 填充）与既有
`document_store.rs` 旁的新 `semantic_manifest.rs`（或并入同一模块，实施评审定）。

**现状锚点**：`document_manifest` 已存在（index_v1.sql:16-26，v21 引入，含
`doc_key/doc_version/file_path/chunk_id/encoding_key/reference_json/record_json`）；
重建策略是 rebuild-on-mismatch（index_migrate.rs:39-70：version 不符→`Mismatch`→
调用方重建）；换库协议 `run_rebuild_protocol`（index_db_rebuild.rs:278-286）已在
swap 时 finalize `max(floor, live)+1` 的 epoch 向量并 `renew` incarnation
（read_generation.rs:42-48）。

**DDL 草案**（追加进 index_v1.sql，一次 v22 节点合并提交）：

```sql
-- ── P6-005: 语义持久化（权威单库内；派生向量本体在 cc-semantic cache，不在本库）──

-- 可见集合（权威）：当前已发布映射；一行 = 一个 doc_key 在一个空间的当前发布。
CREATE TABLE IF NOT EXISTS semantic_manifest (
    doc_key        TEXT PRIMARY KEY REFERENCES document_manifest(doc_key) ON DELETE CASCADE,
    doc_version    TEXT NOT NULL,   -- 发布 CAS 校验的 doc version（fencing 之一）
    file_path      TEXT NOT NULL,   -- 冗余自 document_manifest：filtered exact 先过滤热路径
    encoding_key   TEXT NOT NULL,   -- = document_manifest.encoding_key
    input_digest   TEXT NOT NULL,   -- 实际嵌入输入 digest（fencing 之一；P6-003 定义）
    space_id       TEXT NOT NULL,   -- VectorSpace::digest()（P6-003）
    artifact_ref   TEXT NOT NULL,   -- cache 内容寻址引用（P6-008）
    published_at   TEXT NOT NULL,
    published_incarnation TEXT NOT NULL  -- 发布时 ReadGeneration.incarnation hex
);
CREATE INDEX IF NOT EXISTS semantic_manifest_space   ON semantic_manifest(space_id);
CREATE INDEX IF NOT EXISTS semantic_manifest_file    ON semantic_manifest(file_path);
CREATE INDEX IF NOT EXISTS semantic_manifest_artifact ON semantic_manifest(artifact_ref); -- GC mark 用

-- desired 任务（权威队列可靠性数据；Auxiliary 时钟管辖，不属检索内容）
CREATE TABLE IF NOT EXISTS semantic_outbox (
    task_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_key        TEXT NOT NULL,
    doc_version    TEXT NOT NULL,
    input_digest   TEXT NOT NULL,
    space_id       TEXT NOT NULL,
    op             TEXT NOT NULL CHECK(op IN ('embed','revoke')),
    state          TEXT NOT NULL CHECK(state IN ('pending','claimed','done','failed','superseded')),
    attempt_count  INTEGER NOT NULL DEFAULT 0,
    lease_token    TEXT,            -- 每 attempt 独立 token（lower(hex(randomblob(16)))）
    lease_expires_at REAL,          -- unix seconds
    claim_owner    TEXT,            -- 进程标识（boot id + pid）
    available_at   REAL NOT NULL,   -- retry backoff 基准
    last_error     TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS semantic_outbox_ready ON semantic_outbox(state, available_at, space_id);
CREATE INDEX IF NOT EXISTS semantic_outbox_doc   ON semantic_outbox(doc_key, space_id, state);
-- 合并 pending：同 doc 同空间至多一个活跃任务（P6-013 合并语义的 DB 层保证）
CREATE UNIQUE INDEX IF NOT EXISTS semantic_outbox_live_per_doc
    ON semantic_outbox(doc_key, space_id) WHERE state IN ('pending','claimed');

-- active space 指针与空间生命周期（P6-017 切换三段的状态载体）
CREATE TABLE IF NOT EXISTS semantic_spaces (
    space_id    TEXT PRIMARY KEY,
    spec_json   TEXT NOT NULL,      -- VectorSpace + DocSpec 冻结序列化
    state       TEXT NOT NULL CHECK(state IN ('backfilling','active','revoked')),
    activated_at TEXT
);
```

active space 读取口径：`SELECT space_id FROM semantic_spaces WHERE state='active'`；
不另设 metadata 键（单一来源，避免 metadata 与表漂移）。

**schema 版本策略**：单次 bump 到 22（"按发布节点合并"），不做逐表增量迁移；
mismatch → 既有 rebuild-on-mismatch（P5 数据全量重建），FTS 无任何半升级路径
（新表均为普通表，不触碰 FTS 触发器体系，index_v1.sql:3-13 注释的维护模型不变）。
旧→新"明确重建路径"= 现有 `SchemaStatus::Mismatch`（index_migrate.rs:55）+ 换库协议；
新→旧降级 = 删语义表回 v21 或整库重建（V21 文本在 P6-019 落）。

**验收对照**：V13（新表提交效应归属正确：manifest/outbox 内容提交走 Semantic effect，
outbox 状态机运转走 Auxiliary——在 `EPOCH_RULES` 审计语义之外，effect 属 commit 级别，
需补 effect 级审计测试）、V21（v21 旧库打开→Mismatch→重建成功；v22 库被 v21 二进制
打开→Mismatch 拒绝）。

**风险**：`semantic_outbox_live_per_doc` 唯一索引会暴露 006 实现的合并 bug（同 doc
双任务插入冲突）——这是设计意图，插入必须 `ON CONFLICT` 走 supersede-then-insert
（见 P6-006）；`semantic_manifest.doc_key` FK 指向 `document_manifest`，依赖其
`doc_key` 主键（index_v1.sql:17），删除级联即"撤旧可见 manifest"，P6-006 的删除路径
可直用 CASCADE 但仍需显式 outbox supersede（CASCADE 不触发应用逻辑）。

---

## P6-006 源码事务原子写 outbox（深化）

**ADR 约束**（第 142 行）：outbox 写在源码事务内原子完成（撤旧 manifest + desired
任务 + 删除不发 embedding）；无"半个任务"泄露。

**模块归属**：`crates/cc-db/src/semantic_outbox.rs`（新，SQL 全在此）；
挂接点 = cc-db 既有文件批写事务（`document_store.rs` 的 insert 路径，
document_store.rs:100 的 INSERT 所在事务）与删除路径（`delete_file_data` 家族）；
`crates/cc-index/src/documents/delta.rs` 保持纯函数不动（delta.rs:1），由 cc-index
把 `DocumentDelta`（identity.rs:166-173：upsert/removed/unchanged/reusable_inputs）
传入 cc-db 写接口。

**关键点**：当前 `document_manifest` 的写入已在文件批事务内；P6-006 只是在**同一事务**
内追加两段：对 `delta.removed`/`delta.upsert` 中 doc_key 执行 manifest 撤销，并写
outbox 任务。提交时用 P6-004 的 `commit_with(EffectSet::of(Index)+of(Semantic))`。

**接口草案**：

```rust
// crates/cc-db/src/semantic_outbox.rs
pub struct OutboxPlan<'a> {
    pub upserts:  &'a [cc_model::identity::DocumentRef],
    pub removals: &'a [cc_model::identity::DocumentRef],
    pub space_id: &'a str,
    pub now_unix: f64,
}
pub struct OutboxWriteStats { pub superseded_tasks: usize, pub enqueued: usize,
                              pub manifest_revoked: usize }

/// 在调用方已开启的事务连接上执行（*_on 后缀 = 既有 cc-db 模式，
/// 如 IndexDb::delete_synthetic_call_edges_on，unit_of_work.rs:79）。
pub fn supersede_and_enqueue_on(conn: &Connection, plan: &OutboxPlan) -> CcResult<OutboxWriteStats>;
```

语义（单事务内顺序）：

1. `removed`：`DELETE FROM semantic_manifest WHERE doc_key IN (…)`（FK 级联兜底）+
   `UPDATE semantic_outbox SET state='superseded', updated_at=? WHERE doc_key IN (…)
   AND state IN ('pending','claimed')`（已 claimed 的旧任务也标记 superseded，
   worker 下次 ack 因 token+doc_version 双 fencing 失败而丢弃）。**删除永不产生
   embed 任务**；若同事务先删后加（同 doc_key 重写），顺序保证最终留下一个 embed。
2. `upserts`（doc_version 变化）：对活跃任务 supersede（同上 UPDATE）→
   `INSERT INTO semantic_outbox(…, op='embed', state='pending', …)`；
   `ON CONFLICT(sematic_outbox_live_per_doc)` 不吞——先显式 supersede 后插入，
   冲突仍是 bug（fail-stop，照 unit_of_work.rs:24-28 的 fail-stop 哲学）。
3. `revoke` op 仅用于显式空间撤销（P6-017），本任务不产生。

`semantic_spaces` 无 active 行时（semantic 未配置）：整个函数 no-op 返回零统计——
默认行为零变化（ADR Decision Drivers 第 49 行），cc-index 调用点无需 feature 分支。

**验收对照**：V13（该事务 commit 后 index_epoch+1 且 semantic_epoch+1，恰好各一次；
rollback 后两者均不变）；V14（提交后新文档必有任务：插文档→查 outbox pending 计数；
rollback 不泄露半个任务：drop 未 commit 后 manifest/outbox 均无痕——照
unit_of_work.rs:191-214 测试模式）；删除不发 embedding：removed doc_key 无任何
pending/新增 embed 行。

**风险**：文件批事务现在多了两个表的写放大（50k 量级批量写场景）——supersede 用
集合化 IN 语句，禁止逐行；`available_at`/时间戳由调用方传入（`plan.now_unix`），
不在 SQL 里取时钟，保证测试确定性。

---

## P6-007 实现 claim 与 lease fencing（深化）

**ADR 约束**（第 143 行）：短事务 claim/renew/retry、每 attempt 独立 token；两进程不能
同时持有同一 lease；过期 worker 无法 ack 新 lease。

**模块归属**：SQL 在 `crates/cc-db/src/semantic_outbox.rs`；worker 侧封装在
`crates/cc-semantic/src/queue.rs`。

**接口草案**：

```rust
// cc-db/semantic_outbox.rs —— 每个都是独立 IMMEDIATE 短事务（Auxiliary 效应）
pub fn claim_next_on(conn: &Connection, space_id: &str, owner: &str,
                     now: f64, lease_secs: f64) -> CcResult<Option<ClaimedTask>>;
pub struct ClaimedTask { pub task_id: i64, pub token: String,
                         pub doc_key: String, pub doc_version: String,
                         pub input_digest: String, pub lease_expires_at: f64 }

pub fn renew_lease_on(conn: &Connection, task_id: i64, token: &str,
                      now: f64, lease_secs: f64) -> CcResult<bool>; // false=已丢
pub fn ack_done_on(conn: &Connection, task_id: i64, token: &str, now: f64) -> CcResult<bool>;
pub fn retry_on(conn: &Connection, task_id: i64, token: &str, err: &str,
                now: f64, backoff_secs: f64, max_attempts: u32) -> CcResult<bool>;
```

claim SQL 核心（单条 UPDATE 原子完成 CAS，拒绝 SELECT-then-UPDATE 竞态窗口）：

```sql
UPDATE semantic_outbox
SET state='claimed', lease_token=lower(hex(randomblob(16))),
    lease_expires_at=?now+?lease_secs, claim_owner=?owner,
    attempt_count=attempt_count+1, updated_at=?now
WHERE task_id = (
    SELECT task_id FROM semantic_outbox
    WHERE space_id=?space AND state='pending' AND available_at<=?now
    ORDER BY task_id LIMIT 1)
RETURNING task_id, lease_token, doc_key, doc_version, input_digest, lease_expires_at;
```

（过期 lease 不在本语句回收：`retry_on`/后台扫描把
`state='claimed' AND lease_expires_at<?now` 归还 `pending`，claim 只吃 pending——
职责分离让"两进程同时 claim"在 SQL 层不可能：UPDATE 行级锁 + state 条件。）

fencing 不变式：

- 每 attempt 新 token（claim 时生成）；`renew/ack/retry` 一律
  `WHERE task_id=? AND lease_token=? AND state='claimed'`，rowcount=0 → 返回
  false（lease 已被他人持有或任务已 superseded/done）。
- 过期 worker ack 场景：worker A 过期 → 扫描归还 pending → worker B claim 得新
  token → A 的 ack（旧 token）rowcount=0 被拒（验收 2 的"过期 worker 无法 ack 新
  lease"）。过期→归还前 A ack 会成功——**这正是 lease 语义**，最终一致性由
  publish 侧 CAS（P6-011 的 doc_version/input fencing）兜住慢结果，不能也不必在
  outbox 层掩盖。
- publish 级 fencing 的第五要素 incarnation 不在此表（读 ReadGeneration，见 P6-011）。

`crates/cc-semantic/src/queue.rs`：`LeaseGuard { task: ClaimedTask, db: Arc<IndexDb> }`，
Drop 不自动 ack（显式 finish/retry），renew 由 worker 循环按 `lease_secs/3` 周期调用。

**验收对照**（V14）：两进程并发 claim 同一 pending 集合——双连接交叉测试，恰一个
成功；过期 worker ack 新 lease 被拒；superseded 任务不可 claim；renew 丢 lease 后
ack 被拒。全部 Auxiliary：claim/renew/ack/retry 前后 `read_generation()` 三钟不变
（V13 联动断言）。

**风险**：`RETURNING` 需 SQLite 3.35+（rusqlite 0.40 bundled 满足，Cargo.toml:29
`features=["bundled"]`）；owner 标识 `claim_owner` 仅诊断用，不参与 fencing 判定
（判定只认 token，防多进程同名）。

---

## P6-008 构建内容寻址 artifact cache（深化）

**ADR 约束**（第 144 行）：严格按"派生、可丢弃、可校验"实现；namespace 与
input/spec/checksum 分开验证；无秘密。ADR 第 99-103 行：内容寻址、可逐条校验、
损坏可检测、跨项目默认隔离、无秘密字段。

**模块归属**：`crates/cc-semantic/src/cache.rs`（存储）+ `spec.rs`（digest 消费）。
**不进主库**：向量本体与校验和都在文件系统 cache；主库只存 `artifact_ref`
（P6-005 DDL）。

**布局草案**：

```
<cache_root>/                       -- 默认不存在；首个 put 时惰性创建
  namespace-<project_ns>/           -- 跨项目默认隔离（Q5 裁决 namespace 定义）
    <space_id>/
      <input_digest>/
        <doc_spec_digest>.bin       -- 向量本体（little-endian f32 序列）
        <doc_spec_digest>.meta.json -- {format_version, dimension, model_id,
                                    --  checksum(blake3 hex of .bin),
                                    --  created_at, input_digest, space_id, spec_digest}
  quarantine/                       -- P6-018 隔离坏记录（008 只建目录约定）
```

寻址即路径：`(namespace, space_id, input_digest, doc_spec_digest)` 四元组定位；
**checksum 独立验证**（读时 blake3(.bin) 对比 .meta.json，不是路径的一部分——ADR
"namespace 与实际输入字节/spec/space/checksum 分开验证"）。

**接口草案**：

```rust
pub struct ArtifactCache { root: PathBuf, namespace: String }
pub enum CacheRead { Hit(ValidatedVector), Miss, Corrupt(CorruptReport) }
pub struct ValidatedVector { pub artifact_ref: String, pub dim: u32, pub data: Vec<f32> }

impl ArtifactCache {
    /// 打开不创建任何目录（P6-002 验收"不生成空缓存"的延续）。
    pub fn open(root: impl Into<PathBuf>, namespace: String) -> CcResult<Self>;
    /// 读：校验 checksum+meta 四元组匹配；不匹配 → Corrupt（008 不隔离，018 隔离）。
    pub fn get(&self, space: &SpaceDigest, input: &InputDigest,
               spec: &DocSpecDigest) -> CcResult<CacheRead>;
    /// 写：tmp 文件 + fsync + rename 原子落盘（artifact-before-manifest 的"先"侧）。
    pub fn put(&self, space: &SpaceDigest, input: &InputDigest, spec: &DocSpecDigest,
               vector: &[f32]) -> CcResult<String /*artifact_ref*/>;
}
```

同输入可复用（验收 1）：复用粒度 = `input_digest`（P6-003 的
`DocumentRef.encoding_key` 已含 input_hash，identity.rs:103-108）；不同文件来源
相同输入共享产物但 provenance 留在 manifest 侧（C07，`02-CONTRACTS.md:63`）。
损坏可检测（验收 2）：`get` 的 Corrupt 分支 + 单测（flip 字节）。

**验收对照**：V10（同输入复用计数与 `DocumentDelta.reusable_inputs`，
identity.rs:170-171 语义衔接）、V16（空间隔离：不同 space_id 同 input 不互命中）。

**风险**：cache 根目录位置与 namespace 定义未决（`OPEN-QUESTIONS.md` Q5）；fsync
策略在 macOS/Linux 差异需在 P6-015 崩溃测试中验证，不在本任务宣称 crash-proof——
本任务只保证"正常路径原子可见"。

---

## P6-009 实现 deterministic fake provider（深化）

**ADR 约束**（第 145 行）：worker 侧实现，永不触网；端口冻结前不得开工。
REQUIREMENTS `delegatable_after_ports_freeze[0]`：排他可委托件，"never network;
deterministic counts/errors/cancel"。

**模块归属**：`crates/cc-semantic/src/ports.rs`（trait，owner 冻结）+
`crates/cc-semantic/src/providers/fake.rs`（可委托实现）。

**接口草案**（ports.rs，owner 所有，冻结后不改）：

```rust
pub struct DocumentInput { pub input_digest: InputDigest, pub bytes: Vec<u8> }
pub struct QueryInput    { pub digest: QueryDigest,  pub bytes: Vec<u8> }
pub enum ProviderError { RateLimited{retry_after:std::time::Duration},
                         ServerError, AuthError, Timeout, Cancelled, InvalidInput(String) }

pub trait EmbeddingProvider: Send + Sync {
    fn space(&self) -> &VectorSpace;
    /// 批量有界：调用方保证 batch 大小（P6-013 admission 控制），实现方不拆批。
    fn embed_documents(&self, batch: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError>;
    fn embed_queries(&self, batch: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError>;
}
```

fake.rs（可委托）：向量 = `blake3(input_bytes + salt)` 字节流确定性展开成 f32 并
归一化（同输入恒同向量）；脚本化故障注入：

```rust
pub struct FakeProviderConfig {
    pub space: VectorSpace,
    pub fail_after_n_calls: Option<usize>,   // 次数到即 fail
    pub fail_with: Option<ProviderError>,    // 注入哪种错误
    pub delay_per_call: std::time::Duration, // 延迟注入
    pub zero_vector_inputs: Vec<InputDigest>,// 构造 zero/NaN/Inf 无效向量的开关
}
```

向量有效性门禁在本层：NaN/Inf/zero/维度不符 → `InvalidInput`（V15 的
"数量/index/维度/NaN/Inf/zero"，`06-VALIDATION.md:35`），不让坏向量进 cache。

**验收对照**（V15）：全部状态转移（成功/429/5xx/auth/timeout/cancel/重试耗尽→
failed）在测试中确定重现（同脚本同结果）；fake 结果不计真实语义质量——模块文档 +
capability status 输出携带 `provider: "fake"` 标记（P6-012 落字段）。

**风险**：trait 是 freeze 面，签名一旦被 010/011 实施期需求推动就要重走 owner 冻结；
批处理改 async 与否（C11 允许阻塞调用走有界执行器，不要求全 async，`02-CONTRACTS.md:89`）
——本草案取同步签名 + 调用方有界线程，避免为引入网络重写 DB API（C11 明文）。

---

## P6-010 实现 filtered exact 向量 backend（深化）

**ADR 约束**（第 146 行）：过滤先于 exact top-k、bounded batch、稳定 ties、空间隔离；
删除/异空间向量不可返回。C09：向量 scope 筛选在 top-k 之前（`02-CONTRACTS.md:79`）。

**模块归属**：`crates/cc-semantic/src/vector/exact.rs`（排他可委托件，连同 oracle
测试）。

**接口草案**：

```rust
pub struct ExactSearch<'a> {
    pub space: &'a SpaceDigest,
    pub query: &'a [f32],
    pub filter: &'a cc_model::retrieval::HardScope, // repo/file_paths/prefix/languages 交集
    pub k: usize,
    pub batch_rows: usize,   // 候选装载批上限（内存受控的旋钮）
}
pub struct ScoredDoc { pub doc_key: String, pub score: f64 }

/// 候选来源：semantic_manifest × artifact cache，逐 bounded batch 装载。
pub fn search(cache: &ArtifactCache, manifest: &SemanticManifestReads,
              q: ExactSearch<'_>) -> CcResult<Vec<ScoredDoc>>;
```

算法与不变式（oracle 测试逐条对应 V16，`06-VALIDATION.md:36`）：

1. **空间限定**：SQL 候选集 `WHERE space_id=?`，异空间行根本不进候选（"不同空间
   拒混"在装载层而非打分层保证）。
2. **过滤先于 top-k**：`HardScope` 交集（C09，`02-CONTRACTS.md:75-77`）先裁剪
   `file_path` 候选（`semantic_manifest_file` 索引，P6-005 DDL），`Some(empty)`
   → 空结果不退化全仓。
3. **bounded batch**：每次最多 `batch_rows` 行从 cache 装载向量、算 cosine、维护
   大小为 k 的选择堆；峰值内存 = O(batch_rows·dim + k)。删除文档：manifest 无行
   → 无候选（"删除不可返回"由数据来源结构性保证）。
4. **稳定 ties**：cosine 用 f64 累加；分数相等按 `doc_key` 字典序定序—— comparator
   总序 = `(score desc, doc_key asc)`，与线程/批序无关（C10"固定 tie-break；结果
   顺序不是线程完成顺序"，`02-CONTRACTS.md:85`）。
5. **数值 gold**：oracle 测试用手算小维向量（dim≤4）验证 cosine 值与排序，不依赖
   fake provider 数值。

**验收对照**（V16）：手算 cosine 一致；tie 稳定性（构造同分多 doc）；scope 先过滤
（prefix 命中/不命中、Some(empty)）；删除后不返回；bounded memory（大 batch_rows
断言峰值）；`semantic_manifest` 与 `document_manifest` 不一致时的行为（FK 保证
不会发生，测试证明 CASCADE 同步）。

**风险**：全量 exact 的 O(N) 在 100k 文档规模不可用——本任务定位是底座 oracle/
小规模 backend，ANN 属 V22 可选增强准入（`06-VALIDATION.md:42`），不在 P6 范围；
docstring 与 V22 准入文档必须写明该边界，不得暗示 exact 可直接上生产大库。

---

## P6-011 artifact 到 manifest 发布 CAS

**ADR 约束**（第 147 行）：必须 artifact-before-manifest；五重 fencing（incarnation +
lease token + doc version + input digest + space）；慢旧结果不能挂到同路径新版本；
发布幂等。ADR 第 60-62 行：两存储间无原子提交，crash 由幂等 recovery 重放（P6-015），
不宣称 exactly-once。

**模块归属**：`crates/cc-semantic/src/publish.rs`（编排）+ `cc-db/semantic_outbox.rs`
（CAS 短事务 SQL）。

**流程草案**：

1. `cache.put(...)` 持久化 artifact（fsync rename 完成）——**先**。
2. 打开 IMMEDIATE 短事务，逐重校验：
   - incarnation：`read_generation().incarnation` == worker 启动时快照
     （read_generation.rs:50-82；换库后必然不等 → 拒绝， fencing 住持有旧连接的
     另一进程，进程内 mutex 不足以覆盖——ADR 第 120-124 行）；
   - lease token：`semantic_outbox` 该 task 行 `lease_token` 匹配且 `state='claimed'`；
   - doc version：`document_manifest.doc_version` == 任务 `doc_version`（慢旧结果
     挂不上同路径新版本）；
   - input digest：任务 `input_digest` == 当前 manifest 行 `encoding_key` 派生输入；
   - space：任务 `space_id` == `semantic_spaces.state='active'` 行。
3. 全过 → `INSERT INTO semantic_manifest ... ON CONFLICT(doc_key) DO UPDATE ...`
   （幂等：同内容重复发布覆盖等值行）+ `ack_done_on`（Semantic 效应提交 →
   `commit_with(Semantic)`，semantic_epoch+1；可见集合未变时——重复 ack——
   **不** bump：bump 条件是可见集合实际变化，与 P6-012 的"只可见集合变化 bump"一致）。
4. 任一校验失败 → 事务回滚，任务走 `retry_on` 或按失败类别终态化；artifact 留在
   cache（无害，GC 最终回收）。

**验收对照**（V14）：CAS 全组合负测试（五重各破坏一重→拒）；发布幂等（同任务重放
两次，manifest 单行、epoch 不双 bump）；artifact-before-manifest（故障注入：
put 失败 → manifest 无变化）。

**风险**：第 3 步"重复 ack 不 bump"与"ack 是 Auxiliary"存在效应归类张力——设计取：
`ack_done_on` 本身 Auxiliary；semantic bump 是 publish 编排在确认可见集合变化后
由**同一事务**附带完成（cc-db 提供 `ack_and_publish_on(conn,...) -> PublishOutcome{
published: bool}` 原子封装，把归类细节收进 cc-db）。实施评审定名。

---

## P6-012 覆盖率与 semantic epoch

**ADR 约束**（第 148 行）：分母明确（eligible/published/failed/stale）；Aux 重试不冲刷
完整查询缓存；零 eligible 有原因。

**模块归属**：`crates/cc-server/src/capability_status.rs`（现状：只读快照、
不 probe provider，capability_status.rs:1-2）+ `crates/cc-db/src/epoch_rules.rs`
（effect 审计扩展）。

**设计**：cc-db 新增只读统计（单连接一致读，照 capability_status.rs:42-52 的
read-generation 前后比对模式）：

```rust
pub struct SemanticCoverage { pub eligible: u64, pub published: u64,
                              pub failed: u64, pub stale: u64, pub reason: ZeroEligibleReason }
// eligible   = document_manifest 中 encoding_key 非空且属 active space 的行数
// published  = semantic_manifest 中 active space 行数
// failed     = semantic_outbox state='failed' 活跃计数
// stale      = pending/claimed 中 doc_version 已落后 document_manifest 的计数
```

`capability_status.rs` 在 `result["retrieval"]` 下新增 `semantic_coverage` 与
`provider: "fake"|"none"` 字段（V18：新字段贯穿，旧字段不动）。零 eligible 枚举
`ZeroEligibleReason { SemanticNotConfigured, NoDocuments, EncodingUnsupported, … }`
（验收"零 eligible 有原因"）。

epoch 规则复申：semantic_epoch 只在可见集合（semantic_manifest 行集合）实际变化时
推进；Aux（claim/renew/retry/heartbeat/重复 ack）三钟全静——测试并入 P6-004 审计。

**风险**：coverage 统计若做全表 COUNT 在 50k+ 行上有成本——capability 快照是用户
触发路径（capability_status.rs:1 注释），可接受；如需优化用
`semantic_manifest_space` 索引 count。

---

## P6-013 worker 资源与连续编辑合并

**模块归属**：`crates/cc-semantic/src/worker.rs` + `admission.rs`（新）。

**设计**：

- **合并**：DB 层 `semantic_outbox_live_per_doc` 唯一索引（P6-005 DDL）已保证同
  doc 同空间至多一个活跃任务；连续编辑 = 新 doc_version supersede 旧任务（P6-006
  步骤 2），worker 永不为旧版本做功。
- **队列上限/公平批次**：admission 侧预算——单批 claim 上限 `max_batch`（有界执行器
  容量）、全局 pending 水位上限（超出不阻塞写路径：写路径只 supersede+insert，
  上限由 worker 消费速率兜底 + `available_at` backoff），公平 = 按 `task_id` 轮转
  空间/文件前缀（claim SQL `ORDER BY task_id` 已隐式 FIFO，公平化作为
  `ORDER BY` 可注入参数留评审）。
- **有界关闭**：Drop/join 有 deadline；关闭只停止取新任务，不丢已 claim 任务——
  未 finish 的 claim 靠 lease 过期 + P6-015 扫描回收，outbox 天然"可恢复"
  （C11："关闭时有界等待并留下可恢复 outbox"，`02-CONTRACTS.md:91`）。
- **任何时刻 local 查询可用**（验收）：worker 全程 Auxiliary 效应，永不 bump
  index/evidence；provider 调用在锁外（C11，`02-CONTRACTS.md:89-91`）。

**验收对照**：V14（supersede 后旧任务不可被 ack）、V20（大量保存场景：pending 有界、
worker 消费、local 查询延迟不劣化基线）。

**风险**：worker 线程模型（专用线程 vs tokio）需与 cc-server 现有 `QueryServices`
风格对齐（service_factory.rs 用 RwLock<Option<Arc<dyn …>>>）；无网络调用所以同步
专用线程足够，避免 tokio 依赖进入 cc-semantic。

---

## P6-014 换库 incarnation 与缓存重用

**ADR 约束**（第 150 行）：重建换 incarnation 后从 artifact cache 补 manifest（付费
产物保留），旧 DB 时代回包被 fencing 拒绝。

**模块归属**：`crates/cc-db/src/index_db_rebuild.rs`（协议侧）+
`crates/cc-semantic/src/reconcile.rs`（补齐侧）。

**设计**：

- 换库侧：既有协议已在 swap 时 `renew` incarnation（read_generation.rs:41-48"Called
  on the completed staging DB before publication"）与 finalize epoch 向量
  （index_db_rebuild.rs:254-261）。本任务补：rebuild 完成后旧进程（持有旧连接）的
  claim/publish 全部因 incarnation fencing 失败——P6-011 第 2 步已覆盖，本任务
  只加专项测试（另一 IndexDb 实例指向旧 inode/旧路径的 publish 被拒）。
- 补齐侧 `reconcile.rs`：`reconcile_after_rebuild(cache, db)`：
  1. 从 `document_manifest × active space` 重导 desired 集合（等价全量 upsert 的
     outbox 计划）；
  2. 对每个 `input_digest` 先 `cache.get`——命中且校验过 → 直接走 P6-011 CAS 写
     manifest（**不付费、不调 provider**，验收"索引重建不误删已付费向量"）；
  3. 未命中 → 留 pending 任务给 worker。
- 旧 DB 时代回包拒绝：所有语义读路径（dense lane）入口校验
  `ReadGeneration.incarnation` 与进程启动快照一致，不一致 → lane
  `unavailable` + 重取（严格 strict 读，禁用 legacy 双钟 fallback——REQUIREMENTS
  current_seams.generation 明文"never use legacy reader for publish fencing"）。

**验收对照**：V13（rebuild 后 incarnation 变化、epoch 向量单调——已有测试
read_generation.rs:133-142 扩展语义表断言）、V17（付费向量保留：rebuild 后
cache 命中率 100%、provider 调用 0 次——fake 计数器验证）。

---

## P6-015 崩溃恢复扫描

**模块归属**：`crates/cc-semantic/src/reconcile.rs` +
`crates/cc-eval/tests/semantic_lifecycle.rs`（新集成测试，kill/restart 真实进程边界）。

**设计**：recovery 扫描枚举每个持久化边界的 crash 残态，全部可复算（同输入同输出）：

| crash 点 | 残态 | 恢复动作 |
|---|---|---|
| outbox 提交前 | 无痕 | 事务回滚，无需动作 |
| artifact put 中 | cache 半文件/tmp 残留 | 读时 checksum 失败→Corrupt；启动清扫 `*.tmp` |
| put 后、CAS 前 | cache 有、manifest 无 | 扫描发现 claimed/pending 任务照常重做，CAS 命中已存 artifact 优先复用（验收"已存 artifact 优先复用"） |
| CAS 后、ack 前 | manifest 有、任务 claimed | ack 幂等重放（P6-011 幂等）或 lease 过期后 ack_done 补记 |
| lease 持有中 crash | 任务 claimed 且将过期 | 过期归还 `pending`（retry/扫描） |
| 换库 rename 中 | 旧库/新库并存 | 既有 rebuild 协议 sweep + P6-014 fencing |

扫描实现：启动时 + 周期性（worker 空闲触发），全部 Auxiliary 效应；有界（单次扫描
行数上限，余量下次继续——"恢复有界且可复算"）。

**验收对照**（V17）：`semantic_lifecycle.rs` 对上表每行构造真实 kill（子进程
SIGKILL 于注入断点，fake provider 提供断点钩子）→ restart → 断言终态收敛且
provider 调用计数不超出费用策略允许的重放上界。

**风险**：kill 注入点需要 fake provider / cache 提供 fault-point 回调——属 P6-009
FakeProviderConfig 的扩展（`crash_after_persist: bool` 之类），委托边界注意 ports
冻结时机。

---

## P6-016 GC 与发布协调

**ADR 约束**（第 152 行）：共享同步点（活跃引用 + 活跃 lease + 最短保留期 mark/sweep），
消除"刚发布的 artifact 被 GC 删除"竞态；孤儿最终可回收。

**模块归属**：`crates/cc-semantic/src/cache.rs`（sweep 执行）+ `publish.rs`
（同步点）。

**设计**：

- **mark**：活引用集合 = `semantic_manifest.artifact_ref`（全 space，含非 active——
  旧空间回滚复用依赖它，P6-017）∪ 进行中任务的 `input_digest` 派生目标（活跃 lease
  行）∪ `published_at/last_used > 最短保留期` 之外豁免。
- **同步点**：GC 候选收集与删除决定之间，取一次 DB 短事务快照：若候选在收集后被新
  publish 引用（manifest 行 artifact_ref 匹配），剔除候选——publish 侧在 CAS 事务内
  天然互斥（同一 IMMEDIATE 连接族），故"引用刚被 GC 删除产物"不可能：删除决定基于
  晚于 publish 提交的快照。
- **sweep**：unlink 候选文件 + 删除空目录；孤儿（无任何 DB 引用且超保留期）最终回收。
  全程无 DB 写（或仅 Auxiliary 计数），不刷 epoch。

**验收对照**（V17）：竞态负测试（publish 与 GC 交错，manifest 引用永远可解析）；
孤儿回收（人为注入无主 artifact → 两轮 GC 后消失）；保留期内不删。

---

## P6-017 model space 切换规划

**ADR 约束**（第 153 行）：新空间回填/切 active/撤销三段；不同空间分数永不混排；
旧 cache 经校验可回滚复用。

**模块归属**：`crates/cc-semantic/src/spec.rs`（新 space 冻结）+ `reconcile.rs`
（三段编排）。状态载体 = P6-005 的 `semantic_spaces` 表
（`backfilling → active → revoked`）。

**设计**：

1. **回填**：注册新 space 行（`backfilling`），全量文档按新 space 入 outbox；期间
   dense lane 只读 active space——`semantic_manifest_space` 索引 + lane 查询固定
   `space_id = active`，**不同空间分数永不混排**在读取层结构性保证（验收 1）。
2. **切 active**：单短事务 `UPDATE semantic_spaces SET state='active' WHERE space_id=new;
   UPDATE ... state='revoked' WHERE space_id=old`（Semantic 效应，bump semantic_epoch
   ——可见集合切换）。
3. **撤销/回滚**：旧 space 行 `revoked` 但 cache 产物保留；回滚 = 再次切换指回旧
   space，`cache.get` 校验通过即复用（验收 2"旧 cache 经校验可回滚复用"），缺失部分
   才重新回填。
- "记录用户 revision 与未 pin 限制"（tasks.json steps）：切换事件（旧/新 space_id、
  触发配置 revision）写入 `semantic_spaces.spec_json` 旁的变更记录（实现载体：
  metadata 追加键或表列，实施评审定），声明"未 pin = 不保证跨版本行为"。

**验收对照**：V16（混排拒绝：切换窗口内查询只回 active space 行）、V17（回滚后
provider 调用 0 次）。

---

## P6-018 cache 缺失/损坏降级

**ADR 约束**（第 154 行）：缺失/损坏 → 隔离坏记录、语义 degraded、本地继续；补嵌受
费用策略控制，不静默无界重费。

**模块归属**：`crates/cc-semantic/src/cache.rs`（quarantine 落地）+
`crates/cc-server/src/capability_status.rs`（degraded 透出）。

**设计**：

- `CacheRead::Corrupt(report)` 处理：坏记录移入 `quarantine/`（保留现场供诊断，不
  自动删除）+ 返回 Miss 语义；`capability_status` 透出
  `semantic_state: "degraded"` + `degraded_reason`（P6-012 字段扩展）。
- **不把缺向量当完整空结果**（验收 1）：dense lane 对 Miss/Corrupt 返回
  `LaneOutcome::partial/unavailable`（C10 状态词汇，`02-CONTRACTS.md:83`）并附
  截断理由，本地 lexical/graph 结果照常返回（"本地继续"）；缺向量不缓存为
  complete（C10"不能吞错误后缓存成 complete"）。
- **费用策略**：补嵌（对 Corrupt/Miss 的重新 embed）受 admission 预算控制——
  进程生命周期内重费计数上限（配置项，默认保守值），超限后任务终态 `failed` 带
  原因，绝不静默循环（验收 2）。计数载体：进程内 AtomicU64 + outbox
  `attempt_count` 双轨（持久审计用后者）。

**验收对照**：V17（损坏→隔离→不反复重嵌超预算）、V18（degraded 状态与 reason 可见）。

---

## P6-019 更新存储/恢复/配置文档

**模块归属**：`docs/internals/STORAGE.md`、`docs/internals/CONCURRENCY.md`、
`docs/TROUBLESHOOTING.md`；并完成 ADR-0003 第 173-176 行挂起的 `DESIGN.md` 设计
原则第 3 条与 `STORAGE.md` 开篇的限定性文本（"权威状态单库；显式例外：可丢弃派生
artifact cache"）。

**内容清单**（tasks.json steps"写清 at-least-once、两库顺序、恢复步骤与 namespace"）：

1. at-least-once 语义与 fencing 吸收重复 ack 的机制（不宣称 exactly-once/零重复
   收费——验收红线）。
2. durability 顺序：artifact 先、manifest CAS 后；各 crash 点恢复步骤（P6-015 表格
   收编）。
3. cache namespace 定义与跨项目隔离边界。
4. epoch 协议四效应表（Index/Evidence/Semantic/Auxiliary 各自推进什么）。
5. 降级行为与费用策略配置说明。

**验收对照**（V21）：文档事实漂移检测（对照实现断言文档陈述，如 schema 版本、
表名、状态机枚举值）。

---

## P6-020 P6 无网络语义底座验收

**模块归属**：`docs/roadmap/code-index-v2/`（状态回填）+ `artifacts/benchmarks/`（证据）。

**动作**：

1. fake provider 故障矩阵全量复放（P6-009 注入组合 × P6-015 crash 点）与 exact
   oracle（P6-010）在当前 run-id 证据目录复算。
2. 依赖图/默认包检查（验收"默认包无第二库、无网络、无隐式服务"）：
   - `cargo tree -p cc-server -e normal`（默认 feature）不含 `cc-semantic`；
   - 全依赖图无网络栈 crate（逐 crate 白名单核对）；
   - 默认配置启动冒烟：不创建 cache 目录、不产生第二 `*.sqlite3`。
3. G6 门证据（`06-VALIDATION.md:54`）：V13/V14/V16/V17 无网络闭环 + 正式单库修订
   ADR 在位 + 显式重建/GC/fencing 证据；按第 6 节模板落 `P6-GATE.json`，
   `review`/`rollback_checked` 填真实值。
4. `tasks.json` P6-001~020 状态与 evidence 回填。

**验收对照**：publish/fencing/GC/rebuild 闭环（四条端到端场景各一）；"仍未冒充真实
provider 效果"——所有质量声明限定 fake/oracle 范围（`06-VALIDATION.md:15`
"不把 mock/fake 成功称作真实模型成功"）。
