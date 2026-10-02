# P6-005 实施记录：新表与 schema 初始化（v22 语义持久化表）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md` P6-005
  （含 DDL 草案）、`docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003
  第 141 行：表与索引全部进主库、按发布节点合并 schema 版本、新旧 DB 有明确重建路径、
  FTS 旧数据不半升级）、P6-002/003/004 实施记录（P6-004 的 WriteEffect/EffectSet 与
  `bump_semantic_epoch_on` 为本轮衔接面）。
- 改动范围：仅 `crates/cc-db`（4 个文件，其中 1 个为新增测试文件）+ 两处评审遗留文本
  订正（§6）。零消费方行为变化（cc-index/cc-server 既有调用点零改动）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/sql/index_v1.sql` | 修改+追加 | 头注释 v21→v22；文件尾追加 P6-005 DDL 块（简报草案逐句落地，见 §2），不触碰任何既有对象与 FTS 触发器体系 |
| `crates/cc-db/src/index_migrate.rs:29-51` | 修改 | `CURRENT_SCHEMA_VERSION` 21→**22**（版本历史注释补 v22 条目）；新增 `ADDITIVE_MIGRATION_FROM = 21`（doc 注明：仅当 delta 纯加法时有效，下个 bump 必须重评） |
| `crates/cc-db/src/index_migrate.rs:65-90` | 修改 | `migrate_index_db` 新增相邻版本加法迁移分支：`stored == ADDITIVE_MIGRATION_FROM` 时原位执行 `FULL_SCHEMA_SQL`（全 `CREATE ... IF NOT EXISTS`，只补缺失对象）+ 升 `user_version` + `read_generation::ensure`，返回 `Migrated { from }`；其余非零 stored 维持原 `Mismatch` 路径（warn 日志与语义逐位不变） |
| `crates/cc-db/src/index_migrate.rs:113-118` | 新增 | `SchemaStatus::Migrated { from: u32 }` 变体（"相邻加法前驱已原位迁移、既有数据全保留"） |
| `crates/cc-db/src/index_db.rs:315-318` | 修改 | `open_and_ensure_schema_inner` 的 match 臂扩为 `UpToDate | Initialized | Migrated { .. }` 三态直通；`Mismatch` 分支（destructive reset → rebuild）零改动 |
| `crates/cc-db/src/index_migrate.rs:130-225` | 新增 | 2 个模块级迁移测试（见 §4） |
| `crates/cc-db/tests/semantic_schema.rs` | 新增 | 6 个集成测试：schema 版本断言、v21 文件库经 `IndexDb::open` 端到端原位升级、三表约束正反例（见 §4） |

## 2. 新表 DDL 清单（v22，简报草案逐句落地）

以简报 DDL 为准，共 **3 表 + 7 索引**（任务措辞中的 "claim-lease/artifact
manifest/coverage" 在简报 DDL 中并非独立表，映射见 §5 偏差 3）：

- `semantic_manifest`（可见集合，权威）：`doc_key TEXT PRIMARY KEY REFERENCES
  document_manifest(doc_key) ON DELETE CASCADE` + `doc_version/file_path/encoding_key/
  input_digest/space_id/artifact_ref/published_at/published_incarnation` 全 NOT NULL；
  索引 `semantic_manifest_space(space_id)`、`semantic_manifest_file(file_path)`、
  `semantic_manifest_artifact(artifact_ref)`（GC mark 用）。
- `semantic_outbox`（desired 任务，权威队列可靠性数据）：`task_id INTEGER PRIMARY KEY
  AUTOINCREMENT`；`op CHECK(op IN ('embed','revoke'))`、
  `state CHECK(state IN ('pending','claimed','done','failed','superseded'))`（域类型化，
  即与 EffectSet 衔接的"类型化 effect 列"等价物，见 §5 偏差 2）；claim/lease 三列
  `lease_token`/`lease_expires_at REAL`/`claim_owner`（P6-007 消费，本任务不实现逻辑）；
  索引 `semantic_outbox_ready(state,available_at,space_id)`、
  `semantic_outbox_doc(doc_key,space_id,state)`、**部分唯一索引**
  `semantic_outbox_live_per_doc(doc_key,space_id) WHERE state IN ('pending','claimed')`
  （P6-013 合并语义的 DB 层保证）。
- `semantic_spaces`（active space 指针与空间生命周期，P6-017 状态载体）：
  `space_id TEXT PRIMARY KEY`、`spec_json NOT NULL`、
  `state CHECK(state IN ('backfilling','active','revoked'))`、`activated_at` 可空；
  无索引。active space 读口径 = `SELECT space_id FROM semantic_spaces WHERE state='active'`，
  不设 metadata 键（单一来源，防漂移）。

## 3. schema 版本变化与迁移策略

- **版本**：v21 → **v22**，单次 bump（"按发布节点合并"，简报/OPEN-QUESTIONS Q7 取
  单节点口径）；新库初始化经 `FULL_SCHEMA_SQL` 直接含全部新表。
- **迁移策略（与简报有一处偏差，见 §5 偏差 1）**：
  - 相邻 v21 库 → **纯加法原位迁移**：v21→v22 delta 全部为
    `CREATE TABLE/INDEX ... IF NOT EXISTS`，不删不改任何既有表/列/FTS 触发器，故对
    v21 库原位重放 `FULL_SCHEMA_SQL` 只创建缺失对象，既有行与持久化
    `index_incarnation`/epoch 向量**原样保留**（renewal 仍专属 rebuild/swap 协议）。
  - 其余一切 stored 版本（<21、>22）→ 维持既有 rebuild-on-mismatch
    （`Mismatch` → `open_and_ensure_schema` 内 destructive reset → `Initialized`），
    该路径与 `run_rebuild_protocol`/换库协议零改动。
  - 幂等：迁移后重开返回 `UpToDate`；`migrate_index_db` 对 v22 库不执行任何 SQL。
  - 未来约束：`ADDITIVE_MIGRATION_FROM` 仅在 delta 纯加法时允许推进；非加法 bump
    必须保持 rebuild-on-mismatch（常量 doc 注明，防误用）。
- **FTS 不半升级**：新表均为普通表，不触碰 v6 FTS 触发器体系（ADR 第 141 行约束）。

## 4. 测试清单与证据（原样）

**TDD 红**（实现前）：`SDKROOT=... cargo test -p cc-db --locked --offline` →
`error[E0425]: cannot find value ADDITIVE_MIGRATION_FROM`（×2）、
`error[E0599]: no variant named Migrated found for enum index_migrate::SchemaStatus`
—— 待实现 API 缺失，红确认。

**新增测试（模块级 2 + 集成 6，全部正反例）**：

1. `adjacent_version_migrates_in_place_preserving_rows`（index_migrate.rs）——建 v22
   全 schema → DROP 三表七索引合成 v21 形态（fixture 手法，测试内注明）→ 迁移 →
   断言 `Migrated{from:21}`、`user_version==22`、旧 `files` 行原样、9 个新对象全部
   在场、重开 `UpToDate`（幂等）。
2. `non_adjacent_versions_keep_rebuild_on_mismatch`（index_migrate.rs）——stored 1/20
   → `Mismatch{stored}`（版本守卫正反例；stored 99 反例由既有
   `old_version_returns_mismatch` 覆盖）。
3. `fresh_database_carries_semantic_tables_at_current_version`（semantic_schema.rs）——
   新库含三表 + 部分唯一索引、`user_version==22`。
4. `v21_file_database_opens_migrated_with_data_intact`（semantic_schema.rs）——**端到端**：
   真实文件库（seed `files→chunks→document_manifest` + `index_epoch=7`/
   `evidence_epoch=3`）降为 v21 → `IndexDb::open` → `Migrated{from:21}`、
   `schema_version()==22`、epoch 向量与 incarnation **逐字节保留**（incarnation hex
   手工比对，验证迁移不误触 renewal）、`document_manifest` 行原样；重开 `UpToDate`。
5. `semantic_manifest_pk_fk_and_cascade`——PK 重复拒绝；`foreign_keys=ON` 下幽灵
   doc_key 拒绝；删 `document_manifest` 行级联清 `semantic_manifest`。
6. `semantic_outbox_op_and_state_domains_are_typed`——非法 op `'purge'`/非法 state
   `'queued'` CHECK 拒绝；done 与新 pending 同 doc 共存（正例）。
7. `semantic_outbox_at_most_one_live_task_per_doc_and_space`——同 (doc,space) 第二个
   pending/claimed 拒绝；superseded/done/failed 不算 live；同 doc 异 space、同 space
   异 doc 均独立（部分唯一索引正反例全覆盖）。
8. `semantic_spaces_state_domain_and_active_read_pattern`——三态合法、`'retired'`
   拒绝、PK 重复拒绝、`activated_at` 可空、active 读口径单行返回。

**绿（命令与结果原样）**：

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-db
   --locked --offline` → lib `158 tests / 157 passed; 0 failed; 1 ignored`（本轮 +2），
   集成套件 `13 / 4 / 4 / 4 / 1 / 6(semantic_schema) / 6 / 0` 全部
   `ok, 0 failed`。
2. `cargo test -p cc-db --locked --offline --test semantic_schema` → `6 passed; 0 failed`。
3. `cargo test -p cc-db --locked --offline index_migrate` → `5 passed; 0 failed`
   （既有 3 + 新增 2）。
4. `SDKROOT=... cargo test -p cc-index --locked --offline`（`IndexDb::open` 状态消费方）
   → lib `377 passed`（+1 ignored）及 8 个集成套件全部 `ok, 0 failed`。
5. `SDKROOT=... cargo test -p cc-server --locked --offline`（`SchemaStatus::Initialized`
   的 `needs_initial_index` 消费方）→ `249 / 41 / 9 / 0` 全部 `ok, 0 failed`；
   `Migrated` 不触发 `needs_initial_index`，语义正确（数据保留，无需重扫）。
6. `SDKROOT=... cargo check --workspace --locked --offline` → `Finished dev profile
   ... in 6.42s`，零 warning。
7. `rustfmt`：仅对本轮触碰的 `index_migrate.rs`/`index_db.rs`/`semantic_schema.rs`
   格式化（既有 P6-003/004 文件的 fmt 漂移不属本轮，未触碰）。

## 5. 与简报偏差清单

1. **迁移策略**：简报 §schema 版本策略写"不做逐表增量迁移；mismatch → 既有
   rebuild-on-mismatch（P5 数据全量重建）"。本实施为**相邻 v21** 增加了纯加法原位
   迁移路径。原因：任务红线"现有表/数据零破坏（迁移只增不删不改旧列语义）"与验收
   "打开旧 schema → 迁移 → 新表在场且旧数据不动"在破坏性 reset 路径下不可满足；
   而 v21→v22 delta 客观上纯加法，原位重放安全性由 schema 文件的
   `IF NOT EXISTS` 结构保证并有测试固化。非相邻/更新版本的 rebuild 契约一字未动；
   ADR 的"新旧 DB 有明确重建路径"仍然成立（重建路径保留、降级路径 V21 文本归
   P6-019）。后续任何非加法 bump 不得推进 `ADDITIVE_MIGRATION_FROM`。
2. **不加行级 "effect 列"**：任务衔接项提到"如 outbox 表的 effect 列类型化"。
   P6-004 定案 1/2 明确 `EffectSet` 是 **commit 级声明**（`commit_with`）而非行级
   属性，行级 effect 列会把提交分类错误下沉到存储层。outbox 的域类型化由
   `op`/`state` 两个 CHECK 约束承担（'embed'/'revoke' × 五态）；"manifest/outbox
   内容提交走 Semantic、状态机运转走 Auxiliary"的效应归属由 P6-006 调用方经
   `commit_with(EffectSet)` 声明（V13 的 effect 级审计测试已由 P6-004 固化）。
   schema 侧不重复该维度。
3. **表集合映射**：简报 DDL 仅三表，无独立 coverage 表 / artifact manifest 表 /
   claim-lease 表。按"以简报 DDL 为准"：claim/lease 状态内联于 `semantic_outbox`
   三列；artifact 引用为 `semantic_manifest.artifact_ref`（含 GC mark 索引）；
   coverage 属 P6-012 派生统计（可从 manifest/outbox 导出），非持久化表。
4. `SchemaStatus` 公开枚举新增 `Migrated { from }` 变体：非简报内容，是偏差 1 的
   必要 API 面；全部既有消费点（`index_db.rs` match、`cc-server/engine.rs` 的
   `matches!`、测试断言）经 `cargo check --workspace` + cc-server 全量测试验证无破坏。

## 6. 两处评审遗留文本订正（顺手项，已确认）

1. `artifacts/checkpoints/p6-batch1-20261002/P6-004-IMPLEMENTATION.md` §1 与 §6 的
   生产调用方计数订正：经 grep 复核，`UnitOfWork::commit()` 唯一真实生产调用方为
   `crates/cc-index/src/synthesis_pipeline.rs:110`（begin）/`:125`（commit）；
   `signature_agg.rs:1140` 与 `index_db_edges.rs:1330,1683` 均位于
   `#[cfg(test)] mod tests`（分别起于 :785/:1272）内，属测试调用点。§1 原
   "三个生产调用方"、§6 原"生产调用方 4 处"已改为订正后表述。
2. `crates/cc-semantic/src/lib.rs:12-15`：`try_init` 推迟指向由
   "P6-004/P6-008" 订正为 "P6-008 plus the combination-root wiring round"
   （P6-008 + 组合根接线轮；P6-004 已落地，原指向失效）。

## 7. 未做与剩余风险

- 本任务只落表与 schema：不写任何 `semantic_manifest`/`semantic_outbox`/
  `semantic_spaces` 生产读写方法（`semantic_outbox.rs`/`semantic_manifest.rs` 归
  P6-006/007，简报模块归属条款）；`bump_semantic_epoch_on` 在生产路径仍无调用方。
- v21→v22 原位迁移对"v21 库 + 已崩溃半写状态"无专门处理（与既有机制同级，SQLite
  事务保证 `user_version` 更新与 DDL 原子；若 `execute_batch` 中途失败，事务回滚、
  stored 仍 21，下次打开重试或走 Mismatch 人工兜底）。
- V21 验收的降级矩阵（v22 库被 v21 二进制打开 → `Mismatch` 拒绝）由版本守卫机制
  覆盖（stored 22 ≠ 21 → `Mismatch`，`non_adjacent_versions_keep_rebuild_on_mismatch`
  的 >22 侧由既有 stored 99 测试等价覆盖），完整矩阵文档归 P6-019。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
