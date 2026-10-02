# P7-013 实施记录：查询总 deadline 和模型故障退化

日期：2026-10-03
任务：`docs/roadmap/code-index-v2/tasks.json` P7-013「查询总deadline和模型故障退化」（批次 3 第三任务，depends_on P7-012）+ 规划简报 TASK-BRIEFS.md「P7-013 查询总 deadline 和模型故障退化（深化）」节。
红线遵守：schema 零改动；ports/spec 冻结面零改动（`02-CONTRACTS.md`/ports.rs 零 diff）；P6/P7 冻结交付零修改（P7-010 `semantic_wiring.rs` 仅 additive 接线、P7-011 两个守卫模块 + `semantic_manifest_reads.rs` + `fusion.rs`/`engine.rs`/`lanes.rs`/`exact.rs`/`evidence_hydrator.rs` 全部零 diff）；无 semantic 时查询路径零变化（默认构建 cc-search/cc-server 测试全绿，§4）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-search/src/execution.rs` | **本任务核心新增（具名预算原语）**：`semantic_child_budget(control, policy)`（§1.2 口径原文）——语义 lane 对查询总 deadline 的份额，`QueryControl::child` 内建 `min(now+share, parent_deadline)` 钳制（cc-model/query.rs:91-99 既有不变式，零 diff）；模块文档声明超时处置链（lane Timeout 留痕不阻塞整体 / 总预算耗尽整体 QueryTimedOut） | :15-32 |
| — tests | `semantic_lane_share_never_extends_the_total_deadline`（钳制 + 份额全额适用 + 取消沿共享态传播） | :330 |
| `crates/cc-search/src/lanes/semantic_adapter.rs` | **测试模块新增**（实现本体零 diff——既有八态映射已支持，本任务接通触发面）：deadline 触发矩阵 3 项（§3） | :65-260 |
| `crates/cc-server/src/query_handle.rs` | 策略→QueryControl 装配点接线（原 `control.child(...)` 内联一行替换为具名原语调用，谓词等价：`policy.semantic_timeout_ms` 既有 min 钳制不变） | :92-95 |
| `crates/cc-server/src/semantic_wiring.rs` | **本任务核心新增（故障退化口径）**：三个 reason 常量、`ProviderHealth` 只读健康视图类型（测试注入 seam）、`provider_unhealthy_reason` 裁决函数（§1.3 矩阵原文）；`ExactRecallService` 增 `health` 字段（additive），`assemble_with` 用组合根同一 gate/breaker 单例 + 项目 ledger 构造生产视图；`recall` 第 0 步健康门（任何扫描/锁/缓存读之前）；扫描后新增 `control.check()?` 剩余时间检查点；模块文档补 P7-013 段 | :95/:97/:102 / :108 / :112 / :357 / :366 / :402 / :412 |
| — tests | 故障退化矩阵 3 项（§3） | :1170-1360 |

### 1.2 deadline 预算分配口径（原文，execution.rs:17-31 模块文档）

> The semantic lane's share of the query-total deadline (P7-013).
>
> The lane budget is `policy.semantic_timeout_ms` (itself already
> `min(deadline_ms)`-clamped at policy resolution) cut as a child of the
> running total: `QueryControl::child` clamps the child deadline to
> `min(now + share, parent_deadline)`, so the lane can never extend the
> query's absolute deadline (C11: "request deadline 为总预算；每 lane 有
> 子预算，取消向下传播"). A lane that times out degrades to a `Timeout`
> receipt (`semantic_adapter`), never blocks the whole query — unless the
> PARENT budget itself is exhausted, in which case the total deadline
> governs and the whole query fails with `QueryTimedOut` at its next
> checkpoint.

分配与检查点全景（既有机制 + 本任务接线点，均零实现改动或单点接线）：

| 阶段 | 预算 | 检查点/落点 |
|---|---|---|
| 查询入口 | 总预算 `QueryConfig.deadline_ms`（默认 30s，query.rs:38） | `capture_for_request`→`limit_total`（query_handle.rs:155 既有）；`search_async` 装 control（:68-71 既有） |
| 语义 lane 全程（编码→召回→融合贡献） | 份额 = `semantic_child_budget(control, policy)` = `child(min(semantic_timeout_ms, 总剩余))` | lane 入口 adapter `run_async` 首检（semantic_adapter.rs:22 既有）+ `until` deadline 睡眠（execution.rs 既有）；**扫描后新增检查点**（semantic_wiring.rs:412）；融合贡献前 `append_semantic_outcome` 首行 `control.check()`（lanes.rs 既有） |
| 查询内联编码 | **不存在**——P7-010 裁决维持：查询路径只消费 P7-009 向量缓存，miss → `Unavailable("query_vector_not_encoded")`（简报风险①默认裁决的既成兑现，零 diff） | semantic_wiring.rs:410 既有 |
| 超时处置 | `QueryTimedOut`→`Timeout("semantic_deadline")`、`QueryCancelled` 直透、`QueryBusy`→`Unavailable("semantic_capacity")`、其余→`Error("semantic_read_error")` | semantic_adapter.rs:29-33 既有八态映射（P7-012 融合零票面已交付，本任务测试接通触发） |

### 1.3 故障退化矩阵（原文，semantic_wiring.rs 模块文档 + `provider_unhealthy_reason`）

> Degradation口径: a provider-side failure (circuit breaker open/half-open,
> 429 gate suspension, degradation-ledger `degraded`) silently degrades the
> dense lane to an explicit `Unavailable` receipt — the whole query keeps
> running its local lanes (P7-012: non-fusable receipts cast zero RRF
> votes), and the reason stays visible on the lane surface / capability
> probe. The query path NEVER acquires a gate permit, a breaker probe slot
> or an outbox lease (P6-007 stays worker-side): it only READS health
> state, so there is no nested budget and no waiting across a transaction
> (C11).

| provider 侧状态 | 判定 | receipt | 融合 | 可见性 |
|---|---|---|---|---|
| 断路器开（含 half-open：探针未证实恢复，保守同 `semantic_breaker_open`，镜像 P7-011「疑问永不利 Complete」） | `breaker.state()` ≠ Closed（懒转换只读，不占探针槽） | `Unavailable` + `"semantic_breaker_open"` | 0 票，local 票单不受扰 | lane 面 truncation_reason + P7-012 `LaneCoverageExplain` 投影 |
| gate 429 暂停 | `gate.snapshot().suspended_for.is_some()` | `Unavailable` + `"semantic_gate_suspended"` | 同上 | 同上 |
| degraded（缓存损坏/回填预算耗尽） | `ledger.snapshot().degraded` | `Unavailable` + `"semantic_provider_degraded"` | 同上 | 同上 + 既有 capability 桥（`semantic_state:"degraded"` + `degraded_reason`，P6-018 交付零 diff） |
| 健康 | 三读全过 | 既有全链不变（warm cache → Complete，`fake_provider_full_chain_publish_encode_recall_hit` 既有测试守护） | 正常投票 | 既有 |

裁决优先级：断路器 > gate > ledger（诊断优先序，测试钉死）。健康视图为 `Arc<dyn Fn() -> Option<&'static str> + Send + Sync>` 注入 seam：生产侧在 `assemble_with` 从组合根同一单例（`service_factory::semantic_provider_gate()`/`semantic_circuit_breaker()`，P7-005/P7-006 first-wins 既有）+ 项目 ledger 闭包构造；测试侧注入，**绝不触碰进程级单例状态**（避免测试污染共享故障图景）。

### 1.4 与既有预算机制衔接（任务第 3 项）

- **P6-007 lease**：lease 全部在 worker 回填侧（claim/renew/ack/retry/reclaim，token fencing）；查询路径对 outbox 零接触，对 gate/breaker 只读不获取——无嵌套预算、无双重租约语义。`semantic_child_budget` 与 gate 的 429 暂停互相独立：gate 暂停走 §1.3 退化面（Unavailable 留痕），不占 lane deadline 等待。
- **C11（等待不跨事务）**：健康三读均为短临界区 mutex 读（breaker inner / gate state / ledger 快照），无 await、无 DB 连接；recall 的 `read_conn` 短 checkout 既有格局不变（P7-011 偏差 4 的独立 checkout 口径保持）；「网络不占读写锁」由结构性保证：`QueryHandle` 短锁内 clone（query_handle.rs:32 既有），语义 future 在 `run_async`（无 CPU/RwLock permit）内执行，adapter 矩阵测试回归通过。
- **故障结果不缓存成完整成功**：双重既有闸门零 diff 回归——①结果缓存 get/put 双侧 `request.semantic.is_none()` 门控（cc-search engine.rs:207/:225 既有，semantic 请求根本不进结果缓存）；②降级 receipt 本身 `is_cacheable=false`（Timeout/Unavailable 非既有限定原因集，retrieval.rs:273 既有），P7-012 `degraded_semantic_lanes_cast_no_votes...` + 本轮矩阵测试的 `validate()` 断言守护。

## 2. 查询内联编码裁决（简报风险①，确认既有兑现）

**裁决维持 P7-010 既成口径：不允许**。查询路径只消费 P7-009 查询向量缓存；miss → 该查询 dense lane 记 `Unavailable("query_vector_not_encoded")`（semantic_wiring.rs 既有，本轮零 diff），Q4/Q7 关联维持。理由：同步 `EmbeddingProvider` 不可入查询路径（ports.rs:99），内联编码会把网络带进 C11 禁区；provider 故障时正确行为是 §1.3 退化而非现场编码。

## 3. 测试（矩阵与证据原样）

新增 7 项（具名）：

| 位置 | 测试 | 矩阵覆盖 |
|---|---|---|
| cc-search execution | `semantic_lane_share_never_extends_the_total_deadline` | 份额钳制（5s 份额 × 100ms 总预算 → 钳到总预算；份额 < 父剩余 → 全额适用）+ 取消传播 |
| cc-search semantic_adapter | `budget_exhausted_before_the_lane_runs_is_a_timeout_receipt_not_a_query_error` | **编码超时**面：总预算 lane 入口已耗尽 → `Timeout("semantic_deadline")` receipt，端口不运行、查询不报错 |
| cc-search semantic_adapter | `recall_timeout_degrades_the_lane_and_leaves_the_parent_budget_usable` | **召回超时**：20ms 份额超时 → Timeout receipt；父预算 `check()` 通过；同 control 上 `run_cpu` 本地工作照常执行（**deadline 不影响 local lanes 的隔离证据**） |
| cc-search semantic_adapter | `total_budget_exhaustion_governs_after_the_lane_degrades` | **预算耗尽**：份额钳到父 deadline → lane Timeout；随后本地搜索 admission 检查点整体 `QueryTimedOut` |
| cc-server semantic_wiring | `provider_unhealthy_reason_names_the_three_failure_classes_in_priority_order` | 三类故障判定（真实 breaker MockRetryClock 触发开路 / gate 429 / ledger note_corrupt）+ 健康全过 None + 优先级 |
| cc-server semantic_wiring | `provider_failure_states_silently_degrade_the_lane_with_distinct_reasons` | **故障退化矩阵 × 查询路径**：三类故障各自（warm cache 本可正常召回的前提下）→ `Unavailable` + 对应 reason + 0 候选 + 非 fusable + `validate()` 通过 |
| cc-server semantic_wiring | `degraded_receipt_flows_through_the_lane_adapter_unchanged` | 降级 receipt 过 adapter 收据门原样透出 + generation 回显（explicit semantic「明确不足」面） |

与 P7-012 八态的集成：降级 receipt 的零票融合由既有 `degraded_semantic_lanes_cast_no_votes_and_leave_local_fusion_untouched`（fusion.rs，零 diff）覆盖，本轮矩阵测试补齐生产路径到该 receipt 的触发段（`!status.is_fusable()` 断言）。

auto/explicit 分工：`auto_without_port_is_local_and_explicit_semantic_is_unavailable`（query_policy.rs 既有）+ auto 失败静默回 local（receipt 在 lane 面留痕不报错）、explicit semantic 无端口 `SemanticUnavailable` 错误、有端口时降级 receipt 即「显式 unavailable 状态」（简报口径两分支均满足）。

## 4. 验证（命令与结果原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-server --features semantic -p cc-search -p cc-semantic --locked --offline`
   → cc-search lib **298 passed; 0 failed**（P7-012 收口 294 + 本任务 4）；cc-semantic lib **212 passed; 0 failed**；cc-server lib **276 passed; 0 failed**（P7-011/P7-012 收口 273 + 本任务 3）；其余套件 41/17/4/4/17/9/3/5/5/8/6/2/9 passed，**全部 0 failed，exit 0**。
2. `cargo check --workspace --locked --offline` → `Finished dev profile`，**exit 0**。
3. 零回归补充（无 semantic 默认构建，查询路径零变化证据）：`SDKROOT=… cargo test -p cc-search --locked --offline` → **298 passed; 0 failed**（新测试均不依赖 feature）；`SDKROOT=… cargo test -p cc-server --locked --offline` → **251 passed; 0 failed**（语义模块被 cfg 排除，基线零漂移）。
4. 编译 warning：触及 crate 零新增（既有 `cc-semantic` reconcile_rebuild.rs dead_code 历史在案）。

## 5. 偏差清单

1. **`semantic_wiring.rs`/`query_handle.rs` 触及超出 tasks.json scope**（scope 仅列 execution.rs + handlers/context.rs）：①策略→QueryControl 语义子预算的实际装配点是 `query_handle.rs:92`（handlers/context.rs 是 capture/总预算层且无需改动——`capture_for_request`→`limit_total` 既有链已符合口径），简报「装配点」定位与既成代码格局的偏差按实际落点接线；②故障退化口径的唯一落点在 recall 实现（`ExactRecallService`）——组合根外的任何位置都无法同时拿到 gate/breaker 单例与项目 ledger。两处均 additive，按惯例申报。
2. **`semantic_child_budget` 落在 execution.rs 而非简报草案的参数形状**（草案 `semantic_child_budget(control, policy)` 原样兑现，字段名取既有 `policy.semantic_timeout_ms`）。
3. **half-open 断路器按不健康裁决**：`state()` 非 Closed 一律 `semantic_breaker_open`——探针未证实恢复前 provider 健康未知，保守口径与 P7-011 页预算耗尽裁 Partial 同源（「疑问永不利 Complete/完整声明」）。
4. **ledger degraded → lane Unavailable 的口径裁决**：P6-018 的 degraded（缓存损坏/回填预算耗尽）严格说只影响部分文档，P7-011 范围守卫已能把缺发布裁 Partial；本任务按简报明确口径「degraded → 语义 lane 静默降级为不可用（留痕）」整 lane 降级，属保守收窄（宁可 Unavailable 也不在缓存完整性存疑时出 dense 候选），移交 P7-014 状态机时如需文档级细分可再收窄。
5. **测试注入 seam 而非单例直测**：故障矩阵经 `ProviderHealth` 闭包注入而非真实 trip 进程级 gate/breaker 单例——共享故障图景是全进程状态，测试写入会污染同二进制其他测试；单例装配正确性由「生产闭包读取同一单例」的 assemble 代码路径 + `provider_unhealthy_reason` 纯函数矩阵共同覆盖。
6. **V11/V15 基准证据**：tasks.json deliverables 要求 `artifacts/benchmarks/<run-id>/` 证据「实施时生成」——本轮未生成：V11/V15 正式验证矩阵证据按批次 1/2 既有口径归验收轮（not_run 不推断），本记录以 §4 命令级测试结果为证据；acceptance「网络不占读写锁」「故障结果不缓存成完整成功」由 §1.4 结构性保证 + §3 具名测试落地。
7. 一轮红绿中的测试自缺陷修正：①隔离测试首版父预算取 5s 与默认份额 5s 相撞（份额从 `Instant::now()` 起算被钳到父 deadline，红轮失败为测试参数缺陷），改 20ms 小份额 policy 后转绿；②取消传播断言误跨两个独立父 control 的状态（测试缺陷），改单链断言。实现两处均零改动。

## 6. 移交与边界

- **P7-014**：三个降级 reason（`semantic_breaker_open`/`semantic_gate_suspended`/`semantic_provider_degraded`）与 breaker/gate 状态接入 capability 状态机与 MCP 响应面（本轮可见性止于 lane receipt + 既有 P6-018 degraded 桥）；`LaneCoverageExplain` 接线同 P7-012 移交。
- **验收轮**：V11/V15 基准证据（fake/HTTP 慢请求取消的端到端基准化）。
- 未接线语义路径（默认构建、`semantic.enabled=false`）零行为变化；P7-011 冻结交付（范围守卫、hydrate 守卫、`semantic_manifest_reads`）与 P7-012 冻结交付（`LaneCoverageExplain`、`fused_candidate_ordering`、fusion 契约测试）零 diff。
