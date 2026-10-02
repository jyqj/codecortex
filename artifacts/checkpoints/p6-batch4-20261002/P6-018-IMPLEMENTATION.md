# P6-018 实施记录：cache 缺失/损坏降级（批次 4 收口）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-018 节（Corrupt→quarantine 保留现场 + Miss 语义、degraded 透出、dense lane
  partial/unavailable、费用策略 admission 预算、双轨计数）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 154 行约束：
  "缺失/损坏 → 隔离坏记录、语义 degraded、本地继续；补嵌受费用策略控制，不静默
  无界重费"；ADR-0003 cache 可丢弃条款）、批次交付衔接（P6-008
  `Corrupt/CorruptReport/discard/quarantine_dir` 原语与"隔离搬移归 P6-018"移交、
  P6-010 检索 Miss/Corrupt 跳过、P6-011 put/get 发布门、P6-013 EmbedHandler 的
  retry/死信机、P6-014 reconcile miss→pending、P6-015 recovery 无 attempt 直写回
  `hand_back_semantic_task`（收口②）、P6-016 GC 的 temp/半文件清扫与 quarantine
  结构性不可触及）。
- 改动范围：`crates/cc-semantic` 1 个新模块 + lib.rs 注册/边界文档；`crates/cc-server`
  2 个既有文件的最小扩展（P6-012 偏差 1 的移交兑现点）+ 1 个新集成测试文件 +
  模块内单测。**schema v22 零变更**；P6-008/010/011/013/014/015/016 交付文件
  **零触碰**（`cache.rs`/`publish.rs`/`queue.rs`/`reconcile.rs`/`recovery.rs`/
  `gc.rs`/`vector/exact.rs` 均未改动，只调用/组合）；`tasks.json` status 未改；
  未 git commit；组合根接线未做（`try_init`、drain/reconcile/GC 的调度时机仍归
  接线轮）；无隐式常驻进程（零线程/定时器/守护进程，全部调用方驱动）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-semantic/src/degrade.rs` | 新增（全文，~660 行含单测） | 模块文档（降级矩阵表/ADR 引用/模块归属偏差/可见性口径）+ `QUARANTINE_REPORT_FORMAT_VERSION`(:77) + `QuarantineRecord`(:88) + `quarantine_object`(:119，双半 rename 搬移 + `.report.json` 诊断 sidecar + 竞态容忍 None) + `Admission`(:187) + `DegradationLedger`(:207，Arc 共享/进程生命周期) + `DegradationSnapshot`(:324) + `BudgetedProvider`(:347，`&dyn` 装饰器) + `impl EmbeddingProvider`(:364，批次级 all-or-nothing 准入) + `quarantine_detected`(:412) + `requeue_after_degrade`(:435，无 attempt 直写回) + 4 个内联单测 |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod degrade;`(:70) + 边界文档补 P6-018 段 |
| `crates/cc-semantic/tests/semantic_degrade.rs` | 新增 | 5 个端到端集成测试（§4） |
| `crates/cc-server/src/service_factory.rs` | 修改 | `QueryServices.semantic_degradation` 槽(:32) + `SemanticDegradation` 纯数据结构(:41，无 cc-semantic 依赖) + getter/setter(:87/:93) + 两个构造点同步(:56/:74) |
| `crates/cc-server/src/capability_status.rs` | 修改 | `apply_semantic_degradation`(:100，attached && degraded → `semantic_state: "degraded"` + `degraded_reason` 数组；其余状态原样) + 两个提前/末尾调用点(:40/:88) + 2 个模块内单测(:115) |

零触碰红线核对：`cache.rs` 的 3 个既有 clippy warning 原样保留；`space_switch.rs`、
`semantic_*`（cc-db 侧全部）零触碰；`Cargo.toml`/`Cargo.lock` 本轮零变更
（未引入任何新依赖）。

## 2. 降级矩阵终态（消费点 × 缺失/损坏 × 行为）

| 消费点 | 缺失（Miss） | 损坏（Corrupt） | 落点 |
|---|---|---|---|
| 检索 exact（P6-010，既有） | 跳过候选，不报错，健康行照常 | 跳过候选，不报错（只读路径不隔离） | `vector/exact.rs`（既有，测试 5 复验） |
| 发布门（P6-011，既有） | read-back Miss → `ArtifactNotVerified`，DB 零触碰 | read-back Corrupt → `ArtifactNotVerified` + reason | `publish.rs`（既有） |
| worker 队列（P6-013，既有 + 本轮组合） | provider embed（首次付费，唯一付费方） | 先隔离（本轮 facade）→ 补嵌受预算 → `put` 覆盖自愈 | `degrade.rs` + 既有 `EmbedHandler` |
| 重建复用 reconcile（P6-014，既有） | hand-back `pending` | 同 Miss（隔离由组合根 facade 叠加） | `reconcile.rs`（既有） |
| 恢复 recovery（P6-015，既有） | 无 attempt 直写回 `pending`（收口②） | 同 Miss（同上） | `recovery.rs`（既有） |
| GC（P6-016，既有） | 半文件超期回收 | 损坏对象超期回收；`quarantine/` 结构性不可触及 | `gc.rs`（既有，测试 4 衔接验证） |
| 降级可见性（本轮新增） | Miss 不降级（冷缓存是常态） | `degraded=true` + quarantine 计数 → `semantic_state: "degraded"` + `degraded_reason` | `degrade.rs::DegradationSnapshot` → `QueryServices::set_semantic_degradation` → `capability_status.rs` |
| 费用策略（本轮新增） | —（首嵌不入预算） | 补嵌受进程生命周期预算；超限 provider 拒绝 → fenced retry → 终态 `failed` 带 reason | `BudgetedProvider` + outbox `attempt_count`/`last_error` 双轨 |

口径裁决（依简报逐条）：

1. **缺向量不当完整空结果**（验收 1 的延续）：检索侧 Miss/Corrupt 一律跳过并
   使 dense lane 缺席该文档，绝不缓存为 complete；lexical/graph（本地）不受影响
   ——"本地继续"。测试 5 与测试 3 尾部分别固化"跳过不报错"与"预算耗尽后 dense
   降级为空、manifest 行保留、绝不伪造结果"。
2. **费用策略**（验收 2）：`DegradationLedger`（进程内 `AtomicU64` + `Mutex<
   HashSet>` 双载体）只对 **quarantined input 的补嵌** 计费准入（首次 embed 不入
   预算——"重费"口径 = 对已付费产物的再付费）；`BudgetedProvider` 在调用内层
   provider **之前**整批裁决，拒绝时返回 `ProviderError::InvalidInput("semantic
   re-embed budget exhausted: u/m …")`，经 `EmbedHandler` 的既有通路
   （`provider_reason` → `NeedsRetry` → fenced retry）把该原因持久化进
   `last_error`，attempt 预算耗尽后终态 `failed`——"绝不静默循环"由"拒绝发生在
   provider 之前"结构性保证（测试 3 断言 `call_count` 不再前进）。持久审计轨 =
   outbox 行计数（P6-006 既有），进程内计数器刻意不跨重启（重启后可再支出，
   行上审计仍在）。
3. **可见性口径**（V18）：`degraded` 的判据 = `corrupt_events > 0 || budget_
   exhausted`；纯 Miss 永不降级。`DegradationSnapshot` 为只读纯数据，cc-server
   经 `QueryServices` 的可选槽消费（`SemanticDegradation` 纯结构、无 cc-semantic
   依赖，默认构建不含该路径、不触文件系统）。

## 3. 隔离搬移与诊断保留（quarantine 落地）

`quarantine_object`（degrade.rs:119）：

1. **双半搬移**：`.bin` + `.meta.json` 逐半 `rename` 到
   `<root>/quarantine/<space>-<input>-<spec>-q<seq>.{bin,meta.json}`（同 root 下
   同文件系统，rename 原子；`<seq>` 为进程级原子序号，同一元组多次腐坏可重复
   隔离不碰撞）。搬移后原地址即读 Miss——"隔离 + Miss 语义"一步成立。
2. **诊断保留**：`.meta.json` 原样保留（七字段证据，含 checksum/created_at）；
   另写 `.report.json`（`QUARANTINE_REPORT_FORMAT_VERSION=1`，闭集字段：
   `format_version/quarantined_at/reason/original_bin_path/namespace/space_id/
   input_digest/spec_digest`，无秘密字段）。
3. **竞态容忍**：两半均已不在（GC 清扫或二次隔离赢了我）→ `Ok(None)`，且不落
   report（不为已消失的证据写描述）；半在场（如仅 `.bin`）→ 搬走在场半。
4. **GC 衔接**（P6-016 已保证，本轮验证）：`quarantine/` 在
   `namespace-<ns>/` 之外，`collect_candidates` 的遍历根是 namespace 目录，
   结构性不可达；测试 4 以字节级相等断言 + 多轮 GC pass（时钟远超宽限期）固化。

组合方式偏差（对简报模块归属）：简报写"`cache.rs`（quarantine 落地）"，但
`cache.rs` 是 P6-008 封存交付物且其记录明确"008 不隔离，018 隔离"，批次红线
"不改既有交付物（只调用/组合，缺口补齐走新增模块/方法）"——沿 P6-016 同一
先例落新模块 `degrade.rs`，只调用 `ArtifactCache::quarantine_dir()`（P6-008
预留的约定面）与公开布局常量，`cache.rs` 零改动。cc-server 侧同理：简报写
"`capability_status.rs`（degraded 透出，P6-012 字段扩展）"，该文件非 P6 交付物、
且 P6-012 偏差 1 显式移交本任务，故做最小扩展（一个纯数据槽 + 一个纯函数），
未引入 cc-server → cc-semantic 的默认依赖边（`semantic` feature 仍 optional
未接线，默认构建零行为变化）。

**损坏检测点的接线形态**：封存交付物（reconcile/recovery/publish）内的
`CacheRead::Corrupt` 分支保持既有降级行为不变（跳过/交还），隔离搬移由组合根
在检测点显式调用 `quarantine_detected`（隔离 + ledger 记账）+
`requeue_after_degrade`（claimed 任务无 attempt 直写回）完成——这是 P6-018
的 sanctioned degrade 门面，测试 1/2/3 以该形态驱动完整回路。

## 4. 测试清单（7 个新增，全绿）

**`crates/cc-semantic/src/degrade.rs` 内联单测（4 个）**

1. `quarantine_moves_both_halves_and_writes_a_diagnostic_report`——双半搬移、
   地址回落 Miss、meta 七字段证据保留、report sidecar 字段逐一断言、路径落在
   `quarantine/` 内。
2. `quarantine_is_none_when_the_object_is_already_gone`——从未写过 → None；
   半文件（仅 bin）→ 搬走在场半 Some。
3. `ledger_tracks_corruption_budget_and_visibility`——FirstEmbed 不计费、
   `note_corrupt` 即 degraded、预算 2 用尽后 `BudgetExhausted`、reason 双条、
   无界 ledger 永不耗尽但可见。
4. `budgeted_provider_gates_reembeds_and_never_calls_through_when_exhausted`——
   预算内放行并记账、超限 `InvalidInput` 且内层 provider 零调用（call_count
   不动）、混合批整批拒绝、query 委托不受预算。

**`crates/cc-semantic/tests/semantic_degrade.rs` 集成（5 个，FakeProvider 全链）**

5. `corrupt_quarantine_reembed_republish_self_heals_the_visible_set`(:329)——
   **完整自愈回路**：发布（FirstEmbed，预算 0）→ 翻转 payload → `CacheRead::
   Corrupt` 带诊断 → `quarantine_detected`（evidence 三件套 + degraded 事件）→
   地址 Miss → `bump_version` 重入队 → worker（BudgetedProvider）补嵌恰一次
   （预算记账 1）→ `put` 覆盖 → 发布 CAS done → `Hit` → exact 检索重见
   `["doc-a"]`。
6. `claimed_task_degrade_hands_back_without_consuming_the_attempt_budget`(:395)
   ——**claimed 任务降级**（模拟 reconcile/recovery reuse 循环撞坏对象）：
   `requeue_after_degrade` 落地 `pending`、交还本身不动 attempt 计数（P6-015
   收口② 原语）、reason 入 `last_error`；worker 续跑自愈、预算记账恰一次。
7. `exhausted_budget_dead_letters_the_task_with_reason_and_stops_the_spend`(:454)
   ——**预算耗尽死信**：Some(1) 预算首次补嵌耗尽 → 再次腐坏 + v3 任务 →
   provider 拒绝发生在调用之前（`call_count` 不前进）→ 一次 drain（max_attempts
   =2）内两次 fenced retry 后终态 `failed`，`last_error` 含 `re-embed budget
   exhausted`，死信清点 1；坏对象留在 quarantine、manifest 行保留、dense 检索
   降级为空（绝不伪造结果）。
8. `gc_passes_never_touch_the_quarantine_directory`(:541)——**GC 衔接**：隔离
   后多轮 `run_gc_pass`（时钟超宽限期、批 16），quarantine 三文件字节级原样、
   namespace 树内零删除、原地址 Miss、meta 仍可诊断。
9. `retrieval_degradation_matrix_skips_miss_and_corrupt_serves_the_healthy`(:622)
   ——**降级矩阵检索行**：三文档发布后一行 discard（Miss）、一行翻转（Corrupt）
   → 检索只回健康行、零报错；纯检索只跳过不隔离（`corrupt_events==0`，隔离是
   显式检测点的职责）。

**`crates/cc-server/src/capability_status.rs` 模块内单测（2 个）**

10. `attached_port_with_degradation_reports_degraded_state_and_reasons`——
    attached + degraded 快照 → `semantic_state: "degraded"` + `degraded_reason`
    数组逐项。
11. `unattached_healthy_or_unreported_states_are_never_degraded`——未 attached
    （保持 `not_configured`，陈旧快照也不透出）、attached 健康（保持
    `port_attached_unverified`）、无快照三分支均零降级字段。

## 5. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic --locked --offline --test semantic_degrade
test gc_passes_never_touch_the_quarantine_directory ... ok
test corrupt_quarantine_reembed_republish_self_heals_the_visible_set ... ok
test claimed_task_degrade_hands_back_without_consuming_the_attempt_budget ... ok
test exhausted_budget_dead_letters_the_task_with_reason_and_stops_the_spend ... ok
test retrieval_degradation_matrix_skips_miss_and_corrupt_serves_the_healthy ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-server --locked --offline capability_status
test capability_status::tests::unattached_healthy_or_unreported_states_are_never_degraded ... ok
test capability_status::tests::attached_port_with_degradation_reports_degraded_state_and_reasons ... ok
test result: ok. 2 passed; 0 failed; ...; 249 filtered out

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-db --locked --offline
→ 全套件绿（cc-semantic lib 60 含新 4；semantic_degrade 5；artifact_cache 17、
  publish_cas 4、queue_worker 6、reconcile_rebuild 3、recovery 5、gc 8、
  space_switch 2、manifest_exact_integration 4 等既有套件零回归；
  cc-db lib 173 + 全部集成套件绿）。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-server --locked --offline
→ 251 lib + 41 集成全绿（含新增 2）。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile（零 error；本轮新文件零 warning）

cargo clippy -p cc-semantic -p cc-server --locked --offline
→ cc-server 零 warning；cc-semantic 仅 cache.rs 既有 3 项（P6-008 交付文件，
  红线未触碰）；degrade.rs/semantic_degrade.rs/capability_status.rs/
  service_factory.rs 零 warning。
rustfmt --check（本轮 4 个新/改文件）→ 干净
```

TDD 过程记录（测试暴露的缺陷，均为修复非绕过）：
① 单测 `to_doc` 用 `DocumentInput::from_bytes` 把 digest 串再散列，导致
`is_reembed` 永假——改为显式绑定任务 digest 构造（与真实 worker 一致）；
② harness `bin_path` 少拼 spec 叶目录层（P6-008 布局是
`<ns>/<space>/<input>/<spec>/<spec>.bin`）——补 `join(spec)`；
③ harness `outbox_state` 无 ORDER BY 在多版本行间取行歧义（bump_version 留
done 旧行）——改 `ORDER BY task_id DESC LIMIT 1`；
④ `FakeProvider::call_count` 计入 query 调用、`claim` 本身计 1 次 attempt——
断言口径订正（补嵌恰一次 = 2 文档 + 1 检索；attempt 断言改为"交还前后相等"）。

## 6. 偏差清单

1. **quarantine 落位 `degrade.rs` 新模块而非 `cache.rs`**（§3 详述）：批次红线
   "不改既有交付物"压过简报模块归属草案；P6-016 已有同一裁决先例。
   `quarantine/` 目录的创建权随搬移落到本模块（P6-008 只建约定不建目录）。
2. **degraded 透出经 `QueryServices` 可选槽而非直读 cc-semantic**：默认构建
   不得依赖 cc-semantic（P6-002 依赖口径 + `semantic` feature 未接线），故
   `SemanticDegradation` 是 cc-server 侧纯数据结构，组合根（接线轮）负责把
   `DegradationLedger::snapshot()` 转写进去；本轮交付了槽 + 映射逻辑 + 单测，
   进程内无任何自动刷新（无隐式常驻，快照由调用方显式 set）。
3. **补嵌预算只覆盖"quarantined input 的再付费"**：简报"对 Corrupt/Miss 的重新
   embed"中，Miss 侧的首嵌是首次付费非重费（reconcile miss→pending 的补嵌同此），
   入预算会误伤正常冷缓存回填；Miss 的循环防护由 outbox attempt 预算
   （P6-013/015 既有）承担。Corrupt 侧补嵌 = `is_reembed` 命中 ledger 后整批
   all-or-nothing 准入（部分计费会造成账实不符）。
4. **`BudgetedProvider` 持 `&dyn EmbeddingProvider` 而非泛型 `P`**：避免为冻结
   的 `ports` trait 加 blanket impl（`impl EmbeddingProvider for &P` 会改动
   coherence 面），与 `EmbedHandler::new` 的 `&dyn` 形态对齐，任意 provider
   零包装成本组合。
5. **隔离搬移用 `rename` 而非 copy+delete**：quarantine 与 namespace 同在一个
   cache root 下（同文件系统），rename 原子且零拷贝；跨文件系统场景不存在
   （布局由 P6-008 冻结）。若 rename 报 NotFound 视为竞态容忍（§3.3），其余
   IO 错误照常上抛。
6. **降级矩阵的 reconcile/recovery 行未重复实现**：两处封存交付物的
   `Miss | Corrupt => hand-back` 行为即简报要求的降级语义（P6-014/015 记录在
   案），本轮只在其上叠加隔离与预算，未改动其文件；矩阵一致性由测试 6（与
   recovery 同构的 claimed 交还路径）与既有套件共同覆盖。

## 7. 未做与剩余风险

- **组合根接线不做（红线）**：`quarantine_detected`/`requeue_after_degrade`/
  `set_semantic_degradation` 均无 cc-server 生产调用点；reconcile/recovery 内部
  的 Corrupt 分支仍不自动隔离（跳过/交还后由组合根门面补隔离）。接线轮需要：
  worker drain 外层挂 degrade 门面 + 状态轮询转写降级快照。
- **`DegradationLedger` 为进程生命周期**：重启清零（简报双轨口径，outbox 行计数
  为持久审计）；跨进程聚合的 degraded 口径如需持久化，归接线轮（可用 metadata
  键或 coverage 读面扩展，本轮不动 schema）。
- **既有 flaky 观察（非本轮引入，记录备查）**：cc-db
  `tests/semantic_space_switch.rs` 的两个 `consume_revoke_*` 用例在多 crate
  并行全量跑下偶发 `UNIQUE constraint failed: files.file_path`——根因是该套件
  `TempDirGuard` 用 `SystemTime::now()` 纳秒值做临时目录唯一名，并行用例同刻
  创建时可能撞目录共用同一 `index.sqlite3`（测试基建竞态，非产品代码）。单跑
  该套件 5/5 稳定绿，本轮三包全量复跑亦绿。本轮 cc-db 零触碰，修复归
  P6-017 侧后续轮处理。
- `tasks.json` status 未改；未 git commit；artifacts 冻结链未触碰。
