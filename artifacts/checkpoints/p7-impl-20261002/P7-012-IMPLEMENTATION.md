# P7-012 实施记录：融合与部分覆盖语义

日期：2026-10-03
任务：`docs/roadmap/code-index-v2/tasks.json` P7-012「融合与部分覆盖语义」（批次 3 第二任务，depends_on P7-011）+ 规划简报 TASK-BRIEFS.md「P7-012 融合与部分覆盖语义（深化）」节。
红线遵守：schema 零改动（DB schema 零 diff；cc-model 新增结构为 additive，未接入任何既有 wire 面）；ports/spec 冻结面零改动；P6/P7 冻结交付零修改（P7-011 `semantic_scope_guard.rs`/`semantic_hydrate_guard.rs`/`semantic_wiring.rs` 零 diff，融合走 fusion.rs 任务自有 scope + 既有融合点最小接线，逐条申报见 §5）；**既有融合行为在无 semantic lane 时零变化**（`fuse_outcomes` 本体零 diff，详见 §3）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-model/src/context.rs` | **本任务核心新增**（additive 模型面，简报接口草案原样落地）：`LaneCoverageExplain`（:66）——per-lane coverage explain 投影，五字段 `lane_id/status/candidate_count/truncation_reason/coverage`；`from_lane_outcome`（:77）逐字投影（八态不折叠、reason 不丢）；`semantic_from_outcomes`（:91）——dense lane receipt 的单点投影，`None`=查询根本没有 semantic lane（local 策略/未接线），此时不得对语义覆盖做任何声明 | :60-97 |
| `crates/cc-model/src/context.rs` tests | 投影矩阵 2 项（§2 表） | :375 / :413 |
| `crates/cc-search/src/fusion.rs` | 模块文档补 dense 票权契约（:25-29，见 §2 原文）；**tie-break 具名化**：`fused_candidate_ordering`（:125）——从 engine.rs 内联闭包原样提取（谓词逐 token 相同），使 tie-break 成为被测契约；测试矩阵 6 项（§2 表） | :25 / :125 / :276-452 |
| `crates/cc-search/src/lanes.rs` | re-export 比较器（+1 词） | :26 |
| `crates/cc-search/src/engine.rs` | **既有融合点最小接线**：`search_planned_detailed` 的融合后排序改用 `candidates.sort_by(fused_candidate_ordering)`（谓词逐 token 相同，行为零变化），import +2 词 | :32 / :311-313 |

`fuse_outcomes` 本体、`append_semantic_outcome`、`materialize_lane_outcomes`、`LaneRanks`/`hit_from_chunk` 注记、P7-011 两个守卫模块全部零 diff。

### 1.2 与简报的对照

- 「独立 dense rank RRF，partial/unavailable 透出」：dense 票权消费是既有机制（`append_semantic_outcome` engine.rs:289 追加 receipt → `fuse_outcomes` 通用消费 `weight/(k+lane_rank)`）；本任务以模块文档 + 测试把该契约钉死（fusable=Complete|Partial 投票、其余八态之六零票、receipt 原样透出 explain 面），并补齐简报草案的 `LaneCoverageExplain` 透出载体。
- 「不混 cosine/BM25」：多 lane 层面反向验证（cosine 尺度扰动 ±1e200 → fused bill 逐 bit 不变，fusion.rs:276）；单 lane 版既有测试保留零改动。
- 「timeout 与无命中可区分」：两场景对照断言（fusion.rs:373）——`Timeout + "semantic_deadline"` vs `Complete + candidate_count=0`，CONFIGURATION.md:109 既有口径（complete 且 candidate_count=0 表示已执行但无命中）在 explain 面成立。
- 「declared full coverage 有证据」（V19 对应面）：Complete receipt 的 full 声明必须 `candidate_count == coverage.total_lower_bound == candidates.len()` 且每个候选通过 `validate_hydrated_candidate` 身份链（fusion.rs:392）；更深的 receipt/SourceVerifier 证据链为 P7-010（`append_semantic_outcome` 二次复验）+ P7-011（hydrate fence 守卫）既有交付，本任务零改动复用。
- 「tie-break 固定」：提取为具名契约并测试——exact_identity 层 > fused total 降序 > chunk_id 升序，与 HashMap 迭代序、lane 完成序无关（fusion.rs:420）。
- 风险①（dense 权重进 policy fingerprint，C10）：**本轮无需变更**——semantic 票权固定常量 `weight=1.0`（semantic_adapter.rs:45「caller's policy fixes the weight」），且 `search` 结果缓存的 get/put 均以 `request.semantic.is_none()` 门控（engine.rs:206/:227），semantic 请求根本不进结果缓存，fingerprint 无可翻的键；若 P7-014 引入可配置 dense 权重，届时按 C10 纳入 fingerprint 并随缓存失效（变更记录移交 §6）。
- 风险②（LaneCoverage 与 semantic epoch 关系）：本任务只透出不重算，`semantic_from_outcomes` 是纯投影。

## 2. 融合算法原文（模块文档，fusion.rs:21-29）

> RRF only consumes lane rank and configured lane weight. Raw BM25/grep/path
> scores are diagnostics and never enter this arithmetic.
>
> Dense votes ride the same arithmetic: a fusable semantic receipt
> (`Complete` | `Partial`) votes `weight / (rrf_k + lane_rank)` per
> candidate exactly like a local lane, its cosine raw scores stay
> diagnostic, and a receipt in any other state (`Timeout`, `Unavailable`,
> `Error`, `Cancelled`, `Disabled`, `NotConfigured`) casts no votes at all —
> the fused ordering then rests on the local lanes alone while the receipt
> stays visible on the lane surface for explain.

Tie-break 契约（fusion.rs:121-125 doc + :125 实现，engine.rs:313 消费）：
exact-identity 层（布尔降序）→ fused total 降序（`total_cmp`，NaN 不存在——`fuse_outcomes` 拒收非有限贡献）→ `chunk_id` 升序。

## 3. 零回归证明方式

1. **结构性保证**：semantic lane 只经 `append_semantic_outcome` 进入融合，其入口第一行 `plan.semantic() == None → return Ok(())`（lanes.rs:267-269 既有，零 diff）——未接线/local 策略时 lane 列表与任务前逐字节相同；`fuse_outcomes` 对 lane 一视同仁，无 semantic lane 时执行路径无任何新分支（本轮 `fuse_outcomes` 函数体零 diff，唯一实现改动是把 engine.rs 的排序闭包原样提取为具名函数，谓词逐 token 相同）。
2. **对照测试**：`degraded_semantic_lanes_cast_no_votes_and_leave_local_fusion_untouched`（fusion.rs:354）——同一 local 票单分别并入 Timeout/Unavailable/Error/Disabled/NotConfigured 五种 semantic receipt，fused map 的 key 集、total（逐 bit）、by_lane 与纯 local 融合完全一致。
3. **既有测试零改动全绿**：cc-search 294 passed（P7-011 收口 288 + 本任务 6），其中 `raw_score_scale_does_not_change_rrf_and_trace_replays`、`fuse_outcomes_accumulates_rrf_generically`、annotation/lane-order 契约测试全部原样通过。
4. **默认构建零漂移**：`cargo check --workspace --locked --offline` exit 0；cc-model/cc-search 全部套件无 semantic feature 亦全绿（新测试不依赖 semantic feature——receipt 由测试侧构造，生产侧接线点未动）。

## 4. 部分覆盖表达矩阵

表达面（全部既有承载 + 本任务 explain 载体，无任何折叠）：

| semantic receipt | 投票（RRF） | `lanes[i].status` | explain（`LaneCoverageExplain`） | 融合整体语义 |
|---|---|---|---|---|
| 无 semantic lane（未接线/local） | — | 无该 lane | `semantic_from_outcomes=None`，禁止任何语义覆盖声明 | 纯 local 融合，行为与任务前逐字节一致 |
| `Complete` + n | `Σ 1.0/(k+rank)` | Complete, reason=None | `coverage.complete=true`，`candidate_count == total_lower_bound == n`（full 声明有候选账本背书，:392） | dense 票权全额参与，可声明完整语义覆盖（证据链 = receipt 候选账本 + hydrate 复验） |
| `Complete` + 0 | 0 票 | Complete, reason=None | `complete=true, candidate_count=0`（已执行无命中，≠disabled） | 可声明「语义通道执行过、无命中」 |
| `Partial`（P7-011 `"semantic_coverage_uncovered"` / lexical 系 `candidate_limit` 等） | 正常投票（Partial 可融合） | Partial, reason 原样 | `complete=false`，reason 不丢（:320） | 候选全额可用，但**整体不得声明完整语义覆盖**——缺口由 receipt 点名，不伪装 |
| `Timeout`（`"semantic_deadline"`） | 0 票 | Timeout | `complete=false` + reason（:373 对照面） | 与 Complete+0 可区分：没跑完 ≠ 跑完无命中；fused 排序退化为 local |
| `Unavailable`（`"semantic_capacity"`） | 0 票 | Unavailable | 同上 | 降级口径同 Timeout：local 票单不受扰，receipt 留痕 |
| `Error`（`"semantic_read_error"`）/ `Cancelled` | 0 票 | Error/Cancelled | 同上 | 可选通道失败可降级但不吞错（C10） |
| `Disabled`/`NotConfigured` | 0 票 | Disabled/NotConfigured | `complete=false`，reason=None | 未执行，与上述一切可区分 |

## 5. 测试（命令与结果原样）

新增 8 项（具名）：
- cc-model context：`coverage_explain_projects_the_receipt_verbatim`、`semantic_explain_is_absent_exactly_when_no_semantic_lane_ran`
- cc-search fusion：`dense_votes_fuse_by_rank_only_and_never_mix_raw_scores`、`partial_semantic_receipts_vote_but_never_masquerade_as_complete`、`degraded_semantic_lanes_cast_no_votes_and_leave_local_fusion_untouched`、`semantic_timeout_and_complete_zero_hits_stay_distinguishable`、`declared_full_semantic_coverage_points_at_its_evidence`、`fused_order_ties_break_deterministically`

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-search -p cc-server --features semantic --locked --offline`
   → cc-search lib `294 passed; 0 failed`（= 288 + 6）；cc-server lib `273 passed; 0 failed`（P7-011 收口值零漂移）；其余套件 `41 passed` / `9 passed` / `0 failed`。**exit 0**。
2. `cargo check --workspace --locked --offline` → `Finished dev profile`，**exit 0**。
3. 零回归补充：`cargo test -p cc-model --locked --offline` → `87 passed` + `5` + `3` + `1` + `1`，全 0 failed（+2）。
4. 无编译 warning 新增。

## 6. 偏差清单

1. **engine.rs 触及超出 tasks.json scope**（scope 仅列 fusion.rs + cc-model/context.rs）：排序谓词提取为 `fusion::fused_candidate_ordering` 并在 engine.rs:313 消费——「既有融合点最小接线」的兑现方式：谓词逐 token 相同（行为零变化），使简报要求的「tie-break 固定」成为被测具名契约而非内联闭包。按惯例申报。
2. **`fuse_outcomes` 无实现改动**：简报「dense 票权接入」经核为既有机制（P7-010 已接 `append_semantic_outcome` → 通用 RRF），本任务交付形式为契约钉死（模块文档 + 6 测试）而非新算术；若在此重构即触碰「无 semantic lane 零变化」红线，不做。
3. **`LaneCoverageExplain` 未接入 MCP 响应面**：按任务分工归 P7-014「全链贯通」（schema/sanitize/handler/doc/E2E 闭环）；本轮为 additive 模型 + 投影原语（public，`cc_model::context` 路径），零 wire 面改动。
4. **fingerprint 风险①未触发**：semantic 权重为常量 1.0 且 semantic 请求不进结果缓存（get/put 双侧 `semantic.is_none()` 门控），无可翻缓存键；移交 P7-014 在引入可配置权重时一并处理（§1.2）。
5. **V11/V19 基准证据**：tasks.json deliverables 要求 `artifacts/benchmarks/<run-id>/` 证据「实施时生成」——V11/V19 正式验证矩阵按批次 1/2 既有口径归验收轮（not_run 不推断），本记录以 §5 命令级测试结果为证据；acceptance 的「timeout 与无命中可区分」「declared full coverage 有证据」已由具名测试落地。
6. 一轮红绿中的测试自缺陷修正：cc-model 投影测试首版 local lane 误用 `"semantic"` 作 lane_id 被 `find` 先命中（红轮失败为测试缺陷而非实现缺陷），已改 `"lexical"` 后转绿，实现零改动。

## 7. 移交与边界

- **P7-014**：`LaneCoverageExplain`/`semantic_from_outcomes` 接入 MCP 响应面（schema/sanitize/handler/doc/E2E）；可配置 dense 权重时按 C10 纳入 policy fingerprint。
- **P7-013**：`truncation_reason` 链（`semantic_deadline`/`semantic_capacity`）与查询总 deadline 的衔接——explain 面已保证 reason 不丢，故障痕迹保留契约成立。
- 未接线语义路径（默认构建、`semantic.enabled=false`）零行为变化；P7-011 冻结交付（范围守卫、hydrate 守卫、recall 接线）零 diff。
