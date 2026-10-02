# P6-006 实施记录：源码事务原子写 outbox

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-006（含接口草案与单事务语义）、`docs/adr/0003-semantic-persistence-single-db-boundary.md`
  （ADR-0003 第 142 行：outbox 写在源码事务内原子完成，撤旧 manifest + desired 任务 +
  删除不发 embedding；无"半个任务"泄露）、批次 1 四份实施记录（v22 schema 冻结、
  `commit_with(EffectSet)` 定案、EffectSet 为 commit 级声明）。
- 改动范围：仅 `crates/cc-db`（4 个既有文件 + 2 个新文件）。零 schema 变更（v22 冻结，
  `index_migrate.rs` 零改动）。未改 `tasks.json` status，未 git commit，artifacts 冻结链
  未触碰。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_outbox.rs` | 新增（全模块 SQL 唯一归属，简报模块归属条款） | 类型：`OutboxState`(:50，五态 + `can_transition_to` 封闭迁移表)、`OutboxOp`(:104)、`OutboxUpsert`(:133)、`OutboxPlan`(:141)、`OutboxWriteStats`(:151，`changed_semantic_state` = EffectSet 声明输入)、`OutboxTask`(:168)。函数：`now_unix`(:31，调用方时钟，SQL 不取时钟)、`timestamp_text`(:40，由 `now_unix` 确定性派生 `created_at/updated_at`)、`active_space_on`(:182，单 active 空间指针，多行 fail-stop)、`supersede_live_on`(:197，集合化 IN supersede)、`supersede_and_enqueue_on`(:230，简报接口草案逐句落地)、`apply_file_batch_on`(:293，FileWriteUnit 批次→OutboxPlan 收集器，pub(crate))、`ready_tasks_on`(:371，走 `semantic_outbox_ready` 索引)、`transition_state_on`(:424，类型化迁移守卫) |
| `crates/cc-db/src/lib.rs:47` | 新增 | `pub mod semantic_outbox;` |
| `crates/cc-db/src/index_db.rs:936-950` | 新增 | `ReadOps::semantic_outbox_ready(limit)`——ready 任务读方法（经 `read_conn` 只读池；claim/lease 归 P6-007） |
| `crates/cc-db/src/index_db_write_batch.rs:378-382` | 新增 | `write_incremental_batch` 事务内挂接：`apply_file_batch_on`（在 base deletes **之前**——删除路径的 doc_key 需经 `semantic_manifest.file_path`/`document_manifest.file_path` 解析，行删即级联消失）；`db_semantic_outbox` 计时段 |
| `crates/cc-db/src/index_db_write_batch.rs:445-462` | 修改 | `write_incremental_batch` 原无条件 `bump_index_epoch_on` 改为 **EffectSet 驱动**：`EffectSet::of(Index)`，`semantic_stats.changed_semantic_state()` 时 union `Semantic`；逐声明效应 bump 恰一次（与 `commit_with` 同语义） |
| `crates/cc-db/src/index_db_write_batch.rs:34-40,50-67` | 新增 | `replace_files_batch` 同挂接 + 新 helper `bump_effects_on`（EffectSet→bump 的共享实现，write_batch 三个写点共用） |
| `crates/cc-db/src/index_db_write_batch.rs:525-529` | 新增 | `remove_files_batch`（独立删除路径）同挂接：先 `apply_file_batch_on(&tx, &[], paths)` 撤销+supersede，再删 base，`bump_effects_on` 收尾 |
| `crates/cc-db/tests/semantic_outbox.rs` | 新增 | 17 个集成测试（§3） |

未触碰：`crates/cc-index/src/documents/delta.rs`（保持纯函数，简报明示）；`multi_insert`
全量重建路径（§5 偏差 6）；`apply_config_link_units`（config 文件无 `outcome.documents`，
outbox 计划恒空，不挂接）；FTS 触发器体系；schema。

## 2. 机制要点

1. **原子性**：outbox 行与文档 manifest 写入发生在同一个 `IMMEDIATE` 事务
   （`write_incremental_batch`/`replace_files_batch`/`remove_files_batch` 的 `tx`），
   `supersede_and_enqueue_on` 只接收调用方事务连接（`*_on` 后缀模式）。回滚 = outbox
   零残留（测试 `rollback_leaves_no_outbox_or_manifest_trace` 与生产路径
   `failed_batch_rolls_back_no_half_task` 双向固化）。
2. **单事务语义（简报顺序逐句落地）**：
   - `removed` → `DELETE FROM semantic_manifest WHERE doc_key IN (…)`（FK 级联仅兜底，
     CASCADE 不触发应用逻辑）+ live 任务 supersede（`pending`/`claimed` 双态；claimed 被
     supersede 后由 P6-007 worker 的 token+doc_version 双 fencing 在 ack 时丢弃）。
     **删除永不产生 embed 任务**；同事务先删后加（同 doc_key 重写）顺序保证恰好留一个
     embed（`rewritten_doc_keeps_exactly_one_live_embed`）。
   - `upserts` → 先 supersede 后 `INSERT(…,'embed','pending',…)`；裸 INSERT 不吞
     `semantic_outbox_live_per_doc` 部分唯一索引冲突（fail-stop，
     `duplicate_live_upsert_conflicts_fail_stop`）。
   - `revoke` op 本轮不产生（归 P6-017）。
3. **no-active-space 零成本零变化**：`apply_file_batch_on` 先查 `semantic_spaces`，
   无 active 行即返回零统计、一条 SQL 都不执行——默认构建路径行为逐位不变
   （ADR Decision Drivers；`incremental_batch_without_active_space_keeps_default_behavior`
   固化 `semantic_epoch` 键保持缺失）。
4. **EffectSet 接线（首个生产调用方）**：文件批事务以
   `OutboxWriteStats::changed_semantic_state()` 为可见集合变化的判据，声明
   `{Index}` 或 `{Index, Semantic}`，逐效应 bump 恰一次——即 P6-004 Q4 定案 1/2 在
   真实写路径的首次执行方（语义可见集合变化 = 本次事务写出的 revoke/enqueue/supersede
   本身，无需外部 diff）。
5. **input_digest 来源**：enqueue 时取 `DocumentRecord.input.input_hash`
   （= blake3(输入字节) = P6-003 `InputDigest`，持久化于 `document_manifest.record_json`
   的记录内）。render 失败（`input == None`）的投影不入队——无输入可嵌的任务是
   "半个任务"；下次成功 render 改变 `doc_version` 后自然重新入队。
6. **状态机**：`can_transition_to` 封闭表——`pending→claimed`（P6-007 claim 的类型
   占位）/`pending→superseded`/`claimed→done|failed|superseded`/`failed→pending`（退避
   重试）/`failed→superseded`；`done`/`superseded` 终态；`pending→done` 非法（必须先
   claim）。`transition_state_on` 只写 `state/updated_at/last_error`，**不触碰**
   `lease_token/lease_expires_at/claim_owner/attempt_count`（P6-007 fencing 领域，
   `transition_guards_in_database` 断言 lease_token 保持 NULL）。
7. **时钟纪律**：`available_at`/时间戳全部由调用方传入（`plan.now_unix`），SQL 内不取
   时钟（简报风险段要求）；读侧 `ready_tasks_on` 的 `now_unix` 同样由调用方给
   （`ReadOps` 门面传系统时钟）。

## 3. 测试清单（新增 17 个，`crates/cc-db/tests/semantic_outbox.rs`，全绿）

TDD 红：实现前 `cargo test -p cc-db --test semantic_outbox` → `error[E0432]: unresolved
import cc_db::semantic_outbox`、`error[E0599]: no method named semantic_outbox_ready` 等
（API 未实现）。

原子性/单事务语义（1-7）：

1. `no_active_space_is_a_full_noop`——无 active 空间：零统计、manifest/outbox 均不动；
2. `removals_revoke_manifest_and_supersede_without_embed`——撤销 manifest 行、
   supersede pending 与 claimed 任务、零 embed 入队；
3. `upsert_supersedes_old_and_enqueues_one_pending_embed`——旧任务 superseded，恰一个
   pending embed，`doc_version/input_digest/space_id/available_at` 逐字段断言；
4. `rewritten_doc_keeps_exactly_one_live_embed`——同事务先删后加留恰一个 live embed；
5. `duplicate_live_upsert_conflicts_fail_stop`——部分唯一索引冲突不吞（fail-stop）；
6. `two_active_spaces_fail_stop`——双 active 空间违反 P6-017 单空间不变量 → fail-stop；
7. `rollback_leaves_no_outbox_or_manifest_trace`——**事务回滚零残留**（挂起事务 drop
   后 outbox/manifest 均无痕）。

状态机（8-9）：

8. `state_transition_table_is_exact`——17 组正反例的封闭迁移表；
9. `transition_guards_in_database`——非法迁移类型化拒绝、`pending→claimed→done` 链、
   陈旧 `expected_from` 写不中（Ok(false) 零写入）、`failed→pending` 重试持久化
   `last_error`、lease 列不被触碰。

ready 读（10-11）：

10. `ready_query_filters_and_orders`——future/claimed/done/superseded 全过滤、
    `(available_at,task_id)` 排序、limit 截断、op/space 字段正确；
11. `ready_query_uses_the_ready_index`——`EXPLAIN QUERY PLAN` 含
    `semantic_outbox_ready`（索引在用）。

EffectSet 接线 + 生产写路径（12-17）：

12. `incremental_batch_with_active_space_enqueues_embed_and_bumps_semantic_once`——
    `{Index, Semantic}` 组合：`index_epoch` +1 且 `semantic_epoch` `None→Some(1)`，
    ready 读路径取回任务、`input_digest == blake3(输入文本)`；
13. `incremental_batch_without_active_space_keeps_default_behavior`——零变化守卫：
    无空间时 `semantic_epoch` 键保持缺失、零 outbox 行（index 行为不变）；
14. `file_removal_revokes_manifest_and_supersedes_without_embed`——`to_remove` 路径
    撤销已发布 manifest 行 + supersede live 任务 + **不发 embed**，且 `semantic_epoch`
    +1（撤销是语义可见集合变化）；
15. `failed_batch_rolls_back_no_half_task`——**生产路径原子性**：document/chunk
    coverage 失配使事务中途失败 → outbox 零残留、双钟不动、files 零行；
16. `replace_files_batch_enqueues_embed_with_semantic_effect`——单批替换写路径同接线；
17. `remove_files_batch_supersedes_without_embed`——独立删除路径同接线。

fixture 说明：document 单元经 `DocumentRecord::new` 真实身份/校验路径构造
（`ChunkSource`/`EmbeddingInput` 全字段合法，64-hex digest 门通过），非伪造行。

## 4. 验证命令与结果（原样）

1. TDD 红：`SDKROOT=… cargo test -p cc-db --test semantic_outbox --locked --offline`
   → `error[E0432]: unresolved import cc_db::semantic_outbox` 等（API 未实现）。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-db
   -p cc-index --locked --offline`（fmt 后复跑）→ **passed=666 failed=0**（cc-db lib
   157+1 ignored、`semantic_outbox` 17、`semantic_schema` 6、其余集成套件全绿；
   cc-index lib 377+1 ignored 及 8 个集成套件全绿）。cc-index 既有
   `EPOCH_BUMPS_PER_CONTENT_BATCH=3` 审计原样通过（无 active 空间 → 零额外 bump）。
3. `SDKROOT=… cargo test -p cc-server --locked --offline` → `249 / 41 / 9 passed;
   0 failed` 全绿。
4. `cargo check --workspace --locked --offline` → `Finished dev profile … in 4.05s`。
5. `cargo clippy -p cc-db --locked --offline` → 零 warning。
6. `cargo fmt --check -p cc-db` → exit 0（本轮触碰文件已格式化）。

## 5. 与简报偏差清单

1. **`OutboxPlan.upserts` 为 `&[OutboxUpsert]` 而非 `&[DocumentRef]`**：outbox 行
   `input_digest NOT NULL`，而 `DocumentRef` 不携带输入 digest；入队时无 digest 的
   embed 任务不可执行（"半个任务"）。收集体从 `document_manifest.record_json` 内
   `DocumentRecord.input.input_hash`（即 P6-003 `InputDigest`）解析；render 失败投影
   跳过入队。
2. **`OutboxPlan` 去掉 `space_id` 字段、改为 `active_space_on` 事务内解析**：简报的
   no-op 条件（无 active 行）本就要求读 `semantic_spaces`；由函数内解析使 cc-index
   调用点零知识、零 feature 分支（简报期望），并顺带把"多 active 行"变成 fail-stop
   点。双 active 空间是 P6-017 前不应出现的状态。
3. **`OutboxPlan.removals` 为 doc_key 字符串列表而非 `&[DocumentRef]`**：文件级删除
   （`to_remove` 路径）只有 `file_path → doc_key` 投影（`semantic_manifest.file_path`
   与 `document_manifest` 路径索引），无完整 `DocumentRef` 可用；supersede 只需
   doc_key。
4. **提交方式：EffectSet 驱动的逐效应 bump，而非把批事务改成 `UnitOfWork::commit_with`**
   ——简报"提交时用 `commit_with(EffectSet)`"。文件批事务是 `rusqlite::Transaction`
   （signature_agg/seed token span/社区恢复等专用流程），`UnitOfWork` 仅暴露类型化
   方法、持锁语义不同，改造属结构性重写且在性能敏感路径上。落法：同一事务内以
   `EffectSet` 为唯一声明源、逐声明效应 bump 恰一次（`bump_effects_on`），与
   `commit_with` 的提交语义逐位同构；`commit_with` 本身保持 P6-004 定案不动。
5. **删除路径 doc_key 解析同时覆盖 `document_manifest`**：简报语义只提
   `semantic_manifest`；但"已投影、未发布"的文档可能已有 pending 任务（其文件被删），
   仅查可见集合会漏 supersede，worker 会对已消失文档做功。两表并查（均在删除前、
   各有 file_path 索引）。
6. **全量重建路径（temp-db + swap / DirectWriter / `multi_insert`）不挂接 outbox**：
   重建写入全新 temp 库，`semantic_spaces` 在新库为空，挂接恒 no-op；重建后语义状态
   （空间/manifest/outbox）如何跨 swap 携带属 P6-011/P6-014 换库协议，本轮不裁决。
   `apply_config_link_units` 不挂接：config 文件单元无 `outcome.documents`。
7. **测试断言对唯一索引冲突的匹配放宽为 `UNIQUE constraint failed:
   semantic_outbox.doc_key`**：SQLite 对部分唯一索引的冲突消息报列名不报索引名，
   fail-stop 行为本身按简报要求固化。

## 6. 未做与剩余风险

- claim/lease 消费闭环（token 签发、lease 续期、retry 退避、ack fencing、attempt
  计数）全部归 P6-007；本轮只落表列 + `OutboxState`/`transition_state_on` 类型化守卫
  与 `ready_tasks_on` 读占位（红线遵守）。
- `revoke` op 无生产者（显式空间撤销归 P6-017）；`semantic_manifest` 生产写入方仍为
  零（发布 CAS 归 P6-011），本轮回滚测试中的 manifest 行为测试 fixture 手工种子。
- 写放大：supersede/撤销均为集合化 `IN` 语句（`IN_BATCH_SIZE` 分块），无逐行循环；
  `apply_file_batch_on` 无 active 空间时只付一条 `SELECT … WHERE state='active'`（空表
  主键扫描）。50k 量级批量写的实测属验证轮（V14 正式证据归收口）。
- `semantic_outbox_ready` 读方法尚无生产消费方（P6-007 queue 落地时首次使用）。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
