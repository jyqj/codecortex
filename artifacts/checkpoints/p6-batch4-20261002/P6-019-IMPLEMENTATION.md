# P6-019 实施记录：更新存储/恢复/配置文档（含章程同步与两处顺带修复）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-019 节（at-least-once、两库顺序、恢复步骤与 namespace 五项内容清单）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003 的
  Supersedes 声明与"文档同步义务"条款——`DESIGN.md` 设计原则第 3 条与
  `docs/internals/STORAGE.md` 开篇的限定性文本在本轮落地方算 P6-001 blocked
  子项闭环）、批次 1~4 全部实施记录（`artifacts/checkpoints/p6-batch1~4-20261002/`）、
  待办移交清单（P6-012 收口动作①模块文档措辞统一、DESIGN/STORAGE 章程同步、
  P6-018 §7 记录在案的 `TempDirGuard` flaky 修复）。
- 任务指令口径：文档与实现不一致时以实现为准；不虚构未实现行为（组合根接线
  未做的事如实标"接线轮"）；不宣称跨两库/网络 exactly-once 或零重复收费
  （tasks.json P6-019 验收红线）。
- 改动范围：`DESIGN.md`、`docs/internals/STORAGE.md`、
  `docs/internals/CONCURRENCY.md`、`docs/internals/INCREMENTAL_RECOVERY.md`、
  `docs/CONFIGURATION.md`、`docs/ARCHITECTURE.md`（一行事实订正）、
  `crates/cc-db/src/semantic_coverage.rs`（模块文档措辞统一）、
  `crates/cc-db/tests/semantic_space_switch.rs`（测试基建 flaky 修复）。
  零 schema 变更；全部 P6 交付物（P6-002~018 文件）零触碰；`tasks.json`
  status 未改；未 git commit；artifacts 冻结链未触碰。

## 1. 章程修订对照表（P6-001 遗留兑现）

### 1.1 `DESIGN.md` 设计原则第 3 条

| 原文 | 修订 | 依据 |
|---|---|---|
| **单一数据库。** 所有状态在 `index.sqlite3`。没有 `runtime.sqlite3`、没有会话存储、没有遥测落盘。 | **权威状态单一数据库。** 全部权威状态在 `index.sqlite3`：索引内容、incarnation/generation、语义 manifest 可见集合、outbox/lease 队列与 active space 指针。没有 `runtime.sqlite3`、没有会话存储、没有遥测落盘。**显式例外（唯一）**：可选语义功能的内容寻址、可丢弃、可校验重建的派生 artifact cache 存于库外本地目录——它是付费换来的派生产物，不是权威状态，不承担源码/manifest 权威、不存秘密、跨项目默认隔离。边界与 durability 顺序的正式决策见 docs/adr/0003-…（ADR-0003，限定修订本条）。 | ADR-0003 Supersedes 声明："权威状态仍严格单库，新增一个显式例外：内容寻址、可丢弃、可校验重建的派生 artifact cache 不属于权威状态" |
| （"本项目明确不做"清单）没有 `runtime.sqlite3`（只有 `index.sqlite3`） | 没有 `runtime.sqlite3`（权威状态只有 `index.sqlite3`；唯一的库外例外是 ADR-0003 的派生 artifact cache，它不是第二个权威库，见"设计原则"） | 同上——消除无限定"只有"表述与修订后原则的表述冲突 |
| cc-db 行：`SQLite 索引存储：r2d2 池、WAL、FTS5、21 表（+5 FTS5）、schema v6` | `…29 基表（+5 FTS5 虚拟表）、schema v22` | 实现事实：`index_v1.sql` 计 29 张 `CREATE TABLE` + 5 张 FTS5 虚拟表；`index_migrate.rs:35` `CURRENT_SCHEMA_VERSION = 22`（V21 文档事实漂移检测口径） |

### 1.2 `docs/internals/STORAGE.md` 开篇

| 原文 | 修订 | 依据 |
|---|---|---|
| 所有状态存于单一数据库文件 `index.sqlite3`。没有第二个数据库、没有会话存储、没有遥测落盘 | **权威状态全部存于单一数据库文件** `index.sqlite3`：…（列权威集合）。**显式例外（唯一）**：可选语义功能的派生 artifact cache…它不是权威状态——损坏只降级不污染，删除只损失已付费的嵌入产物，不损失任何可从源码/主库重建的事实。该边界与 durability 顺序的正式决策见 ADR-0003（本节无限定"单一数据库"表述由该 ADR 限定修订） | ADR-0003 Supersedes 声明原文（supersede 措辞与 ADR 中"无限定表述"的指称逐字对应）；例外性质取 ADR Decision Drivers 三条（派生/可校验/可丢弃） |
| 当前 schema 21 保留 P2-C…（长段历史沿革混排） | 拆两段：当前 schema **22**（v21 基础上追加语义三表 + 7 索引）+ 历史沿革指针（DOCUMENTS.md/SOURCE_CHUNKS.md）+ 升级/回滚指向新的 Schema 版本策略节 | 实现事实（P6-005）；原段"20 及更早缓存须隔离重建"随 v21→v22 加法迁移一节一并修正 |

### 1.3 supersede 措辞一致性

`DESIGN.md` 与 `STORAGE.md` 的修订文本均显式写明"限定修订"与 ADR-0003
指向，与 ADR 自身 Supersedes 段（"限定修订……权威状态仍严格单库，新增
一个显式例外"）口径一致；两份文档均保留"没有 runtime.sqlite3、没有会话
存储、没有遥测落盘"原句（这些无限定表述与 ADR 不冲突，ADR 修订的只是
"所有状态"的全称量词）。

## 2. 措辞统一（P6-012 收口动作①移交兑现）

`crates/cc-db/src/semantic_coverage.rs` 模块文档（英文随原文件风格）：

原文（49 行处）：

> Note the epoch's scope: `semantic_epoch` tracks the *visible set* only
> (P6-004 Q4), so eligibility changes do not move it — consumers must cache
> by the snapshot generation, not assume coverage is epoch-invariant.

修订（批次 3 收口裁决口径，统一 P6-006 与 Q4 两约定）：

> Epoch scope (the batch-3 closure ruling, unifying P6-006 with Q4):
> `semantic_epoch` advances with *semantic state* — outbox re-enqueue that
> changed the desired set (the P6-006 nonzero-stat convention) plus
> visible-set changes (P6-011 publish CAS / revocation) — while `Auxiliary`
> commits (claim/renew/retry bookkeeping) never advance it. Eligibility
> changes alone do not move it either, so consumers must cache by the
> snapshot generation, not assume coverage is epoch-invariant.

与任务裁决口径逐点对应："epoch 随语义状态（outbox 重入队 + 可见集合变化）
推进、Auxiliary 永不推进"。该口径与 `epoch_rules.rs` WriteEffect doc、
`semantic_rebuild.rs:49`（P6-006 convention: any nonzero stat is a semantic
change）、`semantic_space_switch.rs:401` 及 P6-014/P6-015 收口裁决记录一致。
纯注释改动，零行为影响（编译与全量测试零回归验证）。

## 3. 顺带修复：`TempDirGuard` 纳秒撞车（P6-017 测试基建 flaky）

`crates/cc-db/tests/semantic_space_switch.rs` 的 `tempdir::TempDirGuard`：
原实现目录名 = `cc-db-p6017-{tag}-{pid}-{SystemTime 纳秒}`。并行用例同刻
创建时纳秒值相同 → 两用例共用同一目录与同一 `index.sqlite3`，偶发
`UNIQUE constraint failed: files.file_path`（P6-018 §7 记录在案，测试基建
竞态、非产品代码）。最小修复：**pid + 进程内单调原子计数器**（同进程内
确定性互异，跨进程由 pid 分隔），删除 `uuid_like()`：

```diff
 mod tempdir {
     use std::path::PathBuf;
+    use std::sync::atomic::{AtomicU64, Ordering};
     pub struct TempDirGuard(PathBuf);
     impl TempDirGuard {
         pub fn new(tag: &str) -> Self {
+            // pid + monotonic per-process counter: two guards created in the
+            // same nanosecond (parallel test cases) used to collide on one
+            // directory and share an index.sqlite3; the counter cannot.
+            static NEXT: AtomicU64 = AtomicU64::new(0);
+            let seq = NEXT.fetch_add(1, Ordering::Relaxed);
             let path = std::env::temp_dir().join(format!(
-                "cc-db-p6017-{tag}-{}-{}",
-                std::process::id(),
-                uuid_like()
+                "cc-db-p6017-{tag}-{}-{seq}",
+                std::process::id(),
             ));
             std::fs::create_dir_all(&path).unwrap();
             Self(path)
         }
@@
-    fn uuid_like() -> u128 {
-        std::time::SystemTime::now()
-            .duration_since(std::time::UNIX_EPOCH)
-            .unwrap()
-            .as_nanos()
-    }
     impl Drop for TempDirGuard {
```

红线归属：测试基建修复（任务第 4 项明示授权）；不改任何被测行为，10 个
既有用例零断言改动全绿。测试基建不属于既有交付物行为面（P6-018 记录已
把它列为待修移交项）。

## 4. 各文档更新清单（全部对照实现，含 file:line 引用）

### 4.1 `docs/internals/STORAGE.md`（主更新）

1. **开篇**：限定式单库表述（§1.2）+ schema v22 事实段。
2. **Epoch 协议：三钟与四效应**（原"Epoch 双时钟"整节重写）：三钟表
   （`index_epoch`/`evidence_epoch`/`semantic_epoch` 各自推进时机与失效
   语义）、四效应提交级规则（`epoch_rules.rs` `WriteEffect` :47、
   `commit_with` `unit_of_work.rs:87`；`Auxiliary`/空集零钟；`None` =
   未就绪绝不折叠 0）、表→钟映射补语义三表行（声明式归属）、
   strict `ReadGeneration` 一致快照与语义诊断读 before/after 纪律
   （`semantic_coverage.rs`）。
3. **UnitOfWork 小节**：补 `commit_with(EffectSet)` 一句（`commit()` 等
   价 `EffectSet::of(Index)`）。
4. **metadata 固定键**：补 `semantic_epoch`。
5. **表结构**：基表版本 v22 + 语义三表行（指向新节）。
6. **新增「语义持久化（P6，schema v22）」主节**，五个子节，P6-019 内容
   清单五项逐项落位：
   - 权威侧三表与状态机（`semantic_manifest` FK CASCADE、`semantic_outbox`
     op/state 域 + 封闭迁移表 + lease 内联列 + `semantic_outbox_live_per_doc`
     部分唯一索引 = 合并语义的 DB 层保证、`semantic_spaces` 三态与单
     active fail-stop）；
   - at-least-once 与 fencing（原子入队 `supersede_and_enqueue_on`
     semantic_outbox.rs:239、删除永不产生 embed、claim 单语句 CAS
     :496、每 attempt 随机 token、token-only fencing、重复 ack 吸收、
     attempt 死信 `retry_on` :621、`reclaim_expired_on` :653；**明确
     写出不宣称跨两库/网络 exactly-once 或零重复收费**——验收红线原文
     落文）；
   - 发布 CAS 与 durability 顺序（`publish_embedding` publish.rs:98
     三步 = artifact durable 在先；五重 fence `publish_and_ack_on`
     semantic_publish.rs:179、拒绝枚举 :68、`publish_semantic` :355；
     "CAS 后 ack 前"残态不可达）；
   - **恢复步骤与 crash 点**（`recover_scan` recovery.rs:209 有界扫描 +
     fence 先行 `generation_at_path`；四行 crash 点表：put 中半文件 →
     Miss/Corrupt 交还 worker、put 后 CAS 前 → 有界重放零新付费、CAS
     提交后 → 原子 + Q4 吸收、换库 rename 中 → ghost freshness fence
     零写入；死信只清点不复活；`converged` 循环驱动、无常驻进程）；
   - cache 布局、namespace 与降级语义（寻址四元组即路径、checksum 独立
     字段读时校验；`namespace_key` cache.rs:96 不绑 incarnation；
     `resolve_cache_root` :128 解析序 + `CACHE_ROOT_ENV` :72；纯 Miss
     永不降级、quarantine `degrade.rs:119`、degraded 判据与
     `capability_status.rs:100` 透出、`BudgetedProvider` :347 预算 +
     attempt 死信——不静默无界重费）；接线未落地处均如实标注；
   - GC 与空间切换（三条件判据 + 3600s 宽限 `gc.rs:93` +
     `IndexDb::semantic_gc_mark` semantic_gc_reads.rs:161 短读快照同步点
     + `run_gc_pass` gc.rs:531 有界显式驱动、quarantine 结构性不可触及；
     切换三段 `switch_active_space_on` semantic_space_switch.rs:308 单
     事务五步 + metadata 审计键 + revoke 消费/回滚复用）。
7. **Schema 版本策略**（原"当前 v12/不做向后迁移"已漂移，按实现重写）：
   v22；相邻 v21 纯加法原位迁移（`ADDITIVE_MIGRATION_FROM = 21`，
   index_migrate.rs:42，`SchemaStatus::Migrated`，幂等、epoch/incarnation
   保留、新表空起点）；其余版本维持 rebuild-on-mismatch；**降级语义** =
   版本守卫 `Mismatch` → 旧二进制按其重建协议重建（降级即重建），cache
   与 incarnation 无关可跨版本校验复用，语义状态经 reconcile 恢复。

### 4.2 `docs/internals/CONCURRENCY.md`

新增两小节（插在"Epoch 失效协议"与"读路径的失败语义"之间）：

- **语义钟扩展（P6）：Auxiliary 永不冲刷缓存**——四效应与三钟的缓存失效
  语义；`Auxiliary` 重试循环不可能冲刷 epoch 键控检索缓存（ADR-0003 动机
  原文）；`None` ≠ 0 不得参与缓存键比对；语义诊断读 before/after 纪律。
- **语义写入的 fencing 并发（P6）**——claim CAS + 每 attempt token；
  发布五重 fence；换库幽灵进程防线（`generation_at_path` 权威路径 fresh
  读，并指出与既有 db_identity 永不复用是同一威胁模型的写侧对偶）；GC
  同步点与宽限的组合防竞态。均链接 STORAGE.md 对应锚点，不重复机制细节。

### 4.3 `docs/internals/INCREMENTAL_RECOVERY.md`

- "Upgrade, rollback and validation" 段补一句：schema 22 对 21 是加法
  扩展（stored-21 原位迁移保留全部行/epoch/incarnation），其余版本维持
  rebuild-on-mismatch。
- 文末新增「语义持久化的恢复事实（P6）」节（简体中文，按任务文档语言
  约定）：换库后语义三表从零 + `semantic_epoch` 缺席；reconcile 三步
  （fence 先行 → desired 重导重入队 → cache 命中零付费复用/miss 回
  worker）；`recover_scan` 有界恢复与 P2-C frontier 恢复相互独立（各管
  各的队列，互不代理）；cache 不随重建丢失；接线归接线轮。

### 4.4 `docs/CONFIGURATION.md`

- 新增「语义缓存与降级（P6，可选）」节（插在 ranking 与仓库规模档位之间）：
  cache 根目录解析三级序（env → macOS 平台默认 → Linux XDG/`~/.cache`，
  `resolve_cache_root` cache.rs:128、空白 env 视为未设、put/get 不读
  env、open 零副作用惰性建目录）；namespace 键（blake3 域分隔 + 跨项目
  隔离 + 不绑 incarnation，身份来源与接线状态如实标注）；降级语义与
  re-embed 预算（纯 Miss 永不降级、quarantine、degraded 判据与透出槽、
  `BudgetedProvider` 整批准入 + attempt 死信、GC 宽限 3600s 默认）。
  开篇明示 `.codecortex.json` 目前**没有**语义相关键（预算/租约是调用方
  参数），配置面归接线轮——不虚构未实现行为。
- 「环境变量覆盖」新增「语义缓存（P6，可选，`semantic` 接线后生效）」
  小节：`CODECORTEX_SEMANTIC_CACHE_ROOT` 行（默认/作用/接线状态）。

### 4.5 `docs/ARCHITECTURE.md`（一行事实订正）

cc-db 行 `21 表 + 5 FTS5、schema v6` → `29 基表 + 5 FTS5、schema v22`
（与 DESIGN.md 同一漂移、同一实现依据；V21 文档事实漂移检测口径）。

## 5. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db --locked --offline
→ 全部套件 0 failed：lib 173 passed (+1 ignored)；semantic_space_switch 10
  passed（TempDirGuard 修复后全绿，含两个 consume_revoke_* 用例）；
  semantic_outbox 17、semantic_lease 9、semantic_publish 11、
  semantic_recovery 4、semantic_schema 6、semantic_coverage 3、
  semantic_queue 2、semantic_rebuild 3、semantic_gc_reads 3（内联）等全绿。

cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.41s
  （零 error/warning）。
```

文档改动无需编译验证的部分做了目视核对：全文引用的实现锚点逐一经
grep 复核为当前行号（`index_migrate.rs:35/:42`、`epoch_rules.rs:47`、
`unit_of_work.rs:87`、`semantic_outbox.rs:239/:496/:602/:621/:653`、
`semantic_publish.rs:68/:179/:355`、`publish.rs:98`、`recovery.rs:209`、
`semantic_gc_reads.rs:161`、`gc.rs:93/:531`、`cache.rs:72/:96/:128`、
`degrade.rs:119/:347`、`capability_status.rs:100`、
`semantic_space_switch.rs:308`）；表数（29 基表 + 5 FTS5 虚拟表）经
`CREATE TABLE`/`CREATE VIRTUAL TABLE` 计数复核；文档内部锚点按 GitHub
slug 规则校正（`#语义持久化p6schema-v22` 等）。

## 6. 偏差清单

1. **`docs/TROUBLESHOOTING.md` 未改**：tasks.json P6-019 范围含该文件，
   但本任务指令的更新清单（章程同步 / 措辞统一 / 配置与存储文档 /
   flaky 修复）未列它；恢复与降级事实已落 STORAGE.md（crash 点表）与
   CONFIGURATION.md（降级语义），避免内容重复维护。若收口轮要求
   TROUBLESHOOTING 面向用户的排障条目，建议从上述两节摘编，归后续轮。
2. **`DESIGN.md` crate 表与 `docs/ARCHITECTURE.md` cc-db 行的版本订正**
   （21 表/v6 → 29 基表/v22）超出"单一数据库表述"的字面范围：属 V21
   验收"文档事实漂移检测（对照实现断言文档陈述，如 schema 版本、表名）"
   的直接对象，且与 ADR 修订文本同处一段，不同步会自相矛盾——按"以实现
   为准"落定，单独列此备查。
3. **STORAGE.md 历史段落整段重写的范围**：原开篇第二段把 P2-C~P4-D 历史
   沿革与版本声明混排且版本声明已漂移（schema 21/v12 混用）；本轮拆为
   "当前 v22 事实 + 沿革指针"两段，P2~P4 细节改由既有专题文档承载，未
   删除任何仍正确的事实表述。
4. **epoch 表→钟映射表新增语义三表行标注"（声明式）"**：`epoch_rules.rs`
   的表→钟审计枚举只覆盖 Index/Evidence 表；语义三表的效应归属是提交点
   声明（P6-006/011/017 各写路径），非枚举驱动——文档如实区分，避免
   读者误以为存在静态枚举行。
5. **INCREMENTAL_RECOVERY.md 新节语言**：该文件存量正文为英文；新节按
   任务文档语言约定（简体中文）书写。存量英文本轮未翻译（超出最小改动）。

## 7. 未做与剩余风险

- **组合根接线仍未做**（如实标注于 CONFIGURATION.md/STORAGE.md/
  INCREMENTAL_RECOVERY.md 三处）：`try_init`、drain/reconcile/recovery/GC
  调度时机、降级快照转写、cache 身份传入均无生产调用点；文档只记录库层
  协议事实与"接线轮"去向，未虚构任何已接线行为。
- **STORAGE.md 其余未触碰段落**（连接模型、FTS、重建协议 PRAGMA、WAL
  等）未做全面漂移审计——本轮范围是 P6 相关事实与章程文本；全文档漂移
  扫描属收口/验收轮口径。
- `tasks.json` status 未改（红线）；未 git commit；artifacts 冻结链未触碰。
- P6-020 验收需的 V21 正式证据（`artifacts/benchmarks/<run-id>/`）未生成，
  本记录 §5 为命令级与目视核对证据。
