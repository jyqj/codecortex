# P6-011 实施记录：artifact 到 manifest 发布 CAS

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-011 节（CAS SQL 草案、五重 fencing、Q4 重复 ack 语义、artifact-before-manifest）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 147 行约束；第 60-62 行
  "无跨两库原子提交，artifact durable 在先、manifest CAS 在后"；第 120-124 行 fencing
  身份链与 strict ReadGeneration）、批次 1/2 交付（P6-004 Q4 交付注释
  `crates/cc-db/src/unit_of_work.rs:296-300` 明文把可见集合 diff 判定责任移交本任务；
  P6-007 fenced lease 骨架；P6-008 ArtifactCache；P6-010 SemanticManifestReads/exact）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新测试文件；
  `crates/cc-semantic` 1 个新模块 + 1 个新测试文件 + lib.rs 文档/注册更新。
  **schema v22 零变更**；P6-006/007/008/010 交付物**只调用未改动**（含其文件零触碰；
  clippy/fmt 报出的既有漂移均在交付文件内，本轮未动）；`tasks.json` status 未改；
  未 git commit。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_publish.rs` | 新增（全文） | 模块文档（CAS 语义原文 + 五 fence + Q4 + 拒绝语义）+ `PublishRejection`(:68，六变体) + `PublishRequest`(:108) + `PublishOutcome`(:128) + `publish_and_ack_on`(:179) + `rejected`(:324，fenced retry 兜底) + `impl IndexDb::publish_semantic`(:345-355，IMMEDIATE 短事务 + 条件 bump) |
| `crates/cc-db/src/lib.rs:49` | 修改 | `pub mod semantic_publish;`（一行注册） |
| `crates/cc-db/tests/semantic_publish.rs` | 新增 | 11 个集成测试（§4） |
| `crates/cc-semantic/src/publish.rs` | 新增（全文） | 模块文档（durability 顺序，无跨库原子性声明）+ `PublishVerdict`(:46) + `Publisher`(:62) + `Publisher::new`(:74) + `publish_embedding`(:98，三步：put → read-back 校验 → CAS) |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod publish;`(:31) + 模块边界文档补 P6-011 段 |
| `crates/cc-semantic/tests/publish_cas.rs` | 新增 | 4 个集成测试（§4） |

## 2. CAS 语义（简报原文与落点）

简报 P6-011 第 3 步原文：**"全过 → `INSERT INTO semantic_manifest ... ON
CONFLICT(doc_key) DO UPDATE ...`（幂等：同内容重复发布覆盖等值行）+ `ack_done_on`"**。
落点：`crates/cc-db/src/semantic_publish.rs:276-292`——doc_key 主键行 upsert，冲突
分支整行替换（doc_version/file_path/encoding_key/input_digest/space_id/artifact_ref/
published_at/published_incarnation），同一事务内随后调用 P6-007 的
`semantic_outbox::ack_done_on`（fenced `claimed→done`）。"比较空间/spec 版本/期望状态
后原子写入；冲突即放弃不覆盖"落为五重 fence 先比后写（任一不过即零写入返回拒绝，
绝不覆盖既有行）：

| Fence | 校验 | 拒绝变体 | 落点 |
|---|---|---|---|
| 1 incarnation | `read_generation::read_on(conn).incarnation == worker 启动快照`（strict ReadGeneration，绝不走 legacy 双钟） | `IncarnationMismatch` | :180-184 |
| 2 lease token | 行 `state='claimed'` 且 `lease_token` 精确匹配（token-only，owner 不参与） | `LeaseLost` | :186-207 |
| 3 doc version | `document_manifest.doc_version == 任务 doc_version`（慢旧结果挂不上同路径新版本） | `DocVersionStale` | :209-224 |
| 4 input digest | 任务 `input_digest == record_json.input.input_hash`（P6-003 实际嵌入输入 digest；最小反序列化 `RecordInputPeek`，不耦合 record schema；`encoding_key` 为 NULL 同样拒绝） | `InputDigestMismatch` | :224-240 |
| 5 space | 任务 `space_id == semantic_spaces 唯一 active 行`（多 active 行 fail-stop，复用 `active_space_on`） | `SpaceNotActive` | :241-247 |
| 基行缺失 | `document_manifest` 无行 | `DocumentMissing` | :225-227 |

**artifact-before-manifest 强制**：库内 CAS 无法感知文件系统，故"先验 cache 对象在场
且校验通过，再写 manifest 行"由 `crates/cc-semantic/src/publish.rs:106-142` 编排强制：
`cache.put`（原子 rename + fsync，P6-008）→ `cache.get` 读回必须 `Hit` 且
`artifact_ref` 与 put 返回一致（checksum + 四元组寻址验证链全过）→ 才发起
`IndexDb::publish_semantic`。put 失败/读回 Miss/Corrupt/ref 不一致 →
`PublishVerdict::ArtifactNotVerified`，数据库零接触（测试
`unverifiable_artifact_refuses_the_publish_before_touching_the_db`）。

**效应归类（简报风险段的裁决落地）**：`ack_done_on` 本身 Auxiliary；semantic bump 由
`IndexDb::publish_semantic`（:355-379）在同一事务内、且仅当
`outcome.visible_set_changed` 为真时附带 `bump_semantic_epoch_on` 完成——
"把归类细节收进 cc-db"按简报建议落在门面层（函数名取 `publish_semantic` 而非草案的
`ack_and_publish_on`，SQL 原语名保持 `publish_and_ack_on`）。

## 3. Q4 责任兑现（重复 ack 的可见集合判定）

P6-004 交付注释（`unit_of_work.rs:295-300`）："The visible-set diff is the
publisher's duty (P6-011 CAS); cc-db is declarative." 本任务兑现：

- 判定逻辑：`crates/cc-db/src/semantic_publish.rs:249-273`——同 doc_key 既有
  semantic_manifest 行的 `(doc_version, input_digest, space_id, artifact_ref)` 与本次
  发布**逐字段相等** → `visible_set_changed=false`：不写 manifest（连等值覆盖也不做）、
  照常 ack、门面不声明 Semantic 效应 → `semantic_epoch` 不 bump。
- 门面按 outcome 声明效应：`published && visible_set_changed` 才
  `bump_semantic_epoch_on`（:365-369）；重复 ack 提交为纯 Auxiliary。
- 测试固化（两库三层）：
  - `crates/cc-db/tests/semantic_publish.rs:247` `duplicate_ack_of_identical_content_acks_without_bumping`：
    同内容重发布（新 claim、新 token）→ `published=true, visible_set_changed=false`、
    manifest 仍单行、`read_generation().semantic_epoch` 保持 `Some(1)`
    （P6-004 钟读断言）、index_epoch 不动。
  - `crates/cc-semantic/tests/publish_cas.rs:252`
    `published_document_is_retrievable_and_duplicate_ack_does_not_bump`：全编排路径
    重复发布 → `Published{visible_set_changed:false}`、钟读 `Some(1)` 不变。
  - 呼应单层：`unit_of_work.rs:302`（P6-004 effect 级审计）继续成立。

## 4. 测试清单（15 个，全绿）

**`crates/cc-db/tests/semantic_publish.rs`（11 个）**

1. `publish_cas_happy_path_bumps_semantic_exactly_once`(:184)——真实 `IndexDb` 门面：
   upsert 字段逐项（含 file_path 冗余解析、32 位 hex incarnation）、task `done`、
   `semantic_epoch None→Some(1)`、index/evidence/incarnation 逐项不变。
2. `duplicate_ack_of_identical_content_acks_without_bumping`(:247)——Q4（见 §3）。
3. `stale_incarnation_is_fenced_out`(:327)——五 fence 之一破：拒绝、manifest 空、
   `semantic_epoch` 键缺失、任务经 fenced retry 回 `pending`
   （`available_at=now+backoff`、lease 清空、`last_error` 持稳定拒绝码）。
4. `lost_lease_is_fenced_out_without_touching_anything`(:351)——伪造 token：拒绝、
   零写入、真实 claimant 行原样（retry 对非持有者天然 no-op）。
5. `stale_doc_version_cannot_attach_to_a_newer_document`(:368)——慢旧结果 vs 新版本。
6. `input_digest_mismatch_is_fenced_out`(:385)——两分支：hash 不一致；
   record 失去可嵌输入（`encoding_key NULL` + 空 input）。
7. `non_active_space_is_fenced_out`(:421)——space revoked（P6-017 切换窗口）。
8. `deleted_document_is_fenced_out`(:438)——基行消失。
9. `rejection_retries_with_backoff_then_dead_letters_on_exhaustion`(:452)——
   拒绝 → backoff pending → 预算耗尽终态 `failed`，死信不可再 claim。
10. `expired_reclaimed_lease_fences_the_old_publish`(:502)——过期回收 + B 重新 claim：
    A 的 publish（旧 token）拒绝零写入，B 的 publish 成功 ack。
11. `facade_rejection_commits_retry_without_any_epoch_bump`(:532)——门面提交拒绝
    outcome（retry 落库为真实队列状态）但三钟全不动。

**`crates/cc-semantic/tests/publish_cas.rs`（4 个）**

1. `unverifiable_artifact_refuses_the_publish_before_touching_the_db`(:204)——
   artifact-before-manifest 故障注入（维度不符 + NaN 两次 put 失败）：DB 零接触、
   任务仍 claimed、`semantic_epoch` 缺失、cache 无半写残留。
2. `published_document_is_retrievable_and_duplicate_ack_does_not_bump`(:252)——
   发布后 filtered exact（真实 `SemanticManifestReads` keyset 扫描 × cache）检索
   可见（score 1.0）+ Q4 重复发布零 bump。
3. `swapped_incarnation_rejects_the_publish`(:346)——换库 fencing：incarnation 更新
   后旧快照 publish 被拒；artifact 留 cache（付费产物不销毁）、任务回队、零 bump。
4. `expired_reclaimed_lease_cannot_publish_but_successor_can`(:384)——与 outbox
   状态机衔接：过期 worker 拒绝、继任者发布成功并检索可见。

## 5. 与 outbox / cache 的衔接方式

- **outbox（P6-006/007 交付，只调用）**：入队经 `supersede_and_enqueue_on`；认领经
  `claim_next_on`；发布成功 = CAS 事务内 `ack_done_on`（token fencing，
  `claimed→done`）；发布拒绝 = 同事务内 `retry_on`（backoff 回 `pending`，预算耗尽
  终态 `failed`；`LeaseLost` 拒绝时 retry 天然 no-op）。outbox 状态机封闭迁移表
  零扩展（`claimed→done`、`claimed→pending`、`claimed→failed` 均为 P6-006/007 既有边）。
- **cache（P6-008 交付，只调用）**：`put` 是 durability "先"侧（幂等，crash 重放
  重 put 收敛同 ref）；`get` 读回是"在场且校验通过"门；拒绝后 artifact 留 cache
  无害（GC 归 P6-016）。
- **epoch（P6-004 交付，只调用）**：bump 职责经 `bump_semantic_epoch_on`，声明条件 =
  可见集合实际变化（Q4）；claim/renew/retry/reclaim 生命周期保持 Auxiliary 零 bump
  （`semantic_lease.rs` 测试 9 未受扰动，全量回归绿）。

## 6. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db -p cc-semantic -p cc-index --locked --offline
→ 26 个 "test result: ok."，0 failed；关键计数：
  cc-db lib 162 passed (+1 ignored)；semantic_publish 11 passed；
  semantic_lease 9、semantic_outbox 17、semantic_schema 6、semantic_manifest_reads 5 等全绿；
  cc-semantic lib 22、artifact_cache 17、manifest_exact_integration 4、publish_cas 4；
  cc-index lib 377 (+1 ignored) 及全部集成套件绿

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 27.52s（零 error）

cargo clippy -p cc-db --locked --offline            → 零 warning（cc-db）
cargo clippy -p cc-semantic --locked --offline      → 3 个 warning，全部位于
  crates/cc-semantic/src/cache.rs（P6-008 交付文件：needless_question_mark ×1、
  doc_lazy_continuation ×2），本轮红线未触碰，留交付方/收口轮处理
rustfmt --check（本轮新增 4 文件）                    → 干净；仓内既有 fmt 漂移
  （spec.rs/exact.rs/fake.rs/ports.rs/cache.rs/semantic_manifest_reads.rs 等交付文件）
  未触碰
```

## 7. 偏差清单（与简报草案）

1. **SQL 原语落新模块 `semantic_publish.rs` 而非 `semantic_outbox.rs`**：简报归属写
   "cc-db/semantic_outbox.rs"，但红线要求 P6-006/007 交付物"只调用"；新文件同模块
   层级、同样只含 `*_on` 自由函数 + `impl IndexDb` 多文件先例（index_db_arch.rs 等），
   职责边界更干净。
2. **门面名 `publish_semantic`**（简报草案 `ack_and_publish_on(conn,...) ->
   PublishOutcome{published:bool}` 的编排收口层）：草案把"归类细节收进 cc-db"的原子
   封装落在自由函数；实现把事务边界（`IndexDb::write_conn` 为 `pub(crate)`，cc-semantic
   无法自开 IMMEDIATE 事务）收进 `impl IndexDb` 门面，内部调用 `publish_and_ack_on`
   原语。`PublishOutcome` 扩展 `visible_set_changed`/`rejection` 两字段（草案仅
   `published`）——Q4 判定与拒绝原因需可观测、可测试。
3. **拒绝路径在同一 CAS 事务内完成 fenced retry**（简报措辞"事务回滚，任务走
   retry_on"）：实现为"manifest 侧零写入（等价回滚）+ 同事务 retry 落库"——两段式
   会在自动提交连接上留下中间态窗口，且与门面"拒绝 outcome 照常提交"一致
   （测试 11 固化）。`LeaseLost` 拒绝时 retry 由 token fencing 自然 no-op。
4. **input fence 实现为 `record_json.input.input_hash` 精确比对**（简报措辞
   "当前 manifest 行 encoding_key 派生输入"）：`encoding_key = hash((spec, input_hash))`
   的 blake3 原像不可逆推，字面"派生"不可行；doc_version 本身覆盖整 record 哈希
   （identity.rs version()），fence 4 在信息上被 fence 3 蕴含，但按简报要求显式实现
   （最小反序列化，不耦合 record schema），测试 6 可独立观测。
5. **`PublishRequest` 增加 `retry_backoff_secs`/`max_attempts`**（草案无）：拒绝路径
   的 retry 策略参数必须来自编排方（费用/预算策略归 P6-013/018），cc-db 不取配置。
6. **重发等值行时不做物理 UPDATE**：`visible_set_changed=false` 分支连等值 upsert 也
   跳过（published_at/incarnation 保持首次值）；简报"同内容重复发布覆盖等值行"的
   幂等语义由"结果等价"满足（manifest 单行、epoch 不双 bump——测试 2 原样断言）。
7. **cc-semantic 侧向量未做 provider 级校验重放**：向量有效性门禁在 P6-008 cache
   层（维度/有限性）与 P6-009 provider 层，编排层不重复校验（测试 1 经 cache 门
   观测拒绝）。

## 8. 未做与剩余风险

- **无跨两库原子提交（ADR 第 60-62 行，如实记录）**：`publish_embedding` 的三步
  （put / read-back / CAS）之间不存在跨存储事务；任一 crash 点的残态 =
  {cache 半写（P6-015 清扫）、cache 有 manifest 无（任务重放，本任务幂等）、
  CAS 已提交（原子）}，recovery 编排归 P6-015。
- **"CAS 后、ack 前"残态在本设计中不可达**（manifest 写与 ack 同事务原子）；P6-015
  恢复表该行的"ack 幂等重放"路径由 §3 判定等价吸收（重复发布 → 零可见变化）。
- **incarnation fence 的覆盖边界**：校验"本连接读到的 incarnation == 快照"；另一
  进程换库后本进程仍持旧 inode 连接的场景，需 P6-014 的跨实例专项测试（简报 P6-014
  明文该测试归彼任务，本任务 fence 原语已就位）。
- **发布编排的 worker 循环**（claim→embed→publish→renew 周期、admission、合并消费）
  归 P6-013；本任务交付 publish 原语 + 编排函数，不含调度。
- cc-semantic 3 个既有 clippy warning 与仓内既有 fmt 漂移未处理（交付文件红线）。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
