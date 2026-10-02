# P6-014 实施记录：换库 incarnation 与缓存重用

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-014 节（换库侧 fencing 专项 + 补齐侧 `reconcile.rs` 三步协议）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 150 行约束；
  第 120-124 行 fencing 身份链与 strict ReadGeneration；第 55-57 行"换库改变
  incarnation……旧 cache 在完整校验后可复用（付费产物保留）"）、用户 Q5 决策
  （**cache namespace 不含 incarnation**——重建解析同一 namespace，付费向量经
  `(space, input digest, spec)` 校验直接复用，不重嵌入；本任务是兑现点）、
  批次交付（P6-011 publish CAS 五重 fence + §8 移交"跨进程旧 inode 场景的
  专项测试归 P6-014"；P6-006 偏差 6"全量重建路径不挂接 outbox"移交本任务；
  P6-013 EmbedHandler 的 call_count 断言面；批次 1 的 incarnation/epoch 迁移
  向量 `finalize_rebuild_generation`）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新测试文件；
  `crates/cc-semantic` 1 个新模块 + lib.rs 注册/边界文档更新 + 1 个新测试文件。
  **schema v22 零变更**；P6-005/006/007/008/009/010/011/012/013 交付物**只调用
  未改动**（含 `semantic_publish.rs`/`semantic_queue.rs`/`semantic_outbox.rs`/
  `index_db_rebuild.rs` 文件零触碰）；`tasks.json` status 未改；未 git commit；
  组合根接线未做（cc-server 零改动）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_rebuild.rs` | 新增（263 行） | 协议侧全量（§2）：`IncarnationFreshness`(:82) + `generation_at_path`(:100，权威路径 fresh 只读连接 strict ReadGeneration) + `semantic_incarnation_freshness`(:112) + `publish_semantic_fenced`(:133) + `claim_semantic_fenced`(:151) + `semantic_active_space`(:166) + `semantic_rebuild_desired_set`(:180，desired 投影 + `RecordInputPeek` :68) + `enqueue_semantic_rebuild_plan`(:229，IMMEDIATE 短事务 + 条件 Semantic bump) |
| `crates/cc-db/src/lib.rs:52` | 修改 | `pub mod semantic_rebuild;`（一行注册） |
| `crates/cc-db/tests/semantic_rebuild.rs` | 新增（316 行） | 3 个集成测试（§4） |
| `crates/cc-semantic/src/reconcile.rs` | 新增（287 行） | 补齐侧全量（§3）：`ReconcileOptions`(:65) + `ReconcileReport`(:87) + `ReconcileVerdict`(:106) + `reconcile_after_rebuild`(:152) + `publish_cached`(:265) |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod reconcile;`(:50) + 模块边界文档补 P6-014 段 |
| `crates/cc-semantic/tests/reconcile_rebuild.rs` | 新增（570 行） | 3 个端到端集成测试（§4） |

## 2. 换库侧协议（重建协议原文与落点）

简报 P6-014 换库侧原文：**"既有协议已在 swap 时 `renew` incarnation
（read_generation.rs:41-48）与 finalize epoch 向量（index_db_rebuild.rs:254-261）。
本任务补：rebuild 完成后旧进程（持有旧连接）的 claim/publish 全部因 incarnation
fencing 失败——P6-011 第 2 步已覆盖，本任务只加专项测试（另一 IndexDb 实例指向
旧 inode/旧路径的 publish 被拒）"**。

**关键发现（测试先行暴露，实施记录如实记载）**：P6-011 fence 1 校验"本连接读到的
incarnation == 快照"，在**同文件 swap** 场景完备（swap 会重开本实例连接）；但
**另一进程持旧 inode** 的场景下，该进程自己的池连接读到的永远是旧 incarnation
（rename 后旧 fd 指向被摘链的 ghost 文件），fence 1 在 ghost 上恒通过——裸
`publish_semantic` 会把结果写进幽灵文件，权威库毫发无损但旧进程却以为发布成功。
P6-011 §8 已记录该覆盖边界并移交本任务。因此本任务新增的权威判定读的是
**权威路径上新开的只读连接**（`generation_at_path`，semantic_rebuild.rs:100）：
路径即权威，fresh open 不可能是 ghost；strict `ReadGeneration` 原语零复用偏差
（绝不走 legacy 双钟，ADR 第 120-124 行）。

- `publish_semantic_fenced`(:133)：Stale → 在任何事务开始前返回
  `IncarnationMismatch` 拒绝，**零写入**（对 ghost 写 fenced retry 只会把队列
  状态丢进旧 inode——对 P6-011 "拒绝也提交 retry" 语义的显式偏差，已注释）；
  Current → 原样转发 `publish_semantic`（五重 fence 完整保留）。
- `claim_semantic_fenced`(:151)：Stale → `Ok(None)` 零写入（"此进程无权再做
  功"）；文档明示 `Ok(None)` 双义（空队/fenced），需区分时直接问 freshness。
- epoch 语义（简报"新 incarnation 的 semantic_epoch 起点按简报/ADR"）：批次 1
  的 `finalize_rebuild_generation` 只写 index/evidence 向量 + renew incarnation，
  staging 库 metadata 为空 → 重建后 `semantic_epoch == None`（缺席 = 语义未就
  绪，绝不当作 0；ADR 第 52 行），首个可见集合变化才写 1。测试固化（§4 第 3/6
  条）。
- 重建从零协议：staging 库 `semantic_spaces` 为空 → manifest/outbox/spaces 都
  不跨 swap 携带；组合根在 swap 后重新注册 active space（测试中以
  `activate_space` 扮演）。

## 3. 补齐侧协议（reconcile.rs 三步，简报原文与落点）

简报 P6-014 补齐侧原文：**"`reconcile_after_rebuild(cache, db)`：
1. 从 `document_manifest × active space` 重导 desired 集合（等价全量 upsert 的
outbox 计划）；2. 对每个 `input_digest` 先 `cache.get`——命中且校验过 → 直接走
P6-011 CAS 写 manifest（不付费、不调 provider，验收'索引重建不误删已付费向量'）；
3. 未命中 → 留 pending 任务给 worker。"**

落点 `crates/cc-semantic/src/reconcile.rs::reconcile_after_rebuild`(:152)：

0. **fence 先行**：`semantic_incarnation_freshness(expected)` Stale →
   `ReconcileVerdict::Fenced`，零写入（幽灵进程连 desired 投影都不做）。
1. **desired 重导**：`IndexDb::semantic_rebuild_desired_set`（cc-db 侧投影，
   `encoding_key IS NOT NULL` 行取 `record_json.input.input_hash`，render 失败
   行跳过 = P6-006 planner 口径）+ `semantic_active_space` 校验（无 active →
   Applied 空报告 = 语义未配置；非本 space → 参数错误，比 N 次 fence 5 拒绝更
   早说清楚）。
2. **重入队（P6-006 偏差 6 移交兑现）**：`IndexDb::enqueue_semantic_rebuild_plan`
   一个 IMMEDIATE 短事务跑冻结的 `supersede_and_enqueue_on` + 仅当
   `changed_semantic_state()` 时 bump `semantic_epoch`（P6-006 效应口径：非零
   stat 即语义状态变化，与增量写路径 `bump_effects_on` 同一约定）。重建本身
   依旧不挂接 outbox（staging 空间表使挂接恒 no-op，红线未动
   `index_db_rebuild.rs`）。
3. **复用或回队**：claim → `cache.get(space, input, spec)`：
   - `Hit`（checksum + 四元组寻址 + 冻结 space 全链校验过）→ `publish_cached`
     (:265) 直接发 P6-011 五重 fence CAS——artifact-before-manifest 由"已验证
     get 在 CAS 之前"结构性成立（artifact 本就 durable，无 put、无 provider）；
     Q4 visible-set diff 照常判定，`visible_changes` 只计真变化；
   - `Miss`/`Corrupt` → fenced retry 留回 `pending`（worker 是唯一付费方）；
   - `revoke` op → 留回 pending（P6-017 消费面）。
   `seen` guard 保证回队任务（backoff=0 时可被本循环再 claim）不造成死循环：
   revisit 即 hand-back 并结束本轮。
- **Q5 决策的兑现**（模块头文档原文落点）：namespace 不含 incarnation →
  重建后组合根用同一 namespace 打开 cache，`cache.get` 按
  `(space, input digest, spec)` 全链校验命中即复用——这是"重建 0 provider 调
  用"成立的机制前提。

## 4. 测试清单（6 个，全绿）

**`crates/cc-db/tests/semantic_rebuild.rs`（3 个）**

1. `pre_swap_process_is_fenced_out_of_publish_and_claim_after_the_swap`(:116)
   ——**跨进程旧 inode 专项（P6-011 §8 移交兑现）**：实例 A 开库、claim 任务、
   快照 I1；实例 B 同路径完整 rebuild → I2。断言链：A 池连接仍读 I1（幽灵分
   叉成立，:143-146）→ freshness 报 `Stale{snapshot, path_incarnation}`(:148)
   → `publish_semantic_fenced` 拒绝 `IncarnationMismatch`（:169-175，拒绝发生
   在任何事务之前）→ `claim_semantic_fenced` → `Ok(None)` → **权威库（B 视角）
   manifest/outbox 计数 0、semantic_epoch 缺席**（:181-186）→ B 自身
   freshness == Current（fence 对正常进程透明）。
2. `enqueue_semantic_rebuild_plan_requeues_desired_set_and_bumps_semantic_once_per_change`(:205)
   ——重入队门面：无 active space 整门面 no-op 且零钟动（默认路径零付费）→
   首次重入队 `enqueued=1`、`semantic_epoch None→Some(1)`、index_epoch 不动
   （重入队不属 Index 效应）→ 重复重入队 supersede+insert 收敛为恰一个活跃
   任务。
3. `rebuild_epoch_vector_advances_and_semantic_epoch_starts_absent`(:289)
   ——批次 1 迁移向量的语义表扩展断言：incarnation 必变、index/evidence 各
   `> before`、**`semantic_epoch` 起点 = 缺席 None**、manifest/outbox/spaces
   全部从零、document 投影由重建闭包恢复。

**`crates/cc-semantic/tests/reconcile_rebuild.rs`（3 个，全链 + 付费断言）**

4. `rebuild_reconcile_reuses_paid_vectors_with_zero_provider_calls`(:323)
   ——V17 核心闭环：换库前 worker 正常发布 2 文档（`provider.call_count()==2`，
   :338）→ rebuild（`call_count` 仍 2，"换库本身零付费"，:352；epoch 起点
   None）→ 重注册空间 → reconcile →
   `Applied{desired:2, enqueued:2, reused:2, requeued:0, visible_changes:2}`，
   **`assert_eq!(world.provider.call_count(), 2, "重建路径对 cache 命中的输入
   必须 0 provider 调用（付费产物保留）")`**（:379）→ manifest 恢复 2 行、
   `semantic_epoch Some(3)`（1 重入队 [P6-006 口径] + 2 可见变化 [Q4]）→
   P6-012 读面 `semantic_coverage`：eligible=2, published=2, uncovered=0,
   failed=0, stale=0（:391-402）→ filtered exact 检索自匹配 score 1.0 双文档
   召回（复用发布走完整可见路径）。
5. `cache_miss_inputs_are_left_pending_and_embedded_once_by_the_worker`(:407)
   ——cache Miss 才付费：换库后新文档 d2 从未嵌入 → reconcile
   `Applied{reused:1, requeued:1}`（d1 复用、d2 留 pending）、`call_count` 仍
   1（"reconcile 自身零付费"，:441）→ worker drain 后 **`call_count()==2`**
   （"worker 只对 cache Miss 的输入调用 provider（恰 +1）"，:451）、d2
   `done`、coverage published=2/uncovered=0。
6. `pre_swap_process_is_fenced_and_the_successor_recovers_from_cache`(:464)
   ——幽灵端到端：换库前发布的付费文档 + 幽灵实例（旧快照）→ 幽灵 reconcile
   `Fenced{snapshot, path_incarnation}`（:499）→ 幽灵 `publish_semantic_fenced`
   拒绝 `IncarnationMismatch`（:523-530）→ **权威库 outbox/manifest 计数 0**
   （幽灵零接触）→ 继任进程（新快照）reconcile `Applied{reused:1}`、
   `call_count()` 保持 1（"继任进程复用付费向量，0 新 provider 调用"，:565）、
   manifest 恢复。

## 5. 与既有交付物的衔接方式（只调用，零改动）

- **rebuild 协议（既有，零触碰）**：`rebuild_with_temp_db` →
  `run_rebuild_protocol` → `finalize_rebuild_generation`（max(floor,live)+1 +
  `read_generation::renew`）照原样；本任务所有 fence 都构建在其产物（新
  incarnation 落在权威路径）之上。
- **publish CAS（P6-011，只调用）**：复用发布走 `IndexDb::publish_semantic`
  完整五重 fence；fenced 变体在其前加权威路径新鲜读，不绕过、不复制 fence
  逻辑。
- **outbox/lease（P6-006/007/013，只调用）**：重入队 = 冻结的
  `supersede_and_enqueue_on`；claim/retry = P6-013 门面原语；状态机封闭迁移表
  零扩展。
- **cache（P6-008，只调用）**：复用 = `cache.get` 既有校验链（format_version
  → 寻址三元组 → blake3 checksum → 维度/模型 → 有限性）；Q5 namespace 决策
  未改动任何 cache 代码。
- **coverage（P6-012，只调用）**：重建后覆盖率恢复经
  `db.reads().semantic_coverage()` 只读验证。

## 6. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db -p cc-semantic -p cc-index --locked --offline
→ 31 个 "test result: ok."，0 failed。关键计数：
  cc-db lib 170 (+1 ignored)；semantic_rebuild 3（新增协议套件）；
  semantic_publish 11、semantic_queue 2、semantic_lease 9、semantic_outbox 17、
  semantic_coverage 3、semantic_schema 6、semantic_manifest_reads 5 等既有套件零回归；
  cc-index lib 377 (+1 ignored) 及全部集成套件绿；
  cc-semantic lib 54；reconcile_rebuild 3（新增端到端套件）；
  queue_worker 8、publish_cas 4、artifact_cache 17、manifest_exact_integration 4 全绿。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.36s（零 error）

cargo clippy -p cc-db -p cc-semantic --locked --offline
→ cc-db 零 warning；cc-semantic 3 个 warning 全部位于
  crates/cc-semantic/src/cache.rs:102/:126/:127（P6-008 交付文件，与 P6-011/
  P6-013 记录在案的同一批既有项），本轮红线未触碰；新增两文件零 warning。
rustfmt --check（本轮新增 4 文件）→ 干净
```

## 7. 偏差清单（与简报草案）

1. **新增权威路径新鲜读 fence（简报未点名该机制）**：简报措辞"rebuild 完成后
   旧进程的 claim/publish 全部因 incarnation fencing 失败——P6-011 第 2 步已
   覆盖，本任务只加专项测试"。实测证明 P6-011 fence 1（本连接 incarnation ==
   快照）**不能**覆盖"另一进程持旧 inode"场景（ghost 连接读到的正是旧
   incarnation，fence 恒通过，写入落 ghost 文件）——这正是 P6-011 §8 记录的
   覆盖边界。因此"只加专项测试"落实为"专项测试 + 使测试可为的最小机制"：新
   模块内 `generation_at_path`（权威路径 fresh 只读连接 + strict
   ReadGeneration）+ 两个 fenced 门面。ADR 第 122-124 行"必须围栏住仍持有旧
   连接的另一进程；权威判定只认 strict ReadGeneration"是直接依据。既有
   `publish_semantic`/`claim_semantic` 语义零改动（fenced 变体在其前组合）。
2. **fenced publish 拒绝不写 retry（对 P6-011 拒绝语义的显式偏差）**：P6-011
   拒绝会在同一事务写 fenced retry；旧 inode 场景下该写入只会落幽灵文件（丢
   失且污染），故 fenced 变体在任何事务前返回拒绝、零写入。测试 1 断言权威
   库零接触固化。
3. **desired 集合投影落 cc-db 而非 reconcile.rs**：简报模块归属写
   "`crates/cc-semantic/src/reconcile.rs`（补齐侧）"；投影 SQL 是权威库读，
   且 cc-semantic 依赖地板（仅 cc-model/cc-db）禁止 rusqlite 直触——落
   `semantic_rebuild.rs::semantic_rebuild_desired_set`，reconcile 只编排。
   `RecordInputPeek` 与 P6-011 CAS 的同构最小反序列化原则一致。
4. **reconcile 未命中任务的回队经 fenced retry（消耗 attempt 预算）**：冻结
   原语集中没有"清 lease 回 pending 不计 attempt"的通道；retry 语义（backoff
   回 pending、预算耗尽死信）与本场景兼容（重建后的重做与 worker 失败同构）。
   预算参数经 `ReconcileOptions` 透出。
5. **重入队门面每次实际变化 bump 一次 semantic_epoch（含重复重入队）**：P6-012
   复申"epoch 只在可见集合变化时推进"，但 P6-006 已定案"非零 outbox stat 即
   语义状态变化 → 声明 Semantic"（增量写路径同口径）。本任务遵循 P6-006 既有
   约定（期望集合变化 = Semantic 效应），测试 4 的 `Some(3)` 分解注释固化该
   归属；若收口轮裁决改为"仅 manifest 行集合"，改动的只是 enqueue 门面内的
   一个条件，不影响本协议结构。
6. **claim 循环用非 fenced claim/facade（fence 只在 pass 入口一次）**：pass
   入口 freshness 已证 snapshot == 权威路径，其后每任务再开 fresh 连接只付成
   本不增正确性；`publish_semantic` 内部五重 fence（含 fence 1）对每个发布
   仍然完整生效。
7. **bounded 扫描未做**：`semantic_rebuild_desired_set` 全表投影无行数上限
   （简报将有界恢复归 P6-015）；模块文档已注明"caller bounds the pass, P6-015
   owns the bounded-recovery budgets"。

## 8. 未做与剩余风险

- **组合根接线不做（红线）**：`reconcile_after_rebuild` 无 cc-server 调用点；
  rebuild swap 后"重注册 active space → reconcile 一遍 → 恢复 drain"的编排
  时机归接线轮。
- **DirectWriter 重建路径未单测**：两条重建策略共享
  `run_rebuild_protocol`（批次 1 已固化），fence 对路径不敏感（只认权威路径
  incarnation）；专项测试走 temp-db 策略。
- **reconcile 与并发写路径的交错**：pass 期间新写入产生的 supersede 由既有
  机制兜底（supersede 行使在途 CAS 拒绝、新版本任务服务下一次 claim）；pass
  本身无跨任务原子性声明（每任务独立短事务，可安全中断重跑）。
- **cache 命中但 corrupt 的计数**：`Corrupt` 与 `Miss` 同归 `requeued`（worker
  重新付费嵌入）；坏对象的隔离/quarantine 归 P6-018，本轮未做区分计数。
- V13/V17 正式验证矩阵证据（`artifacts/benchmarks/<run-id>/`）不推断，归验收
  轮；本记录 §4 为工程级证据。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
