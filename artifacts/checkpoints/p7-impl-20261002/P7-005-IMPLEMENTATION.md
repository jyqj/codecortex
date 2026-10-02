# P7-005 实施记录：全局与项目并发限流

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-005「全局与项目并发限流」
口径：D1/D2 —— 零真实网络（gate 是纯进程内准入原语，无任何传输/连接）；限流器是**被调用组件**，零隐式常驻线程/进程；配置键默认关闭/保守值；`tasks.json` status 未改；未 git commit；`ports.rs`/`spec.rs` 冻结面零改动。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/admission.rs` | **本任务核心新增段**（P7-003 批次规划器零改动，同文件追加 gate 段） | — |
| — `// ## Thread model (Q4 定案)` | 线程模型定案文档段落（全文见 §3） | :397 |
| — `GateLimits` / `GateLimits::validated` / `permissive()` | 两级上限的验证构造器：`max_concurrent ≥ 1`；`max_concurrent_per_project` 为 `Some` 时必须 ≥1 且**严格小于**全局上限（`cap ≥ max` 永远无法阻止单项目占满 gate，正是 V20 禁止的饿死形态，配置错误拒启）；`permissive()` = 默认关闭（unlimited，准入永不等待） | :432 / :455 / :444 |
| — `GateAcquireError { Timeout{waited}, Suspended{cooldown_remaining}, EmptyProjectIdentity }` | 准入三分类；不是 provider 错误，由调用方显式路由 | :492 |
| — `ProviderGate` / `ProviderPermit` | 进程单例共享限流器（组合根装配，**每调用一个独立无限信号量**被结构性排除——permit 只能从共享 gate 获得）+ RAII 许可（Drop 释放并唤醒公平队列） | :553 / :561 |
| — `ProviderGate::validated` / `from_provider_config` | fallible 构造器；`semantic.*` 配置键映射（`!enabled || max_concurrent==0 → Ok(None)` = 限流默认关闭） | :581 / :596 |
| — `try_acquire_permit(project, wait)` / `acquire_permit(project)` | 超时/取消语义：调用方线程上有界等待（`Timeout`）或立即失败（`Suspended`）；`acquire_permit` 无限等待（只有暂停/空身份可失败） | :617 / :693 |
| — `note_rate_limited(retry_after)` / `resume()` / `snapshot()` | 429 协同（见 §4）+ 观测快照 | :712 / :735 / :745 |
| — `core_scan`（公平队列授权扫描） | FIFO 服务等待者但**跳过项目份额暂时打满的队头**（no head-of-line starvation）；新调用者在有人等待时必须排队（no barging） | :791 |
| — tests（16 项 gate 新增） | 见 §5 | :937 起 |
| `crates/cc-model/src/config.rs` | `semantic.*` 并发键（P7-002 配置节先例延续） | — |
| — `SemanticProviderConfig.max_concurrent` | `0` = 不限流（默认，保守值）；`≥1` = 硬信号量 | :95 |
| — `SemanticProviderConfig.max_concurrent_per_project` | `0` = 不限（默认）；设置时必须严格小于 `max_concurrent` | :101 |
| — `SemanticProviderConfig.acquire_timeout_ms` | 默认 30000；只约束**准入等待**，不约束进行中的 provider 调用 | :107 |
| — tests | 默认惰性/显式解析/unknown-key 面板干净（既有 3 测试更新 + 新增 1） | :1395 起 |
| `crates/cc-db/src/semantic_outbox.rs` | 接线轮待办 8 出账（P6-013 偏差 3"公平化 ORDER BY 可注入参数"） | — |
| — `ClaimFairness { Fifo (default), DocRoundRobin }` | claim 候选 ORDER BY 的**闭集枚举**（无任意 SQL 片段注入面）；`DocRoundRobin` = 按每文档 `MAX(updated_at)` 升序（含本行，永非 NULL）、`task_id` 决胜 | :492 |
| — `claim_next_fair_on(..., fairness)` | `claim_next_on` 全语义保持、仅候选排序可注入；`claim_next_on` 委托 `Fifo`（**零行为变更**，SQL 文本等价 `ORDER BY task_id ASC`） | :557 |
| `crates/cc-db/src/semantic_queue.rs` | `IndexDb::claim_semantic_ordered(owner, lease, fairness)` 门面；`claim_semantic` 委托默认 `Fifo`；同一 IMMEDIATE 短事务 | :71 |
| `crates/cc-semantic/src/queue.rs` | `WorkerLimits.claim_order` 字段（`validated` 内默认 `Fifo`，全部 14 处既有调用点零改动）+ `with_claim_order` builder；`drain_pending` 改走 `claim_semantic_ordered` | :99 / :151 / :271 |
| `crates/cc-semantic/tests/queue_worker.rs` | drain 级公平化测试（默认 FIFO 语义不变 / DocRoundRobin 热文档让位）+ 既有结构体字面量断言补字段 | 尾部 |
| `crates/cc-db/tests/semantic_queue.rs` | facade 级轮转序断言（retry 刷新 `updated_at` 造成"到达序 ≠ 触碰序"，FIFO 取旧 task_id、轮转取最久未触碰者） | 尾部 |
| `crates/cc-server/src/service_factory.rs` | 组合根单例挂点（`cfg(feature="semantic")`，默认构建零 cc-semantic 依赖，维持 no-network closure） | — |
| — `provider_gate::{semantic_provider_gate, init_semantic_provider_gate}` | `OnceLock<Arc<ProviderGate>>`：惰性默认 = permissive（限流默认关闭）；`init` 显式 limits、first-wins 幂等；单例 `Arc::ptr_eq` 测试 | :34 / :63 / :145 |
| `crates/cc-semantic/src/lib.rs` | P7-005 crate 文档句（模块既有，无新模块） | 头部 |
| `docs/CONFIGURATION.md` | `semantic` 节三键文档（C14 义务：新键可见） | 语义节 |

### 1.2 两级限流语义（原文口径）

- **全局级**：`max_concurrent` 是进程内共享 provider 调用的硬上限。任一时刻 `in_flight < max_concurrent` 才可能授予（fast path 仅在等待队列为空时生效）。
- **项目级**：`max_concurrent_per_project` 是单项目份额；授予条件为该项目 `in_flight < share`。份额 `<` 全局上限是**结构性**反饿死保证：单项目永远无法持有全部许可（`GateLimits::validated` 拒绝退化配置）。
- **公平队列**：严格 FIFO 服务等待者；队头项目份额打满时**跳过**它服务后续可授权者（公平轮转、无队头阻塞）；等待期间到达的新调用者一律入队尾（禁止插队）。因此"单项目不能饿死其他索引"由份额上限 + skip-blocked-head 共同保证。
- **等待/取消**：等待发生在调用方自己的线程上，有界（`wait`，来自配置 `acquire_timeout_ms` 的调用方缺省）且显式失败（`Timeout{waited}`）；**等待绝不发生在持锁区间**（C11）——gate 公开面不接受任何 DB 句柄/锁/事务（结构性质），接线顺序定为 **acquire → claim**（准入先于 `LeaseGuard::claim`），等待窗口不可能跨越 lease 或 SQL 事务；测试 `admission_wait_never_spans_a_db_transaction` 以真实 rusqlite 事务固定该调用形状。

### 1.3 429 协同（降档/暂停语义，原文）

`note_rate_limited(retry_after)`（`admission.rs:712`）：

> 429 协同 (pause/降档): the provider answered `RateLimited` carrying a
> `Retry-After` (P7-001 maps the header into `ProviderError::RateLimited`). The gate records a GLOBAL cooldown — the quota being exhausted is the shared provider account's, not one project's — and admission then fails fast for everyone until it expires. Repeated reports EXTEND the pause to the latest deadline (max, never shorter). In-flight permits are not revoked; they finish naturally. The retry/backoff loop that consumes this signal is P7-006's.

即：**暂停 = 全局冷却 + 准入快速失败**（`Suspended{cooldown_remaining}`，不泊线程——worker 把任务经 fenced retry 交还队列，重试节奏归 P7-006）；**降档 = 两级上限本身**。冷却自然到期恢复；`resume()` 仅供运维/测试提前解除。`GateAcquireError::Suspended` 文档（:498）明示 fail-fast 语义。

### 1.4 接线轮待办 8 出账（claim 公平化 ORDER BY 注入）

- **落位**：`ClaimFairness` 闭集枚举 + `claim_next_fair_on`（`semantic_outbox.rs:492/:557`）+ `IndexDb::claim_semantic_ordered`（`semantic_queue.rs:71`）+ `WorkerLimits.claim_order`（`queue.rs:99`）。
- **默认 `Fifo`**：`ORDER BY task_id ASC`，与 P6-007 交付语义等价，全部既有调用点（`claim_semantic`、`LeaseGuard::claim`、recovery/reconcile/space_switch）零行为变更。
- **`DocRoundRobin`**：`ORDER BY (每文档 MAX(updated_at)) ASC, task_id ASC`——服务"最久未被触碰"的文档；被连续重编辑/重试的文档其行 `updated_at` 持续刷新、自动让位于等待更久的文档。无需 schema 变更（pending 行自身的 `updated_at` 保证表达式非 NULL）。
- **范围裁决**：outbox 是单库单项目，claim 面没有跨项目维度——简报"项目公平队列"的**跨项目**部分由 `ProviderGate` 的份额上限 + 公平队列承担；ORDER BY 注入承担**项目内文档**轮转。两处口径均在枚举文档（`semantic_outbox.rs:489`）与 `WorkerLimits.claim_order` 文档（`queue.rs:93`）明示。
- round12 审计 `wiring_round_pending_table.items[no=8]` 就此出账；P7-014 收口时随 13 项对账表双向核销。

## 2. TDD 流程

红绿轮次：① 先写 cc-model 三键测试（1 红轮：新字段缺 Default 字面量）→ 补键转绿；② gate 测试先行（4 个红轮修正：`Mutex/Condvar/Arc` 导入、`consume` 借用、`Result<Permit,_>` 不可 `assert_eq!` 改 `matches!`、两处测试设计错误——permit 未持有即释放导致不重叠、批预算把 4 输入合并成 1 批）；③ cc-db 轮转测试一次绿（`claim_next_on` 委托路径零回归同轮确认）；④ drain 级公平化测试一次绿。

## 3. Q4 线程模型定案（文档段落原文，`admission.rs:397`）

```text
// ## Thread model (Q4 定案)
//
// `EmbeddingProvider` is a SYNCHRONOUS trait (`ports.rs:99-106`, frozen):
// an embed call blocks its calling thread for the network round-trip. C11
// (`02-CONTRACTS.md:87-91`) forbids running that call while holding any DB
// lock, connection, or SQL transaction. The split of responsibility this
// round freezes is:
//
// - **The gate only admits.** `ProviderGate` is a passive, called
//   component: it bounds how many provider calls may be in flight and in
//   which fair order admission is granted. It spawns no thread, timer, or
//   daemon (ADR red line: no implicit resident process) — a caller waits on
//   its OWN thread inside `ProviderGate::try_acquire_permit`, and the wait
//   ends in bounded time (`GateAcquireError::Timeout`) or immediately
//   (`GateAcquireError::Suspended`, the 429 pause).
// - **The caller decides the execution context.** This round's only callers
//   are the composition root's EXPLICIT drains (`queue.rs`: claim → renew →
//   handler → fenced ack; no resident loop). Whether those handlers run
//   directly on the drain caller's thread or on a dedicated bounded blocking
//   pool (`std::thread` pool / `spawn_blocking`) is entirely the composition
//   root's decision (P7-010/P7-014 wiring); the gate neither knows nor cares.
//   The query path (P7-013) must NOT inline a blocking encode on a query
//   thread — the planning default (OPEN-QUESTIONS Q4 ②) stands: cache misses
//   leave the dense lane `Unavailable`, encoding happens worker-side.
// - **Admission waits never span DB state.** The gate holds no DB handle,
//   lock, or transaction (structural: its public surface accepts none), and
//   the wired worker order is acquire-then-claim: a caller acquires its
//   permit BEFORE `LeaseGuard::claim` so the wait can never outlive a lease
//   or sit inside a transaction. The tests below fix that call shape.
```

测试固化：`gate_is_send_sync_and_shareable_across_threads`（Send/Sync + 被动组件多线程共享）、`admission_wait_never_spans_a_db_transaction`（C11 调用形状）、`gated_batch_flow_limits_global_concurrency_and_keeps_projects_moving`（调用方自管线程 + 显式 drain 形状 + 批次流集成）。

## 4. 与 P7-001~004 / P6 交付的衔接

- **P7-001**（transport 注入 + 429→`RateLimited{retry_after}`）：gate 是 `retry_after` 的第一消费者（`note_rate_limited`）；P7-006 的 `RetryingProvider` 是第二消费者（重试节奏），两者经共享单例生命周期协同（TASK-BRIEFS P7-006"与 ProviderGate 同点装配"）。
- **P7-002**（`semantic.*` 配置节）：三键并入同一节、同一 default 惰性语义、同一 unknown-key 诊断面板。
- **P7-003**（admission 批次切分）：`gated_batch_flow...` 测试把 `plan_document_batches` 的批逐个经 gate 送入 FakeProvider（`delay_per_call=25ms`），两级上限在并行 worker 下同时成立，两项目各完成 4 批。
- **P6**（queue.rs 显式 drain / IndexDb 独立短事务）：drain 循环零结构改动，仅 claim 走有序门面；gate 不接入 drain 循环本体（acquire→claim 的装配归 P7-010/P7-014，本记录 §3 已固定形状）。

## 5. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-model --locked --offline
→ 16 × "test result: ok."，307 passed / 0 failed。
  cc-semantic lib 156（含 admission 32：P7-003 16 + P7-005 gate 16）；
  queue_worker 9（+drain_claim_order_defaults_to_fifo_and_rotation_is_opt_in）；
  cc-model lib 83（含 semantic_concurrency_keys_parse_and_default_off 等 4 semantic 项）。

gate 16 项（原样，全 ok）：
  admission::tests::degenerate_gate_limits_are_rejected_with_named_reasons
  admission::tests::empty_project_identity_is_refused
  admission::tests::from_provider_config_maps_the_semantic_keys
  admission::tests::permissive_gate_never_makes_anyone_wait
  admission::tests::snapshot_reflects_liveness_fairness_and_pause
  admission::tests::single_project_cannot_starve_the_others
  admission::tests::gate_is_send_sync_and_shareable_across_threads
  admission::tests::per_project_cap_binds_independently_of_global_capacity
  admission::tests::global_cap_bounds_total_concurrent_permits_across_projects
  admission::tests::gated_batch_flow_limits_global_concurrency_and_keeps_projects_moving
  admission::tests::repeated_rate_limit_reports_extend_the_pause_never_shorten_it
  admission::tests::rate_limited_pause_fails_fast_then_expires
  admission::tests::acquire_times_out_explicitly_when_capacity_is_held
  admission::tests::fresh_callers_cannot_barge_ahead_of_the_waiting_queue
  admission::tests::admission_wait_never_spans_a_db_transaction
  admission::tests::fair_queue_serves_fifo_and_skips_a_blocked_head
（admission 套件连跑 3 次全绿，无 flaky。）

SDKROOT=... cargo test -p cc-db --locked --offline
→ 267 passed / 0 failed（semantic_queue 含 claim_fairness_doc_rotation_serves_the_least_recently_touched_doc_first、
  claim_fairness … Fifo==default；semantic_lease/outbox/recovery 等既有套件零回归）。

SDKROOT=... cargo test -p cc-server --features semantic --locked --offline
→ 302 passed / 0 failed（含 service_factory::provider_gate_tests::
  the_provider_gate_is_one_process_wide_shared_instance）。

cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s)（零 error，默认构建不含 cc-semantic 于 cc-server）。

cargo clippy -p cc-semantic -p cc-model -p cc-db (+cc-server --features semantic)
→ 本轮新增告警零；既有告警维持原状（见 §7 偏差 7）。
```

## 6. 验收对照（acceptance）

- "多项目总并发仍受限，单项目不能饿死其他索引" → `global_cap_bounds_total_concurrent_permits_across_projects`（6 线程 2 项目峰值 ≤ 2）+ `per_project_cap_binds_independently_of_global_capacity` + `single_project_cannot_starve_the_others`（p-a 占满份额时 p-b/p-c 16 轮全推进）+ 结构保证（`cap < max` 拒启退化配置）。
- V20（共享限额多项目受限）工程级证据 = 上列测试；`artifacts/benchmarks/<run-id>/` 正式 V15/V20 证据归验收轮（与 P7-001~004 同口径，不推断）。
- V15 协同面：429 映射（P7-001）→ gate 暂停（本轮）→ fenced retry（P6 queue + P7-006）链路语义齐备。

## 7. 未做与剩余风险（偏差清单）

1. **简报接口草案的两处偏差**：`new(max_concurrent) -> CcResult<Self>` 落为 `new(GateLimits)` + `validated(max, per_project)`（两级上限需结构化参数）；`acquire_permit -> ProviderPermit` 落为 `Result<ProviderPermit, GateAcquireError>`（429 fail-fast 需要错误通道），并新增 `try_acquire_permit(project, wait)` 承载超时/取消语义。
2. **WorkerLimits 增字段**：`claim_order` 由 `validated` 默认 `Fifo`，14 处既有调用点零改动；`queue_worker.rs` 既有结构体字面量相等断言同步补字段（该测试是 WorkerLimits 形状的守护，非行为变更）。
3. **跨项目/项目内公平的分界裁决**：单库单项目 ⇒ claim ORDER BY 只做项目内文档轮转；跨项目公平全权归 gate。若未来单进程共享多项目单库，需重审该分界（记为边界，不预实现）。
4. **`acquire_timeout_ms` 是调用方缺省值**：gate 本身不内建默认超时（准入等待时长是调用方预算，与 QueryControl 同型哲学）；组合根接线时从配置传入（P7-010/P7-014）。
5. **正式 V15/V20 基准证据未生成**（`artifacts/benchmarks/<run-id>/`）——归验收轮；本记录 §5 为工程级证据，tasks.json evidence 不回填、status 未改。
6. **`resume()` 的运维面**目前仅库层 API；P7-014 status/配置面接线时决定是否透出。
7. **既有告警维持原状**（非本轮触碰面）：`cache.rs:102/:126/:127`（P6-008，历轮记录在案）、`openai_compatible.rs:218`（P7-001/004 交付文件）、`admission.rs:291` clippy loop-index（P7-003 段 `plan_order`，本轮未改动该函数）。
8. 红线确认：`ports.rs`/`spec.rs` 零改动；未 git commit；零真实网络；默认构建零 cc-semantic 于 cc-server；gate 零线程/零定时器。
