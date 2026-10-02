# P7-006 实施记录：有界重试与断路器

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-006「有界重试与断路器」（批次 2 首任务）
口径：零真实网络（全部内存 mock / FakeProvider / ScriptedProvider，offline 锁定运行）；重试循环与断路器均为**被调用组件**，零隐式线程/定时器，时钟经 `RetryClock` 注入；配置默认保守（调用层重试默认关、断路器默认保守开）；`ports.rs`/`spec.rs` 冻结面零改动；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/providers/openai_compatible.rs` | **本任务核心新增段**（模块 :754–1490 区段 + 测试；文件总 3118 行） | — |
| — `// ## Layering (分层口径…)` | 分层口径文档段（原文见 §2，测试固化） | :754 |
| — `RetryClock` / `SystemRetryClock` / `MockRetryClock` | 注入式时钟 trait（`now_millis` + `sleep`，等待发生在调用方线程）；mock 时钟只在测试显式推进时前进，非定时器 | :799 / :811 / :831 |
| — `RetryPolicy` / `validated` / `disabled` / `from_provider_config` | 单次重试序列的五个上界（attempts / backoff 带 / deadline / cost cap）+ 验证构造器 + `semantic.*` 映射（`retry_max_attempts=0` → disabled） | :881 |
| — `BACKOFF_JITTER_FRACTION` / `backoff_delay` / `jitter_roll` | 指数退避 `min(base×2ⁿ, max)` + 确定性抖动（splitmix64 over clock+attempt seed，只缩不涨 ≤25%，`max_backoff` 仍为硬上界） | :907 / :997 / :1011 |
| — `CircuitState { Closed, Open{remaining}, HalfOpen }` | 三态公开快照 | :1028 |
| — `BreakerLimits` / `validated` / `from_provider_config` | 断路器配置（连续失败阈值，`0`=显式关闭；开路窗长） | :1077 |
| — `CircuitBreaker` / `admit` / `record_success` / `record_failure` / `record_auth_failure` / `record_neutral` | 三态断路器：Mutex + 注入时钟的被动组件；开路窗惰性流逝（无定时器）；半开**单探测槽**；`AuthError` 任意态立即开路 ×10 窗长（`AUTH_OPEN_WINDOW_MULTIPLIER` :1024）；429/InvalidInput/Cancelled 为 neutral 不计数 | :1160 起 |
| — `RetryingProvider` / `with_clock` / `run` | `dyn EmbeddingProvider` 装饰器：断路器准入包围每次尝试；可重试类退避重试、不可重试透传、auth 不重试且开长窗；429 上报共享 gate 并在等待后消费 `Suspended` 信号（至多等待一次即停，不泊线程对抗冷却） | :1321 / :1381 |
| — tests（20 项） | 见 §4 | 尾部 |
| `crates/cc-semantic/src/lib.rs` | P7-006 crate 文档段（仅文档，无新模块） | 头部 |
| `crates/cc-semantic/tests/retry_worker_layering.rs` | 新集成套件（5 项）：分层口径 × drain 全链 / C11 / admission 批次流 / 断路器 fast-fail | 全文件（508 行） |
| `crates/cc-model/src/config.rs` | `semantic.*` 新增 8 键（见 §3）+ Default + tests（1 新增 + unknown-key 面板更新） | :106–171 / :210–218 / :1430 起 |
| `crates/cc-server/src/service_factory.rs` | 组合根断路器单例（与 `ProviderGate` 同点装配、共享生命周期）：`semantic_circuit_breaker` / `init_semantic_circuit_breaker`（first-wins 幂等，惰性默认 = 保守断路器 5×30s）+ `Arc::ptr_eq` 单例测试 | :73 / :80 / :97 / :221 |
| `docs/CONFIGURATION.md` | `semantic` 节 8 键文档（C14 义务） | 语义配置节表 |

### 1.2 断路器三态语义（定案）

- **Closed**：一切调用放行；连续可重试失败（`ServerError`/`Timeout`）计数，成功清零；达到 `breaker_failure_threshold`（默认 5）→ **Open**（`now + breaker_open_ms`，默认 30s）。429（配额问题，归 gate 暂停管）、`InvalidInput`（输入形状）、`Cancelled`（调用方取消）**不计入**——三者均非"provider 已坏"的证据。
- **Open**：`admit` 直接 `Rejected{retry_in}`——快速失败**不触 provider**（`RetryingProvider` 返回一次 `ProviderError::ServerError` 即终，不烧重试预算，测试 `tripped_breaker_fails_fast_in_the_drain_without_provider_or_attempt_cost` 固化零 provider 调用零多余 DB attempt）。窗口流逝在下次 `admit`/`state` 读取时**惰性**转入半开（无定时器推动）。
- **HalfOpen**：一次只放行**一个探测调用**（单探测槽，并发后来者 `Rejected`）；探测成功 → Closed（计数清零）；探测失败 → 重开一个新窗。
- **AuthError**：任意态立即 Open，窗长 = `breaker_open_ms × 10`（凭据不会秒级自愈；恢复 = 修凭据后组合根重初始化）。
- 全局单实例：组合根 `semantic_circuit_breaker()` 单例（与 gate 同一 `service_factory` 装配点、同一 first-wins 语义），`the_circuit_breaker_is_one_process_wide_shared_instance` 固化 `Arc::ptr_eq`。

### 1.3 重试循环与 429 gate 冷却的衔接

每次端口调用开一个全新序列：断路器准入 → 内层调用 → 按 `ProviderError` 变体路由（表见 `RetryingProvider` 文档）→ 预算检查（cost cap → attempts → deadline）→ 等待（`RateLimited` 且尊重开关时等 `Retry-After`，否则指数退避+抖动）→ **等待后**检查共享 gate `snapshot().suspended_for`：仍暂停则立即返回最后错误（信号消费完成，fenced retry 接手；至多一次等待即停）。常见路径中"等待即冷却"（本调用自己的 429），等待后冷却恰好自然到期、循环继续——`rate_limited_waits_retry_after_feeds_the_gate_and_resumes_after_expiry`（真实 gate + 真实时钟 30ms 冷却）与 `a_suspended_gate_stops_the_loop_instead_of_parking`（外来 3600s 冷却，mock 时钟，1 次调用 1 次等待即停）双向固化。

### 1.4 与 outbox attempt 预算的分层（口径原文）

`openai_compatible.rs:754` 模块文档段（分层口径冻结原文）：

> - **One outbox attempt = one `EmbedHandler::handle` call = at most one
>   full call-layer retry sequence.** The sequence's own bound
>   ([`RetryPolicy::max_attempts`], its `total_deadline`, its optional
>   cost cap) is freshly granted at the start of every outbox attempt;
>   whatever happened in the previous attempt does not shrink it.
> - **The call layer NEVER writes queue state.** [`RetryingProvider`] holds
>   no DB handle, lease, or transaction (structural: its public surface
>   accepts none) — every failure it finally gives up on is returned as a
>   plain [`ProviderError`], and the queue's fenced retry
>   (`retry_semantic_task`) is what increments `attempt_count` by exactly
>   ONE per outbox attempt, no matter how many provider calls the call
>   layer made inside it.
> - **The gate cooldown is reported, not waited out, at this layer.** …

测试固化：`call_layer_retry_publishes_within_a_single_db_attempt`（1 个 attempt 内 3 次 provider 调用 → `attempt_count == 1`、`done`、无 last_error）与 `call_layer_exhaustion_hands_back_and_the_db_budget_dead_letters`（每 drain 烧满 3 次调用层预算但 attempt 只 +1；DB 预算 2 耗尽 → `failed` 死信、永不再 claim）。

### 1.5 C11（等待不跨 DB 事务）

结构性保证：`RetryingProvider`/`CircuitBreaker` 公开面不接受任何 DB 句柄/锁/事务（与 gate 同构），等待全部经注入时钟发生在调用方线程。行为固化：`retry_waits_hold_no_db_lock_and_the_fenced_write_still_lands`——重试等待窗（~80ms 真实退避）期间，独立连接成功完成 `BEGIN IMMEDIATE…COMMIT` 写事务（证明等待方未持任何锁），且等待后 lease 仍有效、fenced 写照常落库。

## 2. TDD 流程

红绿轮次：① cc-model 8 键测试先行（1 红轮：新键缺 Default 字面量）→ 补键转绿；② 重试/断路器单元测试与实现同批（2 个编译红轮：`BreakerPhase` Default 推导、ScriptedProvider 返回类型）+ 2 个设计修正（deadline 测试对抖动的敏感性 → deadline 取 175ms 抖动无关值；`Suspended` 消费时序从"等待前"改为"等待后"——原设计会使带 gate 的 429 重试立即自停，与"等待即冷却"语义矛盾）；③ 集成套件 1 个语义红轮（backoff=0 使同一 drain 立即再领取 → max_batch 钉 1）→ 绿。

## 3. 配置键（`semantic.*`，P7-006）

| 键 | 默认 | 含义 |
|---|---|---|
| `retry_max_attempts` | `0` | 调用层重试总尝试次数（**一次 outbox attempt 内**）；`0` = 调用层重试关闭（默认，保守值：重试花钱花延迟）。 |
| `retry_base_backoff_ms` / `retry_max_backoff_ms` | `500` / `8000` | 指数退避基值/上限（上限 ≥ 基值否则拒启）。 |
| `retry_total_deadline_ms` | `30000` | 单次序列墙钟总预算。 |
| `retry_respect_retry_after` | `true` | 429 的 `Retry-After` 是否覆盖该次退避。 |
| `retry_max_cost_units` | — | 单次序列费用封顶（占位费率 1 单位/次，P7-008 收据层接入后接管）。 |
| `breaker_failure_threshold` / `breaker_open_ms` | `5` / `30000` | 断路器连续失败阈值（`0`=显式关闭）/ 开路窗长。 |

不对称口径（文档化于 config 注释与 CONFIGURATION.md）：**重试默认关**（改变成本/时延行为），**断路器默认开**（只对持续失败生效、永不影响成功路径）。

## 4. 测试清单（原样，全 ok）

单元（`providers::openai_compatible::tests`，20 项新增）：

```
retryable_server_errors_are_retried_within_the_bound
non_retryable_errors_pass_through_without_a_second_call          (InvalidInput/Cancelled 透传)
auth_error_is_not_retried_and_trips_the_long_open_window         (×10 长窗 + 快速失败零触 provider)
retry_budget_exhaustion_returns_the_last_error
total_deadline_ends_the_sequence_before_the_attempt_bound        (mock 时钟)
cost_cap_ends_the_sequence
rate_limited_waits_retry_after_feeds_the_gate_and_resumes_after_expiry (真实 gate 30ms)
a_suspended_gate_stops_the_loop_instead_of_parking               (3600s 外来冷却, 1 调用 1 等待即停)
without_respecting_retry_after_the_backoff_is_exponential
backoff_is_exponential_capped_and_jitter_only_shrinks            (纯函数: 100/200/400/800/800 与 75/150/300/600/600)
each_call_gets_a_fresh_sequence_budget_layering
disabled_policy_makes_exactly_one_attempt
empty_batch_is_a_no_op_without_attempts_or_breaker_contact
invalid_policy_combinations_are_rejected_with_named_reasons
consecutive_failures_open_the_circuit_which_fails_fast_without_provider_contact
success_resets_the_consecutive_failure_count
open_window_elapsed_lazily_becomes_half_open_with_a_single_probe_slot
half_open_probe_failure_reopens_for_a_fresh_window
disabled_breaker_never_opens_and_neutral_outcomes_change_nothing
breaker_and_retrying_provider_are_send_sync_shareable
```

集成（`tests/retry_worker_layering.rs`，5 项）：

```
call_layer_retry_publishes_within_a_single_db_attempt
call_layer_exhaustion_hands_back_and_the_db_budget_dead_letters
planned_batches_keep_fresh_retry_budgets_and_the_gate_wired      (admission 批次流 × 重试 × gate)
retry_waits_hold_no_db_lock_and_the_fenced_write_still_lands     (C11)
tripped_breaker_fails_fast_in_the_drain_without_provider_or_attempt_cost
```

cc-model：`semantic_retry_and_breaker_keys_parse_with_conservative_defaults`（新增）+ `semantic_section_keys_are_known_config_keys`（8 新键入 unknown-key 面板）等。
cc-server：`the_circuit_breaker_is_one_process_wide_shared_instance`（新增）。

## 5. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-model --locked --offline
→ 17 × "test result: ok."，0 failed。
  cc-model lib 84 passed（含 semantic_retry_and_breaker_keys... 等 5 semantic 项）；
  cc-semantic lib 176 passed（含本任务 20 项）；
  retry_worker_layering 5 passed；queue_worker 9 / publish_cas 17 / reconcile 等
  既有集成套件零回归。

SDKROOT=... cargo test -p cc-server --features semantic --locked --offline --lib
→ 253 passed / 0 failed（含 provider_gate_tests 两项单例测试）。
  备注：首轮全量跑出现 1 例与本轮触碰面无关的失败（93s 长跑、输出经 grep
  过滤未能留存测试名），立即重跑 253/253 全绿、再次局部重跑亦绿——判定为
  既有时序敏感测试偶发（历轮 admission 套件同类现象见 P7-005 记录"连跑
  3 次全绿"），非本轮引入；未修复（与本任务无关的失败不修）。

SDKROOT=... cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s)（零 error）。

cargo clippy -p cc-semantic -p cc-model --locked --offline
→ 本轮新增代码零告警；既有 5 条维持原状（cache.rs:102/126/127、
  openai_compatible.rs:218 NormPolicy、admission.rs:291 loop-index，
  均历轮记录在案，非本轮触碰）。
```

## 6. 验收对照（acceptance）

- "重试次数、deadline 和费用封顶" → `RetryPolicy` 三上界 + `retry_budget_exhaustion_returns_the_last_error` / `total_deadline_ends_the_sequence_before_the_attempt_bound` / `cost_cap_ends_the_sequence`。
- "429/5xx/timeout 退避与 Retry-After" → 路由表 + `rate_limited_waits_retry_after...` / `retryable_server_errors...` / 退避纯函数测试。
- "auth/永久错误暂停" → `auth_error_is_not_retried_and_trips_the_long_open_window`（断路器长窗即进程级暂停机制；worker 侧 degraded 透出归接线轮，见偏差 4）。
- "失败原因公开脱敏" → 错误沿既有 `provider_reason`（queue.rs）单行白名单口径；断路器开路暴露为 `ServerError`（冻结六变体无专用变体，断路器状态经 `CircuitBreaker::state()` 独立观测）。
- V15（`artifacts/benchmarks/<run-id>/`）→ **not_run/blocked**：live 腿依赖真实 provider（D1/D2 本轮不授权，与 P7-001~005 同口径）；本轮工程级证据为上列测试，benchmark run-id 未生成、tasks.json evidence 不回填。

## 7. 偏差清单

1. **失败率阈值未实现，仅连续失败阈值**：任务指令"连续失败/失败率阈值"取连续失败（`breaker_failure_threshold`）一支——确定性强、无需滑动窗口状态；失败率变体记为将来可选项，未预实现。
2. **`Suspended` 消费时序与直觉相反（等待后而非等待前）**：等待前检查会使带 gate 的 429 重试序列立即自停（本调用自己的冷却把自己挡住）；"等待即冷却"使常见路径自然通过，仅外来/更长的冷却在至多一次等待后被消费。已文档化并双测固化。
3. **断路器开路错误形态**：冻结六变体无"断路器开路"专用变体，开路快速失败暴露为 `ProviderError::ServerError`（重试循环内一次性返回、不再重试）；精确状态经 `CircuitBreaker::state()` 观测。若未来解冻端口可复议。
4. **worker 侧 auth 暂停/degraded 透出未在本轮接线**：简报"AuthError → … worker 暂停新 claim（degraded 透出，复用 degrade.rs 门面）"的 worker 编排腿归接线轮（P7-010/P7-014）——本轮断路器已提供等价的进程级长窗暂停机制，drain 形状零改动（P7-005 先例：acquire→claim 装配同批归接线轮）。
5. **`retry_max_cost_units` 为占位费率**（1 单位/次 provider 尝试）：真实计费口径归 P7-008 收据层；字段与封顶机制已就位，费率替换不动结构。
6. **WorkerLimits/queue.rs 零改动**：调用层重试完全经 `dyn EmbeddingProvider` 装饰器注入，`EmbedHandler` 透明消费——分层隔离的结构性证明（queue 层甚至不知道重试存在）。
7. **cc-server 首轮全量测试 1 例无关失败**：93s 长跑中出现 1 failed（grep 过滤未留存名），重跑 253/253 全绿两次，判定既有 flaky；按约定不修、如实记录。
8. 红线确认：`ports.rs`/`spec.rs` 零改动；零真实网络（无任何生产 transport）；`tasks.json` status 未改；未 `git commit`；重试/断路器零线程零定时器（时钟注入、窗口惰性流逝）；默认构建 cc-server 不含 cc-semantic（断路器单例在 `#[cfg(feature="semantic")]` 内）。
