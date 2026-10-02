# P7-004 实施记录：响应强校验

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-004「响应强校验」
红线遵守：`ports.rs`/`spec.rs` 冻结面零改动；零真实网络（全部 offline 锁定运行 + 内存 mock transport）；响应体/请求体/凭据零泄漏（测试固化）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/providers/openai_compatible.rs` | 本任务核心改造（P7-001 基础结构门 → 四层强校验门） | — |
| — `NormPolicy { Accept, RejectOutside { min, max } }` | 语义级 norm 策略（简报"norm 策略可配"）；`validate()` 校验界有限/非负/有序；`admits(norm)` 界含端点；f64 累积防溢出 | :205 / :224 |
| — `OpenAiCompatibleConfig.norm_policy` | 新配置字段，默认 `Accept`（保持 P7-001 门语义不回归）；构造期 eager 校验（fail-fast） | :270 / 构造器 |
| — `MAX_COMPONENT_JSON_BYTES=64` / `MAX_ENTRY_OVERHEAD_BYTES=256` / `MAX_BODY_SLACK_BYTES=1024` / `MAX_ERROR_FIELD_CHARS=128` | 安全级派生上界与诊断截断常量 | :415-:426 |
| — `max_response_body_bytes(count, dim)` | 响应体大小上限 = `count × (dim×64 + 256) + 1024`（由请求自身派生，无配置旋钮；真实响应必容于界） | :432 |
| — `content_type_is_json(response)` | 协议级 content-type 门：media type（`;` 前）大小写不敏感等于 `application/json`，`charset=utf-8` 参数放行 | :445 |
| — `safe_error_diagnostic(response)` | OpenAI 错误信封 `{"error":{...}}` 的安全诊断：只取 enum 型 `type`/`code` 字符串字段（各截 128 chars）；自由文本 `message` 与 `param` **永不**透出（服务端在此回显请求文本） | :464 |
| — `parse_embeddings_response(space, response, expected_count, norm_policy)` | 强校验门主体：新增第 4 参 norm 策略；四层校验按固定诊断序 | :504 |
| `crates/cc-semantic/src/capability.rs` | 共享门消费方适配（行为零放松） | — |
| — 探针调用点 | `parse_embeddings_response(..., &NormPolicy::Accept)`（探针只断言结构/声明事实，norm 是适配层语义非能力声明） | :448 |
| — `ProbeMockTransport::embeddings_response` | 200 fixture 补 `Content-Type: application/json` 头（门收紧的 fixture 适配，断言零改动） | :774 附近 |
| — 集成测试 config 字面量 | 补 `norm_policy: NormPolicy::Accept` | :1349 |
| `crates/cc-semantic/src/admission.rs` | `RecordingTransport` 的 200 fixture 补 content-type 头（同上，fixture 适配） | :708 附近 |
| `crates/cc-semantic/src/lib.rs` | crate 文档 P7-004 段 | P7-003 段后 |

### 1.2 校验层级清单（模块文档原文，openai_compatible.rs:50-112）

> The shared gate [`parse_embeddings_response`] validates every response at
> four levels, in a fixed diagnostic order:
>
> 1. **Protocol**: only a 2xx status enters the body gate, and only with a
>    JSON content type (media type matched case-insensitively before any
>    `;` parameter, so `application/json; charset=utf-8` passes). Error
>    statuses keep the six-variant mapping; for non-401/403/429 4xx the
>    OpenAI error envelope `{"error": {"type", "code", ...}}` is recognized
>    and only the short enum-like `type`/`code` fields (capped at
>    [`MAX_ERROR_FIELD_CHARS`] chars) may join the diagnostic — the free-form
>    `message` field is *never* surfaced because servers echo request text
>    there. JSON nesting deeper than serde_json's built-in 128-level limit
>    fails parsing and is rejected like any malformed body (depth-bomb
>    defense; also covered by a dedicated test).
> 2. **Security**: the 2xx body is size-capped by a bound *derived from the
>    request itself* (`count × (dimension × 64 bytes/component + 256 bytes
>    entry overhead) + 1024 bytes slack`) and checked **before** parsing, so
>    an oversized body never costs parse work; the `data` count is checked
>    **before** any entry is decoded, so a breadth bomb (millions of entries
>    for a one-item batch) dies at the count gate. Deep JSON is handled by
>    serde_json's 128-level recursion limit. No logging exists in this
>    module and response bodies, request bodies, and the API key never
>    appear in any error message (only short structural facts: lengths,
>    indices, status codes, field types).
> 3. **Schema**: field types are strict — `model` must be a string, `data`
>    an array of objects, `index` a non-negative integer (`0.5` and `"0"`
>    are rejected, not coerced), `embedding` an array of JSON numbers
>    (base64-string embeddings are rejected), `usage` — when present — an
>    object. Unknown extra fields are tolerated (OpenAI-compatible servers
>    routinely add fields).
> 4. **Semantic**: the `model` echo is *required* and must equal the
>    configured model; every vector must have exactly `space.dimension()`
>    finite f32 components (a finite f64 above the f32 range overflows the
>    cast and is rejected), must not be all-zero, and — when the configured
>    [`NormPolicy`] says so — must carry an L2 norm inside the configured
>    inclusive range; the index set must be exactly `0..n` (out-of-order is
>    reordered by index, gaps/duplicates rejected).

### 1.3 状态码/拒绝语义六变体映射（协议级新增行加粗）

| 条件 | 变体 | 可重试性 |
|---|---|---|
| transport 未配置（disabled，fail-closed） | `InvalidInput` | 非重试 |
| transport `Timeout` / `Cancelled` / `Io` | `Timeout` / `Cancelled` / `ServerError` | Io 可重试 |
| HTTP 429 | `RateLimited { retry_after }` | 可重试 |
| HTTP 401 / 403 | `AuthError` | 非重试 |
| HTTP 5xx | `ServerError` | 可重试 |
| **HTTP 3xx（redirect）** | `InvalidInput` | 非重试（endpoint 配置错误；携带凭据的 URL 不允许被静默改写，永不跟随重定向） |
| **HTTP 1xx 或 100..599 之外** | `ServerError` | 可重试（provider/proxy 违反 HTTP 语义，非请求属性） |
| 其他 4xx | `InvalidInput`（+ 错误信封 `type`/`code` 安全诊断） | 非重试 |
| 全部校验失败（协议/安全/schema/语义任一层） | `InvalidInput` | 非重试，**整体拒绝**（一坏俱拒，批级重试归上层策略） |

可重试集合恒为 `{429, 5xx, 1xx/越界状态码, transport Io}`，其余一切拒绝都是非重试 `InvalidInput`。

### 1.4 「错误向量不缓存」的结构性保证

强校验门任一失败 ⇒ `embed` 返回 `Err` ⇒ 调用侧的 `ArtifactCache::put`（cache.rs:340）与 publish 路径根本拿不到向量。测试 `error_vectors_never_reach_the_artifact_cache` 固化：控制组合法响应 put 恰 1 个 artifact，随后异模型回显/零向量/1e39 溢出三类畸形响应逐一拒绝，cache 落盘 `.bin` 计数恒为 1，且控制组 Hit 原样存活。

## 2. TDD 与测试清单（原样）

流程：先扩测试面再改门。红轮 1 次：`redirect_informational_and_unknown_statuses_follow_http_semantics` 暴露 1xx 落入 4xx 兜底分支（`http 100` 被映射为 `InvalidInput` 而非 `ServerError`）——修正为显式 `100..200` 分支后全绿。共新增 **16 项**强校验测试，适配层合计 **40 项**（P7-001 既有 24 项全数保留、断言零放松）。

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline openai_compatible`（原样，40 项）：

```
test providers::openai_compatible::tests::adapter_is_interchangeable_with_fake_provider_on_the_port_and_in_the_cache_loop ... ok
test providers::openai_compatible::tests::api_key_is_redacted_in_debug_and_never_in_error_messages ... ok
test providers::openai_compatible::tests::breadth_bomb_dies_at_the_count_gate_before_decoding_entries ... ok
test providers::openai_compatible::tests::count_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::deeply_nested_json_is_rejected_by_the_depth_limit ... ok
test providers::openai_compatible::tests::dimension_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::duplicate_index_is_rejected ... ok
test providers::openai_compatible::tests::embedding_must_be_a_numeric_array_not_a_string_or_object ... ok
test providers::openai_compatible::tests::empty_batch_short_circuits_without_transport_or_disabled_error ... ok
test providers::openai_compatible::tests::empty_success_response_is_rejected_when_batch_is_nonempty ... ok
test providers::openai_compatible::tests::error_vectors_never_reach_the_artifact_cache ... ok
test providers::openai_compatible::tests::fractional_negative_and_string_indices_are_rejected ... ok
test providers::openai_compatible::tests::http_401_and_403_map_to_auth_error ... ok
test providers::openai_compatible::tests::http_429_maps_to_rate_limited_with_retry_after ... ok
test providers::openai_compatible::tests::http_429_without_retry_after_uses_the_fallback ... ok
test providers::openai_compatible::tests::http_5xx_maps_to_server_error ... ok
test providers::openai_compatible::tests::invalid_endpoint_is_rejected_eagerly ... ok
test providers::openai_compatible::tests::missing_data_array_and_malformed_json_are_rejected ... ok
test providers::openai_compatible::tests::model_echo_is_required_and_must_be_a_string ... ok
test providers::openai_compatible::tests::model_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::negative_zero_vector_is_rejected ... ok
test providers::openai_compatible::tests::no_transport_is_fail_closed_disabled ... ok
test providers::openai_compatible::tests::non_numeric_component_is_rejected ... ok
test providers::openai_compatible::tests::non_utf8_input_is_rejected_before_any_transport_call ... ok
test providers::openai_compatible::tests::norm_policy_accept_is_the_default_and_admits_unnormalized_vectors ... ok
test providers::openai_compatible::tests::norm_policy_is_validated_eagerly ... ok
test providers::openai_compatible::tests::norm_policy_rejects_vectors_outside_the_configured_range ... ok
test providers::openai_compatible::tests::other_http_4xx_maps_to_non_retryable_invalid_input_without_body_echo ... ok
test providers::openai_compatible::tests::oversized_response_body_is_rejected_before_json_parsing ... ok
test providers::openai_compatible::tests::provider_error_envelope_diagnostics_exclude_the_free_form_message ... ok
test providers::openai_compatible::tests::query_path_uses_the_same_request_shape ... ok
test providers::openai_compatible::tests::redirect_informational_and_unknown_statuses_follow_http_semantics ... ok
test providers::openai_compatible::tests::request_carries_model_input_order_and_bearer_auth ... ok
test providers::openai_compatible::tests::response_vectors_are_reordered_by_index_to_batch_order ... ok
test providers::openai_compatible::tests::success_responses_require_a_json_content_type ... ok
test providers::openai_compatible::tests::trailing_slash_on_endpoint_is_trimmed ... ok
test providers::openai_compatible::tests::transport_errors_map_to_timeout_cancelled_and_server_error ... ok
test providers::openai_compatible::tests::usage_field_must_be_an_object_when_present ... ok
test providers::openai_compatible::tests::validation_errors_never_carry_request_or_response_payloads ... ok
test providers::openai_compatible::tests::zero_vector_and_nan_responses_are_rejected ... ok
test result: ok. 40 passed; 0 failed
```

### 2.1 反例矩阵规模（新增 16 项，按层）

| 层 | 测试 | 反例（每项均为被拒/被正确映射的正例对照） |
|---|---|---|
| Schema | `fractional_negative_and_string_indices_are_rejected` | `index = 0.5` / `-1` / `"0"`（不-coercing 类型门） |
| Schema | `embedding_must_be_a_numeric_array_not_a_string_or_object` | base64 字符串 embedding / object embedding |
| Schema+语义 | `model_echo_is_required_and_must_be_a_string` | 缺 `model` / `model: 42` / 异模型回显（三段消息分别可诊断；异模型消息保持 P7-001 原文以维持 capability `Mismatch` 归类） |
| Schema | `usage_field_must_be_an_object_when_present` | `usage: "12 tokens"` 拒 / `usage: {}` 放行 |
| 语义 | `norm_policy_rejects_vectors_outside_the_configured_range` | norm 1.2 出 `[1,1]` 界拒 / norm 恰 1.0（位精确可表示）含端点放行 |
| 语义 | `norm_policy_accept_is_the_default_and_admits_unnormalized_vectors` | 默认 Accept 下 norm 4.0 放行（P7-001 语义不回归） |
| 语义 | `norm_policy_is_validated_eagerly` | `min>max` / 负界 / NaN 界 ⇒ 构造期 panic |
| 语义 | `negative_zero_vector_is_rejected` | `[-0.0; 4]` 全零拒 |
| 协议 | `success_responses_require_a_json_content_type` | 缺 content-type / `text/html` 拒；`application/json; charset=utf-8` 放行 |
| 协议 | `redirect_informational_and_unknown_statuses_follow_http_semantics` | `302`→非重试 InvalidInput；`100`/`999`→可重试 ServerError |
| 协议 | `provider_error_envelope_diagnostics_exclude_the_free_form_message` | 400 信封：`type`/`code` 入诊断，`message`（回显 `CONFIDENTIAL-TEXT`）与 `param` 永不透出 |
| 安全 | `oversized_response_body_is_rejected_before_json_parsing` | 2048 字节非 JSON 体 > 1536 派生界 ⇒ 先于解析拒绝（界值 2048/1536 均断言）；界内合法体放行 |
| 安全 | `deeply_nested_json_is_rejected_by_the_depth_limit` | 300 层嵌套 ⇒ serde_json 深度上限 ⇒ `not valid JSON` |
| 安全 | `breadth_bomb_dies_at_the_count_gate_before_decoding_entries` | 200 个 `null` 条目对 1 条批次 ⇒ count 门先于条目解码（条目非对象也不报解码错），消息与既有 count 门逐字一致 |
| 安全（泄漏） | `validation_errors_never_carry_request_or_response_payloads` | 3 场景（400 回显信封 / 恶意 extra 字段 / 体含标记的畸形 JSON）：请求文本、响应体标记、API key 三者均不出现在 `{err:?}` 渲染 |
| 安全（缓存） | `error_vectors_never_reach_the_artifact_cache` | §1.4：异模型/零向量/1e39 三类错误向量 0 落盘 |

## 3. 验证（原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline`
   → lib `test result: ok. 140 passed; 0 failed`（= 前 124 + 新增 16，含 40 项适配层与 24 项 capability 全绿）+ 集成套件 `17/4/4/8/3/5/8/6/2 passed; 0 failed` 全绿，零 FAILED。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.79s`（零 error 零 warning）。
3. 零网络证据：全程 offline 锁定运行；`cc-semantic/Cargo.toml` 零改动（仍无 HTTP crate）；唯一 transport 为内存 mock。

## 4. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| step「数量/index完整且唯一」 | count 门（先行、消息逐字保留）+ index 集合恰为 `0..n`（乱序按 index 重排恢复、重复/缺口/负数拒） |
| step「dim」 | 每向量恰 `space.dimension()` |
| step「finite」 | f64→f32 cast 后逐分量 `is_finite`（1e39 溢出即拒） |
| step「norm检查」 | `NormPolicy`（默认 `Accept` 保持既有语义；`RejectOutside` 含端点区间拒，f64 累积），构造期 eager 校验 |
| step「错误向量不缓存」 | 整体拒绝语义 + `error_vectors_never_reach_the_artifact_cache` 落盘计数固化（§1.4） |
| acceptance「乱序可正确恢复，重复index/NaN/zero必被拒绝」 | 既有 `response_vectors_are_reordered_by_index_to_batch_order` / `duplicate_index_is_rejected` / `zero_vector_and_nan_responses_are_rejected` 全数保留通过，断言零放松 |
| acceptance「相关旧功能回归通过」 | lib 140 + 集成 9 套件全绿（§3.1）；capability 24 项共享门测试全绿 |
| deliverable「artifacts/benchmarks/<run-id>/ V15/V16 证据」 | **not_run / blocked**：live 腿依赖真实 provider（D1/D2 本轮不授权）；mock 腿证据 = §2 全部测试，benchmark run-id 未生成 |
| validation V16 | **blocked（live 腿）/ done（mock 腿：手算参照 + 畸形矩阵直连输出门，§2.1）** |

## 5. 偏差清单

1. **`model` 回显从「存在且 ≠ 才拒」收紧为「必需且必须为字符串」**：语义级"model 回显"的完整形态；缺回显消息不含 `does not match configured model` 子串，capability `classify` 正确归 `Unavailable`（能力未知）而非 `Mismatch`（声明矛盾）。P7-001 的 `missing_data_array` fixture 补 `model` 后原断言原样通过。
2. **content-type 门收紧为 2xx 必需**：共享门收紧使 P7-001 / capability / admission 三处 200 fixture 需补 `Content-Type` 头——均为 fixture 适配，测试断言零改动零放松。
3. **简报草案 `RawEmbeddingReply`/`validate_reply(batch_len, space, reply, norm)` 独立输出门函数未按草案新建**：P7-002 已把 `parse_embeddings_response` 提取为共享门（探针判定与真实调用同门是更强的不变量），本任务在其上深化并加 `norm_policy` 参数，而非另立第二入口（两个门必然漂移）。
4. **简报"norm 策略可配（记录 or 区间拒绝）"只落地区间拒绝 + 不检查**：「记录」形态在冻结 port（`embed_* -> Vec<Vec<f32>>`，无日志通道，模块零日志）上无出通道，不实现；`Record` 变体留待组合根接线轮若有观测需求再议。
5. **norm 边界测试用位精确可表示值**（`[0.5;4]` 的 norm 恰 1.0）：0.9 一类十进制界在 f32/f64 混合下不精确可表示，含端点断言会假红；实现本身用 f64 比较无误。
6. **3xx 映射为非重试 `InvalidInput`**：简报未覆盖重定向语义；跟随重定向会静默改写携带凭据的目标 URL（安全），重试同请求无意义（endpoint 配置错误），故非重试。1xx/越界状态码映射可重试 `ServerError`（provider/proxy 语义违规，非请求属性）。
7. **响应体大小上界为请求派生值而非配置项**：`count×(dim×64+256)+1024` 随请求伸缩，真实响应必容于界，无需操作者旋钮；避免"上界配小了误杀合法响应、配大了不防炸弹"的两难。
8. **`usage` 若存在必须为 object**（简报未列）：宽松容错畸形信封与"schema 严格"矛盾；值内容不做语义校验（计费口径归 P7-008 收据层）。
9. **index 缺失/类型错误的诊断消息从 "has no numeric index" 改为 "index is not a non-negative integer"**：覆盖负数/小数/字符串三种同型违规，消息更可诊断；无既有断言依赖旧文案。
10. **V15/V16 benchmark run-id 证据未生成**：live 依赖 blocked，如实标 not_run/blocked（§4）。

## 6. 剩余风险 / 后续

- 400 类错误信封的 `type`/`code` 是 provider 控制的短字符串（截 128 chars 后入诊断）；主流实现是 enum 型取值，若某实现把回显塞进这两个字段，理论上仍可透出——`message`/`param` 已永久排除，`type`/`code` 的白名单化留待 live 腿观察真实取值后收紧。
- `NormPolicy` 尚无组合根调用方（配置接线归后续轮）；默认 `Accept` 保证未接线时语义与 P7-001 完全一致。
- 真实 provider 解封后需回填：live 强校验证据、V15/V16 benchmark 证据至本条目 evidence。
- 未执行 `git commit`（红线）；工作区既有未提交改动为先前轮状态，本任务触碰 `openai_compatible.rs`、`capability.rs`（共享门消费适配）、`admission.rs`（fixture 一行）、`lib.rs`（文档段）。
