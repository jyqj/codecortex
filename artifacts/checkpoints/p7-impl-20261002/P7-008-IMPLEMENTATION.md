# P7-008 实施记录：费用与不确定尝试收据

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-008「费用与不确定尝试收据」（批次 2 第三任务）
红线遵守：`ports.rs`/`spec.rs` 冻结面零改动（仅引用）；schema 零改动（收据只在内存聚合，未动 cc-db）；零真实网络；收据零敏感内容（input 文本/凭据有测试固化）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/admission.rs` | **本任务核心新增段**（收据层，模块 :851–1181 区段 + 测试 7 项） | — |
| — `PROVIDER_ATTEMPT_COST_UNITS` | 占位费率：1 单位/实际 provider 尝试。费率所有权从 P7-006 的循环局部计数移交收据层——真实费率替换此常量、零结构变更（P7-006 偏差 5 的「接管」承诺兑现；`RetryingProvider` 序列内计数改用同一常量） | :889 |
| — `AttemptOutcome { Succeeded, Failed, RejectedByBreaker, RejectedByBudget }` | 尝试结果四态（前两者为真实调用，后两者为调用前拒绝） | :893 |
| — `UncertainReason { BreakerOpen, TimeoutIndeterminate }` | 不确定标记两变体（口径见 §1.3） | :907 |
| — `ReceiptPath { Documents, Queries }` | 端口路径标注 | :920 |
| — `UsageReceipt { reported, estimated, cache_reuse }` | P7-003 口径的 usage 拆分载体：`reported`=provider 报告（冻结端口未透出 usage ⇒ 恒 `None`=未知）；`estimated`=声明估算器对确切批 bytes 的估算；`cache_reuse` 命中不计新费用（provider 层恒 `false`——缓存命中根本不达 provider，列保留给聚合/报表面） | :928 |
| — `UsageReceipt::billed_is_unknown` / `unknown_duplicate_risk(attempt)` | 「双缺 ⇒ 费用列 unknown 绝不填 0」不变式；「attempt>1 且 reported 缺 ⇒ 重复计费风险」（attempt 为序列内 1 基序数，自身估算不能消除该疑虑，只有 provider 报告可以） | :942 / :950 |
| — `ProviderCallReceipt` | 单次尝试结构化收据（原文见 §1.2）；字段仅计数/时长/标签/`space_model`，零文本字段 | :958 |
| — `CostBudget { max_units: Option<u64> }` + `admit(used)` | 停机阈值（degrade.rs「预算在调用前拒绝」先例同型）：达到 cap 后调用前拒绝；`Some(0)` 合法 = 全拒 | :984 / :1000 |
| — `ReceiptAggregate` + `fold_receipt` | 聚合读面：attempts/成功/失败/断路器拒/预算拒/uncertain 三列/cost_units/estimated/reported/`attempts_with_unknown_reported`/`unknown_duplicate_risk`/`cache_reuse_hits` + `per_space` 二级视图（嵌套恒空） | :1015 / :1156 |
| — `ReceiptLedger` | 有界进程内收据账本：环形容器（保最近 `capacity` 条，`new(0)` 拒绝）；`record` 由调用方供给时钟读数（无线程无定时器）；**生命周期总额**（`total_cost_units`/`budget_refusals`）跨逐出存活（供停机阈值），**聚合**只覆盖保留窗（窗口读面，两者口径分离有测试固化） | :1046 / :1067 / :1084 / :1101 / :1107 / :1134 |
| — `retained_receipts()` | 原始收据读面（聚合背后的逐条视图） | :1121 |
| — tests（7 项） | 见 §3 | — |
| `crates/cc-semantic/src/providers/openai_compatible.rs` | **挂接点**（`RetryingProvider`，所有含重试的 provider 调用的唯一漏斗） | — |
| — `CallMeta` | 每次端口调用的收据元数据（调用前测量）：space model 名、路径、批条数、P7-003 估算（逐项 `utf8-bytes-div-ceil-4-v1` 求和，与批规划器同口径）——只有计数与标签，**零 bytes** | :1407 |
| — `with_receipts(ledger, budget)` | 装配器（与 `with_clock` 链式）；无 ledger 时行为与 P7-006 逐位一致（预算无数据源则忽略） | :1462 |
| — `record_receipt` | 无 ledger 即 no-op；构造收据（reported 恒 `None`=未知不填 0）后按调用方时钟盖章落账 | :1469 |
| — `run` 重构 | 漏斗内三类收据：①断路器开路拒（:1543，cost 0 + `BreakerOpen`）；②预算停机拒（:1556，cost 0，调用前）；③每次实际尝试（时长经注入时钟测量、cost=占位费率；`Timeout` ⇒ `TimeoutIndeterminate`，:1574）。序列内 `cost_used` 改用 `PROVIDER_ATTEMPT_COST_UNITS`（费率接管） | :1514 |
| — `embed_documents`/`embed_queries` | 空批早退补注「无收据」；两调用点构造 `CallMeta` | :1685 / :1706 |
| — tests（8 项） | 见 §3 | :3400 起 |
| `crates/cc-semantic/src/lib.rs` | crate 文档 P7-008 段（仅文档） | :115–127 |

### 1.2 收据结构定义原文（admission.rs:958–981）

```rust
/// One structured receipt for ONE provider attempt (retries included —
/// every attempt gets its own receipt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCallReceipt {
    /// `VectorSpace::model_id` — configuration identity, the only label a
    /// receipt carries.
    pub space_model: String,
    pub path: ReceiptPath,
    /// Inputs in the batch (a COUNT — never their content).
    pub batch_items: usize,
    pub usage: UsageReceipt,
    /// 1-based ordinal inside ONE retry sequence (P7-006 layering: a whole
    /// sequence lives inside one outbox attempt).
    pub attempt: u64,
    pub outcome: AttemptOutcome,
    /// Attempt duration measured on the injected retry clock (0 for
    /// pre-call refusals — no round-trip happened).
    pub duration_ms: u64,
    /// Placeholder-rate charge: [`PROVIDER_ATTEMPT_COST_UNITS`] per actual
    /// provider call; 0 for pre-call refusals and cache hits.
    pub cost_units: u64,
    pub uncertain: Option<UncertainReason>,
}
```

### 1.3 不确定口径原文（admission.rs:899–918）

```rust
/// Why an attempt's billing outcome is undecidable from the caller's seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UncertainReason {
    /// The breaker refused this call, so THIS call cost nothing — but the
    /// failures that tripped the breaker may still have been processed and
    /// billed server-side, and the work will be retried (possibly after a
    /// restart) into possible double billing.
    BreakerOpen,
    /// The attempt ended in `Timeout`: whether the provider received,
    /// processed, and billed the request is undecidable from here.
    TimeoutIndeterminate,
}
```

`UsageReceipt` 不变式原文（admission.rs:938–956 摘）：

> `reported` — Tokens the PROVIDER reports having billed. `None` = unknown (the frozen port does not surface usage yet). **Never defaulted to 0.** … Any retry whose reported usage is unknown may duplicate an already-billed prior attempt — our own estimate cannot clear that doubt, only the provider's report can.

### 1.4 聚合读面

`ReceiptLedger::aggregate(since_ms: Option<u64>) -> ReceiptAggregate`（admission.rs:1134）：时间窗 `[since_ms, ∞)`（含下界）× 全量 + `per_space: BTreeMap<String, ReceiptAggregate>` 按空间二级视图。列：`attempts`/`succeeded`/`failed`/`rejected_by_breaker`/`rejected_by_budget`/`uncertain_attempts`/`uncertain_breaker_open`/`uncertain_timeout`/`cost_units`/`estimated_tokens`/`reported_tokens: Option<u64>`（窗内无任何 reported ⇒ `None`，构造上不可能 `Some(0)`）/`attempts_with_unknown_reported`（「无 usage 不填 0」显式列）/`unknown_duplicate_risk`/`cache_reuse_hits`。逐条读面 `retained_receipts()`（:1121）。**两套口径分离**：停机阈值读生命周期总额（`total_cost_units`，跨逐出存活）；聚合只覆盖保留窗（环形容器有界，逐出即不可见——测试 `ledger_is_bounded_evicts_oldest_but_keeps_lifetime_totals` 固化）。

### 1.5 持久化口径（如实声明）

**收据只在内存聚合，本轮零落盘/落库。** 简报风险节原文即预置了此路径：「P6 只落 outbox 状态与 last_error；新收据若需跨进程持久化需动 cc-db……本轮先取……方案，落库扩展列入 P7-016 后的收口可选项」。落库（outbox 行内 JSON 字段）需动 cc-db 写路径且与 schema 不改红线冲突的可能未被排除，故本轮取最小实现：进程内有界账本（`DegradationLedger` 同款「进程生命周期预算不跨重启」先例，degrade.rs:200 文档化）；**跨重启费用不确定性**的可观测口径 = outbox 既有 `attempt_count`（fenced retry 逐次 +1，重启后重试 attempt 序数继续增长）+ 收据侧 `unknown_duplicate_risk`（attempt>1 且 reported 缺失）组合暴露。跨进程收据本身不存活，如实记录为偏差 3。

## 2. TDD 流程

红绿轮次：① admission 收据层测试先行 + `todo!()` 桩 → 红轮（7 项 todo panic，既有 32 项全绿）→ 实现 → 2 个测试自身断言修正（`unknown_duplicate_risk` 期望值：attempt=2 的 timeout 失败收据恰为风险项，应为 1 非 0；时间窗下界含边界的窗口选取错误）→ 39 项全绿；② openai_compatible 收据集成测试先行 → 编译红轮（`with_receipts` 未实现，E0599 ×3）→ 实现挂接 → 一次全绿。红绿各两轮，无跳步。

## 3. 测试清单（原样，全 ok）

admission 单元（7 项新增）：

```
test admission::tests::missing_usage_is_unknown_and_never_zero_filled ... ok
test admission::tests::duplicate_risk_needs_a_retry_and_unknown_reported_usage ... ok
test admission::tests::aggregation_sums_outcomes_costs_and_tokens ... ok
test admission::tests::aggregate_splits_by_space_and_time_window ... ok
test admission::tests::cache_reuse_hits_are_counted_and_bill_nothing ... ok
test admission::tests::cost_budget_refuses_at_the_cap_before_any_call ... ok
test admission::tests::ledger_is_bounded_evicts_oldest_but_keeps_lifetime_totals ... ok
```

openai_compatible 单元（8 项新增，P7-006 ScriptedProvider/MockRetryClock 夹具复用）：

```
test providers::openai_compatible::tests::every_attempt_of_a_retry_sequence_leaves_a_complete_receipt ... ok
test providers::openai_compatible::tests::receipt_duration_is_measured_on_the_injected_clock ... ok
test providers::openai_compatible::tests::breaker_rejection_records_an_uncertain_receipt_without_provider_contact ... ok
test providers::openai_compatible::tests::timeout_attempts_carry_the_uncertain_marker_and_duplicate_risk ... ok
test providers::openai_compatible::tests::cost_budget_refuses_before_the_call_and_records_the_refusal ... ok
test providers::openai_compatible::tests::receipts_never_contain_input_text_or_credentials ... ok
test providers::openai_compatible::tests::empty_batch_records_no_receipt ... ok
test providers::openai_compatible::tests::budget_admission_boundary_is_visible_in_the_receipt_chain ... ok
```

任务 TDD 五面对照：
- **字段完整性**：`every_attempt_of_a_retry_sequence_leaves_a_complete_receipt`（attempt 序数 1/2/3、outcome 序列、批条数、估算=ceil(7/4)=2、reported=None 不填 0、cost=占位费率、space/path 标签）+ `receipt_duration_is_measured_on_the_injected_clock`（注入时钟测得 7ms）。
- **重试序列收据链（P7-006 集成）**：上一条 + `cost_budget_refuses_before_the_call_and_records_the_refusal`（Failed/Failed/RejectedByBudget 链，provider 恰 2 次调用）+ `breaker_rejection_records_an_uncertain_receipt_without_provider_contact`（断路器拒零 provider 接触）。
- **不确定标记**：`timeout_attempts_...`（双 Timeout 均 `TimeoutIndeterminate` + `unknown_duplicate_risk==1`）+ 断路器拒收据 `BreakerOpen`。
- **聚合正确性**：`aggregation_sums_outcomes_costs_and_tokens`（全列断言 + per_space + 嵌套恒空）+ `aggregate_splits_by_space_and_time_window`（时间窗 + 空间拆分 + 含下界）。
- **零泄漏**：`receipts_never_contain_input_text_or_credentials`（含 emoji/密钥样式串的 input 经 Documents+Queries 双路径后，收据 Debug + 聚合 Debug 全文断言无任何标记串）。

## 4. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-model --locked --offline
→ 17 × "test result: ok."，0 failed，零 warning。
  cc-model lib 85 passed；
  cc-semantic lib 212 passed（含本任务 15 项新增：7 admission + 8 openai_compatible）；
  cc-semantic 集成套件 17/4/4/9/3/5/5/8/6/2/1/0 全绿（publish_cas 17、
  queue_worker 9、retry_worker_layering 5 等既有套件零回归）。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.30s（零 error 零 warning）。

cargo clippy -p cc-semantic -p cc-model --locked --offline
→ 本轮新增代码零告警；既有 5 条维持原位（admission.rs:291 loop-index、
  cache.rs:102/126/127、openai_compatible.rs:239 NormPolicy derivable-impl，
  均历轮记录在案）。
```

## 5. 验收对照（acceptance）

- **「无usage不填0费用」**：`UsageReceipt.reported` 恒 `None`（冻结端口未透出 usage），`billed_is_unknown` + 聚合列 `attempts_with_unknown_reported`/`reported_tokens: None`（构造上不可能 `Some(0)`）——`missing_usage_is_unknown_and_never_zero_filled` + `every_attempt_...`（断言 `reported: None` 而非 0）双测固化。
- **「重启重试能看见费用不确定性」**：收据 `attempt` 字段（序列内 1 基）+ usage 缺失组合 ⇒ `unknown_duplicate_risk`（`duplicate_risk_needs_a_retry_and_unknown_reported_usage`）；`timeout_attempts_...` 断言聚合 `unknown_duplicate_risk == 1`。跨重启（进程）腿：outbox 既有 `attempt_count` 继续增长 + 内存收据不跨进程（如实声明，偏差 3）。
- **steps「区分 reported/estimated tokens、cache reuse 和未知重复费用」**：`UsageReceipt` 三字段 + `billed_is_unknown`/`unknown_duplicate_risk` + 聚合三列。
- **steps「停机阈值」**：`CostBudget::admit` 调用前拒绝（degrade 先例同型）+ `cost_budget_refuses_before_the_call_and_records_the_refusal` / `budget_admission_boundary_...`（`Some(0)` 全拒）。
- **deliverable「实现/配置或规格变更」**：实现变更（admission 收据层 + RetryingProvider 挂接），零配置键新增（预算/账本容量为程序化装配面，见偏差 5）。
- **deliverable「artifacts/benchmarks/<run-id>/ V15/V20 证据」**：**not_run/blocked**——V15 真实费用证据依赖 live provider（D1/D2 不授权，与 P7-001~007 同口径）；V20 依赖组合根接线（worker 接线轮）。mock 腿证据 = §3 全部测试；benchmark run-id 未生成，tasks.json evidence 不回填。

## 6. 偏差清单

1. **挂接点在 `RetryingProvider::run` 而非新建 `worker.rs`**：tasks.json scope 列有 `worker.rs`，但该文件至今不存在（P7-006 偏差 4 已裁定 worker 编排腿归接线轮）——`RetryingProvider::run` 是当前所有含重试 provider 调用的唯一漏斗（断路器准入、预算检查、退避都在此），收据在此漏斗记录即覆盖任务要求的「每次 provider 调用（含重试）」；为凑 scope 新建空 `worker.rs` 属冗余文件。worker 侧装配（drain 处构造 ledger/预算并注入）归接线轮。
2. **`AttemptOutcome` 增加 `RejectedByBudget` 变体**（任务指令列「成功/失败/断路器拒」三态）：预算停机拒与断路器拒同为「调用前拒绝」，混入 `Failed` 会误导费用归因（拒收据 cost=0）；与 degrade 先例的第四态对齐。零网络调用、不确定标记为 `None`（确定未发出）。
3. **收据跨进程不存活**：内存聚合口径（§1.5），跨重启费用不确定性退化为 outbox `attempt_count` + `unknown_duplicate_risk` 组合可观测；真跨进程收据需 cc-db 落库，与 schema 红线的取舍留 P7-016 收口（简报原文预置的可选项）。
4. **`reported` usage 恒 `None`**：冻结端口 `EmbeddingProvider` 返回值不含 usage（适配层解析了 usage 但端口签名无法透出，ports.rs 冻结不可动）——「reported/estimated 区分」的结构与聚合列已就位，真实 reported 值待端口解冻/usage 透出腿接入，届时零结构变更。
5. **零配置键新增**：`ReceiptLedger` 容量与 `CostBudget.max_units` 为程序化装配（组合根/测试构造），未进 `semantic.*` 配置节——简报草案未要求配置键，P7-002 冻结的 8+ 键面不扩散；接线轮若需操作者可调再加键（届时配套 cc-model 测试与 CONFIGURATION.md）。
6. **断路器拒/预算拒的端口错误形态**：延续 P7-006 口径，两者均暴露为循环终态 `ProviderError::ServerError`（冻结六变体无专用变体）；精确归因经收据 `AttemptOutcome` 与聚合列独立观测。
7. **`cache_reuse` 在 provider 层恒 `false`**：缓存命中不达 provider、天然零费用，该列的存在意义在聚合/报表面（简报 eval 报告列含 cache_reuse）；测试以聚合契约级断言固化「命中不计新费用」。
8. 红线确认：`ports.rs`/`spec.rs` 零改动；schema 零改动（cc-db 零触碰）；零真实网络（offline 锁定运行，无新依赖边）；收据零敏感内容（§3 零泄漏测试）；`tasks.json` status 未改；未 `git commit`；收据层零线程零定时器（时钟读数由调用方供给）。
