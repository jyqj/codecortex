# P7-007 实施记录：代码外发与凭据政策（执行机制腿）

日期：2026-10-02
任务：`docs/roadmap/code-index-v2/tasks.json` P7-007「代码外发与凭据政策」（批次 2 第二任务）
口径：按用户 D1/D2 决策**只做执行机制腿**——默认无网络、key 外部引用、
日志脱敏、重定向剥认证/不跟随、https 强制、超时强制的机制与测试；政策
文本按"默认无网络 + 显式 opt-in"收口。**live 真实外发归 P7-018，本轮
conditional blocked**；敏感文件外发分类矩阵同因 blocked，未建配置面。
零真实网络（全部内存 mock，offline 锁定运行）；`ports.rs`/`spec.rs`
冻结面零改动；`tasks.json` status 未改；未 `git commit`；政策文本不宣称
本轮未实现的保证（无审计/合规认证表述）。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/policy.rs` | **本任务核心新增模块**（1001 行，含 20 项测试） | — |
| — `EgressPolicy { network_opt_in, allow_http }` | 外发政策开关；`bool` 的 `false` 默认即闭合态（派生 `Default`）；`from_provider_config`（`semantic.*` → 政策）、`validate_endpoint`（https 恒可、http 须 `allow_http` 显式 opt-in、其余 scheme 拒启） | :70 / :77 / :89 / :98 |
| — `gate_transport_assembly(policy, endpoint, transport)` | 组合根装配门（"未 opt-in 不构造 transport"）：transport 存在而 `network_opt_in=false` → Config 错误；opt-in 而无 transport → Config 错误（半配置态不静默）；scheme 拒绝；通过则以 `GuardedTransport` 包装返回 | :130 |
| — `GuardedTransport` | 运行时政策守卫（ belt-and-suspenders）：每个请求在触内层 transport **之前**强制——scheme 准入 + 超时强制（零超时拒绝）；错误消息零 URL 零头部材料 | :173 / :184 |
| — `EgressDeclaration` + `embeddings_request` / `probe_request` | 声明外发面：embed 请求 body 恰好 `{model, input, encoding_format}`、头部恰好 `{Content-Type, Authorization, Accept}`；探针请求在 configurable 模式下额外声明 `dimensions`（`fixed` 模式不声明） | :207 / :221 / :241 |
| — `audit_egress(request, declaration, key, policy)` | 机制化外发审计：URL 匹配声明 + scheme 政策、超时非零、body 键集**恰好等于**声明（多余/缺失点名）、头部名集恰好等于声明、凭据材料只允许出现在 `Authorization` 头（body/其他头部出现即违规） | :261 |
| — `resolve_key_reference` / `resolve_key_reference_with` | 凭据引用解析器：仅 `env:NAME` / `file:PATH` 两形式（内联明文显式拒绝且不回显）；env 缺失/空、file 缺失/空 → Config 错误只点名引用名；注入式 env/file 访问器（公开函数委托 std 实现）；返回内存 `EmbeddingApiKey`，无 static/无缓存/无持久化（短生命周期口径） | :363 / :374 |
| — `redact_for_log` + `MIN_REDACTED_TOKEN_CHARS=8` | 日志脱敏：`Bearer <token>`（大小写不敏感、token ≥ 8 字符）替换为 `Bearer [REDACTED]`；普通行文（"the bearer of the token ring"）不误伤 | :443 / :479 |
| — tests（20 项） | 见 §2 | :482 起 |
| `crates/cc-semantic/src/providers/openai_compatible.rs` | 适配层政策接线（结构零重构） | — |
| — `OpenAiCompatibleConfig.egress: EgressPolicy` | 新字段，默认闭合态（`new()` = 无网络 + https-only） | :301 / :332 |
| — 构造器 egress 校验 | `validate_endpoint(endpoint)` expect（fail-fast，组合根惯例）；**transport 存在而未 opt-in → 构造即 panic**（"未 opt-in 不构造"在装配点强制）；存在的 transport 一律包 `GuardedTransport` | :370 / :377-380 |
| — `EmbeddingHttpTransport` 契约条款 | trait 文档新增「Transport contract (P7-007)」四条：① per-request 超时强制；② **重定向永不跟随**——底层客户端无法关闭 auto-follow 时至少必须在任何重定向跳前剥 `Authorization` 头并仍透出最终 3xx（重定向响应永不产出向量）；③ 不重试；④ 不记录请求头/体（凭据与用户输入） | :199 起 |
| — `build_request` 转 `pub` | 供组合根/政策层对真实外发请求跑 `audit_egress`（可见性放宽，逻辑零改动） | :427 |
| — 既有 redirect 测试强化 | 302 → 非重试 `InvalidInput`（原断言保留）+ 新增"seam 恰好收到 1 次请求"断言（绝不二次请求/重发认证） | :2403 起 |
| `crates/cc-semantic/src/lib.rs` | `pub mod policy;` + crate 文档 P7-007 段 | :125 / 头部 |
| `crates/cc-model/src/config.rs` | `semantic.*` 新增 2 键 + Default + 测试 | — |
| — `network_opt_in` / `allow_http` | serde 默认均 `false`（闭合态）；`network_opt_in` 文档明确"`enabled: true` 单独不足以放开网络" | :157 / :162 |
| — `semantic_egress_policy_keys_parse_and_default_closed` | 新测试：默认双 false + 显式解析 | :1520 起 |
| — `semantic_section_keys_are_known_config_keys` | unknown-key 面板补 2 新键 | :1546 起 |
| `crates/cc-semantic/src/capability.rs` | 探针外发面审计测试（1 项新增）+ 集成测试 config 字面量补 `egress` 字段（既有行为零改动） | :1170 起 |
| `crates/cc-semantic/src/admission.rs` | 既有集成测试 `OpenAiCompatibleConfig::new` + loopback http 端点 → 显式设 `egress = {network_opt_in: true, allow_http: true}`（测试双 opt-in，机制不受影响） | :1199 起 |
| `docs/CONFIGURATION.md` | 政策文本「代码外发与凭据政策（P7-007，执行机制腿）」+ `network_opt_in`/`allow_http` 两键文档（C14 义务） | :244 起 / 表尾两行 |

### 1.2 外发面清单（政策文本原文，`docs/CONFIGURATION.md:244` 节）

> 每一次 `/embeddings` 请求**只会**发送以下内容，`policy::audit_egress`
> 机制化审计请求体键集与头部集合"恰好等于声明面"（多余字段/缺失字段/
> 多余头部都判违规并点名）：
>
> 1. **input 文本 bytes**——批内文本（渲染后的文档分块或查询文本；它们
>    **可能包含源码片段**，这是嵌入调用的声明内容本身）；
> 2. **model 名**——冻结 `space.model_id()`；
> 3. **`encoding_format: "float"`**——唯一承认的编码格式；
> 4. **`dimensions` 参数**——**仅探针请求**、且仅 `dimensions_mode =
>    "configurable"` 时（适配器 embed 请求永不携带）。
>
> 头部恰好为 `Content-Type`、`Authorization`（凭据唯一通道）、`Accept`。
> 除上述声明外**零外发**：无遥测、无额外字段、无 tracing 日志。

### 1.3 机制分层（保证不依赖单点）

1. **组合根门**：`gate_transport_assembly`——未 opt-in 拒绝装配（点名
   `semantic.network_opt_in`）；opt-in 无 transport 拒绝；scheme 拒绝。
2. **构造器 fail-fast**：适配器构造期重复校验 scheme + opt-in（expect
   惯例，与 P7-001 端点断言同构）——政策违规在装配点爆，不懒爆 mid-batch。
3. **运行时守卫**：一切注入 transport 被 `GuardedTransport` 包装，每次
   请求前置校验 scheme + 超时（未来实现方契约违约时仍有兜底）。
4. **契约条款**：`EmbeddingHttpTransport` trait 文档四条绑定条款（超时/
   重定向不跟随+剥认证/不重试/不记录头体）。
5. **审计点**：`audit_egress` 对真实 `build_request` 产物逐字段核对声明面
   （政策级审计点——P7-001 请求形状测试的机制化升级）。

## 2. TDD 与测试清单（原样，全 ok）

红绿轮次（4 个语义红轮全部修复）：① `redact_for_log` 首版把
"the bearer of the token ring" 误伤 → 加 `MIN_REDACTED_TOKEN_CHARS=8`
最小 token 长度；② inline secret 的"不支持形式"错误原样回显了引用串
（即密钥本体）→ 改为只报 schema 前缀/声明内联值被拒，永不回显；③
`GuardedTransport` 首版拦截 3xx → `TransportError::Io` → 适配层映射成
**可重试** `ServerError`，破坏既有"3xx = 不可重试 InvalidInput"的更优
错误分类 → 撤守卫层 3xx 拦截，重定向拒收紧守在适配层门（非重试）+
trait 契约条款 + "恰一次请求"机制断言（守卫保留 scheme/超时两前置强制）；
④ http 审计测试先构造 provider 才审计，被构造器 fail-fast 先爆 → 改为
https provider 构造 + 换 URL 后对闭合/放开两种政策分别审计。

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test
-p cc-semantic --locked --offline policy::`（原样，20 项，全 ok）：

```
test policy::tests::adapter_constructor_refuses_transport_without_network_opt_in ... ok
test policy::tests::a_missing_body_field_fails_the_audit ... ok
test policy::tests::an_extra_body_field_fails_the_audit_and_names_the_field ... ok
test policy::tests::audit_rejects_a_zero_timeout ... ok
test policy::tests::audit_rejects_http_destinations_unless_http_is_opted_in ... ok
test policy::tests::credential_material_may_only_travel_in_the_authorization_header ... ok
test policy::tests::disabled_default_provider_still_constructs_and_fails_closed ... ok
test policy::tests::embed_request_egress_surface_is_exactly_the_declaration ... ok
test policy::tests::env_reference_resolves_from_the_lookup_and_debug_stays_redacted ... ok
test policy::tests::errors_that_embed_a_key_value_are_redacted_by_redact_for_log ... ok
test policy::tests::every_resolution_failure_names_the_reference_and_never_echoes_values ... ok
test policy::tests::file_reference_resolves_and_trims_trailing_newlines ... ok
test policy::tests::gate_rejects_http_endpoints_unless_allow_http_is_set ... ok
test policy::tests::gate_refuses_opted_in_but_missing_transport ... ok
test policy::tests::gate_refuses_transport_assembly_without_network_opt_in ... ok
test policy::tests::gate_with_default_policy_and_no_transport_is_the_normal_disabled_path ... ok
test policy::tests::guard_passes_https_2xx_through_unchanged ... ok
test policy::tests::guard_rejects_a_zero_timeout_before_the_inner_transport_is_touched ... ok
test policy::tests::guard_rejects_http_requests_before_the_inner_transport_is_touched ... ok
```

泄漏全路径扫描（`every_resolution_failure_names_the_reference_and_never_echoes_values`）
覆盖 7 条解析失败路径（env 缺失/env 空白/file 缺失/file 空/内联明文/空
引用/非法 env 名）× 2 个渲染面（`Display` + `Debug`）= 14 断言面：零
`sk-` 材料、且点名引用或配置键（可行动性）。

其它任务内新增测试：

```
capability: probe_request_bodies_are_exactly_the_declared_egress_surface
  （configurable 恰 {model,input,encoding_format,dimensions}；fixed 恰 {model,input,encoding_format}）
cc-model: semantic_egress_policy_keys_parse_and_default_closed
cc-model: semantic_section_keys_are_known_config_keys（补 2 新键）
openai_compatible: redirect_informational_and_unknown_statuses_follow_http_semantics
  （强化：3xx → InvalidInput + seam 恰 1 次请求断言）
```

## 3. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic -p cc-model --locked --offline
→ 17 × "test result: ok."，0 failed，零 FAILED。
  cc-model lib 85 passed（含 semantic_egress_policy_keys... 新项）；
  cc-semantic lib 197 passed（含 policy 20 项 + capability 探针审计新项，
  P7-001/002/004/006 既有项零回归）；集成套件 5/3/1/17/4/4/9/3/5/8/6/2/1
  全绿（retry_worker_layering 5、queue_worker 9、publish_cas 17 等零回归）。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.66s
  （零 error 零新增 warning）。

cargo clippy -p cc-semantic -p cc-model --locked --offline
→ 本轮新增代码零告警（policy.rs 首轮 1 条 Default 可 derive 已修——
  `bool` 的 false 默认即闭合态，直接派生）；余 5 条均为历轮遗留
  （admission.rs:291 loop-index、cache.rs:102/126/127、
  openai_compatible.rs:239 NormPolicy Default 可 derive），非本轮触碰，
  按约定不修。

零网络证据：全部 offline 锁定运行；cc-semantic/Cargo.toml 未改（仍无
HTTP crate、零新增依赖）；唯一 transport 为内存 mock；测试断言请求仅
抵达 mock 录制器。
```

## 4. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| step「显式opt-in/敏感文件/endpoint协议/redirect」 | opt-in 双键（`network_opt_in`/`allow_http`）+ 装配门/构造器/守卫三层强制；endpoint 协议 https 默认；redirect 不跟随（适配层门 + trait 契约条款 + 恰一次请求断言）。**敏感文件外发分类矩阵：blocked**（D2 未授权真实外发，未来授权后 P7-018 线补文本与配置面） |
| step「密钥只外部引用」 | `api_key_ref` 解析器（env:/file:，内联拒绝不回显）+ 全路径泄漏扫描 + Debug 脱敏 + 短生命周期口径（zero-on-drop 未实现，如实声明） |
| acceptance「默认无网络」 | 双键默认 false + `gate_transport_assembly` + 构造器 fail-fast + 适配器默认 disabled 语义不变（`disabled_default_provider_still_constructs_and_fails_closed` 固化） |
| acceptance「日志无key/源码」 | 适配层/政策层零日志调用（结构保证）+ `redact_for_log` 对抗样本脱敏 + 错误零回显（14 断言面扫描）+ P7-001 既有"错误零泄漏"测试不回归。"源码不进日志"由零日志 + 错误消息只载结构事实（长度/索引/状态码）承担，沿 P7-004 门层口径 |
| acceptance「重定向不泄露认证」 | 3xx 非重试拒绝 + seam 恰一次请求（认证绝不重发）+ 契约条款（auto-follow 不可关时必须跳前剥 `Authorization`） |
| deliverable「实现/配置或规格变更」 | 新 `policy.rs` 模块 + `semantic.network_opt_in`/`allow_http` 配置键 + 适配层政策接线 + 政策文本 |
| deliverable「artifacts/benchmarks/<run-id>/ V15/V18 证据」 | **not_run / blocked**：live 证据依赖真实 provider（D1/D2 不授权，与批次 1 同口径）；本轮工程级证据 = §2 测试 + §3 验证，benchmark run-id 未生成、tasks.json evidence 不回填 |

## 5. 政策文本位置

`docs/CONFIGURATION.md:244` 起「代码外发与凭据政策（P7-007，执行机制
腿）」：默认无网络口径、数据流向声明（§1.2 原文）、凭据机制（外部引用/
零落盘/短生命周期/zero-on-drop 未实现的如实声明）、传输安全机制
（https 默认/超时强制/重定向契约）、blocked 声明（live 归 P7-018、敏感
文件矩阵未开放、**不宣称审计/合规认证**）。`network_opt_in`/`allow_http`
两键入 `semantic` 节文档表（:240-241 行区）。

## 6. live 政策腿 blocked 声明（双轨口径）

- **机制腿（本轮 done）**：外发面审计、凭据解析与泄漏扫描、日志脱敏、
  https 强制、超时强制、重定向拒绝与契约条款、装配三层强制——全部经
  内存 mock offline 验证。
- **live 腿（conditional blocked，归 P7-018）**：生产 transport 的真实
  重定向/超时/TLS 行为核验、敏感文件外发分类矩阵、V15/V18 真实证据。
  解封时实现 `EmbeddingHttpTransport` + 组合根 `gate_transport_assembly`
  注入即可，本模块与冻结面零改动。

## 7. 偏差清单

1. **`GuardedTransport` 不拦截 3xx**（红轮修正）：守卫层把 3xx 转
   `TransportError::Io` 会被适配层映射为**可重试** `ServerError`，破坏
   "重定向 = 端点配置错误、非重试"的既有更优分类；重定向拒收紧守在
   适配层门（非重试 `InvalidInput`），守卫保留 scheme/超时两前置强制，
   实现方义务由 trait 契约条款② 绑定。守卫文档明示该分工。
2. **`redact_for_log` 设 8 字符最小 token 长度**：无长度下限会误伤普通
   行文（"the bearer of…"红轮实证）；真实 bearer token 远长于该阈值，
   低于 8 字符的短 token 不在脱敏范围（如实声明的能力边界，非保证）。
3. **适配器 `build_request` 由私有转 `pub`**：为让组合根/政策层可对真实
   请求跑 `audit_egress`（政策级审计点要求审计对象是装配产物而非测试
   内部）；纯可见性放宽，逻辑零改动。
4. **`OpenAiCompatibleConfig` 新增 `egress` 字段**：3 处既有测试构造点
   （openai_compatible fixture、capability 集成、admission loopback）
   机械补字段——其中 loopback http 测试点显式双 opt-in；既有断言零改动
   （197 项 lib 测试零回归为证）。
5. **zero-on-drop 未实现**：显式内存擦除需新增依赖（离线闭包内不可得），
   政策文本与模块文档如实声明为已知边界；当前保证=无 static/无缓存/
   无持久化 + Debug 固定脱敏 + 无 Display。
6. **「日志无源码」的承担方式**：本轮无日志调用（结构保证）+ 错误消息
   白名单只载结构事实（P7-004 门层既有口径，`message` 字段永不透出）；
   未新增独立的"源码片段扫描器"——对零日志架构属冗余机制。
7. **V15/V18 benchmark 证据未生成**：live 依赖 blocked，如实标
   not_run/blocked（§4）；未声称任何真实外发行为结论。
8. 红线确认：`ports.rs`/`spec.rs` 零改动；零真实网络；`tasks.json`
   status 未改；未 `git commit`；`cc-semantic` 零新增依赖。

## 8. 剩余风险 / 后续

- `gate_transport_assembly`/`resolve_key_reference` 尚无组合根调用方
  （cc-server 生产接线归接线轮）；当前全部经测试驱动，接入时 P7-002 的
  `resolve_provider` 链路之后串本模块即可。
- 未来真实 transport 落地时：契约条款②的"剥认证"需在该实现内自证
  （本轮 seam 层只能断言"不重发请求"，无法观测跟随行为）。
- 敏感文件外发分类矩阵、zero-on-drop、`redact_for_log` 的 bearer 之外
  凭据形态（如 `x-api-key` 头）——均随 P7-018 live 授权一并收口。
