# P6-007 实施记录：claim 与 lease fencing

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-007 节（含 claim CAS SQL 草案与 fencing 不变式清单）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 143 行：短事务
  claim/renew/retry、每 attempt 独立 token、两进程不能同时持有同一 lease、过期
  worker 无法 ack 新 lease）、`artifacts/checkpoints/p6-batch2-20261002/
  P6-006-IMPLEMENTATION.md`（`transition_state_on` 封闭迁移表与 lease 四列 untouched
  基线——本轮即该 fenced domain 的填充）。
- 改动范围：`crates/cc-db` 1 个既有文件 + 1 个新测试文件。零 schema 变更（v22 冻结，
  `index_v1.sql`/`index_migrate.rs` 零改动，lease 四列本就在表内）。未改 `tasks.json`
  status，未 git commit，artifacts 冻结链未触碰。`cc-semantic` 未触碰（worker 封装
  归 P6-013，见偏差 5）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_outbox.rs:13-23` | 修改 | 模块文档：P6-007 scope boundary 段替换为 lease fencing 机制总述（CAS claim / 每 attempt token / token-only 判定 / reclaim 归 pending / Auxiliary） |
| `crates/cc-db/src/semantic_outbox.rs:92-111` | 修改 | `OutboxState::can_transition_to` 封闭表**显式扩展** `(Claimed, Pending)` 一条边（lease reclaim / 原子 retry 折叠，P6-007）；doc 同步 |
| `crates/cc-db/src/semantic_outbox.rs:472-493` | 新增 | `ClaimedTask`（task_id/token/doc_key/doc_version/input_digest/op/lease_expires_at） |
| `crates/cc-db/src/semantic_outbox.rs:496-548` | 新增 | `claim_next_on`——简报 CAS SQL 逐句落地：单条 `UPDATE ... WHERE task_id=(SELECT ... state='pending' AND available_at<=? ORDER BY task_id LIMIT 1) RETURNING ...`，同语句写 `state='claimed'` + 全部 lease 列 + `attempt_count+1`；token = `lower(hex(randomblob(16)))`；`lease_secs<=0` 类型化拒绝 |
| `crates/cc-db/src/semantic_outbox.rs:550-571` | 新增 | `fenced_lease_update`（私有）——renew/ack/retry 共享的 token fencing 骨架：`UPDATE ... WHERE task_id=? AND lease_token=? AND state='claimed'`，rowcount=0 → `Ok(false)` 零写入 |
| `crates/cc-db/src/semantic_outbox.rs:576-600` | 新增 | `renew_lease_on`——心跳：仅 token 持有者把 `lease_expires_at` 延至 `now+lease_secs`（心跳 deadline 即该列，schema 无独立心跳列）；不消耗 attempt |
| `crates/cc-db/src/semantic_outbox.rs:602-619` | 新增 | `ack_done_on`——token 持有者 `claimed → done`（封闭表边），终态，重复 ack / 陈旧 token 恒 `Ok(false)` |
| `crates/cc-db/src/semantic_outbox.rs:621-650` | 新增 | `retry_on`——单原子语句：`attempt_count >= max_attempts` → 终态 `failed`（死信），否则回 `pending` 且 `available_at = now+backoff`；两条路径都清空 lease 三列、持久化 `last_error` |
| `crates/cc-db/src/semantic_outbox.rs:653-667` | 新增 | `reclaim_expired_on`——第三方过期回收：`state='claimed' AND lease_expires_at<?now` 全部归还 `pending`（清 lease、`available_at=now`），不消耗 attempt；返回回收数（有界周期扫描编排归 P6-015） |
| `crates/cc-db/tests/semantic_outbox.rs:294` | 修改 | `state_transition_table_is_exact` 追加 `(Claimed, Pending, true)` 一组（17→18 组正反例） |
| `crates/cc-db/tests/semantic_lease.rs` | 新增 | 9 个 fencing 不变式集成测试（§3） |

## 2. 机制要点（与 P6-006 状态机的衔接）

1. **claim 是 CAS 单语句**（拒绝 SELECT-then-UPDATE 竞态窗口）：候选挑选、state
   翻转、lease 四列写入、attempt 递增全部在一条 `UPDATE ... RETURNING` 内。两进程
   竞争同一 pending 行时 SQLite 写串行化使恰一者胜出，其余得 `None`——"两进程同时
   持有同一 lease"在 SQL 层不可能（简报原文口径）。claim 只吃 `pending`
   （`available_at<=now`、按 `task_id` FIFO、按 space 过滤），过期回收职责分离给
   `reclaim_expired_on`（简报：claim 只吃 pending）。
2. **fencing 判定只认 token**：`renew/ack/retry` 全部走
   `WHERE task_id=? AND lease_token=? AND state='claimed'`；`claim_owner` 仅诊断，
   永不参与判定（防多进程同名，简报风险段要求）。rowcount=0 一律 `Ok(false)` 且
   零写入——过期 worker 对已被回收再被他人 claim 的任务，renew/ack/retry 全被拒。
3. **每 attempt 新 token**：token 由 claim SQL 内 `randomblob(16)` 现生成，renew
   不换 token，retry/reclaim 清空 token，下一次 claim 必产生新 token。"单调性"
   以等价机制达成（见偏差 3）：旧 attempt 持有者在 retry/reclaim/再 claim 生效的
   瞬间起，其 token 对一切推进语句永久失效。
4. **状态机不绕行**：claim 的 `pending→claimed`、ack 的 `claimed→done`、retry/
   reclaim 的 `claimed→pending` 与 `claimed→failed` 全部是 `can_transition_to`
   封闭表内的边；实现处 `debug_assert!` 边合法性，`transition_state_on` 本体保持
   P6-006 承诺不变（仍不触碰 lease 四列）。lease 列的写权唯一归属本轮五个函数。
5. **Auxiliary 效应**：claim/renew/ack/retry/reclaim 均不 bump 任何 epoch；
   `lease_lifecycle_is_auxiliary_zero_epoch_bumps` 经真实 `IndexDb` 断言三钟 +
   incarnation 逐项不变（V13 联动）。
6. **时钟纪律**：全部函数的 `now_unix` 由调用方传入，SQL 内不取时钟
   （`randomblob` 是唯一语句内非确定源，它正是 token 的来源）；`updated_at` 由
   `timestamp_text(now_unix)` 确定性派生。

## 3. fencing 不变式测试清单（`crates/cc-db/tests/semantic_lease.rs`，9 个，全绿）

1. `concurrent_claim_exactly_one_winner`（:107）——**双连接（同文件库、独立
   connection）竞争 claim 同一 pending 任务：恰一者 `Some`，另一者 `None`**；
   胜者行 `attempt_count=1`、`lease_expires_at=now+60`、owner 正确；重复 claim
   得 `None`；异空间任务不可跨 space claim。
2. `claim_skips_future_and_serves_oldest_with_identity`（:152）——future
   （`available_at>now`）/claimed 行全跳过、按 task_id FIFO；`ClaimedTask`
   逐字段（含 RETURNING token 与持久化 token 一致）。
3. `expired_lease_reclaimed_then_old_owner_fenced_out`（:181）——**过期 lease
   被第三方回收后原 owner 提交被拒**：未过期回收 no-op → 过期回收回 `pending`
   并清 lease 三列（attempt 不消耗）→ worker B claim 得**新 token**（≠A）→
   A 的 renew/ack/retry（旧 token）三者全 `Ok(false)` 零写入（行仍是 B 的
   token/claimed，`last_error` 未被污染）。
4. `wrong_token_and_owner_mismatch_are_fenced`（:224）——伪造 token 的
   renew/ack 全被拒；owner 不构成凭据。
5. `heartbeat_renews_and_lost_lease_heartbeats_fail`（:243）——**心跳续期延长**
   （1000+60 → 1030 续至 1090，attempt 不变）；lease 丢失（回收再 claim）后旧
   心跳失败、旧 ack 失败；现任持有者 ack 成功至终态。
6. `fresh_token_per_attempt_and_old_token_dead_on_retry`（:275）——**token 每
   attempt 更换且旧 token 立即失效**：claim → retry 回 pending（清 lease、
   backoff 生效、last_error 持久化）→ backoff 窗口内不可 claim → 到点后第二次
   claim 得不同 token、attempt=2 → 第一次的 token 对 ack/renew 全部失效。
7. `retry_exhaustion_dead_letters_to_failed`（:319）——**attempt 耗尽死信**：
   `attempt_count >= max_attempts` 时 retry → 终态 `failed`（lease 清空、
   last_error 持久化、available_at 不动）；failed 行不可 claim、不出现在
   ready 读、陈旧 retry 不得推进。
8. `superseded_task_unclaimable_and_unackable`（:349）——**与 P6-006 衔接**：
   写路径 supersede 已 claimed 任务后，原持有者的 ack/renew/retry 全被拒；
   superseded 行不重回 claim 竞争（claim 此后服务的是写路径新入队的替代任务，
   不同 task_id——合并不变式的正面证据）。
9. `lease_lifecycle_is_auxiliary_zero_epoch_bumps`（:386）——真实 `IndexDb`
   生产入队后，完整 lease 生命周期（claim/renew/retry/re-claim/ack/reclaim）
   前后 `read_generation()` 的 index_epoch/evidence_epoch/semantic_epoch/
   incarnation 逐项不变（V13 联动断言）。

另：`tests/semantic_outbox.rs::state_transition_table_is_exact` 扩至 18 组，
固化新增的 `(Claimed, Pending)` 边。

## 4. 验证命令与结果（原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test
   -p cc-db -p cc-index --locked --offline` → **passed=675 failed=0**
   （cc-db lib 157+1 ignored、`semantic_lease` 9、`semantic_outbox` 17（其中
   exact 表测试已扩至 18 组）、`semantic_schema` 6 及其余集成套件全绿；
   cc-index lib 377+1 ignored 及全部集成套件全绿）。
2. `cargo check --workspace --locked --offline` → `Finished ... in 18.64s`（零
   error/warning）。
3. `cargo clippy -p cc-db --locked --offline` → 零 warning（修复 2 处
   `neg_cmp_op_on_partial_ord` 后）。
4. `cargo fmt --check -p cc-db` → exit 0。

## 5. 与简报偏差清单

1. **`ClaimedTask` 增加 `op` 字段**（简报草案结构无）：worker 必须知道任务是
   embed 还是 revoke 才能行动，与 P6-006 `OutboxTask.op` 同口径。
2. **封闭迁移表显式扩展 `(Claimed, Pending)`**：简报明文要求
   "retry_on/后台扫描把 `state='claimed' AND lease_expires_at<?now` 归还
   pending"，该边在 P6-006 交付的表中不存在；选择扩展封闭表（并同步 exact 表
   测试）而非绕过。副作用：`transition_state_on(_, _, Claimed, Pending, _)`
   现为合法（仍只写 state/updated_at/last_error、不清 lease 列）；生产路径的
   该迁移由带 lease 清理的 `retry_on`/`reclaim_expired_on` 承担，claim 永远
   覆写 token，故经 `transition_state_on` 走该边留下的旧 token 无害（下个
   claim 必然替换）。
3. **"token 单调"实现为每 attempt 新随机 token + 旧 token 全语句失效**，无
   跨 attempt 单调计数列——schema v22 冻结（红线），且 `lease_token` 列注释即
   规定 `randomblob` 方案。等价保证：fencing 不依赖顺序比较，只依赖"新 attempt
   必换 token + 一切推进语句 WHERE token 匹配"。
4. **新增 `reclaim_expired_on`**（简报未给独立签名，但"后台扫描把 expired
   claimed 归还 pending"是其明文语义）：把该扫描的 DB 原语落在本模块；
   周期性/有界编排归 P6-015。
5. **`crates/cc-semantic/src/queue.rs`（LeaseGuard）本轮不落**：任务指令明确
   "本轮是 cc-db 层机制 + 测试"，worker 侧封装随 P6-013（worker 资源与合并）
   落地；cc-semantic 本轮零改动。
6. **`retry_on` 为单原子语句折叠 `claimed→pending|failed`**（CASE 按 attempt
   预算分叉），而非 `claimed→failed` + `failed→pending` 两步——两步在自动提交
   连接上有中间 `failed` 残态窗口；两条边均在封闭表内，语义等价且原子。
7. **死信口径**：attempt 耗尽 → 终态 `failed`（本批不可经 claim 复活；P6-006
   的 `failed→pending` 类型化迁移保留给显式 requeue 场景）；lease 过期回收
   永远回 `pending`、不判死信（简报口径：过期回收路径 = 归还 pending）。
8. **未加 IndexDb 写门面**（claim/renew/ack 的 `IndexDb::writes()` 侧方法）：
   简报签名全部是 `&Connection` 自由函数（`*_on` 模式），P6-006 也只落了
   ReadOps 读门面；写门面随 P6-013 worker 消费方（首个生产调用方）一起落，
   避免无消费方的门面膨胀。
9. **测试期内修正 2 处测试断言口径**（非实现缺陷）：backoff 边界按
   `available_at<=now` 语义在到点刻即可 claim（首测误设不可 claim）；supersede
   后 claim 服务的是写路径新入队的替代任务（首测误断言无任务可 claim）。

## 6. 未做与剩余风险

- worker 进程/调度/heartbeat 循环（`lease_secs/3` 周期 renew）、admission、
  合并消费归 P6-013；真实 kill/restart 的 lease 残态恢复矩阵归 P6-015。
- 五重 fencing 中的 incarnation/doc_version/input/space 校验属 publish CAS
  （P6-011）；本批只固化 lease token 一重 + at-least-once 语义下"过期结果不得
  推进状态"的 outbox 层保证（publish 侧兜慢结果，简报明文）。
- `reclaim_expired_on` 无行数上限（当前语义 = 一次回收全部过期行）；P6-015
  要求有界扫描时需加 limit 参数（表量级 = 活跃任务量，非 50k 主表，风险低）。
- 双连接竞争测试在 rollback-journal 模式下依赖 `busy_timeout`（5s）串行化；
  WAL 模式下的多进程竞争行为已由同机制覆盖（单语句 CAS 与 journal 模式无关）。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
