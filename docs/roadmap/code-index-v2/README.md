# CodeCortex Code Index V2：重构优化设计与执行入口

## 当前进度

<!-- code-index-progress:start -->
Code Index V2 共 **192 项任务：150 done / 11 in_progress / 31 todo**。

当前阶段：**P7｜provider与dense端到端**；计划状态：`in_progress`；更新日期：`2026-10-07`。
下一任务：**P7-011｜dense范围与hydrate守卫**（硬依赖已完成）。

| 当前下一项、进行中任务及其未完成前置 | 状态 | 硬依赖（任务状态） |
|---|---|---|
| P7-011｜dense范围与hydrate守卫 | `todo` | P6-020 (done)、P7-009 (done)、P7-010 (done) |
| P7-012｜融合与部分覆盖语义 | `todo` | P7-011 (todo) |
| P7-013｜查询总deadline和模型故障退化 | `todo` | P7-012 (todo) |
| P7-014｜配置/status/MCP全链贯通 | `in_progress` | P7-013 (todo) |
| P7-015｜后台回填与前台查询竞争测试 | `todo` | P7-014 (in_progress) |
| P7-016｜fake全故障矩阵回归 | `todo` | P6-020 (done)、P7-015 (todo) |
| P7-017｜离线默认包和未启用测试 | `todo` | P7-016 (todo) |
| P7-019｜本地加dense的质量/成本消融 | `todo` | P7-017 (todo) |
| P7-020｜P7语义闭环与发布范围验收 | `todo` | P7-001 (done)、P7-002 (done)、P7-003 (done)、P7-004 (done)、P7-005 (done)、P7-006 (done)、P7-007 (done)、P7-008 (done)、P7-009 (done)、P7-010 (done)、P7-011 (todo)、P7-012 (todo)、P7-013 (todo)、P7-014 (in_progress)、P7-015 (todo)、P7-016 (todo)、P7-017 (todo)、P7-019 (todo) |
| P8-001｜锁定release候选与证据输入 | `in_progress` | P7-020 (todo) |
| P8-002｜完成真实多仓native语料认证 | `todo` | P8-001 (in_progress) |
| P8-003｜运行外部兼容套件 | `todo` | P8-002 (todo) |
| P8-004｜封存holdout与反过拟合检查 | `todo` | P8-003 (todo) |
| P8-005｜完整规模1k到100k | `in_progress` | P8-004 (todo) |
| P8-006｜增量规模与fanout曲线 | `in_progress` | P7-020 (todo)、P8-001 (in_progress)、P8-005 (in_progress) |
| P8-007｜多并发与混合负载 | `in_progress` | P8-006 (in_progress) |
| P8-008｜冷建/重开/热查分层 | `in_progress` | P8-007 (in_progress) |
| P8-009｜内存/磁盘/费用总账 | `in_progress` | P8-008 (in_progress) |
| P8-010｜长时soak与连续修改 | `in_progress` | P8-009 (in_progress) |
| P8-011｜端到端故障与恢复认证 | `todo` | P7-020 (todo)、P8-007 (in_progress)、P8-010 (in_progress) |
| P8-012｜MSRV与平台冷构建矩阵 | `todo` | P8-011 (todo) |
| P8-013｜指标/门槛与失败退出最终认证 | `in_progress` | P8-012 (todo) |
| P8-016｜数据库/配置/包回滚演练 | `todo` | P7-020 (todo)、P8-012 (todo)、P8-013 (in_progress) |
| P8-017｜删除临时兼容和重复模块 | `todo` | P8-016 (todo) |
| P8-018｜文档事实与安装契约同步 | `in_progress` | P8-017 (todo) |
| P8-019｜发布工件与完整报告归档 | `in_progress` | P8-018 (in_progress) |

进度入口：[重构总览](README.md) · [逐项 TODO](05-TODO.md) · [唯一任务状态源](tasks.json) · [执行交接](08-HANDOFF.md)。
任务完成数不等同发布认证；以各任务证据和适用验证范围为准。

> 本块由 `scripts/code_index_plan.py --write` 从 `tasks.json` 生成；无参运行校验全部进度入口。
> 源文件 SHA-256：`5d0b41e3ad43d9aead060ec03555f76ccb2b38aeb8b9c32906d7c2651cf445e0`。
<!-- code-index-progress:end -->

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

## 7. 历史入口快照（2026-09-30 至 2026-10-02）

以下保留旧入口的进度、暂停与验证记录；它们只描述当时状态。当前执行导航和计数以顶部生成区为准。

> 2026-10-01：用户已暂停开发/验证/矩阵，当前全部工程与失败进度上传 GitHub；G5/M2未通过。见 [暂停进度](2026-10-01-PAUSED-PROGRESS.md)。


> **118 done / 1 in_progress / 73 todo，P5 为 18/20；P5-016～018 已验收，P5-019 正在实施通用查询质量修复与独立消融。P5-D 整批与 G5/M2 尚未完成。** 已接入能力状态、查询视图租约、LRU 弱登记与取消安全的冷初始化、非阻塞空闲清理，以及 search/context 的显式策略参数；dense 仍明确 disabled。
> 最新见 [P5-D-RUNTIME-IMPLEMENTATION.md](P5-D-RUNTIME-IMPLEMENTATION.md) 与 [P5-D-RUNTIME-GATE.json](P5-D-RUNTIME-GATE.json)。

冻结 623 文件、6675050 字节，摘要 `44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`；38 条命令收据、源码归档、日志和不可变二进制一致。证据目录 `artifacts/benchmarks/p5d-20260930-resume/final-v3`。stable：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored；1.95.0：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored。各组失败 0，忽略项不计通过，重叠组不相加为唯一测试总数。固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。

Git HEAD=`0a56a257f9a92c54d06ea5be0ce1d1763917a527`；未提交、推送、PR 或合并，既有工作与失败证据保留。

**10 个阶段 / 39 批 / 192 项任务。** 115 done / 3 in_progress / 74 todo；P5为15/20，当前P5-D验收未通过，先处理P5-016～018，再进入P5-019。 `tasks.json` 是唯一任务状态源，`05-TODO.md` 为派生视图。

固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。新增 12 组 release 项目清理/租约观测，复跑 90 次旧查询成本和 192 次准入请求；可选 ps 进程树采样在本机停滞后显式关闭，对应 RSS 为 null、原生自身 RSS 单列。核心验证未跳过；不是 P5-019 的完整消融、100k、尾延迟或发行认证。
