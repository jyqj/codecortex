# P7-002 实施记录：模型参数与能力验证

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-002「模型参数与能力验证」
口径：D1/D2 —— **live 腿 conditional blocked，本轮零真实网络**。能力验证 = mock 契约 + 能力探测协议（经 P7-001 注入式 transport seam，全部测试走内存 mock）。红线遵守：`ports.rs` 零改动；`spec.rs` 冻结常量零改动（仅新增类型/模块）；凭据零泄漏；tasks.json status 未改；未 git commit。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/capability.rs` | **本任务核心新增模块**（1351 行，含测试） | — |
| — `PROBE_PROTOCOL_VERSION = 1` / `MAX_PROBE_BATCH_ITEMS = 4_096` | 探针协议版本与可声明批次上限（提额=协议变更需 bump 版本，不静默改） | :85 / :91 |
| — `DimensionsMode { Configurable, Fixed }` | 端点是否接受请求体显式 `dimensions` 字段；`parse` 拒绝未知值 | :104 |
| — `EncodingFormat { Float }` | 协议 v1 唯一承认的 `encoding_format`（适配器只发 `float`） | :137 |
| — `ModelCapability` | 模型声明能力结构（见 §1.2 原文） | :163 |
| — `ModelCapability::validate_bounds` | 声明面结构边界（model_id/维度/token 上限冻结区间/批次 1..=4096/编码格式非空），全部 `CcError::Config` 且点名配置键 | :186 起 |
| — `VerifiedCapability` | 成功探针判定（model_id、dimension、`SpaceDigest`、protocol）；唯一可入缓存的类型 | :229 |
| — `CapabilityProbeError { Mismatch, DimensionsUnsupported, Unavailable(ProviderError) }` | 探针失败三分类 + `into_config_error`（每类失败都是拒启性 Config 错误并写明补救路径） | :239 |
| — `classify` | 共享结构门失败的归类：dimension/model echo 违背声明 → `Mismatch`；其余（transport/HTTP/畸形）→ `Unavailable`（能力未知） | :285 |
| — `CapabilityProber` | 探针器：同一 wire 协议（`{endpoint}/embeddings`、bearer、`encoding_format:"float"`）、同一 `EmbeddingHttpTransport` seam；`new` 构造期校验 endpoint（Config 错误，非 panic）；`Debug` 手写（key redaction、transport 只显 `<configured>`） | :300 |
| — `CapabilityProber::probe` → `probe_dimension` / `probe_batch_limit` | 探针协议 v1（见 §1.3） | :360 / :378 / :413 |
| — `build_request` | 探针请求：固定 marker 文本（`"codecortex capability probe v1"`）、`Configurable` 模式加 `"dimensions": <space.dimension()>`、bearer 凭据只进 Authorization 头 | :449 |
| — `CapabilityCache` | 进程内成功判定缓存，键 = 冻结 `SpaceDigest`；`insert` 只收 `VerifiedCapability`，失败结构性无法入缓存 | :489 |
| — `verify_capability(prober, cache, space, capability, force)` | 缓存口径入口：命中不触 transport；`force=true` 重探并刷新；失败不写缓存 | :526 |
| — `validate_capability(capability, space, doc_spec, query_spec)` | 声明能力 vs 冻结面一致性校验（见 §1.4），错误顺序确定 | :549 |
| — `resolve_provider(&SemanticProviderConfig)` | 配置 → (VectorSpace, ModelCapability)；`enabled=false` 显式拒绝；每个能力相关字段 enabled 时**必填**（缺键点名键名，无宽默）；metric/mode/format 未知值显式拒绝 | :634 |
| — tests（24 项） | 一致性正反例 / resolve 拒绝矩阵 / 探针 mock 全链 / 缓存口径 / 凭据卫生 / 与 P7-001 适配层集成 | :662 起 |
| `crates/cc-model/src/config.rs` | 首组语义配置键 | — |
| — `SemanticProviderConfig` | `semantic.*` 节（enabled/model_id/dimensions/metric/dimensions_mode/endpoint/api_key_ref/max_input_tokens/max_batch_items/encoding_formats/supports_instruction），serde 全字段 default、部分节可解析 | :36 |
| — `ProjectConfig.semantic` | 挂载点（`#[serde(default)]`，旧配置文件零破坏） | :19 |
| — `Default`（手动） | `enabled=false`、`metric="cosine"`、`dimensions_mode="configurable"`、`encoding_formats=["float"]`、其余空 —— C14 语义：关闭时字段可解释但不生效 | :124 起 |
| — tests（2 项新增） | 默认惰性 + 部分节回填；`semantic.*` 键对 unknown-key 诊断面板干净 | :1370 / :1396 |
| `crates/cc-semantic/src/providers/openai_compatible.rs` | **仅重构提取，行为零改动** | — |
| — `parse_embeddings_response(space, response, expected_count)` | 原 `parse_response` 方法体提取为 `pub(crate)` 自由函数，方法委托之；探针与适配器走**同一**结构门（探针判定与真实 embed 调用永不可能对"什么是合法响应"产生分歧） | :296 |
| — `map_transport_error` | `fn` → `pub(crate)`（探针复用六变体映射） | :418 |
| `crates/cc-semantic/src/lib.rs` | `pub mod capability;` + crate 文档一句话 | :70 / 头部 |
| `docs/CONFIGURATION.md` | 「`semantic` 配置节（P7-002，声明面）」文档段（C14 义务：新键可见；修正"没有语义键"旧陈述） | 语义节内 |

### 1.2 能力结构定义原文（`ModelCapability`，capability.rs:163）

```rust
/// The declared capability of one embedding model, as resolved from
/// configuration. All fields are explicit operator assertions — this struct
/// has no constructor that invents a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapability {
    /// Must equal the space's `model_id` (validated by
    /// [`validate_capability`]); it names the model this sheet describes.
    pub model_id: String,
    /// Declared output dimension; must equal `space.dimension()`.
    pub dimensions: u32,
    /// Declared metric; must equal the frozen `space.distance()` (v1:
    /// cosine only).
    pub metric: DistanceMetric,
    pub dimensions_mode: DimensionsMode,
    /// Per-input token limit of the model. Document/query spec
    /// `max_tokens` must not exceed it.
    pub max_input_tokens: u32,
    /// Maximum batch size the endpoint accepts (and the probe verifies).
    pub max_batch_items: u32,
    /// Supported request `encoding_format` values; must contain
    /// [`EncodingFormat::Float`].
    pub encoding_formats: Vec<EncodingFormat>,
    /// Whether the model accepts instruction prefixes. A spec declaring an
    /// `instruction` against `false` is a config error.
    pub supports_instruction: bool,
}
```

### 1.3 探针协议 v1（`PROBE_PROTOCOL_VERSION = 1`）

- **维度探针**：1 条固定 marker 输入。`Configurable` 模式请求体携带显式
  `"dimensions": <space.dimension()>`；`Fixed` 模式不携带。失败归因：
  仅当失败为「4xx 类请求拒绝」（`Unavailable(ProviderError::InvalidInput)`）时
  才发**对照探针**（去掉 `dimensions` 字段重发）——对照成功 ⇒ 判定
  `DimensionsUnsupported`（配置路径 `dimensions_mode = "fixed"`，绝不伪成功）；
  Mismatch（服务端收下请求但答错维度/回显异模型）与非请求性失败
  （transport/5xx）不重发，判定即结论。
- **批次上限探针**：恰好 `max_batch_items` 条 marker 输入（条目带序号后缀
  防服务端去重），响应必须全量返回且过共享结构门（count/index 0..n/维度/
  有限性/零向量/model 回显）。可声明上限被 `MAX_PROBE_BATCH_ITEMS = 4096`
  封顶（提额 = 协议变更）；逐项 token/byte 级准入归 P7-003，不在本协议内。
- **失败映射**：`Mismatch` = 观测违背声明（拒启性，重试无意义）；
  `DimensionsUnsupported` = 字段被拒（typed，独立于 Mismatch）；
  `Unavailable(ProviderError)` = 无法下结论（Timeout/Cancelled/ServerError/
  畸形响应/HTTP 429/401/5xx/4xx）。三者的 `into_config_error()` 都映射为
  `CcError::Config` 并写明补救路径。
- **缓存口径**：只有成功判定入 `CapabilityCache`（键 = 冻结 `SpaceDigest`，
  判定自带 digest 防跨空间误挂）；失败永不入缓存；缓存命中零 transport
  调用；`force=true` 重探刷新。启动时验证 = 组合根调
  `verify_capability(prober, Some(&cache), …)`；显式 probe = `force=true`。
- **凭据卫生**：key 仅经 `EmbeddingApiKey` 注入、仅出现在 Authorization 头；
  `CapabilityProber` 手写 `Debug`（redaction + `<configured>`）；
  `CapabilityProbeError` 渲染零 key（测试固化）。

### 1.4 一致性校验（`validate_capability`，顺序确定）

`capability.model_id == space.model_id()` → `capability.dimensions ==
space.dimension()` → `capability.metric == space.distance()`（v1 编译期
admission gate：新增 `DistanceMetric` 变体在此编不过）→ `encoding_formats`
含 `Float` → doc/query spec 绑定同一 space（ADR-0003 不混空间）→
`doc_spec.max_tokens() <= capability.max_input_tokens`、query 同 →
spec 声明 instruction 而 `supports_instruction == false` → 拒启（doc/query
各自独立报错）。任一失败 = `CcError::Config`，点名配置键。

## 2. TDD 与测试清单（原样）

流程：先定测试面；三轮红（对照探针无条件重发耗尽 mock 脚本 → 改为仅
4xx 类请求拒绝触发对照；查询侧 max_tokens 用例 doc spec 同超限先命中
doc 检查 → 换窄 doc spec；空端点用例传空白串未触空检查 → 改传空串），
终态全绿。共 **24 项 capability 测试 + 2 项 cc-model 配置测试**。

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline capability`（原样，24 项）：

```
test capability::tests::a_verdict_cannot_be_filed_under_a_foreign_space ... ok
test capability::tests::adapter_embeds_after_capability_probe_over_the_same_mock_chain ... ok
test capability::tests::batch_count_mismatch_is_unavailable_not_a_mismatch ... ok
test capability::tests::batch_items_outside_the_probe_protocol_bound_are_rejected ... ok
test capability::tests::cached_verdicts_avoid_the_transport_and_failures_are_never_cached ... ok
test capability::tests::capability_dimension_mismatch_is_a_config_error ... ok
test capability::tests::capability_matching_space_and_specs_passes ... ok
test capability::tests::capability_model_mismatch_is_a_config_error ... ok
test capability::tests::dimension_probe_configurable_sends_the_dimensions_field ... ok
test capability::tests::dimensions_unsupported_maps_to_the_typed_verdict_via_the_control_probe ... ok
test capability::tests::disabled_config_is_rejected_by_resolve ... ok
test capability::tests::fixed_mode_probe_omits_the_dimensions_field ... ok
test capability::tests::instruction_without_support_is_rejected_on_both_paths ... ok
test capability::tests::missing_float_encoding_support_is_a_config_error ... ok
test capability::tests::missing_required_fields_name_their_config_keys ... ok
test capability::tests::model_echo_mismatch_is_a_mismatch ... ok
test capability::tests::probe_errors_never_leak_the_api_key ... ok
test capability::tests::prober_rejects_bad_endpoints_as_config_errors ... ok
test capability::tests::resolve_provider_builds_a_space_with_the_frozen_identity ... ok
test capability::tests::spec_max_tokens_above_the_model_limit_is_rejected_on_both_paths ... ok
test capability::tests::specs_bound_to_a_foreign_space_are_rejected ... ok
test capability::tests::transport_failures_map_to_unavailable_with_the_provider_taxonomy ... ok
test capability::tests::unknown_metric_mode_and_format_strings_are_rejected_not_swallowed ... ok
test capability::tests::wrong_dimension_in_the_response_is_a_mismatch ... ok
```

cc-model 配置节 2 项（原样）：

```
test config::tests::semantic_section_is_inert_by_default_and_fills_defaults_when_partial ... ok
test config::tests::semantic_section_keys_are_known_config_keys ... ok
```

要点覆盖：
- 一致性正例 + 全部反例矩阵（model/维度/异空间绑定/doc+query 双路径
  max_tokens 超限/双路径 instruction 不支持/缺 float 编码）。
- `resolve_provider` 拒绝矩阵：disabled、四个缺失键各自点名、未知
  metric/dimensions_mode/encoding_formats（不能吞）、批次越界（0 与 >4096）、
  坏 endpoint 为 Config 错误非 panic。
- 探针 mock 全链：`dimensions` 字段上线、fixed 模式不下发字段、
  供应商不支持 dimensions → 对照探针 → typed `DimensionsUnsupported` +
  配置补救路径（验收原句）、错维度/异模型回显 → `Mismatch`、批次数不符 →
  `Unavailable`、transport 三错误 → `Unavailable` 六变体映射、
  缓存命中零调用 / force 重探 / 失败不入缓存 / 判定不可跨空间挂载。
- 凭据卫生：探针错误 Debug 与 `into_config_error` 渲染零 key。
- **与 P7-001 集成**：同一 mock 全链 resolve → validate → probe（入缓存）
  → `OpenAiCompatibleProvider::embed_documents`，共享 `parse_embeddings_response`
  门，探针判定维度与适配器实际输出一致。

## 3. 验证（原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline`
   → lib `test result: ok. 108 passed; 0 failed`（含 24 项 capability）+
   集成套件 `17/4/4/8/3/5/8/6/2 passed; 0 failed` 全绿，零 FAILED。
2. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-model --locked --offline`
   → `test result: ok. 82 passed; 0 failed`（含 2 项 semantic 配置节）+ 5/3/1/1 全绿。
3. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile ... target(s) in 12.48s`（本任务文件零 error 零 warning；
   既有 `reconcile_rebuild` 的 `digest_table` never-used warning 为先前轮遗留，未触碰）。
4. 零网络证据：全部 offline 锁定运行；`cc-semantic/Cargo.toml` 未改（仍无
   HTTP crate）；唯一 transport 为内存 mock；测试断言请求仅抵达 mock 录制器。

## 4. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| step「模型revision、dimensions、metric、query instruction显式校验」 | `validate_capability`（§1.4）：维度/metric/instruction 显式校验 + model 身份/space 绑定/max_tokens 上限/编码格式；"revision" 由冻结 `ENCODING_SPEC_VERSION`（spec.rs:39，未改）+ `VectorSpace.spec_version` 校验承担，能力面新增 `PROBE_PROTOCOL_VERSION` 独立版本化 |
| step「支持差异不能吞」 | 拒绝矩阵全覆盖：未知 metric/mode/format、缺失必填键、每个不匹配维度均 `CcError::Config` 点名配置键，无静默默认 |
| acceptance「供应商不支持dimensions时给错误/配置路径，不伪成功」 | 探针对照协议 → typed `DimensionsUnsupported` → `into_config_error` 给出 `dimensions_mode = "fixed"` 补救路径；测试 `dimensions_unsupported_maps_to_the_typed_verdict_via_the_control_probe` 固化 |
| deliverable「实现/配置或规格变更」 | 新 `capability.rs` 模块 + `cc-model` 首组语义配置键 + CONFIGURATION.md 文档 |
| deliverable「artifacts/benchmarks/<run-id>/ V15 证据」 | **not_run / blocked**：V15 真实 provider 证据依赖 live 调用（D1/D2 不授权）；mock 腿证据 = §2 全链测试，benchmark run-id 未生成 |
| validation V15 | **blocked（live 腿）** / done（mock 腿） |
| validation V18 | done（mock 腿）：新配置键全 serde-default，旧 `.codecortex.json` 零破坏（`partial_config_fills_missing_fields_with_defaults` 等既有测试不回归）；unknown-key 诊断对 `semantic.*` 干净；默认 `enabled=false` 即"默认无网络无 key 可用" |

## 5. live 腿 blocked 声明（双轨口径）

- **mock 腿（本轮 done）**：能力结构、一致性校验、探针协议语义、失败三分类、
  缓存口径、与适配器共享结构门，全部经内存 mock transport 验证。
- **live 腿（conditional blocked）**：真实端点上的维度/上限探测、真实
  `max_input_tokens` 与计费 token 的偏差核实、V15/V18 真实证据。解封时：
  组合根注入真实 `EmbeddingHttpTransport` + `CapabilityProber` 即可，
  本模块与冻结面零改动。

## 6. 偏差清单

1. **`SemanticProviderConfig` 落在 `cc-model/src/config.rs` 而非 `spec.rs`**；
   `spec.rs` 仅被引用、零改动（含常量）。简报的「spec.rs 校验深化」以新增
   `capability.rs` 模块实现（红线允许的"扩展走新增类型"），冻结面零触碰。
2. **探针只做结构上限（维度 + 批次数），不做 token 上限实测**：真实 token
   计数依赖 tokenizer 与计费口径（P7-008 收据区分 reported/estimated 的同源
   问题），本轮 `max_input_tokens` 走声明 + 与 spec `max_tokens` 的一致性
   硬校验；实测归 live 腿。
3. **`max_batch_items` 引入协议级上限 4096**：简报未定值；超限声明拒启并
   点名"probe protocol vN bound"，提额需 bump `PROBE_PROTOCOL_VERSION`。
4. **`dimensions_mode`（configurable/fixed）为简报草案之外的新配置键**：
   验收句"供应商不支持 dimensions 时给错误/配置路径"需要一条显式配置路径，
   该键即路径本身；默认 `configurable`。
5. **配置键命名沿用简报 `semantic.*` 草案**（OPEN-QUESTIONS Q6 未裁决，
   按草案落地）；若 Q6 另有裁决，迁移面 = `SemanticProviderConfig` 的
   serde 键名 + 本文档，逻辑零改动。
6. **对照探针仅在「4xx 类请求拒绝」时触发**（初版无条件触发会为 Mismatch/
   transport 失败浪费请求并耗尽 mock 脚本，红轮修正）；协议语义见 §1.3。
7. **`openai_compatible.rs` 的 `parse_response` 提取为 `pub(crate)` 自由
   函数**：为满足"探针与真实调用共享同一响应门"，行为零改动（24 项
   P7-001 测试不回归为证）。
8. **V15/V18 benchmark run-id 证据未生成**：live 依赖 blocked，如实标
   not_run/blocked（§4）；未声称任何真实 provider 能力结论。
9. **`docs/CONFIGURATION.md` 超出条目 scope（scope 仅两 crate）**：C14
   配置可见性义务 + 修正"没有语义键"旧陈述，最小段落，未触及其它内容。

## 7. 剩余风险 / 后续

- `resolve_provider`/`validate_capability`/`verify_capability` 尚无组合根
  调用方（cc-server 接线归后续轮）；当前全部经测试驱动。
- 探针为显式调用形态；"启动时自动探测"的触发点与失败降级形态
  （`semantic_state` 透出）归组合根接线轮决策。
- 真实 provider 解封后需回填：live 探测证据、`max_input_tokens` 实测校准、
  V15/V18 benchmark 证据至本条目。
- 未执行 `git commit`（红线）；工作区中既有的 tasks.json/CONFIGURATION.md
  等未提交改动为先前轮工作区状态，本任务未触碰其内容
  （CONFIGURATION.md 仅新增本文档段落）。
