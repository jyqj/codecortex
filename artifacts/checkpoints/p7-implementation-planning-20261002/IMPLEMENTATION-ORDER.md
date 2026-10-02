# P7-001~020 实施顺序与批次划分

> 规划日期：2026-10-02。只读规划产物，不改 `crates/`、`docs/roadmap/code-index-v2/tasks.json` 或锁定链。
> 依据：`docs/roadmap/code-index-v2/tasks.json` P7 全部任务（tasks.json:9294-10147，20 项，status 全 `todo`）、
> `02-CONTRACTS.md`（C10-C14）、`06-VALIDATION.md`（V05/V09/V11/V14-V21、G7）、
> `artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json`（接线轮 13 项待办表权威位置）、
> `artifacts/checkpoints/p789-blocking-analysis-20261002/DECISIONS-RECORDED.json`（D1+D2/D3 用户拍板）。
> 逐任务设计见同目录 `TASK-BRIEFS.md`；未决事项见 `OPEN-QUESTIONS.md`。
> 文档风格延续 `artifacts/checkpoints/p6-implementation-planning-20261002/`。

## 0. 批次 0 前提（已满足，核对记录）

P6 已于 2026-10-02 收口（round13）：`P6-020 status=done`，G6 =
`passed_declared_local_scope_library_layer`（`docs/roadmap/code-index-v2/P6-GATE.json`），
计数 192 = 140 done / 0 in_progress / 52 todo，`next_task=P7-001`（tasks.json:45
execution_note 尾段，明确"仅为依赖图指针"）。因此 P7 全链 `depends_on: ["P6-020"]`
前提成立，**无需批次 0 动作**，可直接开工。

本轮开工前的三条既有口径（非动作，是约束）：

1. **D1+D2 已拍板**（DECISIONS-RECORDED.json `decisions[1]`）：本轮不授权真实付费
   provider；报告双轨——local 完整发布 + live 证据栏显式 blocked。已落 tasks.json
   implementation_notes：P7-007（tasks.json:9545 块）与 P7-018（tasks.json:10014 块）。
   由此 P7-018 维持 conditional 不执行；P7-007 只做执行机制腿（见第 3 节）。
2. **P6 库层完成、组合根未接线**：`crates/cc-semantic` 已有 ports 冻结面
   （`crates/cc-semantic/src/ports.rs:99`）、FakeProvider 六变体故障注入
   （`crates/cc-semantic/src/providers/fake.rs:78`）、cache/publish CAS/queue/
   recovery/gc/space_switch/degrade（lib.rs 模块注释逐条列明，lib.rs:1-64）；
   cc-server 侧只有纯数据槽（`crates/cc-server/src/service_factory.rs:24` 的
   `semantic: RwLock<Option<Arc<dyn SemanticRecall>>>`、service_factory.rs:41
   `SemanticDegradation`），`semantic` feature 已声明未接线
   （crates/cc-server/Cargo.toml:35,38）。接线轮 13 项待办表全文在
   `artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json`
   `wiring_round_pending_table.items`——P7 阶段必须消纳（落位见第 2 节）。
3. **冻结纪律**：P5-020 建立 F0 冻结锚点、P6 各批末增量重冻结；P7 延续——
   每批结束在新 SHA 重冻结一次，批内共享（P6 惯例，见 P6 规划 IMPLEMENTATION-ORDER.md
   批次 1 节）。`ports.rs`+`spec.rs` 属 P6 冻结面：P7-001/P7-002 的 scope 虽列
   ports.rs/spec.rs，但**新增类型不改既有签名**；若确需改 `EmbeddingProvider`
   （ports.rs:99-106）签名，按 P6 冻结协议重跑冻结并记录。

## 1. 拓扑与批次总览

`tasks.json` 的 batch 字段给出 P7-A~P7-D 四个声明批次；`depends_on` 在全部 20 个
任务上构成**一条严格串行链**（001→002→…→017，随后 018/019 两个分支，020 收口），
另有三条跨批冗余边（P7-006 额外依赖 P7-001；P7-011 额外依赖 P6-020+P7-009；
P7-016 额外依赖 P6-020）。**批内没有任何 depends_on 允许的并行点**；唯一天然并行
出现在 P7-017 之后（018 与 019 互不依赖），但 018 已裁决不执行，实际只剩 019。

| 批次 | 任务（串行链） | 声明 batch | 批内可并行点 | 主题 |
|---|---|---|---|---|
| 1 | P7-001 → 002 → 003 → 004 → 005 | P7-A | 无 | provider 适配与批次规划 |
| 2 | P7-006 → 007 → 008 → 009 → 010 | P7-B | 无 | 重试/政策/费用/查询缓存/召回接线 |
| 3 | P7-011 → 012 → 013 → 014 → 015 | P7-C | 无 | 范围/融合/deadline/全链贯通/竞争测试 |
| 4 | P7-016 → 017 →（018 blocked 记账 ∥ 019 fake 腿） | P7-D | 018 仅剩记账，与 019 可并列收口 | 故障矩阵/离线默认/消融 |
| 5 | P7-020 验收 + G7（双轨报告） | P7-D | — | 闭环与发布范围 |

依据说明：

- 批内顺序完全由 `tasks.json` `depends_on` 推导，未放宽任何边。可选放宽候选
  （均需 owner 改 tasks.json，本规划不代行，见 `OPEN-QUESTIONS.md` Q2）：
  - `P7-006 depends_on P7-005`：重试/断路器只消费 provider 错误分类
    （ports.rs:81-92）与 worker 编排，与并发限流耦合最弱；
  - `P7-016 depends_on P7-015`：fake 全故障矩阵主要消费 P6 已交付的
    queue/recovery/gc 与 P7-014 的接线面，竞争压力测试（015）非其全部前提。
  两者放宽收益有限（链已按主题分段），默认不放宽。
- P7-019 的 `depends_on: ["P7-017"]` 保持不放宽：消融跑的是整包二进制
  （`crates/cc-eval/src/benchmark/ablation.rs:1-2`"full-factorial over separately
  built local MCP binaries"），离线默认包验证（017）是其前置。019 内部拆
  **fake 消融腿（本轮执行）**与**正式质量腿（live，blocked）**，见第 3 节。
- P7-018 `conditional`（tasks.json:10014 块）+ D1 拍板 → 整任务不执行，
  只做 blocked 记账（报告栏位与 tasks.json 状态回填），不创建 live manifest。

## 2. 组合根接线（13 项待办）的落位

结论：**13 项不落在单独"接线批次"，而是两波消纳——P7-010 最小装配腿 +
P7-014 全链贯通腿（主体）**，P7-005/P7-011/P7-015/P7-016 各消纳与其主题同源
的个别项。理由：接线的最小不可拆单元是"为注入 `SemanticRecall` 而构造语义
子系统"（P7-010 的任务本体），而配置面/status/MCP/调度是 P7-014 的任务本体；
P7-015 的竞争测试是调度装配的验收腿，P7-016 的故障矩阵是收窄/页帽化的回归腿。

逐项落位（编号 = round12 审计 `wiring_round_pending_table.items[].no`）：

| # | 待办项 | 落位任务 | 理由（代码锚点） |
|---|---|---|---|
| 1 | try_init 组合根接线与语义子系统初始化 | **P7-010**（最小装配：feature 开 + 配置存在才构造，产出可注入的子系统实例）；**P7-014**（完整 try_init：配置键驱动启停、全量初始化语义） | `SemanticHandle` 仍是零尺寸占位（`crates/cc-semantic/src/lib.rs:92-95`，lib.rs:87 注明真实构造函数"added together with the artifact cache"之后）；注入槽已就绪（service_factory.rs:84 `set_semantic`、engine.rs:310 `set_semantic_recall`） |
| 2 | drain/reconcile/recovery/GC 调度时机 | **P7-014**（组合根调度点落地），**P7-015**（竞争测试为其验收腿） | 四原语均为 caller-driven 显式 API：`drain_pending`（queue.rs:243）、`reconcile_after_rebuild`（reconcile.rs:152）、`recover_scan`（recovery.rs:209）、`run_gc_pass`（gc.rs:531）；lib.rs:26-31 注明"whether and when to drain stays the composition root's decision" |
| 3 | worker drain 外层挂 degrade 门面 | **P7-014** | `quarantine_detected`（degrade.rs:412）/`requeue_after_degrade`（degrade.rs:435）已导出，无生产调用点；与 2 同一装配点 |
| 4 | 降级快照转写（DegradationLedger::snapshot → QueryServices::set_semantic_degradation） | **P7-010**（随最小装配，一行桥接） | 消费槽已就绪：service_factory.rs:87 `semantic_degradation()`、capability_status.rs:100 `apply_semantic_degradation`；生产写入点缺失即 round12 审计 `unwired_honesty` 所述状态 |
| 5 | cache 根目录与项目身份传入 | **P7-010** | `resolve_cache_root_with`（cache.rs:128/:143）+ `namespace_key`（cache.rs:96）已是纯函数；组合根传参即可 |
| 6 | GC 宽限 min_retention_secs 配置面 + 非零下限校验 | **P7-014** | 配置结构化校验归 C14 面（02-CONTRACTS.md:114-118）；P7-002 建立的 provider 配置节同模式 |
| 7 | GC 审计计数落库（Auxiliary 计数，按需） | **P7-014**（status 透出需要）；若证明只对验收有意义可降级为 P7-016 记录 | `run_gc_pass`（gc.rs:531）返回计数；"按需"口径见 round12 审计第 7 项出处 |
| 8 | 语义 worker 公平化 ORDER BY 注入 | **P7-005** | 与"项目公平队列"同主题（P7-005 steps）；claim 面 `claim_next_on`（`crates/cc-db/src/semantic_outbox.rs:496`） |
| 9 | 机会性 reclaim 收窄 | **P7-016**（建议，待 owner 确认） | `reclaim_expired_on`（semantic_outbox.rs:653）；收窄属有界性行为修正，由故障矩阵回归覆盖；备选 P7-008。见 `OPEN-QUESTIONS.md` Q3 |
| 10 | dead_letter/coverage.failed 页帽化 | **P7-014** | 均为 status/query 表面字段（V18 新字段贯穿链）；页帽化防 status 爆量 |
| 11 | 有界 desired 投影回接 reconcile | **P7-015**（建议，待 owner 确认） | 竞争测试需要回填 desired 集有界；备选 P7-016。见 `OPEN-QUESTIONS.md` Q3 |
| 12 | dense lane 对覆盖率的消费（范围声明/查询守卫） | **P7-011**（任务本体）+ **P7-012**（coverage 透出） | `SourceVerifier`（`crates/cc-search/src/evidence.rs:105`）、`LaneCoverage`（`crates/cc-model/src/retrieval.rs:222`）已就绪 |
| 13 | semantic_space_switch_log 上限与切换组合根触发面 | **P7-014**（触发面）；上限参数随 P7-002/P7-014 配置校验 | `register_backfill_space`/`enqueue_backfill`/`activate_space`/`drain_space_revocations`（space_switch.rs:60/:73/:84/:116）均 caller-driven |

配套义务：P7-014 收口时在 tasks.json implementation_notes 与 round12 审计之间
建立双向对账（13 项逐条标 done/移交），避免 P6 式"归接线轮"悬置再次滚动。

## 3. D1/D2 政策腿的双轨处理

- **P7-007（执行机制腿，本轮执行）**：默认无网络（未 opt-in 不构造 transport）、
  密钥只外部引用（配置存引用不存明文）、日志零 key/源码、重定向不泄露认证。
  政策文本按"默认无网络 + 显式 opt-in"收口；敏感文件外发分类矩阵标 blocked
  （D2 未授权）。V15/V18 只采 local 证据（`06-VALIDATION.md:59`："对没有真实
  provider 的环境，P7 工程集成可验证，live 认证标 blocked/deferred"）。
- **P7-018（conditional，不执行）**：不创建 live manifest、不做任何真实调用；
  在 P7-020 报告的 live 栏显式记 blocked，引用 DECISIONS-RECORDED.json 与本文件。
  tasks.json 状态回填口径：blocked（非 done、非 todo），由 owner 收口时落。
- **P7-019（拆两腿）**：
  - fake 消融腿（本轮执行）：以 FakeProvider 作为 dense 后端跑
    local/dense/hybrid 三臂机制消融，产出 heldout/exact 退化/费用三列的
    **机制验证报告**——结论只证明"消融管线、费用记账、CI 机制可用"，**不得
    解释为语义效果**（V15 fake/live 区分，06-VALIDATION.md:37；G7"fake全过，
    live受授权单独记"，06-VALIDATION.md:57）。
  - 正式质量腿（blocked）：真实公开 corpus + 真实 provider 的 V19 全口径
    （hard negatives/holdout/CI），依赖 P7-018 型授权，归 P8 live 线
    （D3 拍板只解决 P8 语料来源，不解锁 live provider）。
- **P7-020（双轨报告模板）**：G7 报告分 `engineering/fake profile` 与
  `live semantic-effect` 两栏；local 栏填实测证据，live 栏逐项
  `blocked（DECISIONS-RECORDED.json D1+D2）`；"无live证据不声称已证明真实语义
  收益，本地发布不被模型密钥强绑"（tasks.json P7-020 acceptance，tasks.json:10104 块）。

## 4. 每批的验收门、触碰面与冻结闭包影响

### 批次 1（P7-A：P7-001~005，provider 适配轮）

- **触碰面**：新增 `crates/cc-semantic/src/providers/openai_compatible.rs`、
  `admission.rs`；`ports.rs` 仅增量类型（冻结面）；`spec.rs`（参数校验）；
  `crates/cc-model/src/config.rs`（首个 provider 配置节，现无任何语义键——
  CONFIGURATION.md:207-208 如实记载"没有语义相关键"）；
  `crates/cc-index/src/documents/render.rs`（消费侧，按 P7-003 最小触碰）；
  `crates/cc-server/src/service_factory.rs`（P7-005 限流器单例挂点）。
- **验收门 G1-P7**：V15 协议 stub 成功/错误映射（provider 可替换——trait 对象
  安全已有测试 ports.rs:203）；V18 参数显式校验不伪成功；V09 按最终输入
  bytes/token 计 batch；V20 共享限额多项目受限。批末重冻结（F7-1）。
- **风险面**：HTTP 客户端 crate 的放置（cc-semantic 依赖地板 cc-model+cc-db，
  lib.rs:66-67）——本规划取 transport seam 方案，见 `OPEN-QUESTIONS.md` Q1，
  该裁决影响 P7-001 能否按草案落码。

### 批次 2（P7-B：P7-006~010，重试/政策/接线轮）

- **触碰面**：`openai_compatible.rs`（重试/断路器包装）、新增 `worker.rs`、
  `policy.rs`（P7-007）、`admission.rs`（费用收据）、`spec.rs`+`cache.rs`
  （query 向量缓存）、`crates/cc-search/src/lanes/semantic_adapter.rs`
  （已有 receipt 校验，semantic_adapter.rs:11，补生产 recall 后端）；
  **cc-server 生产代码首次接线**：`service_factory.rs`（P7-005 已碰 +
  P7-010 最小装配：待办 1 部分/4/5）。
- **验收门 G2-P7**：V15 重试次数/deadline/费用封顶、失败原因脱敏；V20 多项目
  总并发受限；V11 query cache 键完整（instruction 变更失效、不跨空间）；
  V16 乱序/重复 index/NaN/zero 拒绝（含 P7-004 深化项，若批次 1 未全绿在此
  补齐）；V18 日志零 key。批末重冻结（F7-2），首次含 cc-server 生产面。
- **红线条目**：provider 调用不持 DB 锁/连接（C11，02-CONTRACTS.md:87-91）；
  错误向量不缓存（校验先于 cache.rs:340 `put`）；降级结果不进完整结果缓存
  （C12，02-CONTRACTS.md:93-107 表）。

### 批次 3（P7-C：P7-011~015，范围/融合/贯通轮）

- **触碰面**：`crates/cc-semantic/src/vector/exact.rs`（filter-before-topk
  已有，lib.rs:18-21，补守卫语义）+ `crates/cc-search/src/evidence.rs`
  （hydrate 二次校验）；`fusion.rs`（dense 独立 RRF）+ `crates/cc-model/src/context.rs`
  （coverage explain 字段）；`execution.rs` + `handlers/context.rs`
  （总 deadline/退化）；`tools.rs`+`capability_status.rs`+`docs/MCP_TOOLS.md`
  （全链贯通：待办 1 完整/2/3/6/7/10/13）；新测试
  `crates/cc-eval/tests/semantic_lifecycle.rs` + `sampler.rs`（竞争测试）。
- **验收门 G3-P7**：V05/V16 semantic 找回不越 hard 范围、不被 soft scope 误删；
  V11/V19 timeout 与无命中可区分、declared full coverage 有证据、不混
  cosine/BM25；V11/V15 总 deadline 取消传播、auto 回 local/explicit 语义明确
  不足、故障不缓存成完整成功；V18 未配置/关闭/回填/失败/就绪五态真实一致、
  新字段 schema/sanitize/dispatch/status/文档/E2E 一体、原 mode 不变；
  V14/V17/V20 慢模型不饿死 local、过期发布 0。批末重冻结（F7-3，含 cc-model
  context 兼容面）。

### 批次 4（P7-D：P7-016~019，故障矩阵/离线/消融轮）

- **触碰面**：`semantic_lifecycle.rs` + 新 `mcp_v2_contract.rs`（可重放 seed
  全故障矩阵，消费 fake.rs:78 脚本化故障）；`crates/cc-server/Cargo.toml` +
  `crates/cc-eval/tests/benchmark_adapters.rs`（离线默认包正式回归）；
  `ablation.rs` + `artifacts/benchmarks/`（fake 消融腿）；018 只产出 blocked
  记账（无代码）。
- **验收门 G4-P7**：V14/V15/V17/V18 组合故障可恢复、每条 seed 可重放、进程级
  SIGKILL 正式化（P6 留边界）；V18/V21 零 key/feature 关/enabled=false 全本地、
  不创建语义 cache 文件；V19/V20 fake 消融机制可跑、费用三列齐全、报告不
  声称语义效果。批末重冻结（F7-4）。
- **P6 移交的收口项在此批兑现**：待办 9（P7-016）、待办 11（P7-015，若按
  建议落位）——连同批次 3 的 2/3/6/7/10/13，13 项全部出账。

### 批次 5（P7-020 + G7）

- **动作**：按 06-VALIDATION.md 第 6 节模板填 `P7-*-GATE.json`；G7 =
  "V15–V19；fake全过，live受授权单独记；默认离线零网络；语义覆盖和费用可解释"
  （06-VALIDATION.md:57）；双轨报告（第 3 节模板）；13 项待办对账定稿；
  tasks.json P7 状态回填 + `current_phase` 推进核对（见 `OPEN-QUESTIONS.md` Q8）。
- **硬失败口径不变**（06-VALIDATION.md §4）：scope 泄漏、错误版本、删除复活、
  错误空间、缓存历史改变确定性结果等仍阻断；fake 分数不得抵扣 live 声明。

## 5. 串行/并行结论

- **严格串行主干**：001→002→003→004→005→006→007→008→009→010→011→012→013→
  014→015→016→017→019→020（019 fake 腿）；018 不在执行图上，仅记账。
- **无批内并行**。唯一形式并行：批次 4 内 018 记账与 019 收口互不依赖。
- **可选前置起草**（不违反 depends_on 的只读工作）：P7-020 双轨报告模板骨架、
  G7 检查脚本、P7-019 的消融 variant 定义可在批次 3 起草；结论性文本/证据
  必须晚于对应任务收口。
- **owner 不可委托范围**：组合根两波接线（P7-010/P7-014 的全部 server 侧改动）、
  ports/spec 冻结面的任何变更、provider 配置节定名（C14 兼容面）、政策腿文本。
  可委托候选：P7-016 故障矩阵用例编写（在 014 接线面冻结后）、P7-019 fake 腿
  的跑批执行。

## 6. 每批通用回滚（tasks.json 通用 rollback 的落地口径）

四批通用：关闭远程语义与重试（配置不 opt-in → 组合根不构造 transport/子系统），
切 local；保留 outbox/cache 和费用收据供恢复（outbox 行是持久审计轨，
TROUBLESHOOTING.md 语义节既有口径）。与 P6 的差异：P7 后组合根已接线，
"关闭"从"不装配"变为"配置关闭 + 已构造实例停机"——P7-014 的配置面必须包含
运行时关闭路径（capability_status 回到 `not_configured`，不残留半开状态），
该点纳入 G3-P7 的 V18 五态验收。
