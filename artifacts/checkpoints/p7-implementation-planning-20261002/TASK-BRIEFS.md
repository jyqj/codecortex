# P7-001~020 逐任务设计简报

> 每节含：目标与约束、模块归属、接口草案、验收对照、风险。
> 所有签名均为**草案**，实施时按现有代码风格评审（不猜未实现 API：现有代码引用均带
> 文件:行号；新模块的接口是设计草案，不是既有事实）。
> 通用约定：错误走 `cc_model::CcError`；provider 调用不持 DB 锁/连接
> （C11，`02-CONTRACTS.md:87-91`）；D1+D2 口径 = 本轮 local/fake 完整、live blocked
> （`DECISIONS-RECORDED.json` decisions[1]）。六项深化任务：P7-001/004/006/009/012/013。

---

## P7-001 OpenAI-compatible provider 适配（深化；本轮 fake-only）

**目标与约束**：为未来真实 provider 建立已就绪适配层（D1 拍板"未来授权后走 P7
已就绪的适配层，无需重构"），本轮不做任何真实网络调用与计费。协议映射目标已由
P6 冻结：`ProviderError` 六变体（`crates/cc-semantic/src/ports.rs:81-92`，
`RateLimited{retry_after}/ServerError/AuthError/Timeout/Cancelled/InvalidInput`），
适配层只做 HTTP 语义 → 该分类的映射，不发明新错误类型。

**模块归属**：新 `crates/cc-semantic/src/providers/openai_compatible.rs`；
`ports.rs` 仅增量类型。硬约束：cc-semantic 依赖地板是 cc-model+cc-db
（`crates/cc-semantic/src/lib.rs:66-67`），网络客户端 crate **不能**进 cc-semantic
默认依赖。本规划取 **transport seam** 方案：

```rust
// crates/cc-semantic/src/providers/openai_compatible.rs（草案）
/// 同步传输缝：由组合根（cc-server，已可选依赖 cc-semantic）注入实现，
/// 或测试注入 in-process stub。cc-semantic 自身零网络依赖。
pub trait EmbeddingTransport: Send + Sync {
    fn post_json(&self, url: &str, headers: &[(String, String)], body: &[u8])
        -> Result<TransportReply, TransportError>;
}
pub struct TransportReply { pub status: u16, pub retry_after_ms: Option<u64>, pub body: Vec<u8> }
pub struct OpenAiCompatibleConfig {
    pub space: VectorSpace,          // 复用冻结 spec（spec.rs:95），model_id/dimension 进身份
    pub endpoint: String,            // base path，如 https://HOST/v1/embeddings
    pub api_key_ref: String,         // 只存外部引用名（env var），不存明文（P7-007 联动）
    pub timeout: Duration,
    pub max_batch_items: usize,      // 与 admission（P7-003）对齐；trait 约定调用方已切批（ports.rs:94-95）
}
pub struct OpenAiCompatibleProvider { /* 实现 EmbeddingProvider（ports.rs:99） */ }
```

`EmbeddingProvider` 三方法签名不变（`space`/`embed_documents`/`embed_queries`，
ports.rs:99-106）；trait 是同步的（P6 冻结决定），意味着阻塞 I/O 运行在 worker
drain 的调用方线程——线程模型裁决见 `OPEN-QUESTIONS.md` Q4。

**验收对照**：acceptance"协议 stub 覆盖成功/错误，provider 可替换"→ 用 in-process
stub `EmbeddingTransport` 覆盖 2xx 成功、429+Retry-After、5xx、401/403、超时、
畸形 body 六类映射为六变体；可替换性由既有证据背书：`EchoProvider`
（ports.rs:113）、`FakeProvider`（fake.rs:118）、object-safety 测试（ports.rs:203）。
V15 证据只采 local leg；live leg 显式 blocked（06-VALIDATION.md:59）。

**风险**：① Q1 未裁决前 HTTP crate 无法选定（推荐 reqwest blocking 或 ureq，
落 cc-server 侧，须过 G6 式"默认依赖图零网络边"检查的对应新检查——`semantic`
feature 构建树新增网络 crate 需在 P7-GATE 里如实声明，不能沿用"闭包零网络客户端
crate"结论）；② 冻结面 ports.rs 若确需改 trait，触发重冻结；③ 伪成功风险：
本轮不得出现"实现了适配层 = 已验证真实 provider"的表述。

---

## P7-002 模型参数与能力验证

**目标与约束**：模型 revision、dimensions、metric、query instruction 显式校验，
"支持差异不能吞"。校验原语大半已冻结：`VectorSpace::validate`
（`crates/cc-semantic/src/spec.rs:118`）、`digest`（spec.rs:167）、
`DocumentEncodingSpec/QueryEncodingSpec`（spec.rs:181/:195，digest spec.rs:269/:319）、
`DistanceMetric`（spec.rs:81）。

**模块归属**：`spec.rs`（校验深化）+ `crates/cc-model/src/config.rs`
（首个 provider 配置节；现文件仅 `ProjectConfig{ query: QueryConfig }`
（config.rs:7-13），无任何语义键——CONFIGURATION.md:207-208 明示
`.codecortex.json` 没有语义键，本任务开第一组键）。

**接口草案**：

```rust
// cc-model/src/config.rs（草案；键名待 OPEN-QUESTIONS Q6 裁决）
pub struct SemanticProviderConfig {
    pub enabled: bool,               // 默认 false（C14：默认无网络）
    pub model_id: String,            // 构造 VectorSpace（spec.rs:105）的材料
    pub dimensions: u32,
    pub metric: DistanceMetric,      // 显式，不允许默认漂移
    pub endpoint: String,
    pub api_key_ref: Option<String>, // 外部引用（P7-007）
}
```

**验收对照**：acceptance"供应商不支持 dimensions 时给错误/配置路径，不伪成功"
→ 显式 dimensions 参数走请求体（OpenAI 兼容协议的 `dimensions` 字段），stub 返回
不支持时必须落到类型化错误或配置拒绝路径；query instruction 变化体现在
`QueryEncodingSpec` digest（spec.rs:319），为 P7-009 失效键打底。V15/V18。

**风险**：配置键定名触发 C14 迁移诊断义务；`enabled=false` 时其余字段必须是
"可解释但不生效"（C14"feature 关闭仍能解释 config 状态而非崩溃"）。

---

## P7-003 真实输入尺寸与批次规划

**目标与约束**：按**最终输入**（渲染后的确切 bytes）计 token/bytes/batch；超限
重切或明确 skip，不平均池化掩盖。P6 已立约："the caller guarantees batch size
(P6-013 admission), implementations do not split batches"（ports.rs:94-95）——
本任务把 caller 侧保证正式化为 admission 模块。

**模块归属**：新 `crates/cc-semantic/src/admission.rs`；
`crates/cc-index/src/documents/render.rs`（消费侧最小触碰：暴露已渲染 bytes 的
清单接口，不重写渲染）。

**接口草案**：

```rust
// crates/cc-semantic/src/admission.rs（草案）
pub struct InputBudget { pub max_items: usize, pub max_bytes: usize, pub max_tokens: usize }
pub enum PlannedInput { Batch(BatchPlan), Skipped { doc_key: String, reason: OversizeReason } }
pub fn plan_document_batches(rendered: &[(impl DocKey, Vec<u8>)], budget: &InputBudget,
    tokenizer: &str /* spec.rs:263 已有 tokenizer 声明 */) -> CcResult<Vec<PlannedInput>>;
```

**验收对照**：acceptance"每项和总 batch 受限，文本与向量所代表文档版本一致"→
单项超限产 `Skipped`（带原因，进 coverage 分母的显式失败计数，不静默）；批间
输入 digest 与 `DocumentInput::from_bytes`（ports.rs:38）构造一致，digest↔bytes
绑定不可漂移（ports.rs:46 `verify` 已可复核）。V09（实际 bytes 与预算）/V15。

**风险**：tokenizer 估计与真实计费 token 有偏差——收据层（P7-008）必须区分
reported/estimated，本任务只保证 bytes 上限是硬的。

---

## P7-004 响应强校验（深化）

**目标与约束**："数量/index 完整且唯一、dim、finite、norm 检查；错误向量不缓存"。
P6 已把校验原则冻结进错误分类：`InvalidInput`"Rejected here so bad vectors never
reach the cache"（ports.rs:89-91）；FakeProvider 的 zero-vector 注入
（fake.rs:94 注释）就是本任务的既有测试面。本任务把校验从 fake 内部约定升格为
**适配层输出门**，并处理 OpenAI 兼容响应的乱序/重复 index。

**模块归属**：`providers/openai_compatible.rs`（输出门函数）；复用
`cache.rs:192` 的 `ValidatedVector`（校验通过的载体，先于 cache.rs:340 `put`）。

**接口草案**：

```rust
// openai_compatible.rs（草案）
pub struct RawEmbeddingReply { pub data: Vec<(usize /*index*/, Vec<f32>)>, pub usage: Option<Usage> }
/// 输出门：批长==输入长、index 集合恰为 0..n 且唯一（乱序按 index 重排恢复）、
/// dim==space.dimension（spec.rs:151）、全部 finite（NaN/Inf 拒绝）、zero 向量拒绝、
/// norm 策略可配（记录 or 区间拒绝）。任一失败 → ProviderError::InvalidInput（整体拒绝）。
pub fn validate_reply(batch_len: usize, space: &VectorSpace, reply: RawEmbeddingReply,
    norm: NormPolicy) -> Result<Vec<ValidatedVector>, ProviderError>;
```

**验收对照**：acceptance"乱序可正确恢复，重复 index/NaN/zero 必被拒绝"→
V16 oracle 面：手算参照（exact.rs 的 oracle 惯例）+ fake.rs 故障注入的 zero 变体
直连输出门；"错误向量不缓存"→ 校验失败路径上不存在任何 `ArtifactCache::put`
（cache.rs:340）与 publish 调用（测试断言调用计数）。V15/V16。

**风险**：整体拒绝 vs 部分接受的取舍——本规划取**整体拒绝**（一坏俱拒，
批级重试），与"不缓存错误向量"和费用封顶（坏批不值得部分付费入缓存）一致；
若真实供应商普遍部分成功，P7-018 授权后需重审（记入 brief 风险，不预先实现）。

---

## P7-005 全局与项目并发限流

**目标与约束**："共享 provider 级限额和项目公平队列；避免每调用创建独立无限
信号量"。provider 限流器必须是组合根单例持有的共享对象（service_factory）。

**模块归属**：`admission.rs`（限流原语）+ `crates/cc-server/src/service_factory.rs`
（单例装配）；**消纳接线待办 8**（worker 公平化 ORDER BY 注入，claim 面在
`crates/cc-db/src/semantic_outbox.rs:496` `claim_next_on`）。

**接口草案**：

```rust
// admission.rs（草案）
pub struct ProviderGate { /* 进程内 semaphore + per-project 公平轮转队列 */ }
impl ProviderGate {
    pub fn new(max_concurrent: usize) -> CcResult<Self>;
    pub fn acquire_permit(&self, project: &ProjectIdentity) -> ProviderPermit; // RAII
}
```

**验收对照**：acceptance"多项目总并发仍受限，单项目不能饿死其他索引"→
FakeProvider latency 注入（fake.rs:91 per-call latency）构造慢场景，多项目并发
计数断言 ≤ max 且每项目都有推进；ORDER BY 公平化用 outbox 行断言轮转顺序。
V15/V20。接线待办 8 出账在本任务收口记录。

**风险**：permit 跨 await/线程的持有模型受 Q4（同步 trait + worker 线程模型）
影响；限流等待不得发生在持锁区间（C11）。

---

## P7-006 有界重试与断路器（深化）

**目标与约束**："429/5xx/timeout 退避与 Retry-After；auth/永久错误暂停；
重试次数、deadline 和费用封顶，失败原因公开脱敏"。分类已冻结
（ports.rs:81-92），`RateLimited{retry_after}` 携带时长——重试策略是分类的
消费者，不需要改端口。

**模块归属**：`providers/openai_compatible.rs`（`RetryingProvider` 装饰器）+
新 `worker.rs`（编排：围绕 P6 的显式 drain 原语，lib.rs:26-31"explicit drain,
no resident thread"红线不变；worker.rs 是 `EmbedHandler`（queue.rs:335）外层的
重试分类/断路/收据编排封装，**不是常驻线程**）。断路器状态由组合根单例持有
（与 P7-005 `ProviderGate` 同点装配，共享生命周期）。

**接口草案**：

```rust
// openai_compatible.rs（草案）
pub struct RetryPolicy {
    pub max_attempts: u32, pub base_backoff: Duration, pub max_backoff: Duration,
    pub respect_retry_after: bool, pub total_deadline: Duration,   // 与 QueryControl::limit_total 同型（query.rs:102）
    pub max_cost_units: Option<u64>,                                // 联动 P7-008 收据
}
pub struct RetryingProvider { inner: dyn EmbeddingProvider, policy: RetryPolicy, breaker: Arc<CircuitBreaker> }
enum CircuitState { Closed, Open { until: Instant }, HalfOpen }
// 可重试集合：RateLimited（退避=retry_after）、ServerError、Timeout、Cancelled 不重试直接透传；
// AuthError → 断路器 Open{长窗}+worker 暂停新 claim（degraded 透出，复用 degrade.rs:412/:435 门面）。
```

**验收对照**：acceptance"重试次数、deadline 和费用封顶，失败原因公开脱敏"→
fake.rs:87-88 `fail_after_n_calls`+`fail_with` 脚本化故障直测重试计数与退避序列；
deadline 到期未完成的 claim 走 fenced retry（queue.rs `BatchReport` 记账，
queue.rs:205）而非无限占用；`last_error` 脱敏断言（无 key、无源码片段——与
TROUBLESHOOTING 既有 failed 死信口径衔接）。V15。

**风险**：① 断路器"暂停"与 outbox lease 的交互：暂停必须同时停 renew 或确保
lease 过期可回收（reclaim_expired_on，semantic_outbox.rs:653），否则 auth 故障
期间 lease 假活；② total_deadline 与查询子预算的关系——worker 重试预算与查询
deadline（P7-013）是两套预算，简报明确：worker 侧预算约束**回填**路径，查询
路径走 QueryControl（Q4 裁决后统一）。

---

## P7-007 代码外发与凭据政策（只做执行机制腿）

**目标与约束**（D1+D2 拍板，tasks.json:9545 块 implementation_notes）：本轮
local 完整 + live blocked。执行机制腿全做：默认无网络（未显式 opt-in 不构造
transport/P7-001）、密钥只外部引用（`api_key_ref` 存引用名，解析发生在调用点，
日志与错误路径零明文）、重定向不泄露认证（transport 实现侧：跨 host 重定向前
剥 `Authorization`——stub 传输测试覆盖）。政策文本按"默认无网络 + 显式 opt-in"
收口；敏感文件外发分类矩阵标 blocked（D2 未授权，未来授权后补文本与配置面）。

**模块归属**：新 `crates/cc-semantic/src/policy.rs`（机制：redaction 工具 +
opt-in 门）+ `crates/cc-model/src/config.rs`（opt-in 键，与 P7-002 配置节合并）。

**接口草案**：

```rust
// policy.rs（草案）
pub fn resolve_key_reference(api_key_ref: &str) -> CcResult<String>; // 只从外部环境解析
pub fn redact_for_log(input: &str) -> String;                        // key/Authorization/源码片段脱敏
pub struct EgressPolicy { pub network_opt_in: bool }                 // 默认 false；false 时 transport 构造直接 Err
```

**验收对照**：acceptance"默认无网络、日志无 key/源码、重定向不泄露认证"→
三断言各自独立测试：默认配置下 transport 构造失败且无任何连接尝试（stub 计数
为 0）；redact 单测（含错误消息注入 key 的对抗样本）；重定向跨 host 断言
Authorization 头已剥。V15/V18（local leg）。

**风险**：政策文本与机制的边界若混写，会造成"文档宣称已支持外发审批"的
超范围声明——文本只写默认无网络 + opt-in 语义，审批矩阵显式标 blocked。

---

## P7-008 费用与不确定尝试收据

**目标与约束**："区分 reported/estimated tokens、cache reuse 和未知重复费用；
停机阈值；无 usage 不填 0 费用，重启重试能看见费用不确定性"。先例：
DegradationLedger 的"预算在调用前拒绝"模式（degrade.rs:223 `new(max_reembeds)`、
degrade.rs:274 `admit`、re-embed budget exhausted 死信口径）直接复用为费用
停机阈值的实现模式。

**模块归属**：`admission.rs`（收据结构 + 停机阈值）+ `worker.rs`（挂接点）。

**接口草案**：

```rust
// admission.rs（草案）
pub struct UsageReceipt { pub reported: Option<Tokens>, pub estimated: Option<Tokens>, pub cache_reuse: bool }
// 不变式：reported 与 estimated 都缺 → 费用列记 unknown，绝不填 0；
// cache_reuse=true 的命中不计新费用；attempt>1 且 usage 缺失 → 标 unknown_duplicate_risk。
pub struct CostBudget { pub max_units: Option<u64> } // 触顶 → 调用前拒绝（对齐 degrade 先例），死信带原因
```

**验收对照**：acceptance 两条直接映射（无 usage 不填 0；重启重试的费用不确定性
可见——attempt 字段 + usage 缺失组合断言）。V15/V20。

**风险**：收据持久化位置——P6 只落 outbox 状态与 last_error；新收据若需跨进程
持久化需动 cc-db（Auxiliary 计数先例，round12 待办 7 同模式），本轮先取
outbox 行内 JSON 字段方案，落库扩展列入 P7-016 后的收口可选项。

---

## P7-009 查询编码与缓存（深化）

**目标与约束**："实现 QueryEncodingSpec 键和有界 query cache；instruction 变更
失效；不需无谓重嵌文档，不跨空间复用 query 向量"。键的材料全部已冻结：
`QueryEncodingSpec::digest → QuerySpecDigest`（spec.rs:319）、`QueryDigest`
（ports.rs:27-32，构造即绑定 bytes，ports.rs:60-65）、`namespace_key`
（cache.rs:96）。缓存依赖矩阵的 dense 行要求含 semantic epoch / vector-space /
query encoding spec（C12 表，02-CONTRACTS.md:93-107）。

**模块归属**：`spec.rs`（键类型）+ `cache.rs`（有界 query 缓存；与文档 artifact
cache 同根不同区，复用 namespace 隔离与损坏处理惯例）。

**接口草案**：

```rust
// cache.rs（草案）
pub struct QueryVectorCache { /* LRU：容量 + bytes 双上限（有界硬要求） */ }
impl QueryVectorCache {
    pub fn get(&self, key: &QueryCacheKey) -> Option<ValidatedVector>;
    pub fn put(&self, key: QueryCacheKey, v: ValidatedVector);
}
pub struct QueryCacheKey {
    pub namespace: String,     // namespace_key(project_identity)（cache.rs:96）
    pub spec: QuerySpecDigest, // 覆盖 instruction/max_tokens/tokenizer/space（spec.rs:319）→ instruction 变更天然失效
    pub query: QueryDigest,    // ports.rs:30
    pub semantic_epoch: u64,   // C12 dense 行要求；保守口径：回填推进即失效（风险②）
}
```

**验收对照**：acceptance"不需无谓重嵌文档"（文档路径不经过本缓存——文档
artifact cache P6-008 已有，本任务不触碰其键，回归断言文档缓存命中率不变）；
"不跨空间复用 query 向量"（`VectorSpace` 进 spec digest，spec.rs:95-118，跨
space 的 key 必不同——测试断言）。V11（缓存 key 完整）/V15。

**风险**：① semantic_epoch 入键使每次回填推进清空 query 缓存——保守正确但
浪费；备选是不入键、只靠 spec digest（query 向量与文档集合无关）——两者都
满足"不跨空间"，取舍影响 V11 证据口径，**本规划默认按 C12 表保守入键**，
列为 `OPEN-QUESTIONS.md` Q5 请 owner 裁决；② 内存上限的绝对值需配置化
（P7-002 配置节扩展），防大项目常驻。

---

## P7-010 dense 召回端口接线

**目标与约束**："将语义服务注入 SemanticRecall，返回 doc 版本/空间/coverage；
搜索不依赖具体 HTTP 客户端，local 策略不调用端口"。接收端已全部就绪：
`SemanticRecall` 端口（`crates/cc-model/src/semantic.rs:25`，async + QueryControl）、
receipt 校验适配器（`crates/cc-search/src/lanes/semantic_adapter.rs:11` `recall`，
已做 panic 捕获/取消透传/超时/容量分类/weight 归一 semantic_adapter.rs:24-45）、
注入槽（service_factory.rs:84、engine.rs:310）、语义候选入账
（`crates/cc-search/src/lanes.rs:263` `append_semantic_outcome`，已校验
semantic_top_k 与身份/跨度/hard scope，lanes.rs:277-302）。

**模块归属**：`lanes/semantic_adapter.rs`（补生产 recall 实现：后端 =
P6 的 `ExactSearch`/`search`，`crates/cc-semantic/src/vector/exact.rs:123/:136`，
query 向量来自 P7-009 缓存）+ `service_factory.rs`（**最小装配腿**：接线待办
1 部分/4/5——构造子系统实例、cache 根与项目身份传入、DegradationLedger
snapshot→set_semantic_degradation 一行桥接；完整 try_init 归 P7-014）。

**接口草案**：

```rust
// service_factory.rs / 新 server 侧 semantic 模块（草案）
// feature = ["semantic"] 时：
let cache = ArtifactCache::open(resolve_cache_root_with(env)?, namespace_key(project_identity)?); // cache.rs:229/:143/:96
let recall = ExactRecallService { cache, query_cache, exact: space_manifest_reads(..)? };        // exact.rs:95
services.set_semantic(Some(Arc::new(recall)));                                                   // service_factory.rs:84
services.set_semantic_degradation(ledger.snapshot().into());                                     // 待办 4 桥接
```

**验收对照**：acceptance"搜索不依赖具体 HTTP 客户端"（`SemanticRecall` 实现闭包
零传输类型；cargo tree 断言 search 树无网络 crate）+"local 策略不调用端口"
（engine.rs:206/:224 已有 `request.semantic.is_none()` 缓存排除先例，补 local
策略短路断言）；"返回 doc 版本/空间/coverage"（`LaneOutcome`/`CandidateRef`
已带 doc_version——fusion.rs:53-56 版本一致性检查可证）。V11/V16。

**风险**：① 本任务是组合根第一次接生产语义路径，P6 的 `semantic_state:
"port_attached_unverified"`（capability_status.rs:27）字符串在 P7-014 前仍会
出现——诚实口径：010 后状态为 attached+可查询，状态机全量化归 014；②
ExactSearch 后端在小规模是 oracle 级正确但 O(N) 扫描——规模边界要在 status
diagnostics 如实透出，不在本任务引入 ANN（V22 可选轨，P6-010 已定 Cosine-only
冻结 spec v1）。

---

## P7-011 dense 范围与 hydrate 守卫

**目标与约束**："过滤在 topk 前且最终二次检验 manifest/source；删除和 scope
测试；semantic 找回结果也不会被 softscope 误删或越过 hard 范围"。P6 已交付
filter-before-top-k（lib.rs:18-21"filter-before-top-k, bounded batch loads and
the `(score desc, doc_key asc)` total order"）；本任务是**搜索侧守卫闭环**。

**模块归属**：`vector/exact.rs`（范围过滤断言强化）+
`crates/cc-search/src/evidence.rs`（hydrate 二次校验：`SourceVerifier`
（evidence.rs:105）的 `path_current`（:124）/`hit_current`（:143）复用）。
**消纳接线待办 12**（dense lane 对覆盖率的消费）。

**接口草案**：无新签名——组合既有件：

```text
semantic lane 候选 → lanes.rs:263 append_semantic_outcome（hard scope/身份/跨度校验已有）
                  → SourceVerifier::hit_current 二次校验（manifest/source 时效）
                  → soft scope 只作预选约束，绝不删除已过 hard scope 校验的 semantic 命中
```

**验收对照**：acceptance 两句各自成测试：删除文档后 semantic 候选被
`hit_current` 拒（V05"删除复活"硬失败项，06-VALIDATION.md §4）；hard scope 外
候选在 adapter/lanes 两层都拒（semantic_adapter.rs 与 lanes.rs:302 双保险）；
soft scope 不吞 semantic 命中（对照 P1 的 soft 预选语义，V05"soft 预选外命中"）。
V05/V16。

**风险**：二次校验增加每候选 DB 读——`SourceVerifier` 已带 diagnostics
（evidence.rs:183）与批次惯例，控制在校验批次内；守卫过严会把合法命中打成
partial，coverage 口径要与 P7-012 统一。

---

## P7-012 融合与部分覆盖语义（深化）

**目标与约束**："独立 dense rank RRF，partial/unavailable 透出；不混
cosine/BM25；timeout 与无命中可区分，declared full coverage 有证据"。RRF 的
rank-only 原则已冻结在代码里：`fuse_outcomes` 只消费 lane rank 与配置权重，
"Raw BM25/grep/path scores are diagnostics and never enter this arithmetic"
（`crates/cc-search/src/fusion.rs:13-18`，入口 fusion.rs:24）；可融合状态仅
`Complete|Partial`（`crates/cc-model/src/retrieval.rs:213` `is_fusable`）。
语义 lane 的状态分类已建：Timeout→`"semantic_deadline"`、Unavailable→
`"semantic_capacity"`、Error→`"semantic_read_error"`
（`crates/cc-search/src/lanes/semantic_adapter.rs:31-34`）。

**模块归属**：`fusion.rs`（dense 票权接入：`append_semantic_outcome` 已在
engine.rs:289 调用，本任务保证 fused 路径消费 semantic 的 `by_lane` 权重与
tie-break 固定）+ `crates/cc-model/src/context.rs`（coverage explain 字段，
先例是 `SearchHardScopeExplain.semantics` 字符串字段，context.rs:33）。

**接口草案**：

```rust
// context.rs（草案）
pub struct LaneCoverageExplain {
    pub lane_id: String,
    pub status: LaneStatus,          // retrieval.rs:202 八态原样透出，不折叠
    pub candidate_count: usize,
    pub truncation_reason: Option<String>, // timeout("semantic_deadline") 与 Complete+0 可区分的载体
    pub coverage: LaneCoverage,      // retrieval.rs:222；not_run() retrieval.rs:229
}
```

**验收对照**：acceptance"timeout 与无命中可区分"→ 两场景对照断言：
`LaneStatus::Timeout + truncation_reason=Some("semantic_deadline")` vs
`Complete + candidate_count=0`（CONFIGURATION.md:109 既有口径"complete 且
candidate_count=0 表示已执行但无命中，不等于 disabled"）；"declared full
coverage 有证据"→ full 声明必须能回指 lane receipt/SourceVerifier 诊断
（V19"declared full coverage有证据"）；"不混cosine/BM25"→ fusion 算术
单测：改 raw score 不变 fused 序（fusion.rs 既有原则的反向验证）。V11/V19。

**风险**：① dense 权重进 policy fingerprint（C10 要求，02-CONTRACTS.md:81-85）
——fingerprint 变化会翻缓存键，随 C12 最终上下文缓存一起失效，属预期但要写进
变更记录；② `LaneCoverage` 语义与 semantic epoch 的关系已在 P6-012 立约
（可见集合变化才推进），本任务只透出不重算。

---

## P7-013 查询总 deadline 和模型故障退化（深化）

**目标与约束**："fake/HTTP 慢请求测取消；auto 回本地、explicit semantic 明确
不足；网络不占读写锁，故障结果不缓存成完整成功"。预算原语已冻结：
`QueryControl` 不可变绝对 deadline（`crates/cc-model/src/query.rs:71-77`）、
`child` 子预算不可超过父（query.rs:91-99）、`limit_total`（query.rs:102）；
执行面 `ExecutionPool`（`crates/cc-search/src/execution.rs:27`，`run_async`
承载语义 future，semantic_adapter.rs:25-29）；取消分类已建
（`QueryCancelled` 直透、`QueryTimedOut`→Timeout、`QueryBusy`→Unavailable，
semantic_adapter.rs:29-34）。默认 30s（query.rs:38 `deadline_ms: 30_000`）。

**模块归属**：`crates/cc-search/src/execution.rs`（语义子预算策略）+
`crates/cc-server/src/handlers/context.rs`（策略→QueryControl 装配点）。

**接口草案**：

```rust
// execution.rs（草案）
pub fn semantic_child_budget(control: &QueryControl, policy: &QueryPolicy) -> QueryControl {
    control.child(Duration::from_millis(policy.semantic_deadline_ms)) // child 内建 min(父) 钳制（query.rs:91-95）
}
```

策略语义（落地 tools.rs:295-297 既有声明："auto without a semantic port stays
local. An unconfigured explicit semantic request fails instead of pretending
ready"）：

```text
auto   : 语义失败/超时 → 静默回 local 结果 + coverage 透出 Timeout/Unavailable（不报错给用户）
local  : 不构造语义子预算，不调端口（P7-010 短路）
semantic: 失败/超时 → 明确不足（错误或显式 unavailable 状态），绝不回退 local 伪装完成
```

**验收对照**：acceptance"网络不占读写锁"→ 断言语义 lane 执行区间内无
RwLock guard 存活（QueryHandle Arc-owned 短锁内取得后释放——C11 全文，
02-CONTRACTS.md:87-91）；"故障结果不缓存成完整成功"→ 复用 engine.rs:206/:224
的 `request.semantic.is_none()` 缓存排除 + C12"降级结果不进普通完整结果缓存"
（02-CONTRACTS.md:93-107），补语义故障场景测试；慢请求取消用 FakeProvider
latency 注入（fake.rs:91）+ stub transport 慢响应双路。V11/V15。

**风险**：① 同步 `EmbeddingProvider`（ports.rs:99）在查询路径不可直接用——
查询路径走 `SemanticRecall`（async）+ ExactSearch 本地计算，provider 只在
worker 回填侧出现；若 P7-009 miss 发生在查询路径（query 向量未缓存），必须在
**本任务裁决查询内联编码是否允许**（默认：不允许，查询路径只消费缓存，
miss → 该查询的 dense lane 记 Unavailable，Q4/Q7 关联）；② auto 回退不得
吞错误——coverage/诊断必须保留故障痕迹（与 P7-012 的 truncation_reason 衔接）。

---

## P7-014 配置/status/MCP 全链贯通

**目标与约束**："新字段 schema/sanitize/handler/doc/E2E 一体；原 mode 语义
不变；未配置、关闭、回填、失败、就绪状态真实一致"。C14 全文适用
（02-CONTRACTS.md:114-118：新 retrieval_strategy 字段需 schema/sanitize/
dispatch/handler/status/文档/stdio E2E 一次闭环；MCP_TOOLS.md:20 已声明
strategy 语义与 symbol 模式限制）。`dense_state` 现为硬编码
`"disabled","dense_reason":"provider_and_vector_publication_not_implemented"`
（capability_status.rs:22）——本任务换成真实状态机。

**模块归属**：`crates/cc-server/src/tools.rs`（schema/sanitize/dispatch，先例
`validate_enum` tools.rs:102 与 strategy 解析 tools.rs:16）+
`capability_status.rs`（状态机）+ `docs/MCP_TOOLS.md` + E2E。
**消纳接线待办**：1（完整 try_init，配置键驱动）/2（四编排调度点）/3/6/7/10/13。

**接口草案**（capability_status 状态机，草案）：

```text
semantic_state: not_configured | disabled | degraded（capability_status.rs:100 既有消费不变）
              | ready | backfilling { pending: N } | failed { reason }
dense_state:   disabled | partial { published, desired } | ready（替换 capability_status.rs:22 硬编码）
```

**验收对照**：acceptance 五态各一条 E2E：未配置（零 key 启动 = 现状回归）、
关闭（enabled=false 运行时关闭，回滚口径见 IMPLEMENTATION-ORDER 第 6 节）、
回填（outbox pending>0）、失败（degraded/dead-letter 可见）、就绪；14 工具
不丢、`mode=symbol` 只收 local（MCP_TOOLS.md:20）。V18。

**风险**：① 状态字段的 wire 兼容——新枚举值对旧客户端是未知值，sanitize 侧
要保证"旧客户端可解析子集"；② 调度点（待办 2）触发时机是行为面：索引完成 /
查询前 opportunistic / 空闲，选择影响 V17/V20——简报取"显式 + 索引事务后
一次性"最小集，机会性触发仅 reclaim 收窄后（待办 9）才放开，否则默认不开。

---

## P7-015 后台回填与前台查询竞争测试

**目标与约束**："partial backfill 时查询/写入/删除/切模型；测 queue 和 CPU/DB
占用；慢模型不会饿死 local 索引/查询，过期发布 0"。资源归因已有底座：
`Resources` 进程 RSS 快照（`crates/cc-eval/src/benchmark/sampler.rs:17`，
runner/server/tree 分列）与 `retrieval_work` 收据提取（sampler.rs:28）。

**模块归属**：新 `crates/cc-eval/tests/semantic_lifecycle.rs` + `sampler.rs`
（扩展采集点）+ 被测面即 P7-014 装配的调度点。
**消纳接线待办 11**（有界 desired 投影回接 reconcile，reconcile.rs:152）。

**接口草案**：无新生产 API；测试矩阵：

```text
场景 × { partial backfill 慢模型（FakeProviderConfig.latency，fake.rs:91） }
并发腿：查询 / 写入（触发 outbox）/ 删除 / 切模型（space_switch.rs:84 activate_space）
断言：local 查询 P99 不退化超阈值；outbox drain 占用有界；过期（旧 space）发布对查询可见数=0；
      Resources 三列（runner/server/tree）归因闭合
```

**验收对照**：acceptance 逐条映射（V14/V17/V20）；"过期发布 0"复用 P6 五重
fencing/visible-set 语义的集成断言。

**风险**：并发测试 flake 是历史已知风险类（P5 benchmark_fixture flake 先例，
execution_note）——所有时序断言带余量与重放 seed，环境性失败按既有
documented_environment_flake 判例处理，不静默重跑改判定。

---

## P7-016 fake 全故障矩阵回归

**目标与约束**："组合重试/close/rebuild/delete/lease/GC；每条保留可重放 seed；
依赖独立实验证明恢复，不把 fake 分数解释成语义效果"。P6 留边界："带可重放
seed 的全故障矩阵正式回归与真实子进程 SIGKILL 级正式化归属 P7-016"（tasks.json:45
execution_note round13 段）。故障源已 seed 化：`FakeProviderConfig`
（fake.rs:78，fail_after_n_calls/fail_with/latency/zero-vector）。

**模块归属**：`crates/cc-eval/tests/semantic_lifecycle.rs`（扩展）+ 新
`crates/cc-eval/tests/mcp_v2_contract.rs`。
**消纳接线待办 9**（机会性 reclaim 收窄，semantic_outbox.rs:653）。

**接口草案**：每用例 = `{ seed, 故障脚本, 操作序列, 恢复断言 }`，组合维度：
重试（P7-006）× 进程 close/restart × rebuild（reconcile.rs:152）× 删除竞争 ×
lease 过期（reclaim）× GC（gc.rs:531）；断言复用 P6 验收的不变式（预算不
复活死信、缓存命中零新调用、epoch 不被辅助写推进）。

**验收对照**：acceptance"依赖独立实验证明恢复，不把 fake 分数解释成语义效果"
→ 报告字段分层：恢复结论（工程）与向量内容（无语义声明）分列。V14/V15/V17/V18。

**风险**：矩阵规模爆炸——按 P6 fake.rs:429 注释"The P6-016 fault matrix
replays every variant through the same gate"的既有惯例收敛为代表性组合
（全因子只对 P6 已覆盖单变体，P7 做关键组合），组合清单在实施记录里列全。

---

## P7-017 离线默认包和未启用测试

**目标与约束**："禁网络环境启动所有旧工具；检查不创建语义cache文件；零key、
feature关、semantic.enabled=false都保持本地功能"。P6-020 已做一次性探针
（默认二进制 stdio 14 工具、零 cache 目录，tasks.json:45 round13 段）——本任务
把它固化为**正式回归**。

**模块归属**：`crates/cc-server/Cargo.toml`（默认 feature 断言面）+
`crates/cc-eval/tests/benchmark_adapters.rs`（离线启动矩阵）。

**接口草案**：无新 API。三口径启动矩阵：`cargo build -p cc-server`（默认）/
`--features semantic`（enabled=false）/ 零环境变量；每口径跑 14 工具 stdio
探针 + 断言：无网络 socket 尝试（stub transport 计数为 0 或无传输类型构造）、
无 cache 目录创建、`semantic_state=not_configured`、`dense_state=disabled`。

**验收对照**：acceptance 三句逐口径映射；V18/V21。P7-014 的运行时关闭路径
（IMPLEMENTATION-ORDER 第 6 节）在此回归。

**风险**：P7-001 落 cc-server 侧的网络 crate 会让 `--features semantic` 树含
网络依赖——"默认离线"的准确口径是**默认包零网络 + semantic 未 opt-in 零连接
尝试**，不是 feature 树零网络 crate；该表述差异要在 GATE 里写明，防止重演
P6-008/009 的 `cargo tree` 口径订正（execution_note round10 段）。

---

## P7-018 受授权的真实provider小集认证（conditional，不执行）

**处置**（D1 拍板，tasks.json:10014 块）：整任务不执行。产出仅两项记账：
① P7-020 双轨报告 live 栏 `blocked（DECISIONS-RECORDED.json D1+D2，2026-10-02）`，
逐 V 编号列 not_run/blocked 而非 done（tasks.json 通用 acceptance 第二句）；
② tasks.json 状态回填 blocked 由 owner 收口落。**不创建**
`crates/cc-eval/benchmarks/manifests/` 下的 live manifest（防虚假 manifest，
见 `OPEN-QUESTIONS.md` Q7）。未来授权后的执行面 = P7-001 适配层 + P7-018 步骤
原文，无需重构。

---

## P7-019 本地加dense的质量/成本消融（拆 fake 腿 / 正式质量腿）

**目标与约束**：D3 与本阶段无关，但 fake 消融腿先行。消融底座已冻结：
"Locked full-factorial experiments over separately built local MCP binaries.
Counterfactual source lives in isolated snapshots, never production feature
flags"（`crates/cc-eval/src/benchmark/ablation.rs:1-2`；`Control` ablation.rs:17、
`Variant` ablation.rs:24 携带独立 source_root/binary/build_receipt）。

**模块归属**：`ablation.rs`（variant 扩展：dense on/off/hybrid 三臂，dense 臂
以 FakeProvider 为后端）+ `artifacts/benchmarks/`（机制报告）。

**接口草案**：

```rust
// ablation.rs（草案）
// Variant.enabled 追加 "semantic_dense" 因子；dense 臂二进制 = --features semantic + fake provider 注入
// 报告列：heldout / exact退化 / 费用（含 cache_reuse 与 unknown 列，P7-008 口径）+ profile 标注 engineering/fake
```

**验收对照**：acceptance"收益CI与能力差异可解释，不为单题写特殊权重"→
fake 腿只验证机制（三臂可跑、费用列齐全、CI 计算可复算、同输入同 budget 断言），
**效果列全部标注 fake-profile 无语义解释力**（V15 fake/live 区分、G7
"live受授权单独记"，06-VALIDATION.md:37/:57）；正式质量腿（真实 corpus +
真实 provider 的 V19 全口径）整体 blocked，归 P8 live 线。

**风险**：fake 向量跑出的 heldout 数字若进入任何汇总表，必须带
`profile=fake` 列并禁止参与结论句——这是 G7"不把 fake 完成冒充真实效果"的
落地点；报告模板在 P7-020 统一定稿。

---

## P7-020 P7语义闭环与发布范围验收（G7）

**目标与约束**：G7 = "V15–V19；fake全过，live受授权单独记；默认离线零网络；
语义覆盖和费用可解释"（06-VALIDATION.md:57）；无 provider 环境的诚实口径
（06-VALIDATION.md:59）。depends_on 覆盖 001-017+019（不含 018），
`conditional_dependencies` 记 018 仅在 live certification 时需要
（tasks.json:10104 块）。

**模块归属**：`docs/roadmap/code-index-v2/`（GATE 与 TODO 回填）+
`artifacts/benchmarks/`（run-id 证据）。

**接口草案**（双轨报告模板，草案）：

```text
engineering/fake profile 栏：V15/V16/V17/V18 实测证据 + V19/V20 机制证据（P7-019 fake 腿）
live semantic-effect 栏：V15(live)/V18(live)/V19/V20 全部 blocked（引用 DECISIONS-RECORDED.json D1+D2）
13 项接线待办对账表：逐条 done/移交（round12 审计 ↔ 本轮实施记录双向）
发布能力限定句：M3-engineering 按实发面声明；M4-semantic 声明 local 范围 + live blocked
```

**验收对照**：acceptance"无live证据不声称已证明真实语义收益，本地发布不被
模型密钥强绑"→ 前者由报告栏位隔离保证，后者由 P7-017 离线默认包证据保证
（零 key 启动全功能）；V16（六项清单）逐条挂 run-id。

**风险**：`current_phase` 仍为 "P5"（tasks.json:11549）与 G6 已过的记账关系
未明——收口时按 Q8 裁决处理，不在本任务擅自改状态字段。
