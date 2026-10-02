# c4 混合负载竞态修复实施记录（方案 d：P0+P1+P2）

- 日期：2026-10-02
- 对应提案：同目录 `ROOT-CAUSE-AND-FIX-PROPOSAL.md`（方案 d）
- TDD 流程：先红后绿，红阶段为行为性失败（非仅编译失败）
- 红线遵守：未触碰 `crates/cc-db/`（写路径/epoch bump/lease/池替换零改动）；未动 artifacts 锁定链、formal-v4 raw、tasks.json；未 git commit；改动仅读路径 3 文件 + 测试

## 1. 改动文件:行号（最终态）

### `crates/cc-search/src/engine_cache.rs`（P1）

| 位置 | 内容 |
|---|---|
| `:28` | `FENCE_BACKOFF_STEP_MS: u64 = 30`（新增，每尝试退避步长） |
| `:34` | `FENCE_BACKOFF_MAX_DELAY_MS: u64 = 60`（新增，单次退避硬上限） |
| `:38` | `FENCE_MAX_ATTEMPTS: u32 = 3`（新增，由 `with_stable_generation` 内局部 const 提升为模块常量，语义不变） |
| `:45-48` | `fence_backoff_delay(attempt)`（新增）：`min(30ms × attempt, 60ms)`，即 30ms→60ms，单次 fence 累计 ≤90ms，有界 |
| `:218-252` | `with_stable_generation`：`yield_now()`（原 `:209`）替换为 `thread::sleep(fence_backoff_delay(attempt+1))`（`:243-244`），且最后一次失败尝试后不再退避；取消/超时类错误短路路径不变；耗尽仍返回 `RetrievalChanged{attempts:3}`（终态语义不变） |

与提案的偏差：提案建议 5–20ms 步长；实现取 30/60ms。原因：单元测试在套件并行负载下验证发现 <20ms 的退避窗口无法与 writer 提交延迟可靠分离（负载抖动 ≥10ms）；30/60ms 仍远低于重建提交间隔 ≥500ms，fence 全程 ≤~180ms，压灭 attempts:3 的机制不变。

### `crates/cc-server/src/handlers/freshness.rs`（P2）

| 位置 | 内容 |
|---|---|
| `:11-19` | `accepted_generation_of(value)`（新增）：从 envelope 的 `/evidence_summary/source_freshness/generation`（hydrator 由外层 fence 接受代写入的同一指针）解析 fence 接受代；无该指针时返回 `None` |
| `:64-118` | `attach_observed` 签名增加 `accepted: Option<ReadGeneration>` 参数；新增精化分支（`:78-96`）：当 dispatch 时窗内 `before.index_epoch != after.index_epoch` 且 `accepted.index_epoch == after.index_epoch`（与既有 dispatch 比较同为 index_epoch 粒度）时，不再置 `complete=false/changed_during_query`，改为在 `resolution_freshness` 内写入显式观测字段 `dispatch_observed_generation_change {from_index_epoch, to_index_epoch}`（`:99-106`）；其余所有变更路径维持严格 `changed_during_query` 标注 |

### `crates/cc-server/src/handlers/context.rs`（P0 + P2 接线）

| 位置 | 内容 |
|---|---|
| `:41-56` | `finalize_search_response`（新增，search_async 序列化闭包体收口）：attach（带 accepted 精化）→ `pack_value`。**已删除 `validate_envelope_generation` 硬校验调用**（原 `:55`），doc comment 说明契约归属 |
| `:104-107` | `search_async` 闭包改为调用 `finalize_search_response` |
| `:108` | `symbol_search_async` 的 attach 调用补传 `accepted_generation_of`（该响应无 generation 指针 → `None` → 严格语义不变） |
| `:198` | `context_async_with_strategy` 的 attach 调用补传 accepted（finish_context_with 后的 `code_index_context` 结果保留同一 generation 指针） |
| `:214-216` | **删除第二处 fence 外 `validate_envelope_generation` 硬校验**（提案 §2.3 第 5 条同型调用，原 `:189`；提案仅点名 `:55`，此处为同一模式一并拆除，见 §5 偏离说明） |

`cc_search::evidence_hydrator::validate_envelope_generation` 函数本身保留在 cc-search（`evidence_hydrator.rs:19-37` 未动），`crates/cc-eval/tests/p5c_hydration.rs` 的既有契约测试不受影响。

## 2. 新增测试与断言内容

> 计数订正：本节新增测试共 **8 个**（第 7 项 `generation_change_is_never_reported_ready` 为既有测试，不计入新增）。

### cc-search（`engine_cache.rs` tests 模块）

1. `fence_backoff_rides_out_a_back_to_back_commit_storm`（测试 b，对应 R1 `attempts:3` 签名）
   - 场景：事件驱动三次提交风暴——commit1/commit2 分别由 fence 进入 work 尝试 1/2 门控（必然落在对应尝试窗口内），commit3 在尝试 2 返回后 8ms 触发；每次 work 窗口 60ms。
   - 断言：`result.ok()`（fence 不硬失败）；`generation == 当前 read_generation`（接受代即当前代，无有界 stale 泄漏）；`calls >= 2`（风暴确实发生）。
   - 时序原理：无退避时 commit3（flag+~8..12ms）落在尝试 3 窗口 `[flag+2, flag+62]` 内 → 三连跨 → 耗尽；有 30ms 退避时 commit3 在尝试 3 before-read（`flag+~32ms`）前排空 → 稳定接受。writer 门控全部带 5s 超时上限，病态调度下以断言失败而非挂死收场。
2. `fence_backoff_is_bounded`（P1 守卫）：每次退避 >0；累计 ≤ `MAX_DELAY × MAX_ATTEMPTS`（有界、无界等待不可达）。

既有 4 个 fence 测试（`generation_fence_*`）全部保持绿：同 epoch incarnation 变更检测、混合代丢弃后接受新代、成功/失败混合尝试各有界、稳定 DB 错误不被重试掩盖（`attempts:3` 耗尽终态与 retryable 契约不变）。

### cc-server（`handlers/freshness.rs` tests 模块）

3. `accepted_generation_matching_current_is_not_flagged_incomplete`（测试 b 的 "经退避成功" 侧 / P2 主张）：`before=epoch1, after=epoch2, accepted=epoch2` → `complete==true`、`status=="ready"`、`dispatch_observed_generation_change.from==1/to==2`。
4. `true_stale_accepted_generation_still_marks_changed_during_query`（测试 c + d 守卫）：`accepted=epoch1, after=epoch2` → `complete==false`、`changed_during_query`、**无**观测字段（真 stale 不被洗白、不享受精化）。
5. `missing_accepted_generation_keeps_strict_annotation`（测试 d 守卫）：`accepted=None` → 严格 `changed_during_query`（任何无接受代路径不得把变更洗成 `complete=true`）。
6. `accepted_generation_of_reads_envelope_pointer`：指针解析往返正确 + 无指针返回 `None`。
7. 既有 `generation_change_is_never_reported_ready` 保持绿（`{"results":[]}` 无接受代 → 严格标注，验证通用 seam 未放松）。

### cc-server（`handlers/context.rs` tests 模块）

8. `serialized_window_generation_change_is_annotated_not_a_hard_error`（测试 a，对应 R1 `attempts:1` 签名）：真实 `CodeIndex` + 真实 db；envelope 内嵌 fence 接受代 G0；`build_index(false)` 提交落地 G1 模拟"序列化窗口内提交"；`finalize_search_response` 必须 `Ok`（不得硬 error）且 `complete==false`、`status=="changed_during_query"`、`reason=="index_generation_changed_during_query"`（freshness 如实）。
9. `fence_recovered_result_is_complete_with_observed_change`（P2 端到端 seam）：fence 恢复到新代（accepted==当前代）而 dispatch before 为旧代 → `complete==true` + 观测字段 from/to。

## 3. 红 → 绿证据（原样摘要）

红阶段方式：先做最小签名脚手架（`attach_observed` 增参但忽略、`finalize_search_response` 收口但保留硬校验、`fence_backoff_delay` stub 返回 ZERO），使新测试对旧行为产生行为性失败。

### 红（cc-search）

```
test engine_cache::tests::fence_backoff_is_bounded ... FAILED
test engine_cache::tests::fence_backoff_rides_out_a_back_to_back_commit_storm ... FAILED
test result: FAILED. 0 passed; 2 failed; ...; 279 filtered out
```
（风暴测试失败即 R1 `attempts:3` 硬 error 签名：fence 耗尽返回 `RetrievalChanged{3}`，期望为 Ok）

### 红（cc-server）

```
test handlers::freshness::tests::accepted_generation_matching_current_is_not_flagged_incomplete ... FAILED
test handlers::context::tests::fence_recovered_result_is_complete_with_observed_change ... FAILED
test handlers::context::tests::serialized_window_generation_change_is_annotated_not_a_hard_error ... FAILED
    assertion `left == right` failed
      left: Bool(false)
      right: true
test result: FAILED. 4 passed; 3 failed; ...; 242 filtered out
```
（`serialized_window_...` 失败即 R1 `attempts:1` 硬 error 签名：`finalize` 返回 `Err(RetrievalChanged{1})`，期望 Ok+标注。守卫测试 4/7/5 及既有 `generation_change_is_never_reported_ready` 红阶段即绿，属预期——守卫对旧行为也应成立。）

### 绿（最终，`SDKROOT=... cargo test -p cc-search -p cc-server --locked --offline`）

```
test result: ok. 281 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (cc-search lib)
test result: ok. 249 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (cc-server lib)
test result: ok. 9 passed; 0 failed; ... (cc-server 集成 mcp_stdio)
```

稳定性复核：全套件连跑 6 次，6/6 全绿（风暴测试为时序敏感测试，已按事件驱动门控设计消除负载敏感；红阶段曾出现一次 wall-clock 版本在并行负载下的假接受，随即改为事件门控版本）。

### workspace check

```
SDKROOT=... cargo check --workspace --locked --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.15s
```

全 workspace test 按轮次约定未跑（终轮统一回归）。

## 4. P0/P1/P2 落实对照

| 项 | 提案目标 | 落实 | 出口对应 |
|---|---|---|---|
| P0 | 拆除 fence 外无重试硬校验，消灭 `attempts:1` 尾部 | `context.rs` 两处 `validate_envelope_generation` 调用删除；校验职责并入 `attach_observed`（accepted 精化 + 严格标注） | 分支 β（seq154 类）→ 改为如实 `changed_during_query` 标注 |
| P1 | fence 重试有界短退避，压灭 `attempts:3` 耗尽 | `engine_cache.rs` `yield_now` → `min(30k,60)ms` 有界退避；取消/超时短路、耗尽终态不变 | 分支 γ（seq85 类）→ 重建 cadence 在退避窗内排空 |
| P2 | 用 fence 接受代精化 dispatch freshness，消灭假阳性 Partial | `freshness.rs` 精化分支 + `dispatch_observed_generation_change` 观测字段；真 stale（accepted≠after 或无 accepted）严格不变 | 分支 α（seq306/seq26 类）→ 新代自洽结果 `complete=true` 且变更仍显式可观测 |
| 语义守护 d | 任何路径不得把真 stale 洗成 `complete=true` | 测试 4/5/7 守卫 + 精化分支仅在 `accepted.index_epoch == after.index_epoch` 时触发 | 覆盖 |

契约不放松确认：`error.rs` "绝不返回或缓存混合代结果" 契约未动——fence 丢弃混合代尝试的逻辑一字未改；P0 删除的检查发生在 fence 接受之后，从不提供 attach_observed 之外的保证（提案 §5-P0 原结论）。

## 5. 偏离与注意事项

1. **P1 步长 30/60ms 高于提案建议的 5–20ms**：负载稳健性取舍，机制与有界性不变（见 §1）。
2. **`context_async_with_strategy` 的第二处同型硬校验一并拆除**：提案仅点名 `search_async` 的 `:55`；该处为同一 fence 外模式、同一出口类型，保留它会在 context 工具上保留同类 `attempts:1` 尾部。
3. **精化比较为 index_epoch 粒度**：`ResolutionFreshness` 本身仅携带 `index_epoch`，既有 dispatch 比较也是该粒度；incarnation 级一致性由 fence 自身（全代相等）保证。
4. **cc-eval 的 known-red 清单对账未做**：提案 §5-P2 已注明需在 P5-020 后单独核对；本轮仅保证字段为 additive（既有字段语义不变，仅新增 `dispatch_observed_generation_change`）。
5. **残余风险**：`RetrievalChanged{3}` 耗尽终态保留，极端调度下仍可能复现（概率进一步塌缩但非零）；风暴测试为时序敏感测试，虽按事件驱动设计并 6/6 稳定，理论上仍受极端负载影响（失败方向为断言失败而非挂死或假绿）。

独立评审结论（2026-10-02）：8 项测试全部 PASS，总裁决「可纳入 source-v6 冻结」；遗留清单——rustdoc `FENCE_BACKOFF_BUDGET_MS` 失效引用已修（`engine_cache.rs`）、风暴测试 flake 窗口已知（时序敏感，失败方向为断言失败）、cc-eval known-red 对账推迟（见 §5 第 4 条）。
