# P6-017 实施记录：model space 切换规划

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-017 节（三段设计/双空间并存/回滚复用原文，引用见下）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 153 行约束表
  "新空间回填/切 active/撤销三段；不同空间分数永不混排；旧 cache 经校验可回滚
  复用"）、批次 1-4 交付（P6-005 `semantic_spaces` 三态表、P6-006 outbox
  `op='revoke'` 枚举（生产者悬空）+ `supersede_live_on` doc_key 级 supersede、
  P6-011 publish CAS fence 5 "space 必须是唯一 active"、P6-013 EmbedHandler
  拒绝 revoke op + `semantic_queue.rs:17-18` "revoke-task consumer (P6-017)
  adds its own ack path"、P6-014 reconcile 复用路径、P6-016 GC "非 active space
  manifest 行仍保护对象"）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新测试文件；
  `crates/cc-semantic` 1 个新模块 + 1 个新测试文件 + lib.rs 一行注册。
  **schema v22 零变更**（切换事件载体走 metadata 键，不加表/列）；P6-002~016
  交付文件零触碰；`tasks.json` status 未改；未 git commit；组合根/配置触发面
  未做（归接线轮，本轮是库层协议）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_space_switch.rs` | 新增（716 行） | 模块文档（三态机/单事务切换/revoke 第二消费方/回滚复用契约/审计与未 pin 声明）+ `SWITCH_LOG_KEY`(:89) + `SpaceState`(:93) 与闭转换表 `can_transition_to`(:125) + `SpaceSwitchEvent`(:138，`pinned:false` 字段) + `SpaceSwitchStats`(:148)/`RevokeOutcome`(:175) + `space_state_on`(:194) + `register_space_on`(:217，拒任意状态已存在行) + `append_switch_log_on`(:241，metadata 读改写) + `supersede_space_live_tasks_on`(:272，按空间批 supersede) + `switch_active_space_on`(:308，调用方事务内五步) + `enqueue_backfill_on`(:403，`backfilling` 状态守卫) + `live_revoke_count_on`(:471) + `consume_revoke_on`(:491，删行+fenced ack 同事务) + `impl IndexDb`(:512)：`register_semantic_space`(:516)/`enqueue_semantic_backfill_plan`(:538)/`switch_semantic_active_space`(:579)/`claim_semantic_space`(:619)/`consume_semantic_revoke`(:653)/`live_semantic_revokes`(:703)/`semantic_space_state`(:709) |
| `crates/cc-db/src/lib.rs:52` | 修改 | `pub mod semantic_space_switch;`（一行注册） |
| `crates/cc-db/tests/semantic_space_switch.rs` | 新增（436 行） | 10 个测试（§4） |
| `crates/cc-semantic/src/space_switch.rs` | 新增（213 行） | 模块文档（三段协议/模块归属偏差/未 pin 限制/无常驻进程）+ `space_spec_json`(:51，冻结 canonical 序列化) + `register_backfill_space`(:60)/`enqueue_backfill`(:73)/`activate_space`(:84) + `RevocationDrainReport`(:96) + `drain_space_revocations`(:116，显式驱动 revoke 消费) + 2 个内联单测（spec_json 稳定性） |
| `crates/cc-semantic/src/lib.rs:67` | 修改 | `pub mod space_switch;`（一行注册） |
| `crates/cc-semantic/tests/space_switch.rs` | 新增（500 行） | 2 个端到端测试（§4） |

## 2. 切换协议与三态守卫（原文对照）

**简报 P6-017 原文（三段）**：

> 1. **回填**：注册新 space 行（`backfilling`），全量文档按新 space 入 outbox；
>    期间 dense lane 只读 active space——`semantic_manifest_space` 索引 + lane
>    查询固定 `space_id = active`，**不同空间分数永不混排**在读取层结构性保证。
> 2. **切 active**：单短事务 `UPDATE semantic_spaces SET state='active' WHERE
>    space_id=new; UPDATE ... state='revoked' WHERE space_id=old`（Semantic 效应，
>    bump semantic_epoch——可见集合切换）。
> 3. **撤销/回滚**：旧 space 行 `revoked` 但 cache 产物保留；回滚 = 再次切换指回
>    旧 space，`cache.get` 校验通过即复用，缺失部分才重新回填。

**落点**：

- **三态闭转换表**（`semantic_space_switch.rs:125`）：`backfilling→active`、
  `active→revoked`、`revoked→active`（回滚边）恰三条；`revoked` 永不回
  `backfilling`（撤销空间的未完成回填在 re-activate 后直接续跑，见模块文档）。
  与 outbox `OutboxState` 同纪律：非法转换即调用方 bug，fail loudly。
- **第 1 段**：`register_backfill_space`（spec validate → digest → 冻结
  `spec_json`）+ `enqueue_backfill_on`（守卫：目标行必须恰为 `backfilling`；
  P6-006 写路径固定 active 空间、不可能给非 active 空间产任务，故回填计划
  自带守卫写入器）。并存窗口语义由既有交付物结构性保证：
  `IndexDb::claim_semantic` 只认领 active 空间（`semantic_queue.rs:65`）、
  `scan_space` 固定 space_id（`semantic_manifest_reads.rs:80`）——本任务零改动，
  由 e2e 断言验证（切换前 B 回填任务零消费、provider 零调用）。
- **第 2 段**：`switch_active_space_on`（:308）在调用方事务内按序五步：
  ① 旧 active → `revoked`（守卫 `Active→Revoked`，rowcount≠1 即并发移动、
  fail）→ ② 旧空间**全部** live 任务 supersede（revoked 空间的 pending embed
  本来就会被 CAS fence 5 拒绝，supersede 省下这笔必浪费的付费 embed；同时
  清掉同空间遗留 live revoke 任务，避免撞 `semantic_outbox_live_per_doc`
  唯一索引）→ ③ 目标 → `active` + `activated_at`（守卫
  `backfilling|revoked→active`）→ ④ **revoke 生产者**：`INSERT..SELECT` 每
  旧空间 manifest 行一个 `op='revoke'` pending 任务（集合化，禁止逐行——
  P6-006 风险注）→ ⑤ 追加审计事件。已 active 目标的重复切换是幂等 no-op
  （`activated=false`，零写入）。
- **第 3 段**：撤销 = `drain_space_revocations`（显式驱动，认领显式空间）；
  回滚 = 再次 `activate_space`（`revoked→active` 边）+ 照常 P6-014 reconcile
  （desired 重推 → `cache.get` 校验复用 → 缺失才回 worker 付费回填）。

**tasks.json step "记录用户 revision 与未 pin 限制"**：schema 红线禁加列，
载体取 metadata 键 `semantic_space_switch_log`（追加式 JSON 数组，切换事务内
读改写，`append_switch_log_on`）。每次有效切换记录
`{at, from, to, revision, pinned:false}`——`pinned:false` 即"未 pin = 不保证
跨版本行为"的机器可读声明。切换是罕见人工操作，本层不设保留策略（记录于
模块文档）。

## 3. revoke 生产者/消费者链路

**生产者**（P6-006 悬空 op 的归属收口）：唯一生产点是切换事务第 ④ 步；
`semantic_outbox.rs:237` 既有注释 "revoke tasks are only produced by explicit
space revocation (P6-017), never by this function" 现在有了对应实现。

**消费者**（`semantic_queue.rs:17-18` 预留的"第二个生产消费者"）：

```
claim_semantic_space(space_id=旧空间)      -- Auxiliary，按显式空间认领
  → renew（活性门，lost → 跳过零写入）
  → op≠Revoke → fenced retry 交还（防御：撤销窗口队列只应有 revoke）
  → consume_semantic_revoke                -- 一个 IMMEDIATE 事务：
       DELETE FROM semantic_manifest WHERE doc_key=? AND space_id=?
       + ack_done_on（fenced claimed→done）
       -- ack=false（lease lost）→ 整事务 ROLLBACK，删行不存活
       -- 删行数>0 → bump semantic_epoch（Q4：恰在可见集合变化时）
```

own-space 守卫（`AND space_id`）保证已在新空间重新发布的 doc 行绝不会被旧
空间的 revoke 任务误删。可见集合变化 bump 走既有 CAS/Q4 口径：与
`publish_semantic`(:368) 完全同构——facade 在 `visible_set_changed` 为真时
才声明 Semantic 效应。

**epoch 口径汇总**（守卫测试断言）：注册（Auxiliary，不动钟）→ 首次激活
（可见集合空→空，不 bump，`semantic_epoch` 键保持 absent="not ready"，P6-004
红线）→ 回填入队（队列状态实际变化 → bump，随已交付
`enqueue_semantic_rebuild_plan` 的 P6-006 约定"nonzero stat = semantic
change"）→ 切换（可见集合切换 → bump）→ 每次删行的 revoke 消费（删行才
bump，幂等重放不 bump）。

## 4. 测试证据（原样）

**`cargo test -p cc-db -p cc-semantic --locked --offline` 全绿**（cc-db 173+13+
… 全部通过；新文件贡献如下）：

```
running 10 tests (crates/cc-db/tests/semantic_space_switch.rs)
test space_transition_table_admits_exactly_the_three_edges ... ok
test register_inserts_backfilling_and_refuses_any_existing_row ... ok
test switch_requires_known_space_and_legal_target_state ... ok
test first_activation_has_no_previous_active_and_no_epoch_bump ... ok
test switch_revokes_old_activates_new_and_produces_revoke_tasks ... ok
test switch_appends_pinned_false_audit_event_to_metadata_log ... ok
test re_switch_of_a_revoked_space_supersedes_stale_live_revokes_before_reenqueue ... ok
test consume_revoke_deletes_own_space_row_and_bumps_exactly_on_change ... ok
test consume_revoke_never_touches_another_space_row_and_loses_to_stale_token ... ok
test space_switch_facades_end_to_end_epoch_and_states ... ok

test result: ok. 10 passed; 0 failed
```

```
running 2 tests (crates/cc-semantic/tests/space_switch.rs)
test revoked_space_objects_become_gc_eligible_after_revoke_drain ... ok
test switch_end_to_end_new_embeds_old_revokes_retrieval_sees_only_new_then_rollback_reuses ... ok

test result: ok. 2 passed; 0 failed
```

**端到端关键断言**（`space_switch.rs:299` 起，FakeProvider 计数精确）：

- 并存窗口：`enqueue_backfill` 后 `outbox_count(B,'embed','pending')==2`；
  再次 worker drain → `provider_a.call_count()` 不变（2），B 回填任务零消费；
- 切换：`switch.activated && old_revoked && visible_set_switched`、
  `revoke_tasks_enqueued==2`、epoch 恰 +1（"可见集合切换 → 恰一次 bump"）；
- 撤销：`drain == (claimed:2, revoked:2, visible_changes:2)`、
  `manifest_count()==0`、`outbox_count(A,'revoke','done')==2`；
- 新空间 embed：`provider_b.call_count()==2`（恰好按任务数付费）；
- **检索只见新空间（V16）**：B 命中 2 文档；A 的 cache 产物虽在、
  `run_search(A)==[]`（"旧空间 revoked 后检索为空（分数永不混排）"）；
- **回滚复用（V17）**：切回 A + `enqueue_semantic_rebuild_plan` +
  `reconcile_after_rebuild` → `reused==2, requeued==0`、
  `provider_a.call_count()+provider_b.call_count()` 前后相等
  （"回滚路径 provider 调用必须为 0"）、A 检索恢复 2 命中；
- **revoke 后 GC 衔接（P6-016 判据反向）**：revoke 消费前过期 GC
  `deleted_objects==0`（manifest 行保护），消费后同一过期 pass
  `deleted_objects==1` 且 `cache.get(A,…)==Miss`。

## 5. 切换事务性与回滚语义

- **切换原子性**：状态翻转、旧空间任务 supersede、revoke 生产、审计追加在
  同一 `IMMEDIATE` 事务；任一步失败整体 ROLLBACK（facade 三分支与
  `publish_semantic` 同构），不存在"半切换"状态。epoch bump 在事务内、
  仅 `visible_set_switched` 时执行。
- **中途失败回滚语义**：事务级失败=零变化（库保持切换前一致态，可安全
  重试）；**业务级回滚**（切换已提交后放弃新空间）= 简报 step 3：再次切换
  指回旧空间（`revoked→active` 边，e2e 已验），desired 重推后 P6-014
  reconcile 复用旧 cache（provider 0 次），缺失部分回 worker 回填。
- **旧空间产物保留窗口**：`revoked` 行本身不删任何 cache 产物。revoke 消费
  前，旧 manifest 行受 P6-016 mark 保护（全 space，含非 active）——回滚窗口
  内对象绝不丢；revoke 消费删行后对象成为孤儿，过期 GC 可回收（衔接测试
  已验）。回滚若发生在 GC 回收之后，`cache.get` Miss → worker 重嵌（简报
  "缺失部分才重新回填"的字面语义），无正确性风险。

## 6. 偏差清单

1. **模块归属**：简报归属 `spec.rs + reconcile.rs`；实际落新模块
   `cc_db::semantic_space_switch` + `cc_semantic::space_switch`。理由=批次
   红线"不改既有交付物（只调用/组合）"，先例=P6-016 sweep 落 `gc.rs` 的
   同款偏差注记；`reconcile.rs` 对 revoke op "不在此消费"的既有行为保持
   不变。
2. **首次激活不 bump**：简报 step 2 括注 "Semantic 效应，bump semantic_epoch"
   未区分是否有旧 active；实现取 `visible_set_switched = 旧 active 存在且
   ≠目标`——首次激活可见集合空→空，不 bump，`semantic_epoch` 保持 absent
   （P6-004 "None = not ready" 红线优先）。有旧 active 的切换严格按简报
   bump。
3. **回填入队 bump**：简报未规定回填入队的 epoch；实现随已交付
   `enqueue_semantic_rebuild_plan` 的 P6-006 约定（nonzero stat → bump）。
   端到端断言依赖该基线并注明。
4. **审计载体**：简报 "spec_json 旁的变更记录（实现载体：metadata 追加键或
   表列，实施评审定）"——schema 红线禁列，取 metadata 键
   `semantic_space_switch_log`；追加式数组无保留策略（切换为罕见人工操作，
   记录于模块文档，接线轮可按需收口）。
5. **发现（不改，既有交付物语义）**：P6-006 `supersede_live_on` 按
   `doc_key` **跨空间** supersede。回滚重推会撤销尚未消费的新空间 revoke
   任务；一致性成立的前提是"重推必然经 CAS upsert（doc_key 主键）覆盖同
   doc 的 manifest 行"，e2e 第 8 步已验证（B revoke 任务 superseded、
   manifest 无 revoked 空间残留）。若未来出现"重推不改行"的路径需复查该
   前提。
6. **V16/V17 证据归档**：tasks.json deliverables 要求
   `artifacts/benchmarks/<run-id>/` 证据（实施时生成）；本任务以测试证据
   （§4 原样）+ 本记录代替，benchmark 归档轮未在本轮范围内，未生成。

## 7. 验证命令与结果

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db -p cc-semantic --locked --offline   # 全绿（0 failed）
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline              # Finished, 无错误
```

## 8. 剩余风险 / 注意事项

- `claim_semantic_space` 是按显式空间认领的通用原语：调用方传入非 revoked
  空间属合法（如诊断），误用面由调用方约束；revoke 消费事务内的 own-space
  守卫保证了即使认领错空间也不会误删行。
- `semantic_space_switch_log` 为无界追加（单键 JSON 数组）；如需上限归接线轮。
- 撤销 drain 的 `max_batch` 有界、收敛靠显式多轮驱动（同 `queue`/`gc`
  纪律）；无常驻进程。
