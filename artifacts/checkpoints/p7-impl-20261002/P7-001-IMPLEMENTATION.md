# P7-001 实施记录：OpenAI-compatible provider 适配

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-001「实现OpenAI-compatible provider适配」
用户决策口径：D1/D2 —— **本轮不授权真实 provider**。适配层实现完整工程机制；全部测试走本地内存 mock transport，零真实网络调用。live 腿按 conditional blocked 留接口（见 §7）。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/providers/openai_compatible.rs` | 新增模块（本轮唯一新增代码文件） | — |
| — `EmbeddingApiKey` | 注入式凭据 newtype；`Debug` 固定输出 `EmbeddingApiKey([REDACTED])`，无 `Display`，唯一读出口 `expose_secret()` 仅用于构造 `Authorization` 头 | :74 |
| — `HttpRequest` / `HttpResponse` | transport seam 的请求/响应载体（url、headers、body、per-request timeout） | :101 / :114 |
| — `TransportError` | seam 自身三变体：`Io(String)` / `Timeout` / `Cancelled` | :126 |
| — `EmbeddingHttpTransport` | **可注入 HTTP transport trait**，单方法 `post_json`；实现须遵守 per-request timeout、不做重试（重试属调用方策略） | :143 |
| — `OpenAiCompatibleConfig` | 配置结构：`space`（model 即 `space.model_id()`）、`endpoint`、`api_key`、`timeout`（默认 30s）、`transport: Option<Arc<dyn EmbeddingHttpTransport>>`（**默认 None = disabled**） | :151 |
| — `OpenAiCompatibleProvider` | 适配器，实现冻结面 `EmbeddingProvider` | :199 |
| — `OpenAiCompatibleProvider::embed` | 共享管线：空批短路 → UTF-8 守卫（触网前）→ 组包 → transport 调用 → 状态码映射 → 结构校验 | :223 |
| — `build_request` | POST `{endpoint}/embeddings`，体为 `{"model": <space.model_id()>, "input": [...], "encoding_format": "float"}`，`Authorization: Bearer <key>` | :256 |
| — `parse_response` | 严格校验：model 回显比对、count、index 恰为 `0..n`（乱序重排、缺口/重复拒绝）、维度、非数值分量、非有限分量（f64→f32 cast 后判定）、零向量；畸形 JSON/缺 data 数组拒绝 | :283 |
| — `map_transport_error` | transport 三变体 → ProviderError 映射 | :407 |
| — tests + `MockTransport` | 内存 mock transport（脚本化响应 + 请求录制），24 个测试 | :442 / :450 / :496 |
| `crates/cc-semantic/src/providers.rs` | 仅追加模块声明与文档段（P6 冻结交付物未改动逻辑） | 尾部 |

`crates/cc-semantic/src/ports.rs`：**零改动**（冻结面未触碰）。任务条目 scope 含 ports.rs，但按红线以「新增模块」方式落地。

### 1.2 transport 注入设计

- `OpenAiCompatibleConfig.transport: Option<Arc<dyn EmbeddingHttpTransport>>` 是唯一的网络入口。
- **默认 None = fail-closed disabled**：任何非空批 embed 调用返回非重试的 `ProviderError::InvalidInput`，消息明示 disabled 与本轮 blocked 事实。未注入 transport 时不存在任何网络代码路径。
- 本轮**不附带任何生产 transport 实现**（blocked，见 §7）。将来授权真实 provider 时：实现该 trait（一个 POST-JSON 方法）并在组合根注入，本模块零改动。
- 空批在 disabled 检查之前短路返回 `Ok(vec![])` —— 与参考实现 `FakeProvider` 的「空批返回空成功」契约对齐，保证两实现可互换。

### 1.3 六变体 ProviderError 映射表

| 条件 | 变体 | 备注 |
|---|---|---|
| transport 未配置（disabled，fail-closed） | `InvalidInput` | 非重试；消息点名 disabled 状态 |
| transport `Timeout` | `Timeout` | |
| transport `Cancelled` | `Cancelled` | |
| transport `Io(..)` | `ServerError` | 可重试；IO 细节（可能含 host 字符串）不进错误 |
| HTTP 429 | `RateLimited { retry_after }` | `Retry-After`（秒）头解析，缺省 1s |
| HTTP 401 / 403 | `AuthError` | |
| HTTP 5xx | `ServerError` | 可重试 |
| 其他 4xx（400/404/413/422…） | `InvalidInput` | 非重试；消息只带状态码，**响应体永不进错误**（防输入文本回显泄漏） |
| 响应体非 JSON / 非 JSON 对象 / 缺 `data` 数组 | `InvalidInput` | |
| count 不匹配（`data` 条数 ≠ 批大小） | `InvalidInput` | |
| index 集合 ≠ 恰好 `0..n`（重复/缺口/乱序修复后校验） | `InvalidInput` | 乱序按 index 重排后返回批序 |
| 维度 ≠ `space.dimension()` | `InvalidInput` | |
| 响应 `model` 字段存在且 ≠ 请求 model | `InvalidInput` | model 属冻结空间身份，异模型向量不得入 cache |
| 分量非数值 / f64→f32 后非有限（NaN/Inf） | `InvalidInput` | |
| 零向量 | `InvalidInput` | |
| 输入 bytes 非 UTF-8 | `InvalidInput` | 防御守卫（sanctioned 构造器已保证 UTF-8），触网前拒绝 |

凭据卫生：api_key 只经配置注入，`Debug` 固定 redacted，无 `Display`；错误消息与请求体之外无处出现；本模块零日志（不使用 tracing）。

### 1.4 依赖策略

**零新增依赖。** `cc-semantic/Cargo.toml` 未改动（无 reqwest/ureq 等）；网络能力完全经 trait seam 外置，crate 依赖图维持 P6-020 no-network closure。与 workspace 版本策略无冲突（无新条目）。

## 2. TDD 与 mock 测试清单（原样结果）

流程：先定测试面（含 4 个编译红轮修正：`AtomicUsize`/`Ordering` 导入、`catch_unwind` `AssertUnwindSafe`、`1e400` 字面量溢出改 `1e39` raw-body、空批短路次序），终态全绿。

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline` 适配层 24 项（原样）：

```
test providers::openai_compatible::tests::api_key_is_redacted_in_debug_and_never_in_error_messages ... ok
test providers::openai_compatible::tests::empty_batch_short_circuits_without_transport_or_disabled_error ... ok
test providers::openai_compatible::tests::count_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::dimension_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::duplicate_index_is_rejected ... ok
test providers::openai_compatible::tests::empty_success_response_is_rejected_when_batch_is_nonempty ... ok
test providers::openai_compatible::tests::http_401_and_403_map_to_auth_error ... ok
test providers::openai_compatible::tests::http_429_maps_to_rate_limited_with_retry_after ... ok
test providers::openai_compatible::tests::http_429_without_retry_after_uses_the_fallback ... ok
test providers::openai_compatible::tests::http_5xx_maps_to_server_error ... ok
test providers::openai_compatible::tests::invalid_endpoint_is_rejected_eagerly ... ok
test providers::openai_compatible::tests::missing_data_array_and_malformed_json_are_rejected ... ok
test providers::openai_compatible::tests::no_transport_is_fail_closed_disabled ... ok
test providers::openai_compatible::tests::model_mismatch_is_rejected ... ok
test providers::openai_compatible::tests::non_utf8_input_is_rejected_before_any_transport_call ... ok
test providers::openai_compatible::tests::non_numeric_component_is_rejected ... ok
test providers::openai_compatible::tests::other_http_4xx_maps_to_non_retryable_invalid_input_without_body_echo ... ok
test providers::openai_compatible::tests::query_path_uses_the_same_request_shape ... ok
test providers::openai_compatible::tests::request_carries_model_input_order_and_bearer_auth ... ok
test providers::openai_compatible::tests::trailing_slash_on_endpoint_is_trimmed ... ok
test providers::openai_compatible::tests::response_vectors_are_reordered_by_index_to_batch_order ... ok
test providers::openai_compatible::tests::transport_errors_map_to_timeout_cancelled_and_server_error ... ok
test providers::openai_compatible::tests::zero_vector_and_nan_responses_are_rejected ... ok
test providers::openai_compatible::tests::adapter_is_interchangeable_with_fake_provider_on_the_port_and_in_the_cache_loop ... ok
```

要点覆盖：
- 请求形状（model/input 顺序/bearer 头/content-type/endpoint 尾斜杠裁剪/timeout 透传）。
- 成功语义（index 乱序重排、空响应拒绝）。
- 结构拒绝（维度/model/count/重复 index/非数值/非有限/零向量/畸形 JSON）。
- 全六变体状态映射 + transport 三错误映射。
- 凭据卫生（Debug redaction、错误渲染零泄漏、配置 Debug 零泄漏）。
- 非 UTF-8 输入触网前拒绝（请求录制为空断言）。
- **互换性集成**：`FakeProvider` 与适配器同 `dyn EmbeddingProvider` 槽位、同 cache 回路（embed → `ArtifactCache::put` → `get` → 逐位一致），各自维度断言。

## 3. 验证（原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline`
   → `test result: ok. 84 passed; 0 failed`（lib，含 24 项适配层）+ 集成套件 `2/17/4/4/8/3 passed; 0 failed` 全绿，doc-tests 0。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile ... target(s) in 0.70s`（零 error 零 warning 增量）。
3. 零网络证据：`cargo test` 全程 offline 锁定运行；cc-semantic 依赖表未动（无 HTTP crate）；唯一 transport 为内存 mock。

## 4. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| 明确 endpoint/base 路径和认证 | `endpoint + "/embeddings"`，`Authorization: Bearer`；endpoint 归一化（尾斜杠裁剪）+ 构造期 http(s) 校验 |
| trait 不暴露具体客户端类型 | 冻结面 `EmbeddingProvider` 未触碰；网络类型只存在于 `openai_compatible` 模块内部，port 上仅 `dyn EmbeddingProvider` |
| 协议 stub 覆盖成功/错误，provider 可替换 | §2 测试清单；互换性测试证明 FakeProvider ↔ 适配器可替换 |
| deliverable「实现/配置或规格变更」 | 新模块 + 配置结构 |
| deliverable「artifacts/benchmarks/<run-id>/ V15 证据」 | **not_run / blocked**：V15 真实 provider 证据依赖 live 调用（D1/D2 本轮不授权）；本轮以 mock 全链测试 + offline 验证作为替代证据，benchmark run-id 未生成 |
| validation V15 | **blocked（live 腿）** / done（mock 腿，见 §2） |

## 5. live 腿 blocked 声明（双轨口径）

- **mock 腿（本轮 done）**：协议语义、错误映射、结构校验、凭据注入、cache 互换回路，全部经内存 mock transport 验证。
- **live 腿（conditional blocked）**：真实 OpenAI-compatible 服务调用、真实 transport 实现、V15 真实模型 benchmark 证据。依据 D1/D2 本轮不授权；接口已就绪（实现 `EmbeddingHttpTransport` + 组合根注入即可），解封时不需改动本模块与冻结面。

## 6. 偏差清单

1. `ports.rs` 在任务 scope 中但零改动 —— 红线「不动冻结面」优先，条目 scope 理解为「适配需参照 ports.rs」。
2. 生产 transport 不随本轮交付 —— 与条目「协议 stub」一致，属 blocked 而非缺失；trait + fail-closed 默认即「留接口」形态。
3. 空批 + disabled 组合返回 `Ok(empty)` 而非 disabled 错误 —— 为与 FakeProvider 参考契约互换（有意决策，测试固化）。
4. 其他 4xx 映射为 `InvalidInput` 而非 `ServerError` —— 非重试语义更符合六变体分类意图；已在模块头映射表文档化，若上层重试策略需要可复议。
5. V15 benchmark 证据未生成 —— 见 §4，live 依赖 blocked，如实标注 not_run/blocked。
6. 测试中 `invalid_endpoint_is_rejected_eagerly` 用 `AssertUnwindSafe` 捕获构造期 panic —— 构造器沿用 `FakeProvider::new` 的 expect 惯例（组合根 fail-fast）。

## 7. 剩余风险 / 后续

- 响应 schema 强校验深化（字段别名、`usage`、数组外层变体容错）按计划留 P7-004；本轮已覆盖验收要求的维度/model/畸形容错到六变体。
- 未来接真实 transport 时须补：per-request timeout 的真实执行、重试策略（上层）、以及 live 腿 V15 证据回填本条目 evidence。
- 未执行 `git commit`（红线）；改动仅为新增文件与 `providers.rs` 模块声明。
