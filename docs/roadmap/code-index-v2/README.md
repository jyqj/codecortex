# CodeCortex Code Index V2：重构优化设计与执行入口

> **115 done / 3 in_progress / 74 todo；P5为15/20。P5-016～018正在收口，P5-019/020尚未开始。** 当前最新P5-D final-v3验收为failed：Rust 1.95 workspace退出101，空闲项目回收测试预期2、实际1，独立audit.json未生成。三个任务不勾选完成，G5/M2仍未通过。
> 当前见[快照与阻塞](CHECKPOINT-2026-09-30.md)及[P5-D-PROGRESS.md](P5-D-PROGRESS.md)。本次保存开发进度，不发布版本标签；源码、任务状态、派生TODO与轻量失败收据一起版本化，大型原始产物仅保留本地。

**以下为上一批P5-C的历史验收记录，不代表当前P5-D已通过。** 详见[P5-C-COMPLETION.md](P5-C-COMPLETION.md)及[P5-C-GATE.json](P5-C-GATE.json)；旧P5-C-IMPLEMENTATION/PARTIAL-GATE/PROGRESS继续保留历史范围与失败。

冻结616文件、6605476字节，摘要`471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473`。38条命令收据，源码/归档/日志/不可变二进制均核验；最终接受版本对当前源码重新运行全套验证；中间版本曾复用的旧收据只保留历史，不代替最终结果。证据目录`artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930`。两工具链各workspace 1767 passed/57 ignored、HTTP 255 passed/50 ignored、专项 71 passed/2 ignored、真实stdio 24 passed、取消协议1 passed、watcher17 passed。各组内部失败0，忽略项不计通过；测试组有重叠，不相加为唯一测试总数。固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。

## 1. 目标

保留 CodeCortex 的 Rust 核心和 MCP-first 产品边界，把现有结构/词法索引演进为：**多语言增量正确、召回范围明确、源码证据可验证、语义检索可选、性能可以归因**的代码索引引擎。

不是把 Astrolabe、OCE 和 CodeCortex 三个引擎拼起来。Astrolabe 主要参考项目配置感知的模块解析与按需 LSP；OCE 主要参考语义边界切块、异步 embedding、独立召回、覆盖选择。参考实现中的局限必须经过本项目测试，不继承它们的性能或正确性结论。

## 2. 阅读与执行顺序

| 文档 | 用途 |
|---|---|
| [00-BASELINE.md](00-BASELINE.md) | 实际代码证据、已验证与待复现的区分、参考 SHA、现有能力保留清单 |
| [01-ARCHITECTURE.md](01-ARCHITECTURE.md) | 目标架构、完整变更范围文件树、模块职责、原模块迁移映射 |
| [02-CONTRACTS.md](02-CONTRACTS.md) | 公共接口、模块解析、身份、版本、硬软范围、缓存、源码证据契约 |
| [03-SEMANTIC.md](03-SEMANTIC.md) | embedding/向量后端、任务状态、两存储协调、费用与隐私、故障恢复 |
| [04-PHASES.md](04-PHASES.md) | P0–P9 分阶段目标、入口/出口、批次划分、并行限制 |
| [05-TODO.md](05-TODO.md) | 从 tasks.json 生成的逐项任务清单：范围、步骤、依赖、验收、验证、回滚 |
| [06-VALIDATION.md](06-VALIDATION.md) | 验证矩阵、属性测试、真实 MCP 验收、性能/召回/成本评测方法 |
| [07-ROLLOUT.md](07-ROLLOUT.md) | MCP/配置兼容、数据库演进、发布/回滚、风险与决策记录 |
| [08-HANDOFF.md](08-HANDOFF.md) | 下一轮从哪里开始、如何交接、证据记录模板 |
| [09-BENCHMARK.md](09-BENCHMARK.md) | oce-benchmark 源码参考、兼容/原生评分、公开协议适配、数据集、增量 oracle、性能/成本与 CI 门禁 |
| [tasks.json](tasks.json) | **唯一任务状态源**；Markdown 清单是它的派生视图 |
| [PLAN-CHECK.json](PLAN-CHECK.json) | 本轮计划结构检查结果；不是未来代码验收结果 |

初次阅读：本页 → 00 → 01 → 04。实施某个批次：08 → 04 中对应阶段 → 05 对应任务 → 相关契约 → 06 验证编号。

**10 个阶段 / 39 批 / 192 项任务。** 115 done / 3 in_progress / 74 todo；P5为15/20，当前P5-D验收未通过，先处理P5-016～018，再进入P5-019。 `tasks.json` 是唯一任务状态源，`05-TODO.md` 为派生视图。

固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。新增30次最终证据装配release观测，并复跑60次旧lane成本和192次有界准入请求；不是100k、峰值RSS或尾延迟认证。schema21不变，public adapter7、retrieval policy16、packing spec2。预算针对code_index_context结果对象，不包含JSON-RPC帧；其他旧工具形状保留原预算。没有真实provider/vector、持久化semantic epoch、原子文件系统快照、公开holdout、跨平台或发行认证。

## 3. 不改变的边界

- 不做通用 Agent runtime、任务工作流、记忆/知识库、UI、云端多租户服务；embedding job 只是索引派生产物的内部维护任务，不是用户任务系统。
- 默认启动、索引、搜索和图工具不需要网络、不要求 API key、不启动向量服务或 LSP。
- 保留七个现有 crate；只在语义实现阶段新增一个可选 `cc-semantic`，不为每个概念新建 crate。
- 保留 SQLite/FTS、增量扫描、build gate、类型化写接口、单向依赖、现有图能力与真实 MCP eval。
- embedding 的相似性不写成调用/引用/继承等结构事实；排序相关性、解析置信度、新鲜度分别表达。

## 4. 本设计的核心选择

1. **先修正确性与召回，再接模型。** BM25 单调性、硬范围/软线索、跨语言接口变化、确定性消歧先完成。
2. **按垂直能力迁移。** 修复一条完整调用链并锁定回归，不先做全仓机械搬家。
3. **项目模型独立于语法提取。** imports 的语法事实与语言/包/配置规则分开；配置变化进入增量依赖。
4. **源码实体、检索文档、向量产物分离。** 不让路径+序号同时承担三个生命周期。
5. **检索与上下文选择分开。** 多路独立召回 → 确定性融合 → 可选重排 → 任务覆盖选择 → 原文校验。
6. **可选语义能力独立失败。** 网络请求不持有 CodeIndex 锁、DB 连接或写事务；超时/部分覆盖有结构化说明。
7. **单一权威索引，允许显式的可选派生缓存。** 默认仍只有 `index.sqlite3`；P6 提议在启用语义功能时增加 `semantic-cache.sqlite3`，仅保存可复用向量及其编码规格。文档映射、发布可见性与 outbox 仍由 `index.sqlite3` 管理。此项是对 DESIGN.md 物理单库条款的**显式修订提案**，P6 实施时须先落正式 ADR，不在本轮偷偷修改章程。
8. **先 exact vector baseline，后决定 ANN。** 不预设 Milvus/HNSW/某云厂商为必需项；ANN、LSP、LLM rerank 都有独立收益门槛。

## 5. 发布里程碑

| 里程碑 | 必须完成 | 可交付能力 |
|---|---|---|
| M0 | P0 | 可复现的基线、真实缺陷夹具、受控构建环境 |
| M1 | P1 + P2 + P3 | 不依赖模型的检索修复与跨语言增量/模块解析增强 |
| M2 | P4 + P5，继承 M1 | 源码一致的切块、版本化文档、覆盖选择与扩展型查询执行 |
| M3-engineering | P6 + P7，继承 M2 | 可选 embedding 的持久化、发布、恢复和 dense 工程闭环；fake/协议证据与 live 效果证据分别声明 |
| M4-local / M4-semantic | P8 中对应发布范围的认证 | 本地与语义配置分别出具发布证据；无模型授权不阻止 local，缺 live 证据不作真实语义收益声明 |
| 可选增强 | P9 的独立决策门 | ANN / LSP / LLM rerank，仅在收益被证明时逐项启用 |

每个里程碑都有独立价值，不能把“等 embedding 做完”作为修复现有错误的前提。P9 不阻塞 M4；不选择某增强时记录 `deferred` 与理由，不伪造已完成。

## 6. 执行纪律

实现任务均以 `P<phase>-<序号>` 标识。`tasks.json` 中的 `status` 使用 `todo / in_progress / blocked / done / deferred`，默认全部 `todo`。只有附带当前目标 SHA、测试日志位置和评审结论后才可改 `done`。旧证据不因复制到新阶段自动变成新版本验收。

阶段完成标准不是“文件已拆开”或“新增了接口”，而是对应行为从模型、存储、编排、查询到 MCP 响应贯通；相应负面场景通过；回滚可执行。发现基线已变化时先建立差异映射，不覆盖新工作、不照旧路径盲改。

本设计不承诺未经测量的加速倍数、自然语言召回率或 embedding 费用。阶段阈值分为硬正确性门槛与待基线校准的性能目标，详见验证文档。
