# P6-004 实施记录：扩展类型化 write effects（Index/Evidence/Semantic/Auxiliary）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md` P6-004、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003，第 92-95 行四类
  typed write effects 与第 140 行 P6-004 约束）、P6-002/003 实施记录。
- 改动范围：仅 `crates/cc-db`（4 个文件），零 schema 变更（P6-005 才加表），零既有
  写路径行为变化。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/epoch_rules.rs:40-122` | 新增 | `WriteEffect` 封闭枚举（`Index`/`Evidence`/`Semantic`/`Auxiliary`，commit 级规则 doc）+ `EffectSet(u8)` 位集（`EMPTY`/`of`/`contains`/`union`、`From<WriteEffect>`、`BitOr`、自定义 `Debug` 如 `EffectSet(Index|Semantic)`） |
| `crates/cc-db/src/epoch_rules.rs:306-310` | 新增 | 测试 helper `generation_triple`（strict 读 `(index, evidence, semantic: Option<u64>)`） |
| `crates/cc-db/src/epoch_rules.rs:320-344` | 修改 | `assert_bumps` 改读 `ReadGeneration`（三钟），新增"声明的钟之外 `semantic_epoch` 不得移动"断言——既有 Index/Evidence 审计全部自动叠加语义钟守卫 |
| `crates/cc-db/src/epoch_rules.rs:518-610` | 新增 | 5 个新测试（见 §4） |
| `crates/cc-db/src/index_db_types.rs:17-22` | 新增 | `SEMANTIC_EPOCH_KEY = "semantic_epoch"` 常量 + None≠0 严格读契约 doc（经 `index_db.rs:139` 的 `pub use crate::index_db_types::*` 自动入面） |
| `crates/cc-db/src/index_db.rs:627-640` | 新增 | `bump_semantic_epoch_on(&Connection)`（复用既有 `bump_epoch_on`，`None`→`1` 起步，`read_generation.rs:52` 的 SELECT 已覆盖该键） |
| `crates/cc-db/src/unit_of_work.rs:10-14` | 修改 | 模块 doc 补 `commit_with` 语义段 |
| `crates/cc-db/src/unit_of_work.rs:43,73-104` | 修改 | `commit()` 保留原签名与语义，委托 `commit_with(EffectSet::of(Index))`；新增 `commit_with(effects: EffectSet)`——每个声明效应在**同一提交事务内**恰 bump 一次；`Auxiliary`/`EMPTY` 不动任何钟 |

未改动的既有事实：`UnitOfWork` 唯一真实生产调用方是
`crates/cc-index/src/synthesis_pipeline.rs:110`（begin）与 `:125`（commit）——全部走
`commit()`，行为逐位等价（`bump_index_epoch_on` → `COMMIT` 顺序不变）。
（P6-005 订正：`signature_agg.rs:1140` 与 `index_db_edges.rs:1330,1683` 均位于
`#[cfg(test)] mod tests` 内，是测试调用点而非生产调用方；本节原"三个生产调用方"
计数有误。）
`snapshot_write_txn.rs` 全量重建 seam 未触碰（其 epoch 由 floor 向量协议管辖，模块
doc 明示不适用 `bump_*_on`）。

## 2. Q4 定案（ADR-0003 组合 WriteEffect 语义，本轮显式裁决）

简报与 ADR 对以下三点未完全定案，按"最小惊讶 + 可测试"裁决如下，全部以测试固化：

1. **组合效应 `{Index, Semantic}` = 单事务内两个钟各 bump 恰好一次**。
   不存在"一次 bump 代表两个钟"或共享计数的语义；`commit_with` 对每个声明效应
   独立调用对应 `bump_*_on`，同一 `IMMEDIATE` 事务内提交。固化测试：
   `combined_index_and_semantic_commit_bumps_each_exactly_once`
   （epoch_rules.rs:592）、`commit_with_index_and_semantic_bumps_both_exactly_once`
   （unit_of_work.rs:270，含真实数据写入）。
2. **重复 ack（可见集合未变）不 bump = 声明式表达，cc-db 不做可见集合 diff**。
   `EffectSet` 是提交者对效应的**声明**；"semantic 可见集合是否实际变化"的判定
   责任在发布编排方（P6-011 publish CAS，其 `ack_done_on` 本身是 Auxiliary，bump
   由同事务内确认可见集合变化后才声明 Semantic——简报 P6-011 风险段既有口径）。
   cc-db 层固化的是机制：**不声明 Semantic 效应的提交绝不 bump semantic_epoch**。
   固化测试：`repeat_ack_without_visible_change_does_not_bump_semantic_epoch`
   （unit_of_work.rs:303：首次声明 Semantic → `Some(1)`；重复 ack 只声明
   Auxiliary → 仍 `Some(1)`）。若未来需要 db 层原子封装，按简报 P6-011 草案
   `ack_and_publish_on(...) -> PublishOutcome` 收进 cc-db，不在本轮。
3. **`Auxiliary` 与空 `EffectSet` 是 epoch 等价的**，区别仅是声明意图：
   `Auxiliary` 位让 bookkeeping-only 提交（claim/renew/heartbeat/retry）能显式
   说明自身类别（审计/日志可读），`EMPTY` 表示什么都没声明；两者都推进零时钟。
   固化测试：`auxiliary_commit_advances_no_clock`（epoch_rules.rs:541，V13 审计）、
   `empty_effect_set_commits_without_touching_any_clock`（epoch_rules.rs:559）。

**与 semantic_epoch 的关系**：`Semantic` 效应首次 bump 把键从缺失写为
`'1'`（`bump_epoch_on` 的 INSERT 路径）——strict 读语义下
`None`（键缺失 = 未就绪）与 `Some(0)` 结构性不可混淆：
`semantic_effect_starts_the_clock_from_none_to_one`（epoch_rules.rs:574）断言
首 bump 结果恰为 `Some(1)`。键名 `semantic_epoch` 与
`read_generation.rs:52` 已就绪的读路径字面量一致（该 SELECT 由本轮测试首次
真实走到写侧，读侧代码零改动）。

## 3. 守卫审计（简报风险段："任何 `unwrap_or(0)` 都要 grep 排查"）

全 workspace `grep -rn "semantic_epoch" crates/` 结果（本轮后）：

- `crates/cc-model/src/generation.rs:12` — `Option<u64>` 类型定义，None 语义 doc 在位；
- `crates/cc-server/src/handlers/freshness.rs:159` — 测试 fixture 字面量 `None`；
- cc-db 内：`read_generation.rs`（读，None→`None` 不折叠）+ 本轮新增写侧；
- **零处 `unwrap_or(0)` 或将 `Option<u64>` 折叠为 0 的读路径**。简报风险项闭环。

## 4. 测试清单（本轮新增 8 个，正反例）

模块级（`crates/cc-db/src/epoch_rules.rs`）：

1. `effect_set_membership_and_union_are_exact`（:523）——位集正反例：contains 精确、
   union 幂等/交换序、`From<WriteEffect>` 等价；
2. `auxiliary_commit_advances_no_clock`（:541）——Auxiliary 提交后三钟逐项不变（V13）；
3. `empty_effect_set_commits_without_touching_any_clock`（:559）——空集零时钟；
4. `semantic_effect_starts_the_clock_from_none_to_one`（:574）——`None`→`Some(1)`，
   且 index/evidence 不动；
5. `combined_index_and_semantic_commit_bumps_each_exactly_once`（:592）——Q4 定案 1。

`crates/cc-db/src/unit_of_work.rs`：

6. `default_commit_keeps_index_only_semantics_and_semantic_absent`（:250）——零行为
   侵入守卫：`commit()` 后 index+1、evidence 不动、semantic 键仍缺失；
7. `commit_with_index_and_semantic_bumps_both_exactly_once`（:270）——Q4 定案 1
   （含真实写 + 事务内数据落库验证）；
8. `repeat_ack_without_visible_change_does_not_bump_semantic_epoch`（:303）——Q4 定案 2；
9. `commit_with_evidence_bumps_evidence_only`（:326）——Evidence 效应单钟推进。

既有测试强化（非新增）：`assert_bumps`（epoch_rules.rs:325）对全部 10 个既有
Index/Evidence 审计叠加 `semantic_epoch` 不变量；`drop_without_commit_rolls_back...`
（unit_of_work.rs:220）追加 `semantic_epoch == None` 断言（rollback 不推进语义钟）。

## 5. 验证命令与结果（原样）

1. TDD 红：`SDKROOT=... cargo test -p cc-db --locked --offline`（实现前）
   → `error[E0432]: unresolved imports super::EffectSet, super::WriteEffect`、
   `error[E0599]: no method named commit_with` 共 16 errors（API 未实现）。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-db --locked --offline`
   → 全部套件 `ok, 0 failed`：lib `155 passed; 0 failed`（+1 ignored）及 7 个集成
   套件（13/4/4/6/1/2 passed 等，doc-tests `0 passed`），零 warning。
3. `cargo test -p cc-db --locked --offline epoch_rules` → `10 passed; 0 failed`。
4. `cargo test -p cc-db --locked --offline unit_of_work` → `8 passed; 0 failed`。
5. `SDKROOT=... cargo test -p cc-index --locked --offline`（UnitOfWork 直接消费方）
   → 全套件 `ok, 0 failed`：lib `377 passed`（+1 ignored）及 8 个集成套件。
6. `SDKROOT=... cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile ... in 10.67s`，零 warning。
7. `SDKROOT=... cargo check -p cc-db --locked --offline` → `0` warning。

## 6. 零行为侵入证明

- 默认 `commit()` 签名未变、委托路径 `bump_index_epoch_on` → `COMMIT` 逐位等价
  （unit_of_work.rs:73-80）；唯一真实生产调用方 `cc-index/src/synthesis_pipeline.rs:110/125`
  无需改动（其余引用点为测试调用点，见 §1 订正）。
- 既有测试**零断言修改**即绿：`epoch_rules::declared_clock_matches_observed_bump_for_every_table`、
  `evidence_method_family_bumps_evidence_only`、`boost_http_edge_confidence_bumps_evidence_clock`、
  `unit_of_work_commit_bumps_index_exactly_once`、`unit_of_work::commit_applies_all_writes...`、
  `signature_agg::unit_of_work_synthesis_writes_converge` 等全部原样通过。
  （两处测试内部实现细节调整：`drop_without_commit...` 追加一行语义钟断言、
  `assert_bumps` 换用 strict 读——均只加强、不放松。）
- cc-index 全量 377 lib 测试绿（唯一生产消费方）。
- schema 未动：`index_migrate.rs` 零改动，无新表/新列/新迁移（P6-005 边界）。
- `semantic_epoch` 键在本轮前**无任何写侧**（简报 P6-004 现状锚点属实），本轮后
  仅有 `commit_with(… Semantic)` 一条写路径，且生产代码无调用方——纯 Index 路径
  物理上不可能产生该键。

## 7. 与简报偏差清单

1. `EffectSet` 不提供 `EffectSet::index()` 等具名构造常量：单一来源走
   `EffectSet::of(WriteEffect::Index)`（`From<WriteEffect>` 同价），减少平行 API 面。
2. `Auxiliary` 作为显式位而非"空集即 Auxiliary"：简报草案 `bit0..3 对应四类` 本就
   含 Auxiliary 位，落实现时保留并显式记录二者 epoch 等价（Q4 定案 3）。
3. 简报"是否引入轻量 `AuxTx`"评审项：**本轮不引入**。现有 claim/renew/heartbeat
   尚无 cc-db 调用方（P6-007 才落），提前封装违反最小改动；维持
   `IMMEDIATE` 短事务 + 跳过 bump 的既有模式，P6-007 实施时再评审。
4. `EffectSet` 自定义 `Debug`（如 `EffectSet(Index|Semantic)`）替代派生：测试失败
   消息可读性需要，无行为影响。
5. 简报验收项"扩展 `assert_bumps` 支持四效应"：未把 `EpochClock` 参数改成四态枚举，
   而是保留钟级审计 + 叠加语义钟不变量，另以 `EffectSet` 级独立测试覆盖四效应
   （钟审计与效应审计分层，既有审计零断言修改）。

## 8. 未做与剩余风险

- `commit_with` 尚无生产调用方（P6-006 源码事务 = 第一个 `{Index, Semantic}` 调用
  点）；本轮 API 未被任何真实组合路径消费，Q4 定案 2 的"可见集合 diff 归发布方"
  责任在 P6-011 落地时才首次被执行方承接。
- `IndexGeneration`（`index_db.rs` 旧双钟向量）未加 semantic 维度：全量重建 floor
  协议（`finalize_rebuild_generation`）对 semantic_epoch 的处理（换库后语义钟如何
  携带/重置）属 P6-005/P6-014 范围，本轮不裁决。
- V13 正式验证证据（`artifacts/benchmarks/<run-id>/`）属收口轮产物；本轮为命令级
  测试证据。未改 `tasks.json` status，未 git commit，冻结链 artifacts 未触碰。
