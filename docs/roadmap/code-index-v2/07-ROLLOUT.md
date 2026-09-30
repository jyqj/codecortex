# 07｜迁移、兼容、风险与回滚

## 1. 垂直迁移而非全仓搬家

每批按一个行为从model→DB→index/search→server→MCP→eval贯通。先有红绿测试再改行为；同一PR不同时更换AST库、SQL引擎、chunker和模型。保留existing framework/dispatch/infra/community/Cypher/graph_read_model等已验证能力。文件树是职责图，不要求第一批就创建所有空文件。

已有七crate保留。cc-semantic只有P6开始才添加；eval新增binary是开发工具，不将产品变为CLI应用。默认包不引入OCE服务或向量数据库服务、LSP、模型凭据。

## 2. 需要在实施时正式落下的ADR

| 决策 | 阶段 | 核心内容 |
|---|---|---|
| ADR-scope-and-candidates | P1/P5 | hard scope与soft hints分离；通道失败与预算契约 |
| ADR-public-surface-dependencies | P2 | 语言无关public surface、negative lookup、确定性歧义 |
| ADR-project-model | P3 | config版本与纯模块解析；编译器模式和heuristic边界 |
| ADR-document-identity | P4 | raw snapshot、DocVersion、输入hash与legacy chunk id |
| ADR-semantic-derived-cache | P6 | 单一权威index +可选派生cache；章程修订、费用和隐私边界 |
| ADR-semantic-publication | P6 | outbox/lease/fencing/两库顺序/GC/rebuild |
| ADR-query-execution | P5/P7 | 锁外网络、generation guard、deadline和bounded work |
| ADR-benchmark-protocol | P0 | 公开MCP/HTTP、兼容vs native评分、输入锁、CI退出 |
| ADR-optional-backends | P9 | ANN/LSP/rerank各自收益与默认配置 |

这些是拟议决策主题，不占用尚未分配的正式编号；实施按docs/adr现有编号顺序登记。不要把规划文本复制成第二份永远不更新的运行文档：落地后ADR记录why，internals记录how，roadmap只记录任务状态和迁移结果。

## 3. schema与昂贵缓存

当前源码schema v7，旧文档v6需要同步。按发布里程碑合并必要schema变更；开发阶段可以有schema草稿，但避免每个小任务都迫使用户重新索引。当前索引rebuild-on-mismatch策略可继续，不为可重建的结构索引引入大型迁移平台。

P2/P4新表需要老索引重建/能力重新生成；P6增加manifest/outbox必须与默认禁用行为兼容。语义artifact cache单独schema/version，旧binary不得误读新空间/新格式。结构索引换库变更incarnation，index/evidence/semantic的恢复来源明确。

回滚binary遇到新index schema可以受控重建，绝不能触及用户源码。语义cache保留，不因结构索引schema变更自动清零付费向量；需要不可逆cache迁移时先导出/备份并明确范围，默认不做破坏性自动升级。

## 4. MCP与配置兼容

保留全部14工具、search.mode语义、项目切换与参数验证。新的retrieval_strategy等字段采用additive方式；unknown参数继续拒绝，避免拼错配置静默启用默认。工具顶层返回shape如需从数组改信封，必须显式版本/兼容适配和端到端测试，不能借新diagnostics顺手破坏旧调用。

现有local能力不需要密钥；semantic.enabled=false不创建cache文件、不探测模型 endpoint、不启动worker。feature不可用但配置请求开启时status明确说明，而非直接panic。模型/space/config热更新须决定哪些对象重建、哪些缓存失效、哪些任务supersede，并纳入generation指纹。

默认启用新功能的门槛是完整认证，不是已写实现。结构正确性修复没有长期旧错误开关；为了对比保留的旧算法只在benchmark/test profile，不在正常产品同时维护两套。

## 5. 主要风险登记

| 风险 | 检测 | 缓解/回退 |
|---|---|---|
| soft范围放开使grep成本暴增 | 扫描/解压/SQL计数与tail latency | 有界分段扫描、诊断partial，不退回硬漏召回 |
| surface变化传播过宽 | body/API分层mutation、dirty文件计数 | 更精确依赖；未知时保守而透明 |
| negative依赖存储膨胀 | name bucket/cardinality与DB增长曲线 | scope化依赖、批量存储和清理，不只靠无限表 |
| 模块规则错误造成假精确 | 编译器/手写gold对照、mode matrix | typed ambiguity/unsupported，不全局模糊补全 |
| 新chunker召回或费用退化 | 同检索器消融、文档数/input size/复用 | 独立参数校准，保留可回滚版本 |
| query无锁后读取混代 | 并发写与generation故障测试 | 有限重试/版本守卫/失败不缓存 |
| 双库发布和GC竞态 | crash/lease/GC矩阵 | artifact-first、CAS、namespace协调与query验证 |
| 模型静默换版本 | revision记录/同输入探针差异 | pin或operator revision，无法证明时声明不确定 |
| 远端重复收费 | attempt receipts、usage/timeout | 有界重试、幂等key可用则用、费用上限 |
| benchmark只优化公开题 | heldout、family split、生产特判审查 | 题库变更独立评审、盲测、对照重跑 |
| benchmark错误当成功 | 故意注入锁/协议/质量失败 | nonzero exit与gate.json，inconclusive不算passed |
| 100k测试挤爆开发机 | profile资源估算/隔离runner | nightly/release档运行，不在普通PR强跑 |
| 过度拆crate/重复运行时 | 依赖图、owner映射、删除清单 | 只新增cc-semantic，不迁入通用Agent机制 |

## 6. 发行范围与回滚顺序

M1：结构/词法修复。M2：文档身份/上下文质量与扩展执行能力，本地完整交付。M3-engineering：fake/协议验证的可选语义工程闭环，声明live认证状态。M4-local与M4-semantic：按相应profile通过的正式发布，后者必须包含实际模型/固定公开语料的效果证据。P9独立可选。

回滚顺序：停止新optional任务→切local策略→fence旧worker→切回已验证binary/config→必要时重建index→对cache/manifest对账→跑关键stdio与source/scope回归。禁止为了快速恢复直接忽略manifest检查。保留故障run原始证据和费用不确定记录，不把重跑成功覆盖首个失败。

## 7. 技术债删除清单

P1删除隐式预选硬过滤旧分支；P2移除JS专属公共指纹唯一入口与历史顺序tie；P3收拢通用import_resolver和Cargo fallback到一处项目模型；P4生产只保留一套chunking入口和明确legacy wire mapper；P5只保留一套候选/状态/预算owner；P6不保留多个job状态源；P8清临时feature实验、重复scorer和陈旧文档数字。

不删除用户业务数据、不清理未读无关文件、不把上一轮设计永久并列另起v3。实现后的历史why进入Git/ADR，当前运行文档只有一个入口。
