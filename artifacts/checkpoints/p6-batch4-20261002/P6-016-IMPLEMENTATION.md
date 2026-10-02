# P6-016 实施记录：GC 与发布协调

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-016 节（mark 集合 / 同步点 / sweep 原文，引用见下）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 108-109 行
  "GC 与发布共享同一同步点（活跃引用 + 活跃 lease + 最短保留期 mark/sweep）"；
  第 152 行约束表；第 60-62 行 artifact-before-manifest 无跨两库原子提交）、
  批次 1-3 交付（P6-008 `ArtifactCache` 的 discard/quarantine_dir/原子写残迹、
  P6-011 publish CAS 与 artifact-before-manifest 顺序 ⇒ GC 判据 =
  "未被任何 manifest 行的 artifact_ref 引用"）。
- 改动范围：`crates/cc-db` 1 个新只读模块 + lib.rs 一行注册；`crates/cc-semantic`
  1 个新模块 + 1 个新测试文件 + lib.rs 注册/模块文档。**schema v22 零变更**；
  P6-008/011/013/014/015 交付文件零触碰（含 cache.rs 的 3 个既有 clippy
  warning 原样保留）；`tasks.json` status 未改；未 git commit。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_gc_reads.rs` | 新增（全文） | 模块文档（边界/只读/全 space/保守对判据）+ `manifest_references_artifact_on`(:34，精确 ref mark，`semantic_manifest_artifact` 索引即 P6-005 预留的 "GC mark 用") + `manifest_holds_input_on`(:54，`(space_id,input_digest)` 保守对) + `live_task_holds_input_on`(:80，`pending`/`claimed`) + `GcMarkProbe`(:102)/`GcMark`(:110 四变体) + `mark_batch_on`(:134，批内同序判定) + `impl IndexDb::semantic_gc_mark`(:157-168，**同步点**：`read_conn` + `unchecked_transaction` 一次短读快照) + 3 个内联单测(:213/:223/:232) |
| `crates/cc-db/src/lib.rs:48` | 修改 | `pub mod semantic_gc_reads;`（一行注册） |
| `crates/cc-semantic/src/gc.rs` | 新增（全文） | 模块文档（判据三条件/同步点/模块归属偏差/有界性/sweep 物理学）+ `DEFAULT_MIN_RETENTION_SECS`(:93，3600s) + `GcConfig`(:99)/`GcPosition`(:120)/`GcEntryKind`(:130)/`GcEntry`(:151)/`CollectedBatch`(:160)/`GcCounters`(:169) + `collect_candidates`(:200，keyset 有界候选收集) + `classify`(:303)/`fresh_timestamp`(:352) + `sweep_batch`(:384，Pass1 新鲜度 → Pass2 一次快照 mark(:413) → Pass3 unlink → Pass4 空目录修剪) + `run_gc_pass`(:531，collect+sweep 显式驱动) |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod gc;`(:55) + 模块边界文档补 P6-016 段 |
| `crates/cc-semantic/tests/semantic_gc.rs` | 新增 | 8 个集成测试（§4） |

## 2. GC 判据与宽限机制（原文对照）

**简报 P6-016 mark 原文**：

> **mark**：活引用集合 = `semantic_manifest.artifact_ref`（全 space，含非 active——
> 旧空间回滚复用依赖它，P6-017）∪ 进行中任务的 `input_digest` 派生目标（活跃 lease
> 行）∪ `published_at/last_used > 最短保留期` 之外豁免。

**落点**（`crates/cc-semantic/src/gc.rs:11-31` 模块文档）：对象可删需同时满足三条件，
判据保守（红线"不确定即保留"）：

1. **新鲜时间戳宽限**：对象的 `created_at`（meta sidecar，发布器每次 re-put 都刷新）
   距 `now_unix` 小于 `min_retention_secs`（默认 3600s）⇒ 豁免。这就是
   artifact-before-manifest 窗口的机械闭合：一个正要被 publish 引用的对象必然刚被
   `publish_embedding` 第 1 步 re-put（P6-011 编排），时间戳必新；
2. **manifest 未引用**：checksum 可从 sidecar 还原时走精确 `artifact_ref` 匹配
   （`semantic_gc_reads.rs:34`，全 space、含非 active——P6-017 回滚复用依赖）；
   checksum 不可还原（sidecar 损坏/半文件）时降级为 `(space_id, input_digest)`
   保守对匹配（`semantic_gc_reads.rs:54`）；
3. **无活跃任务持有**：无 `pending`/`claimed` outbox 行以该 `input_digest` 为嵌入
   输入（`semantic_gc_reads.rs:80`；简报只要求活跃 lease 行，`pending` 为保守加项
   ——排队任务可复用缓存产物，误删 = 把付费向量换回一次重嵌）。

**简报同步点原文**：

> **同步点**：GC 候选收集与删除决定之间，取一次 DB 短事务快照：若候选在收集后被新
> publish 引用（manifest 行 artifact_ref 匹配），剔除候选——publish 侧在 CAS 事务内
> 天然互斥（同一 IMMEDIATE 连接族），故"引用刚被 GC 删除产物"不可能：删除决定基于
> 晚于 publish 提交的快照。

**落点**：`sweep_batch`（`gc.rs:384`）Pass 2 对整批非新鲜候选构建 probe 后调用
`IndexDb::semantic_gc_mark`（`semantic_gc_reads.rs:161`）——读池连接 +
`unchecked_transaction()`（deferred）**一次短读快照**内完成整批判定；快照之前提交的
publish 对 mark 可见（其产物判 ManifestRef 保留），快照之后提交的 publish 由宽限
机制覆盖（该 publish 的 re-put 已刷新 `created_at`）。两机制合成使
"manifest 引用刚被 GC 删除产物"不可达。

**简报 sweep 原文**：

> **sweep**：unlink 候选文件 + 删除空目录；孤儿（无任何 DB 引用且超保留期）最终回收。
> 全程无 DB 写（或仅 Auxiliary 计数），不刷 epoch。

**落点**：Pass 3 unlink（bin+meta 两半——`discard()` 的物理孪生）；Pass 4 自底向上
修剪空目录（spec 叶目录 → input → space，`remove_dir` 只对空目录成功，namespace
目录本身永不删，`<root>/quarantine` 在 namespace 之外结构性不可触及）。全程零 DB 写、
零 epoch 移动。

**temp/半文件清扫**（P6-008 遗留"半文件物理清扫归 P6-016/P6-018"）：`atomic_write`
残迹（`*.tmp-<pid>-<seq>`）与半对象（bin 无 meta / meta 无 bin——meta 是 P6-008 的
完备性标记，两态读皆 Miss）同受宽限约束；半文件因地址可判而走 DB mark（保守对 +
活跃任务），temp 无 DB 关涉只看时间。

## 3. 竞争防护测试证据（原样）

`SDKROOT=… cargo test -p cc-semantic --test semantic_gc --locked --offline`：

```
test fresh_unpublished_object_is_graced_then_reclaimed ... ok
test corrupt_object_is_swept_after_grace_and_reads_miss_like_discard ... ok
test manifest_referenced_object_is_never_swept ... ok
test publish_committing_after_the_sweep_is_covered_by_the_grace ... ok
test temps_and_halves_are_swept_and_emptied_dirs_pruned_without_touching_quarantine ... ok
test revoked_space_object_stays_protected_by_its_manifest_row ... ok
test publish_committing_between_collect_and_sweep_is_protected_by_the_snapshot ... ok
test bounded_backlog_converges_over_rounds_and_reference_survives_every_round ... ok
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s
```

V17 竞态负测试的交错序列（`semantic_gc.rs:376-434`
`publish_committing_between_collect_and_sweep_is_protected_by_the_snapshot`）：

1. worker 在 `T0-GRACE` put（旧时间戳，GC 时钟下已超宽限且无引用——教科书候选）；
2. GC `collect_candidates` 收集到该候选（`assert_eq!(batch.entries.len(), 1)`）；
3. **交错点**：`Publisher::publish_embedding` re-put + CAS 提交，
   `Published { visible_set_changed: true }`；
4. GC `sweep_batch` 在晚于 publish 提交的快照内 mark → 断言
   `counters.kept_referenced == 1; counters.deleted_objects == 0`；
5. V17 终态断言：manifest 行的 `artifact_ref` 与 `cache.get` 的 `Hit` ref 逐字符
   相等——"manifest 引用永远可解析"。

对偶测试 `publish_committing_after_the_sweep_is_covered_by_the_grace`
（:436-464）：sweep 先行、publish 后至的场景由宽限覆盖（fresh put 在后续任何
pass 中 `kept_fresh`，CAS 后转 `kept_referenced`），零删除。

## 4. 测试清单（11 个新增，全绿）

**`crates/cc-semantic/tests/semantic_gc.rs`（8 个）**

1. `manifest_referenced_object_is_never_swept`(:307)——引用保护：真实 Publisher
   发布后超宽限 GC，`kept_referenced==1`、零删除、ref 可解析（V17）。
2. `fresh_unpublished_object_is_graced_then_reclaimed`(:341)——宽限内
   `kept_fresh`（Hit 保留），宽限后回收（Miss）+ 3 级空目录修剪。
3. `publish_committing_between_collect_and_sweep_is_protected_by_the_snapshot`
   (:376)——§3 竞态负测试。
4. `publish_committing_after_the_sweep_is_covered_by_the_grace`(:436)——对偶窗口。
5. `bounded_backlog_converges_over_rounds_and_reference_survives_every_round`
   (:466)——有界性：7 孤儿 + 1 引用对象，`batch_entries=3`，显式驱动循环
   3 轮收敛（`total_deleted==7`、引用对象每轮存活、rounds<20 防失控）。
6. `corrupt_object_is_swept_after_grace_and_reads_miss_like_discard`(:532)——
   P6-008 `Corrupt` 检测 → 宽限内保守保留 → 宽限后 unlink 两半 →
   读回 `Miss`（`discard()` 等价可观测）。
7. `temps_and_halves_are_swept_and_emptied_dirs_pruned_without_touching_quarantine`
   (:567)——temp/半文件双态：新鲜全保留；`min_retention_secs=0` 后
   `deleted_halves==2, deleted_temps==1`、空目录修剪、namespace 目录存活、
   `quarantine/` 文件零触碰。
8. `revoked_space_object_stays_protected_by_its_manifest_row`(:634)——全 space
   mark：无 `semantic_spaces` 行（非 active）的空间，其对象仍被 manifest 行保护
   （P6-017 回滚复用前提）。

**`crates/cc-db/src/semantic_gc_reads.rs` 内联单测（3 个）**：精确 ref mark 的
checksum 域隔离（:214）、保守对的 spec/checksum 无关与 space 隔离（:224）、
活跃任务 mark 的 pending/claimed 覆盖与 done/failed/superseded 不持（:233）。

## 5. 有界性证据

- 判据：`collect_candidates`（`gc.rs:200`）每 pass 最多 `batch_entries` 个候选
  （`batch_entries==0` 显式拒绝），keyset cursor =
  `(space_id, input_digest, leaf_dir, file_name)` 逐层字典序推进，无 OFFSET 重扫、
  无全树物化（惰性逐层 `read_dir`+排序）；`exhausted` 终点显式。
- 证据：测试 5——存量 8（7 孤儿 + 1 引用）、批次 3 ⇒ 恰 3 轮
  （3+3+2）收敛、`total_deleted==7`、引用对象每轮 `kept_referenced` 累计恰 1、
  终态 `Hit` 且 ref 相等。
- 无隐式常驻进程：`run_gc_pass` 是纯函数式单 pass，调度归组合根（与
  `queue`/`recovery` 同纪律）；模块内无线程/定时器/循环驱动。

## 6. 偏差清单

1. **模块归属偏离简报（`cache.rs` sweep 执行 + `publish.rs` 同步点）**：两者均为
   P6-008/P6-011 密封交付物，批次红线"不改既有交付物（只调用/组合）"禁止触碰——
   与 P6-011 把 SQL 原语移出 `semantic_outbox.rs` 同一先例。sweep 落新模块
   `cc-semantic/src/gc.rs`，只调用 P6-008 原语与 cc-db mark。
2. **同步点快照事务落在 cc-db 侧**（`IndexDb::semantic_gc_mark`）而非
   cc-semantic：cc-semantic 生产依赖面只有 `cc-model`/`cc-db`（ADR 行 P6-002，
   `Cargo.toml` 中 rusqlite 仅为 dev-dependency），无法自开事务；把 deferred
   短快照收进 cc-db 门面与 P6-011 `publish_semantic` 收 IMMEDIATE 事务的先例一致。
3. **mark 增加 `pending` 状态**（简报只列活跃 lease 行）：保守红线——排队任务
   可能复用该输入的缓存产物，误删即重嵌付费；`semantic_gc_reads.rs:80` 注释明示。
4. **半文件/meta 不可读对象走 `(space_id, input_digest)` 保守对 mark**（精确
   checksum 无法还原时）：同样"不确定即保留"；精确 ref 可还原的对象仍走精确匹配
   （该地址未被引用即真未引用，不因同输入他 spec 的引用而误保）。
5. **`run_gc_pass` 的 resume 语义**：仅当 `!exhausted` 才返回 cursor——测试驱动的
   循环终止条件更直白（cursor==None 即收敛）。
6. **宽限时钟源 = meta `created_at`，缺失时降级 mtime，再缺失按新鲜保留**：
   简报"最短保留期"未指定载体；`created_at` 是 P6-008 put 的调用方时钟（确定性
   测试），mtime 是唯一无副作用的兜底；"无法定年即视为新鲜"是保守方向的必然。
7. **temp 文件不做 DB mark**：temp 名永远不是可发布地址（publish 读回门禁只认
   完整对象），仅受宽限约束——非保守化简，是判据边界的如实刻画。
8. **P6-018 归属提示**：quarantine 搬移仍归 P6-018；本任务只保证 sweep 结构性
   不触及 `<root>/quarantine`（在 namespace 目录之外）并有测试 7 守护。
9. clippy：本轮新增文件零 warning（原 4 个 warning 中 1 个为本轮 gc.rs 的
   `match` 单臂，已修；其余 3 个为 cache.rs P6-008 交付既有项，红线未触碰）；
   `rustfmt --check` 三新文件干净。

## 7. 未做与剩余风险

- **宽限参数与发布延迟的关系**：`DEFAULT_MIN_RETENTION_SECS=3600` 要求大于
  put→CAS 最坏延迟；组合根接线轮应把该值纳入配置面（本轮不改配置系统）。
- **快照后 publish + 宽限归零的组合**：`min_retention_secs=0` 时同步点单独不再
  充分（快照后的 publish 引用可能被删）——配置校验（非零下限）归接线轮；
  默认值下两机制正交覆盖。
- **另一进程换库（incarnation swap）后旧 namespace 孤儿**：namespace 不绑
  incarnation（Q5），跨 incarnation 的旧空间对象由"manifest 全 space 引用 +
  宽限"保护；P6-017 空间切换后 revoked space 的最终回收（切换方显式清理）属
  P6-017 scope，本任务不越界。
- **GC 无 Auxiliary 计数落库**（简报允许"或仅 Auxiliary 计数"，非要求）；
  `GcCounters` 为返回值，审计落库归接线/收口轮按需补。
- 未改 `tasks.json` status；未 git commit；artifacts 冻结链未触碰。

## 8. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-db --locked --offline
→ 25 个 "test result: ok."，0 failed；关键计数：
  cc-db lib 173 passed (+1 ignored，含 semantic_gc_reads 新 3 个内联单测)；
  semantic_lease 9、semantic_outbox 17、semantic_publish 11、semantic_recovery 4、
  semantic_schema 6、semantic_coverage 3、semantic_queue 2、semantic_rebuild 3 等全绿；
  cc-semantic lib 54、semantic_gc 8（本轮新增）、artifact_cache 17、
  publish_cas 4、manifest_exact_integration 4 全绿

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.16s（零 error/warning）

cargo clippy -p cc-db -p cc-semantic --locked --offline
→ 仅 cache.rs 3 个 P6-008 既有 warning（:102 needless_question_mark、
  :126/:127 doc_lazy_continuation），本轮交付文件零 warning
rustfmt --check（本轮 3 个新文件）→ 干净
```
