# 03｜可选语义检索与 embedding 生命周期设计

> 主线阶段 P6/P7；C07/C09/C11/C12 是前置契约。本文规定目标行为，不代表当前已经接入模型或向量库。

## 1. 产品边界和第一版选择

默认完全离线；启用语义检索是显式配置。只新增可选 cc-semantic，不嵌入完整 OCE 服务，不新增通用 worker 平台。第一版采用 provider-neutral 编码接口、OpenAI-compatible HTTP provider、deterministic fake 和本地 filtered exact vector backend。ANN/LSP/LLM rerank 进入 P9 独立收益门，不作为第一版必需依赖。

向量只是候选召回证据，不产生 CALLS/REFERENCES/IMPLEMENTS。无 embedding 时现有 lexical/graph/impact 必须可用。运行时状态独立暴露 lexical_ready/structural_ready/semantic coverage，不能以一个 ready=true 概括全部。

## 2. 三种规格与身份

- VectorSpaceSpec：provider/model identity + provider revision（如能获取）+ dimensions + distance metric + normalization +可影响坐标的 provider options。编码空间改变必须隔离存储，不能只按维度相同复用。
- DocumentEncodingSpec：VectorSpaceSpec + 文档 input 模板/分块输入策略版本。EmbeddingInputHash = hash(实际输入 bytes + 规格)，不是 DocKey。
- QueryEncodingSpec：VectorSpaceSpec + query instruction + query preprocessing version。只改 query instruction 不必重新嵌入文档，但必须失效 query-vector/cache，并在 benchmark 记录。

远端可能在相同 model name 下更新模型。不能伪称能检测所有静默更新：优先 pin provider revision；不支持时提供 operator-managed model_revision，status 显示 unpinned，正式可比 benchmark 必须固定并记录这一限制。

## 3. 存储所有权与拟议表

默认依旧只写 index.sqlite3。启用语义能力后拟新增 semantic-cache.sqlite3，P6 先通过正式 ADR 修订物理单库约定。此缓存放在当前项目 cache namespace 下，不默认跨项目共享，避免隐私边界和付费归属混淆。

| 位置 | 逻辑表 | 权威含义 |
|---|---|---|
| index.sqlite3 | retrieval_documents | 当前 DocKey/DocVersion、source span、input hash、doc kind |
| index.sqlite3 | semantic_manifest | 当前空间下哪些文档映射已发布；检索可见性的唯一权威 |
| index.sqlite3 | semantic_outbox | desired input、操作、lease token、attempt、retry_at、basis |
| index.sqlite3 | metadata/epochs | incarnation、semantic epoch、schema/投影规格 |
| semantic-cache.sqlite3 | encoding_specs | 文档/空间规格，可版本化且不含密钥 |
| semantic-cache.sqlite3 | vector_artifacts | input hash+document spec → validated vector/checksum |
| semantic-cache.sqlite3 | cache_metadata | schema、容量、访问统计/GC 必需元信息 |

表名可在实现中映射既有 schema；不能为同一 job 再维护第二份逻辑状态。派生缓存内容是昂贵但可重建产物，不替代源码索引。缓存损坏可以隔离重建；不能自动重复付费而无状态/预算说明。

## 4. 索引事务与 outbox

同一结构索引写事务原子完成：更新源码事实/文档；撤销旧文档 manifest；记录新的 desired embedding 工作；推进应变更的 epochs。旧向量可继续留在 artifact cache，但在 manifest 撤销后不得对当前搜索可见。

job logical key 使用 project namespace + DocKey + desired version + document spec。对连续编辑可合并/取代未开始的工作；已运行的旧任务允许生成可复用 artifact，但不准发布到新文档。纯删除不发模型请求；只撤销可见映射并安排后续 GC。

lease/heartbeat 是辅助写，不推进内容 epoch；publish 改变可检索集合才推进 semantic_epoch。保持现有 UnitOfWork 的默认 index write 行为，通过封闭 WriteEffect 类型显式扩展，不散布手工 bump。

## 5. 状态机

```text
pending -> leased -> encoded -> artifact_stored -> published
   ^          |           |              |
   |          +-> retry_wait ------------+
   |          +-> permanent_failed
   +--- lease expires / restart

任意未发布状态 -> superseded / cancelled（文档或规格已变）
已发布映射 -> withdrawn（删除、修改、切模型）；artifact 不立即删除
```

encoded/artifact_stored 可作为尝试日志/可恢复检查点，不强制每个瞬时状态独立数据库写。对外聚合 pending/running/retry/failed/ready/superseded，附 eligible/published/stale/failed 文档计数。coverage=当前 manifest 已发布且 artifact 有效的文档数 / 当前所选空间 eligible 文档数；分母为 0 要说明原因，不虚报完成 100%。

采用 at-least-once 执行+幂等落库。不能承诺跨远端 API、缓存库、索引库的 exactly-once，也不能承诺崩溃时零重复收费。供应商支持 idempotency key 时使用；不支持时账单不确定尝试单独记录，重试受预算限制。

## 6. 发布协议与 fencing

1. 短事务 claim，拿到唯一 lease token/attempt、index incarnation、desired DocVersion、input hash、encoding spec；释放连接。
2. 查 cache；命中时校验 checksum、维度、finite/norm、规格。命中不调用 provider。
3. 未命中时经过 admission，把请求放到有界批次；无 DB/CodeIndex 锁持有；调用 provider。
4. 验证响应数量、index 完整无重复、向量维度、所有元素 finite、非退化向量、normalize 策略。无效结果不缓存、不发布。
5. 幂等写 artifact cache，持久化完成后才尝试 publish。
6. index 短事务 compare-and-set：incarnation、lease token、current desired version、input hash、active space 全部匹配；确认 artifact receipt；写 manifest，推进 semantic_epoch。
7. 任一条件不匹配仅标 superseded，不能把老结果挂到同路径的新代码。lease 过期后老 worker 不得替新 worker完成状态。

发布验证与输入更改共用事务隔离，不能把“先检查然后异步写回”当成原子操作。cache 与 index 无跨库原子性：先 artifact 后 manifest 是可恢复顺序；query 仍需验证 artifact 存在。GC 和 publish 共用缓存 namespace 的协调协议，防 publish 引用刚被 GC 删除的 artifact。

## 7. 崩溃、重建与并发

| 故障窗口 | 恢复动作 |
|---|---|
| claim 后未调用模型 | lease 过期后重入，记录旧 attempt |
| 模型完成但 artifact 未存 | 可能重复调用；按 provider 支持与费用不确定策略处理 |
| artifact 已存，publish 前退出 | 重试先命中 cache，再做 CAS，不重新收费 |
| publish 成功，ack 前退出 | 读 manifest，幂等确认完成 |
| file 编辑/删除/rename 与慢响应竞争 | desired mismatch → superseded；删除不复活 |
| staging 全量换库 | 新 incarnation；旧 worker 停止发布，新 manifest 从缓存重建 |
| 进程重启 | 扫 pending/过期 lease/manifest missing artifact，受预算恢复 |
| cache 被手动删除或损坏 | semantic degraded，完整性扫描和重建显式；local search 可用 |
| 模型规格切换 | 新空间先回填，按策略切 active space，不跨空间融合 cosine |
| 两进程同项目 | DB claim CAS 与共享缓存锁保护；查询/构建继续使用既有跨进程失效守卫 |

不能假设 in-process build gate 覆盖其他进程。gc 使用 mark/sweep + generation/锁/最短保留期，避免删除活跃或租约引用。schema 重建清索引事实，不自动清已付费 artifact；cache schema 不兼容时只隔离到版本目录或明确迁移，不静默误读。

## 8. 请求调度、费用与隐私

admission 同时限制每 input token/bytes、batch 项数与总量、project 并发、provider 全局并发、每分钟速率、排队长度。429/5xx/timeout 遵循有界指数退避+jitter+Retry-After；鉴权失败和持续规格错误停机并透出，不进行无限重试。断路器按 provider/endpoint，不因一个项目刷坏全局服务。

费用只依据 provider usage 或明确标记的估算。配置额度、每日/本次上限与操作计数独立于搜索结果；不记录密钥、完整私有源码或完整模型请求到普通日志。URL endpoint 不允许任意跳转携带 Authorization；开发本地 HTTP 需要显式许可。远端源码发送必须 opt-in；ignore/sensitive/generated policy 在入队前生效，查询也不能带被排除文件内容。

没有服务端持续遥测数据库。内部可靠队列需要的 attempts/usage receipt 属于可选语义维护状态；报告和长期性能数据由 cc-eval 输出。任何与 DESIGN.md 边界冲突的持久化字段在 P6 ADR 明确说明。

## 9. exact backend 与后续 ANN

exact backend 是可验证的质量 oracle：同空间的 dot/cosine、过滤在 top-k 前、有界 batch 读取、稳定 tie-break；对每个有效 artifact 只存一次数值，不每查询把全仓向量复制到 Vec。memory-mapped/批次缓存是否值得引入由 profile 决定。量化在 P9，不能悄悄改变 oracle。

首轮规模按文件数+doc 数+向量维度共同评估。100,000 × 1024 × 4 bytes 的原始 f32 约 390.625 MiB，不含索引元数据/额外缓存。ANN 的准入依据是实际延迟/内存超预算、对 exact 的过滤 recall、更新/删除一致性和打包可用性，而非“向量必须 ANN”。

## 10. 查询路径

local/auto/semantic 策略由 cc-search 规划，dense 通过 SemanticRecall port 接入；cc-search 不依赖 provider crate。query vector 可按规范化查询+QueryEncodingSpec 缓存；内容转换必须记录。dense 返回 DocKey/Version 与 artifact identity，最终用当前 manifest 和 HardScope 再验。

semantic coverage 不足时 auto 返回本地结果+部分语义候选并声明状态；显式 semantic 不允许悄悄当作全量语义检索。可选 LLM rerank 不是第一版前提。RRF 分数与 cosine 原值分别报告，不跨原始量纲相加。

## 11. 配置示意（拟新增，当前不可直接使用）

```json
{
  "semantic": {
    "enabled": false,
    "provider": "openai_compatible",
    "endpoint": "https://provider.example/v1/embeddings",
    "api_key_env": "CODECORTEX_EMBED_API_KEY",
    "model": "explicit-model-id",
    "model_revision": "operator-pinned-revision",
    "dimensions": 1024,
    "metric": "cosine",
    "backend": "exact",
    "document_template_version": 1,
    "query_instruction": "",
    "limits": {"max_concurrency": 2, "max_pending_documents": 10000},
    "remote_code_transfer": "explicit_opt_in"
  }
}
```

示意数值不是实测最优值。实施前补充单位、上下限、缺省值和热更新行为；Secret 只引用环境/凭据接口，不写入可导出配置。status 只输出脱敏摘要。schema 兼容、网络禁用和无 key 启动是 P7 必过验收。
