# P6-015 实施记录：崩溃恢复扫描（批次 3 收尾）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-015 节（crash 点表格、"恢复有界且可复算"、V17）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 151 行 P6-015 约束；
  第 60-62/104-107 行 "artifact durable 在先、manifest CAS 在后；任一 crash 点由幂等
  recovery 重放，重复 ack 被 fencing 吸收"；否决方向 "不引入独立队列服务、常驻进程
  或网络端点"）、批次 3 交付（P6-007 遗留 "reclaim_expired_on 无界扫描有界化归
  P6-015"；P6-011 §8 "CAS 后残态不可达 + 幂等重放吸收"；P6-013 drain/reclaim 口径
  与偏差 7 "P6-015 的有界化若给 reclaim 加 limit 参数，此调用点随之收窄"；P6-014
  reconcile 三步、偏差 4 "fenced retry 消耗 attempt 预算"、偏差 7 "desired 投影
  无行数上限归 P6-015"）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新测试文件；
  `crates/cc-semantic` 1 个新模块 + lib.rs 一行注册/边界文档一句更新 + 1 个新测试
  文件。**schema v22 零变更**；P6-005/006/007/008/009/010/011/012/013/014 交付物
  **只调用未改动**（含 `semantic_outbox.rs`/`semantic_queue.rs`/`reconcile.rs`/
  `semantic_rebuild.rs` 文件零触碰；`reclaim_expired_on`/`drain_pending` 原语义
  原样保留）；`tasks.json` status 未改；未 git commit；组合根接线未做
  （cc-server 零改动）；无隐式常驻进程（零线程/定时器/守护进程）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_recovery.rs` | 新增（212 行） | 有界恢复原语全量（§2）：`BoundedReclaim`(:35) + `reclaim_expired_semantic_bounded`(:81) + `semantic_dead_letter_count`(:143) + `semantic_rebuild_desired_set_bounded`(:168) |
| `crates/cc-db/src/lib.rs:53` | 修改 | `pub mod semantic_recovery;`（一行注册） |
| `crates/cc-db/tests/semantic_recovery.rs` | 新增（280 行） | 3 个集成测试（§4） |
| `crates/cc-semantic/src/recovery.rs` | 新增（333 行） | 恢复编排全量（§2/§3）：`RecoveryOptions`(:67，默认 scan_batch=64/lease 30s/backoff 1s/attempts 3) + `RecoveryReport`(:96) + `RecoveryVerdict`(:128) + `hand_back`(:149) + `publish_replay`(:173) + `recover_scan`(:206) + 模块文档 crash 点表映射 |
| `crates/cc-semantic/src/lib.rs:58` | 修改 | `pub mod recovery;`（一行注册）+ 边界文档补 P6-015 一句 |
| `crates/cc-semantic/tests/semantic_recovery.rs` | 新增（702 行） | 5 个端到端 kill 模拟测试（§4） |

## 2. 恢复编排原文（`recover_scan` 步骤与有界参数）

一次 `recover_scan(db, cache, space, doc_spec, expected_incarnation, options)` 调用 =
**1 次有界 reclaim（≤ scan_batch）+ 1 轮有界重放（≤ scan_batch 次 claim）+ 1 次死信
清点**，单次调用有界返回，调用方按 `RecoveryReport.converged` 循环驱动——零线程、
零定时器、零守护进程（ADR 红线）。

0. **fence 先行**（crash 表"换库 rename 中"行）：`semantic_incarnation_freshness`
   Stale → `RecoveryVerdict::Fenced{snapshot, path_incarnation}`，在任何事务前返回、
   零写入（幽灵进程的 reclaim/claim/publish 都只会落旧 inode，P6-014 机制复用）。
1. **孤儿甄别 = 有界过期回收**：`reclaim_expired_semantic_bounded(scan_batch)`——
   至多 `scan_batch` 条 `claimed 且 lease_expires_at < now` 的行回 `pending`
   （与冻结的 `reclaim_expired_on` 逐字段同一变换：state/available_at=now/lease 三列
   清空/attempt 不耗），页内 `task_id` 升序，页子查询使变异自推进（被回收行离开
   `claimed` 谓词，无需游标）；`exhausted` 在**同一 IMMEDIATE 事务内**判定
   （`(reclaimed, exhausted)` 是一个一致观测）。**孤儿口径**：进程死亡在本设计中
   唯一可观测信号就是 lease 过期（无 liveness registry、无进程表），因此"过期回收"
   即"孤儿回收"；未过期 lease 持有者视为存活，P6-007 结构上禁止回收。
2. **中断发布的有界重放吸收**：claim 循环（≤ scan_batch 次，附 P6-014 seen guard
   惯例——revisit 即 hand-back 并结束本轮）逐任务：
   - `cache.get` 命中（checksum + 四元组寻址 + 冻结 space 全链校验过）→
     `publish_replay` 走**完整 P6-011 五重 fence CAS**——artifact-before-manifest
     结构性成立（artifact 在原 publisher 崩溃前已 durable，重放无 put、无 provider，
     "已存 artifact 优先复用"）；`published=true` 计 `replayed`，其中
     `visible_set_changed=false`（Q4 等值吸收）计 `replay_absorbed`；
   - `Miss`/`Corrupt`（cache 半文件/checksum 失败，crash 表"artifact put 中"行）→
     fenced retry 交还 `pending`，worker 是唯一付费方；
   - `revoke` op → hand-back（P6-017 消费面）；
   - CAS 拒绝 → 不计 requeued 的 hand-back（拒绝已在 CAS 事务内写了自己的 fenced
     retry，行已 settle）。
   **"CAS 后、ack 前"残态不可达**（P6-011：manifest 写与 ack 同事务原子）；其
   "幂等重放吸收"由恢复侧验证承担：对等值已发布内容的重放产生
   `visible_set_changed=false`、零 bump（测试 2 原样固化）。
3. **死信清点**：`semantic_dead_letter_count()` 只读统计 active space 的终态
   `failed`。**死信口径**：failed 是终态，恢复扫描**只清点绝不复活**——attempt
   预算是有意耗尽的，静默重开会绕过费用策略（P6-018）；重置/补偿归费用策略与显式
   任务侧动作，不归恢复路径。
4. **收敛判定**：`converged = reclaim_exhausted && (队列本轮 claim 尽 || 本轮
   reclaimed==0 && replayed==0)`——"无操作轮"是稳态信号（正常 worker 积压不是
   crash 残态，不得让恢复循环空转）；`false` = 调用方继续驱动下一轮。

有界参数（`RecoveryOptions`）：`scan_batch`（reclaim 页与重放轮共享的单调用上限，
默认 64）、`lease_secs`（重放认领的租约长，默认 30s）、`retry_backoff_secs`（默认
1s）、`max_attempts`（默认 3，与 worker 共享行上计数）。

## 3. 与既有交付物的衔接（只调用，零改动）

- **claim/retry（P6-007/013 冻结原语）**：重放认领走 `claim_semantic` 门面、hand-back
  走 `retry_semantic_task`；状态机封闭迁移表零扩展；reclaim 的新有界变体是
  **同变换加页帽的新函数**，`reclaim_expired_on` 与其 `reclaim_expired_semantic`
  门面原样保留（红线"只做最小包装/新增有界变体"）。
- **publish CAS（P6-011，只调用）**：重放发布走 `IndexDb::publish_semantic` 完整
  五重 fence；`publish_replay` 与 `reconcile::publish_cached` 同构（本地副本，
  P6-014 文件不动）。
- **reconcile（P6-014，只调用）**：fence 判定复用 `semantic_incarnation_freshness`；
  seen guard 惯例保留；**半途 reconcile 续跑** = 残态恰是本模块吸收的形态（已重入队
  的 desired 任务 + 可能一个在 reuse 循环中被 kill 的 claim），recovery 以同一
  reuse-or-requeue 逻辑续完，付费向量全程只付一次（测试 5 原样固化）。
- **desired 投影有界化（P6-014 偏差 7 移交兑现）**：
  `semantic_rebuild_desired_set_bounded(after_doc_key, limit)`——同投影
  （`encoding_key IS NOT NULL`、render 失败跳过、P6-006 planner 口径），`doc_key`
  keyset 分页（无 OFFSET 重扫，与 `semantic_manifest_reads`/`uncovered_on` 同纪律）；
  原无界 `semantic_rebuild_desired_set` 原样保留。
- **coverage（P6-012，只调用）**：恢复效果经 `semantic_coverage` 只读验证（测试 5）。

## 4. 测试清单（8 个新增，全绿）

**`crates/cc-db/tests/semantic_recovery.rs`（3 个）**

1. `bounded_reclaim_pages_through_expired_leases_and_reports_exhaustion`(:112)——
   4 任务 3 过期（deterministic：lease 时限推入过去）：页 1 恰 2 行且
   `exhausted=false`（d3 仍过期）→ 页 2 恰 1 行 `exhausted=true` → 页 3 (0,true)；
   未过期 lease（d4）全程不被触碰；lease 清空、attempt 不耗（=1）；三钟全静
   （ReadGeneration 整体相等断言）；limit 0 拒绝。
2. `dead_letter_census_counts_only_the_active_space_failures`(:178)——未配置 = 0；
   预算耗尽 failed → 1；revoked 空间的死信不计入 active space 口径（仍 1）。
3. `desired_set_bounded_keyset_pagination_matches_the_unbounded_projection`(:238)——
   5 投影 2 不可嵌（render 失败 + encoding NULL）→ 无界 3 行；limit=2 三页
   （2+1+0），拼接 == 无界投影逐字段相等；limit 0 拒绝。

**`crates/cc-semantic/tests/semantic_recovery.rs`（5 个，kill 模拟端到端）**

kill 模拟的两半（测试头注释明文）：**guard 不处置地丢弃**（P6-013 定案的 Drop 语义
= 零 DB I/O，即进程死亡在库内的等价物：任务留在 claimed）+ **lease 时限推入过去**
（wall-clock 时间流逝的确定性等价物）。无真实子进程 SIGKILL——偏差 1。

4. `killed_worker_leases_reclaim_bounded_and_converge_without_paying`(:372)——
   5 worker 认领后全灭：`scan_batch=2` 驱动至 converged，**4 轮**多轮收敛
   （断言 `rounds >= 3`）、每轮 `reclaimed <= 2`、`total_reclaimed == 5`；
   全程 `provider.call_count()==0`（恢复零付费）；worker 续跑 drain 后
   `call_count()==5`（恰一次付费、无重复 embed）、manifest 5 行、任务全 done。
5. `crash_after_artifact_put_recovery_replays_publish_and_absorbs_duplicates`(:449)——
   崩溃点注入"put 后 CAS 前"：artifact durable、manifest 空、任务 claimed-过期 →
   recover `reclaimed=1, replayed=1, replay_absorbed=0, converged=true`，
   **provider 保持 1 次**（重放零新付费）、manifest 1 行、`semantic_epoch None→Some(1)`
   （恰一次可见变化）→ 等值任务再入队再 recover → `replayed=1, replay_absorbed=1`、
   epoch 仍 `Some(1)`（Q4 幂等吸收零双 bump）、manifest 仍单行、exact 检索自匹配
   score≈1.0 召回。
6. `dead_letters_are_counted_but_never_resurrected`(:539)——预算耗尽 failed →
   recover `dead_letters=1, reclaimed=0, replayed=0, converged=true`；任务保持
   failed、不可 claim（drain claimed=0）、provider 0 次、epoch 缺席（零钟动）。
7. `recovery_fences_the_pre_swap_ghost_with_zero_writes`(:584)——换库前快照作
   expected：recover → `Fenced{snapshot, path_incarnation≠snapshot}`；outbox 计数
   不变、manifest 0（幽灵零接触；继任进程已重入队 1 任务在先，可观测零写入）。
8. `interrupted_reconcile_is_completed_by_recovery`(:634)——**与 P6-014 的组合**：
   换库前 2 文档付费（call_count=2）→ rebuild（语义表从零、epoch 缺席、
   call_count 仍 2）→ 模拟半途 reconcile：重入队完成（epoch `None→Some(1)`）+
   reuse 循环首个 claim 被 kill → recover
   `reclaimed=1, replayed=2, requeued=0, converged=true`、**call_count 保持 2**
   （付费产物保留，续跑零新付费）、manifest 2 行、epoch `Some(3)`
   （1 重入队 [P6-006] + 2 可见变化 [Q4]）、coverage `eligible=2 published=2
   uncovered=0 failed=0`、exact 检索双文档自匹配召回。

## 5. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db -p cc-semantic --locked --offline
→ 24 个 "test result: ok."，0 failed。关键计数：
  cc-db lib 170 passed (+1 ignored)；semantic_recovery 3（新增）；
  semantic_publish 11、semantic_queue 2、semantic_lease 9、semantic_outbox 17、
  semantic_rebuild 3、semantic_coverage 3、semantic_schema 6 等既有套件零回归；
  cc-semantic lib 54；semantic_recovery 5（新增）；
  queue_worker 8、reconcile_rebuild 3、publish_cas 4、artifact_cache 17、
  manifest_exact_integration 4 全绿。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.10s（零 error）

cargo clippy -p cc-db -p cc-semantic --locked --offline --all-targets
→ 本轮新增 4 文件零 warning；报出的 warning 全部位于既有交付文件
  （cc-semantic lib = cache.rs 3 项 [P6-008 记录在案]；tests = artifact_cache/
  publish_cas/manifest_exact_integration 各 1 项与 reconcile_rebuild 的
  `digest_table` unused [P6-014 批次既有，本轮未触碰]）；cc-db 零 warning。
rustfmt --check（本轮新增 4 文件）→ 干净
```

## 6. 偏差清单（与简报草案 / 移交预期）

1. **测试落 `crates/cc-semantic/tests/semantic_recovery.rs` 且 kill 为进程内模拟，
   非简报的 `cc-eval/tests/semantic_lifecycle.rs` 真实子进程 SIGKILL**：任务指令
   明文"kill 模拟：lease 过期后 reclaim→恢复发布"；本实施以两个确定性等价物合成
   kill——guard 不处置丢弃（P6-013 Drop 定案 = 库内等价）+ lease 时限推入过去
   （时间流逝等价）。真实子进程 kill + fake provider fault-point 注入（简报风险段
   的 `crash_after_persist` 扩展）属 ports 冻结面变更，归验收轮 V17 正式证据。
2. **恢复编排落新模块 `recovery.rs` 而非简报归属的 `reconcile.rs`**：红线"不改既有
   交付物"，`reconcile.rs`（P6-014）文件零触碰；新文件同层级、同"编排 + 本地
   `publish_replay` 副本"先例（P6-011 落 `semantic_publish.rs` 同理）。
3. **有界原语落新模块 `semantic_recovery.rs`**：`reclaim_expired_on`（P6-007）与
   `semantic_queue.rs`（P6-013）零触碰，按红线"只做最小包装/新增有界变体"落平行
   有界变体（同变换 + 页帽 + 同事务 `exhausted` 判定）。
4. **`drain_pending` 的机会性 reclaim 未切换到有界变体**：P6-013 偏差 7 预告
   "此调用点随之收窄"，但那是对 P6-013 交付文件的行为性改动（红线不允许）；worker
   机会性 reclaim 维持原样，其单条 UPDATE 的量级风险不变（表量级 = 活跃任务量）。
   切换归组合根接线轮。
5. **recovery 重放对 miss 任务的 hand-back 消耗 attempt 预算**（P6-014 偏差 4 同
   口径）：冻结原语集中没有"清 lease 回 pending 不计 attempt"的通道；多轮 recovery
   反复重查同一 miss 任务会经 fenced retry 烧预算（测试 4 首版即暴露：默认预算 3
   在 4 轮收敛中把 miss 任务打成死信）。`converged` 的无操作轮判据使生产路径最多
   两轮触及 miss 任务，风险有界；"不计 attempt 的 hand-back 原语"列收口裁决点。
6. **dead_letter_count 为全量 COUNT**（与 P6-012 `coverage.failed` 同口径同实现）：
   只读聚合、单值返回，未做页帽——严格"有界扫描"语义下这是一个 O(死信数) 的读，
   如需页帽化归收口轮统一裁决（与 P6-012 是否加帽一致处理）。
7. **`RecoveryReport.converged` 的判据含"无操作轮"分支**（简报无此措辞）：miss-only
   残态下重放轮永远无法"claim 尽"（backoff=0 时 hand-back 即刻可再 claim），纯
   `queue_exhausted` 判据会让调用方空转；"本轮 reclaimed==0 && replayed==0"作为
   稳态信号补齐收敛性（测试 4 固化 4 轮收敛）。
8. **`semantic_rebuild_desired_set_bounded` 未回接到 `reconcile_after_rebuild`**：
   P6-014 偏差 7 的无界投影按红线原样保留，有界变体交付待接线轮让 reconcile 换用
   （或由收口轮裁决是否废弃无界版）。

## 7. 未做与剩余风险

- **组合根接线不做（红线）**：`recover_scan` 无任何 cc-server 调用点；"启动时 +
  worker 空闲触发"的简报节奏归接线轮（本模块已是显式调用方驱动形态，接线即
  `loop { if report.converged { break } }` + 触发时机决策）。
- **cache 半文件/tmp 的物理清扫**未做（crash 表"artifact put 中"行的读侧已覆盖：
  半文件表现为 Miss/Corrupt → 交还 worker；unlink 清扫归 P6-016 GC / P6-018
  quarantine）。
- 真实子进程 SIGKILL + provider fault-point 注入的 V17 正式证据归验收轮（偏差 1）。
- `reconcile_rebuild.rs` 的 `digest_table` unused warning 为 P6-014 批次既有
  （本轮未触碰该文件）；cc-semantic lib 3 个 cache.rs warning 为 P6-008 记录在案。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。

## 8. 批次 3 收口裁决点清单（移交）

1. **P6-014 偏差 5（重入队 bump 口径张力）**：`enqueue_semantic_rebuild_plan` 按
   P6-006 既有约定"非零 outbox stat 即 Semantic 效应"每次实际变化 bump；P6-012
   复申"epoch 只在可见集合变化时推进"。两口径在"期望集合变化但可见集合未变"窗口
   冲突（reconcile 测试 4 的 `Some(3)` 分解注释固化了当前归属）。裁决改"仅 manifest
   行集合"则只动 enqueue 门面内一个条件。
2. **recovery miss hand-back 的 attempt 预算消耗**（本记录偏差 5 / P6-014 偏差 4
   同根）：是否补"不计 attempt 的 hand-back 原语"（`claimed → pending` 直写、
   last_error 留痕、不耗预算），或接受现状并文档化"恢复重查烧预算是有界风险"。
3. **dead_letter_count / coverage.failed 的页帽化**（本记录偏差 6）：只读聚合是否
   需要与变异扫描同一"有界"标准。
4. **P6-013 偏差 3（claim 公平化 ORDER BY 注入）与偏差 7（drain 机会性 reclaim
   收窄到有界变体）**：均为 claim/worker 路径行为改动，归接线轮一并评审。
5. **有界 desired 投影的回接**（本记录偏差 8）：`reconcile_after_rebuild` 换用
   bounded 变体 + 无界版去留。
6. **kill 注入的 V17 正式化**（本记录偏差 1）：真实子进程 SIGKILL + fake provider
   fault-point（ports 冻结面扩展）归验收轮证据；当前工程级证据为进程内 kill 模拟。
