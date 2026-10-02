# P7-003 实施记录：真实输入尺寸与批次规划

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-003「真实输入尺寸与批次规划」
红线遵守：`ports.rs`/`spec.rs` 冻结面零改动（仅引用）；零真实网络（全部 offline 锁定运行 + 内存 mock transport）；截断/估算口径如实声明（无 tokenizer 不冒充精确 token 数）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/admission.rs` | **本任务核心新增模块**（838 行，含测试；caller 侧 admission 正式化） | — |
| — `InputBudget { max_items, max_bytes, max_tokens }` | 批级预算（items / 硬 bytes / 估算 tokens）；`validated` 拒 0 上界 | :64 / :76 |
| — `InputBudget::from_capability` | 从 P7-002 `ModelCapability` 派生预算：`max_batch_items`/`max_input_tokens` 直通，bytes 上限留操作者决定（能力表声明 tokens 不声明 bytes） | :90 |
| — `OversizeReason { EmptyInput, BytesTooLarge, TokensTooLarge }` + `describe` | 结构化跳过原因（可审计、可计入 coverage 分母），`describe` 是稳定单行（进 dead-letter/日志），TokensTooLarge 文案内显式标注 "not a tokenizer count" | :114 / :135 |
| — `BatchPlan<K>` / `PlannedInput<K>` | 批计划（key↔`DocumentInput` 配对、保序）+ 计划产物两态：`Batch` / `Skipped { key, reason }`；`total_bytes`/`total_estimated_tokens` 供调用方复核 | :155 / :168 / :174 / :184 |
| — `QueryBatchPlan<K>` / `PlannedQueryInput<K>` | 查询路径对偶（`QueryInput` digest 绑定） | :196 / :212 |
| — `estimate_tokens` | 声明估算器 `utf8-bytes-div-ceil-4-v1`（bytes/4 向上取整），与全库单一来源 `cc_model::approx_tokens`/`TOKEN_ESTIMATOR` 同式 | :223 |
| — `tokenizer_gate` | admission 只承认声明的 workspace 估算器；外来 tokenizer 名（如 `cl100k_base`）显式 `CcError` 拒绝并点名，**不冒充不吞** | :230 |
| — `admit_item` | 单项准入：预算优先、冻结 `MAX_INPUT_BYTES` 结构性兜底（宽松操作者预算无法把超冻结界输入带过门） | :247 |
| — `plan_order` | 确定性切分核心：顺序保持 first-fit 贪心，批恰在下一项放不进时关闭；跳过项原地记录 | :276 |
| — `plan_document_batches` / `plan_query_batches` | 公开入口：tokenizer gate → budget 复检 → 切分 → 按输入位置交错 Batch/Skipped 输出；`from_bytes` 失败（仅剩非 UTF-8 = caller bug）响亮失败而非夹带 | :320 / :358 |
| — tests（16 项） | 边界/确定性/空集/单元素/结构化跳过/digest 绑定/FakeProvider 集成/适配层集成/估算保守性/能力派生 | :391 起 |
| `crates/cc-semantic/src/lib.rs` | `pub mod admission;` + crate 文档 P7-003 段 | :74 / :62-72 |
| `crates/cc-index/src/documents/render.rs` | **消费侧最小触碰**（不重写渲染） | — |
| — `RenderedManifestEntry` | 清单条目：最终输入 bytes（`EmbeddingInput.text` 原样）+ 渲染期盖章的 `token_estimate`/`token_estimator` + 显式 `metadata_truncated` | :163 |
| — `manifest(input)` | 渲染产物 → admission 清单；拒绝外来估算器与漂移估算值（两侧声明口径不可能静默不一致） | :180 |
| — tests（2 项） | bytes 恰为最终输入（含 `CODECORTEX_EMBED_V1` 前缀断言）+ 估算器一致；外来估算器/漂移估算拒绝 | :198 起 |

### 1.2 尺寸与切分口径定义（模块文档原文，admission.rs:1-55）

> - **bytes are hard.** `max_bytes` is enforced on exact rendered input
>   bytes, and the frozen per-input structural bound
>   ([`crate::spec::MAX_INPUT_BYTES`]) is re-checked as a backstop. A bytes
>   verdict from this module is a guarantee, not an approximation.
> - **tokens are an estimate, never a count.** No real tokenizer ships in
>   this workspace. The estimate reuses the codebase-wide frozen estimator
>   `utf8-bytes-div-ceil-4-v1` ([`cc_model::chunk_policy::TOKEN_ESTIMATOR`]:
>   1 token ≈ 4 UTF-8 bytes, ceiling). That estimator can **undercount**
>   real tokenizers on multi-byte scripts (CJK: ≈ 1 token per character at
>   3 bytes per character ⇒ bytes/4 < characters). `max_tokens` is
>   therefore an advisory budget under the declared estimator only; the
>   planner refuses any other tokenizer name ([`tokenizer_gate`]) rather
>   than silently approximating a tokenizer it does not have. Differences
>   against provider-reported/billed tokens are the receipt layer's concern
>   (P7-008 reported/estimated split), not admission's.

切分口径：**按最终输入（渲染后的确切 bytes）计**——admission 消费的是 render 产物 `EmbeddingInput.text` 的字节（render.rs manifest 接口透出，`manifest_reports_exact_final_bytes...` 测试断言以 `CODECORTEX_EMBED_V1\n` 前缀开头即最终成帧输入，非原始 chunk text）；切分算法为顺序保持 first-fit 贪心（`plan_order`，admission.rs:276），同输入+同预算+同估算器 ⇒ 位级同计划（`planning_is_deterministic_same_input_same_cut` 固化）。

截断语义：**admission 层零截断**——放不进任何批的输入产出显式 `Skipped { key, reason }`，不静默、不平均池化、不掩盖（简报原文"超限重切或明确 skip，不平均池化掩盖"的 skip 支）。截断仅存在于 render 层且必带显式 `metadata_truncated` 标记，source text 从不截断（render 既有语义，未改动）。

### 1.3 worker 衔接（P6-013 drain 单输入口径）——**零改动，记录决策**

- `crates/cc-semantic/src/queue.rs` 本轮**零改动**。简报 P7-003 的落点是"把 caller 侧保证（ports.rs:94-95 'the caller guarantees batch size (P6-013 admission), implementations do not split batches'）正式化为 admission 模块"，未要求 worker 升级批次消费。
- 现 `EmbedHandler` 每 attempt 恰嵌 1 条输入（`queue.rs:373-383` 单输入批）：1 ≤ 任何合法 `max_items`，单项 bytes/token 由 `DocumentInput::from_bytes`（冻结 1 MiB + UTF-8 + 非空）与 P7-002 配置期 `spec.max_tokens ≤ capability.max_input_tokens` 硬校验承担——单输入口径**平凡满足**每个批界，无需升级。
- 批式消费 seam 已就绪且经测试固化：组合根聚合多任务时 = 收集渲染 bytes（`render::manifest`）→ `plan_document_batches` → 逐 `BatchPlan` 原样喂 `embed_documents`（不拆批）。`digest_binding_and_provider_order_survive_planning`（FakeProvider，2 批 ⇒ 恰 2 次 provider 调用）与 `adapter_receives_planned_batches_unsplit_in_request_order`（OpenAI 兼容适配层 + mock transport，每请求 input 数 == 批大小）双侧固化。
- worker 路径上若出现真实超模型 token 上限的输入（render 后仍超声明上限）：走 provider `InvalidInput` → fenced retry → dead-letter `failed`，显式不静默；组合根可在 `resolve_input` 前置本模块做零成本预跳过（seam 已备）。

## 2. TDD 与测试清单（原样）

流程：先写测试面 + `todo!()` 桩 → 红轮（编译红 4 处：const 表达式进 match pattern、空集泛型推断、两处缺 `EmbeddingProvider` trait 导入；运行红 13 项 todo panic）→ 实现规划核心 → 绿前修 3 处（`plan_order` 闭包 HRTB 生命周期改泛型切片、u32/usize 求和、适配层 mock 响应按请求 input 数回向量）→ 终态全绿。共 **16 项 admission 测试 + 2 项 render manifest 测试**。

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline admission`（原样，16 项）：

```
test admission::tests::adapter_receives_planned_batches_unsplit_in_request_order ... ok
test admission::tests::budget_construction_from_capability_rejects_zero_bytes_bound ... ok
test admission::tests::digest_binding_and_provider_order_survive_planning ... ok
test admission::tests::empty_input_is_skipped_explicitly ... ok
test admission::tests::empty_set_yields_nothing_and_single_item_yields_one_batch ... ok
test admission::tests::estimator_matches_the_declared_bytes_div_ceil_4_v1 ... ok
test admission::tests::every_planned_batch_respects_every_budget_bound ... ok
test admission::tests::exact_limits_pass_and_one_over_is_skipped ... ok
test admission::tests::foreign_tokenizer_is_refused_not_approximated_silently ... ok
test admission::tests::from_capability_bounds_flow_through_planning ... ok
test admission::tests::frozen_input_byte_bound_is_a_backstop_against_a_loose_budget ... ok
test admission::tests::greedy_fill_matches_the_hand_computed_plan_and_preserves_order ... ok
test admission::tests::planning_is_deterministic_same_input_same_cut ... ok
test admission::tests::query_path_plans_query_inputs_with_the_same_cut ... ok
test admission::tests::skipped_items_never_reach_batches_and_carry_structured_reasons ... ok
test admission::tests::zero_bound_budgets_are_rejected ... ok
```

cc-index manifest 2 项（原样）：

```
test documents::render::tests::manifest_refuses_foreign_estimator_or_drifted_estimate ... ok
test documents::render::tests::manifest_reports_exact_final_bytes_and_the_declared_estimate ... ok
```

要点覆盖（对照任务 TDD 四面）：
- **尺寸边界**：bytes 恰上限过 / +1 skip（`BytesTooLarge{17,16}`）；token 恰上限过 / +1 skip（`TokensTooLarge{3,2}`）；空输入 `EmptyInput`；冻结 1 MiB 兜底（宽松预算下 `MAX_INPUT_BYTES+1` 仍拒）。
- **确定性**：同输入两次切分 `PartialEq` 全等。
- **空集/单元素**：空集 ⇒ 空计划；单元素 ⇒ 单批。
- **贪心对照**：手算计划逐一比对（bytes 界关闭批、items 界关闭批两个变体）；每批 items/bytes/est-tokens 三界性质断言。
- **digest↔bytes 绑定**：计划产物 `verify()` 通过且与直接 `DocumentInput::from_bytes` 逐项 `PartialEq` 相等（简报 acceptance"digest 与 DocumentInput::from_bytes 构造一致，不可漂移"）；查询路径 `QueryInput::verify()` 对偶。
- **provider/适配层集成**：FakeProvider 逐批 embed 数序一致、2 批恰 2 调用（不拆不并）；OpenAI 兼容适配层经 mock transport 收到的每请求 input 数 == 批大小。
- **估算保守性**：`estimate_tokens` 与 `cc_model::approx_tokens` 同式（含 CJK 文本）；外来 tokenizer 名 doc+query 双路径拒绝且错误点名两侧。

## 3. 验证（原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline`
   → lib `test result: ok. 124 passed; 0 failed`（含 16 项 admission）+ 集成套件 `17/4/4/8/3/5/8/6/2 passed; 0 failed` 全绿，零 FAILED。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-index --locked --offline`
   → lib `test result: ok. 379 passed; 0 failed; 1 ignored` + 集成套件 `16/19/14/11/2/2/17 passed` 全绿（`1 ignored` 为既有忽略项，非本轮产生）。
3. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile ... target(s) in 4.69s`；除既有 `reconcile_rebuild` 的 `digest_table` never-used warning（先前轮遗留，未触碰）外零 warning。
4. 零网络证据：全程 offline 锁定运行；`cc-semantic/Cargo.toml`、`cc-index/Cargo.toml` 零改动（无新依赖边）；适配层集成测试唯一 transport 为内存 `RecordingTransport`，请求仅抵达 mock 录制器。

## 4. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| step「按最终输入计token/bytes/batch」 | admission 全部度量基于 render 产物最终 bytes（render.rs `manifest` 透出、测试断言成帧前缀）；bytes 硬界 + 声明估算器 token 预算 + 三界贪心切分（§1.2） |
| step「超限重切或明确skip而非平均池化掩盖」 | 单项放不进任何批 ⇒ 显式 `Skipped { key, reason }`（结构化三变体 + `describe` 稳定单行）；批间放不下 ⇒ 关批新开（重切）；无任何池化/平均路径 |
| acceptance「每项和总batch受限」 | 每项 ≤ 预算三界 + 冻结 1 MiB 兜底；每批 `total_bytes ≤ max_bytes`、`total_estimated_tokens ≤ max_tokens`、`len ≤ max_items`（`every_planned_batch_respects_every_budget_bound` 性质断言） |
| acceptance「文本与向量所代表文档版本一致」 | 批产物 = `DocumentInput::from_bytes(最终渲染 bytes)`，digest↔bytes 构造期绑定 + `verify()` 复核 + 与直接构造逐项相等（测试固化）；outbox 侧 doc_version 一致性由 P6-005/006 既有 per-(doc_key,space) live 唯一 + publish CAS 承担（未触碰） |
| deliverable「实现/配置或规格变更」 | 新 `admission.rs` 模块 + render.rs 消费侧清单接口 + lib.rs 文档段 |
| deliverable「artifacts/benchmarks/<run-id>/ V09/V15 证据」 | **not_run / blocked**：V15 真实 provider 证据依赖 live 调用（D1/D2 不授权）；V09 实际 bytes 与预算的端到端证据依赖组合根接线 + 真实索引流水线。mock 腿证据 = §2 全部测试；benchmark run-id 未生成 |
| validation V09 | **blocked（live/接线腿）** / done（mock 腿：render bytes ↔ admission 度量同源 + 预算性质断言） |
| validation V15 | **blocked（live 腿）** / done（mock 腿：计划批喂 FakeProvider 与适配层全绿） |

## 5. 偏差清单

1. **简报草案的 `tokenizer: &str` 参数保留但语义收紧为"估算器名门"**：草案注释（"spec.rs:263 已有 tokenizer 声明"）暗示透传 spec 的 tokenizer 字段；实现为 `tokenizer_gate`——只接受 `utf8-bytes-div-ceil-4-v1`，其余名字显式拒绝。理由：spec tokenizer 字段是自由字符串（1..=256 bytes），透传 `"cl100k_base"` 而用 bytes/4 估算 = 冒充 tokenizer（红线禁止）；显式拒绝让口径漂移在计划期暴露。
2. **`OversizeReason` 增加 `EmptyInput` 变体**（简报草案只有超限语义）：空输入虽非"超限"但同样"永不合法"，走同一显式 skip 通道而非静默报错/静默丢弃；`describe()` 保留稳定文案。
3. **worker 衔接零改动**（简报允许"若需升级，最小改动并记录"）：判定为不需要——单输入口径平凡满足一切批界（§1.3）；批式消费 seam 经 FakeProvider/适配层双侧集成测试固化，组合根聚合多任务时零新增抽象即可接入。
4. **render.rs 清单接口为 key-free 单条形态**（简报草案 `rendered: &[(impl DocKey, Vec<u8>)]` 的 key 由调用侧持有）：admission 入口保持 `&[(K, Vec<u8>)]` 泛型 key（简报原样）；render 侧只负责"最终 bytes + 声明估算值"的诚实透出（含外来估算器/漂移拒绝），doc_key 配对留给组合根（ChunkRecord 的 doc_key 语义属 identity 层，不在本任务 scope）。
5. **无跨 crate（render→admission）同进程集成测试**：workspace 无任何现成 crate 同时依赖 cc-index 与 cc-semantic（cc-eval 只依赖 cc-index），新增依赖边超出本任务最小改动；以两侧镜像测试（render 断言 bytes=最终输入、admission 断言度量=同源估算器）+ 共享单一来源 `TOKEN_ESTIMATOR` 常量替代，接缝一致性由类型与常量单一来源保证。
6. **`from_capability` 的 bytes 上限是独立入参**：`ModelCapability` 声明 tokens/batch 不声明 bytes（P7-002 冻结面），故 `max_batch_bytes` 留操作者决定；测试固化派生路径与 0 值拒绝。
7. **V09/V15 benchmark run-id 证据未生成**：live 依赖 blocked，如实标 not_run/blocked（§4）；未声称任何真实 provider 预算结论。

## 6. 剩余风险 / 后续

- `plan_document_batches`/`plan_query_batches` 尚无组合根调用方（cc-server 接线归后续轮）；当前全部经测试驱动。接入点：reconcile/重嵌流水线聚合渲染集 → 计划 → 逐批喂 provider；skip 计数进 coverage 分母（P7-008 收据与 P6-014 coverage 语义对齐时需带上 `OversizeReason::describe()`）。
- token 预算在 CJK 重文本下会低估真实计费 token（估算器 `bytes/4` 固有偏差，模块文档已声明）；bytes 界是唯一硬保证，live 解封后由 P7-008 收据层 reported/estimated 对账校准。
- 未执行 `git commit`（红线）；工作区既有未提交改动为先前轮状态，本任务仅新增 `admission.rs`、触碰 `render.rs`（追加清单接口+测试）与 `lib.rs`（模块声明+文档段）。
