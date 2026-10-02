# P6-002~020 实施顺序与批次划分

> 规划日期：2026-10-02。只读规划产物，不改 `crates/`、`docs/roadmap/code-index-v2/tasks.json` 或锁定链。
> 依据：`docs/adr/0003-semantic-persistence-single-db-boundary.md`（边界权威，含逐任务约束表）、
> `docs/roadmap/code-index-v2/tasks.json` P6 各任务、`02-CONTRACTS.md`、`06-VALIDATION.md`、
> `artifacts/checkpoints/todolist-p6-owner-preparation-20261001/REQUIREMENTS-PREPARATION.json`。
> 逐任务设计见同目录 `TASK-BRIEFS.md`；未决事项见 `OPEN-QUESTIONS.md`。

## 0. 批次 0 前提（硬门，先于一切 crates/ 改动）

**红线：P5-020 收口后、批次 1 开始前，任何 `crates/` 改动都会使 P5 冻结闭包失效，触发整链重认证。**

现状核对（2026-10-02）：

- `tasks.json` `current_phase=P5`、`next_task=P5-019`（in_progress）、`P5-020 status=todo`
  （tasks.json:45 `execution_note` 与 tasks.json:8018 起）。即 **P6 全链的 `depends_on: ["P5-020"]`
  前提尚未成立**——这也是 ADR-0003 第 193-197 行如实记录的冲突 1。
- 现行冻结闭包：`06-VALIDATION.md:7` 记载"冻结 623 文件、6675050 字节，摘要
  `44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`"，证据目录
  `artifacts/benchmarks/p5d-20260930-resume/final-v3`。委托方描述为"629 文件冻结闭包"，
  仓库内未找到 629 的记载——**以哪个数字/哪个 SHA 作为批次 0 重认证基线，列
  `OPEN-QUESTIONS.md` Q2 待裁决**；本规划按"批次 0 产出新冻结闭包"编写，数字以届时收口为准。
- `tasks.json:45` 另记：P5-D 旧冻结仅覆盖接受时源码，后续变更需重新冻结验收。

批次 0 的动作（全部属于 P5 收口轮，不属于 P6 实施）：

1. 完成 `P5-019`（in_progress）并按 tasks.json 既有验收收口 `P5-020`（G5）。
2. 在收口 SHA 上重新冻结源码闭包（新的文件数/字节/digest/命令收据），证据落
   `artifacts/benchmarks/<run-id>/`。该 SHA 即"冻结锚点 F0"。
3. F0 之后 `crates/` **零改动**直达批次 1 第一个提交；任何插入改动 = 重走批次 0。
4. `P6-001` 状态回填裁决：ADR-0003 已交付但 `tasks.json` 仍 `todo`（ADR 第 188-189 行明确
   回填留给收口轮）。批次 1 放行条件之一是 `P6-001` 被 owner 记为 done（含文档同步义务的
   处置，见 `OPEN-QUESTIONS.md` Q5）。

**验收门 G0-P6（批次 0 出口）**：P5-020 status=done 且带 G5 证据；F0 冻结闭包落盘；
P6-001 状态回填完成。未满足则批次 1 不得开工。

## 1. 拓扑与批次总览

`tasks.json` 的 batch 字段给出 P6-A~P6-D 四个声明批次；`depends_on` 在批内构成串行链。
结合跨批依赖（P6-006/P6-011/P6-016 均额外依赖 `P5-020`）与 ports 冻结委托点
（REQUIREMENTS-PREPARATION.json `delegatable_after_ports_freeze`），实施顺序为
**批次 0（前提）+ 5 个实施/验收批**，共 19 个任务：

| 批次 | 任务（串行链） | 声明 batch | 批内可并行点 |
|---|---|---|---|
| 0 | 前提：P5-020 收口 + F0 冻结 + P6-001 回填 | — | — |
| 1 | P6-002 → P6-003 → P6-004 → P6-005 | P6-A | 无（depends_on 全链） |
| 2 | P6-006 → P6-007 → P6-008 →（P6-009 ∥ P6-010） | P6-B | ports/spec 冻结后 009/010 可并行（009 可整体委托） |
| 3 | P6-011 → P6-012 → P6-013 → P6-014 → P6-015 | P6-C | 无（depends_on 全链） |
| 4 | P6-016 → P6-017 → P6-018 →（P6-019 文本可提前起草、随 018 收口） | P6-D | 019 正文起草可与 016~018 重叠，final 版必须晚于 018 |
| 5 | P6-020 验收 + G6 | P6-D | — |

依据说明：

- 批内顺序完全由 `tasks.json` `depends_on` 推导，未擅自放宽任何边。若 owner 想在批次 2
  提并行度，唯一有据可依的放宽候选是 `P6-008 depends_on P6-007`（cache 只消费 P6-003 的
  spec digest，不消费 claim/lease 状态），放宽需 owner 修改 `tasks.json`，本规划不代行
  （见 `OPEN-QUESTIONS.md` Q6）。
- P6-009/P6-010 的并行与委托以 REQUIREMENTS-PREPARATION.json `delegatable_after_ports_freeze`
  为准：`providers/fake.rs`（含故障测试）与 `vector/exact.rs`（含 oracle 测试）在 owner 稳定
  ports/spec 后可排他委托，两者永不触网，结果不计为真实语义质量（ADR 第 117-119 行）。

## 2. 每批的验收门、触碰面与冻结闭包影响

### 批次 1（P6-A 尾：P6-002~005，schema/底座轮）

- **触碰面**：
  - 新增 `crates/cc-semantic/`（Cargo.toml、lib.rs）+ 根 `Cargo.toml` workspace members
    （Cargo.toml:3-11）+ `crates/cc-server/Cargo.toml`（feature `semantic` + optional dep）。
  - `crates/cc-model/src/identity.rs`（P6-003：VectorSpace/编码 spec 冻结）。
  - `crates/cc-db/src/epoch_rules.rs`、`crates/cc-db/src/unit_of_work.rs`（P6-004：四类
    typed write effects）。
  - `crates/cc-db/src/sql/index_v1.sql`、`crates/cc-db/src/index_migrate.rs`
    （P6-005：新表 + `CURRENT_SCHEMA_VERSION` 21→22）。
- **验收门 G1-P6**：V18（14 工具不丢、默认无网络无 key）、V21（默认/semantic 两包、
  schema 降级重建、cache 不误读）、V10/V16（spec digest 或 oracle 的第一批单测）、
  V13（Aux 不刷 index、commit 恰好推进预期 epoch）。附加硬检查：默认构建产物中不存在
  cache 目录（P6-002 验收"不生成空缓存"）。
- **对冻结闭包的影响**：这是 F0 之后第一次触碰 `crates/`。批次 1 结束即需在新 SHA
  重冻结（F1）——涉及 `Cargo.toml`/`Cargo.lock`、cc-db schema、cc-model identity，
  均属既有 V01/V21 冻结语义；cc-eval/cc-search 本批**零触碰**（保持旧功能回归面最小）。
  批次 1 内所有提交共享 F1 重认证一次，批内不再逐 commit 重冻结。

### 批次 2（P6-B：P6-006~010，队列与可委托件轮）

- **触碰面**：
  - `crates/cc-index/src/documents/delta.rs` 消费方（P6-006 的 outbox 挂接点在 cc-db 写
    路径；`delta.rs` 本体是纯函数，见 `crates/cc-index/src/documents/delta.rs:1`，本批
    尽量不改其签名，新增消费在 cc-db/document 批写路径）。
  - `crates/cc-db/src/semantic_outbox.rs`（新模块；P6-006/007）。
  - `crates/cc-semantic/src/queue.rs`、`cache.rs`、`spec.rs`、`ports.rs`、
    `providers/fake.rs`、`vector/exact.rs`（新）。
- **验收门 G2-P6**：V13/V14（原子 outbox、claim CAS、过期 lease、删除不发 embedding、
  两进程互斥）、V10/V16（cache 复用与损坏检测、filtered exact oracle）、V15（fake
  provider 全状态转移可重现）。**ports 冻结门**：`ports.rs`+`spec.rs` 接口在 P6-008
  收口时冻结（F2a），之后 009/010 委托实现，owner 只 review 不改接口。
- **对冻结闭包的影响**：cc-index 只动 documents 写路径的下游（若 `delta.rs` 零改动则
  cc-index 冻结面=0）；cc-db 新增 `semantic_outbox.rs` 属纯增量模块；cc-semantic 内部
  文件不影响其他 crate 的 API 面。批次 2 产出冻结 F2。
- **红线条目**（照 ADR/REQUIREMENTS hard_risks 逐条落实）：
  - 源码事务内原子 supersede manifest + desired outbox；删除文档永不产生 embedding 请求。
  - heartbeat/retry/renew 全部走 Auxiliary effect，不刷任何检索 epoch。
  - provider 调用期间不持有 DB 锁/连接（02-CONTRACTS C11，`02-CONTRACTS.md:89-91`）。

### 批次 3（P6-C：P6-011~015，发布与恢复轮）

- **触碰面**：`crates/cc-semantic/src/publish.rs`、`reconcile.rs`、`worker.rs`、
  `admission.rs`（新）；`crates/cc-db/src/semantic_outbox.rs`（CAS 写侧）、
  `epoch_rules.rs`（Semantic effect 审计）、`index_db_rebuild.rs`
  （finalize 时 renew incarnation 已存在于 `crates/cc-db/src/read_generation.rs:42-48`，
  P6-014 只补 fencing 消费）；`crates/cc-server/src/capability_status.rs`；
  `crates/cc-eval/tests/semantic_lifecycle.rs`（新集成测试）。
- **验收门 G3-P6**：V14（artifact-before-manifest、五重 fencing、慢旧结果不挂新版本、
  发布幂等、重复 ack 被吸收）、V13（覆盖率分母、semantic epoch 只在可见集合变化时推进）、
  V17（换库不误删付费向量、旧 DB 时代回包被拒）、V20（批量保存不无限排队）。
- **对冻结闭包的影响**：本批首次引入 `cc-eval` 语义生命周期集成测试；`capability_status.rs`
  输出新增字段属 V18 兼容面（新字段贯穿 schema/sanitize/dispatch/status，照 C14、
  `02-CONTRACTS.md:114-118`）。批次 3 产出冻结 F3。

### 批次 4（P6-D：P6-016~019，GC/切换/降级/文档轮）

- **触碰面**：`cache.rs`（GC mark/sweep、降级隔离）、`publish.rs`（GC 协调点）、
  `spec.rs`+`reconcile.rs`（space 切换三段）、`capability_status.rs`（degraded 状态）、
  `docs/internals/STORAGE.md`、`docs/internals/CONCURRENCY.md`、`docs/TROUBLESHOOTING.md`。
- **验收门 G4-P6**：V17（无"manifest 引用刚被 GC 删除产物"竞态、孤儿最终可回收、
  恢复可复算）、V16（切换期不同空间分数永不混排、旧 cache 经校验回滚复用）、
  V18（degraded 语义透出且不冒充完整结果）、V21（文档事实与实现一致、不宣称
  exactly-once/零重复收费）。P6-019 同时完成 ADR-0003 第 173-176 行挂起的
  `DESIGN.md`/`STORAGE.md` 章程文本同步义务。
- **对冻结闭包的影响**：文档改动不触发源码重冻结；`cache.rs`/`publish.rs` 行为改动
  在 F3 基础上做增量收口（F4）。

### 批次 5（P6-020 + G6）

- **动作**：fake provider 故障矩阵 + exact oracle 全量复放；依赖图与默认包检查
  （`cargo tree` 证明默认包不含 cc-semantic、无网络依赖 crate、无第二库文件创建）；
  按 `06-VALIDATION.md` 第 6 节模板填 `P6-*-GATE.json` 证据；`tasks.json` P6 状态回填。
- **验收门 G6**（`06-VALIDATION.md:54`）：V13/V14/V16/V17 无网络闭环、正式单库修订 ADR、
  显式重建/GC/fencing 证据。fake 完成不冒充真实 provider 效果（`06-VALIDATION.md:59`）。

## 3. 串行/并行结论

- **严格串行主干**：002→003→004→005→006→007→008→011→012→013→014→015→016→017→018→019→020。
- **唯一批内并行点**：批次 2 中 P6-009 与 P6-010（前提：ports/spec 已冻结）；P6-009 可
  整体委托，P6-010 可整体委托，两者互不依赖（各自只依赖 008 产出的 cache/ports 面）。
- **可选前置起草**（不违反 depends_on 的只读工作）：P6-019 文档骨架、P6-020 检查脚本
  可在批次 3 起草，但结论性文本/证据必须在对应任务收口之后。
- **owner 串行集成范围**（ADR 第 111-116 行）：cc-semantic Cargo/lib/spec/ports、cc-model
  identity、cc-db generation/schema/migration/outbox/claim/CAS/rebuild、cc-index 原子
  document delta、cc-server 组合根与 capability status、cache/publish/GC/reconcile 协调
  与全部 rollback——以上不得委托；可委托仅限 009/010 两件。

## 4. 每批通用回滚（tasks.json 通用 rollback 的落地口径）

禁用语义 worker（feature 关闭/组合根不装配）并撤销新 manifest；保留派生 cache；
索引按 incarnation 重建（既有 `rebuild_with_temp_db` 协议，`crates/cc-db/src/index_db_rebuild.rs:302`）。
关掉 semantic feature 后系统回到"无限定单库"的 P5 行为，cache 目录成为无害孤儿
（可整目录删除），主库语义表按 P6-005 重建路径处理（ADR 第 158-161 行）。
