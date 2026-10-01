# 第一轮：全部 todolist 独立验收范围与缺口

## 结论和范围

用户要求用多轮 multiagent 完成全部 todolist；本 agent 只读生产源码、任务清单和历史证据，独占本审计目录，不改变 tasks.json。当前任务权威为 docs/roadmap/code-index-v2/tasks.json，不以 Markdown 勾选或计划结构检查作为行为证据。第一轮盘点有 192 项、115 done、3 in_progress、74 todo，所有 219 个历史证据引用路径存在；存在不等于通过，更不等于当前 release 认证。

本报告采用通用工程审计结构（flavor = null）。无项目相关 memory 有效命中，未采用历史 memory 事实。Code Index 已初始化；Rust 文件深符号读取不可用时降级限定范围源读取。未执行本轮完整 Cargo 测试，正在由 implementation owner 冻结执行。

## Evidence → Finding → Path

- E01：round-01.json 固定当前 HEAD、tasks SHA-256、Git 状态、每项全部 steps/deliverables/acceptance/validations/rollback/conditional/required_for、219 文件存在性/轻量 hash/JSON receipt 状态。复现：`python3 artifacts/checkpoints/todolist-completion-audit/snapshot.py round-02.json`（输出新名称，不覆盖旧证据）。F01：115 个 done 是历史本地分批范围，不足证明全部目标达成。P01：最终候选重新验证所有对应行为与依赖闭包。
- E02：artifacts/benchmarks/p5d-20260930-runtime/final-v3/validation.json 是 failed；Rust 1.95 workspace exit 101，cached idle close 预期 2 实际 1，audit.json 缺失。F02：P5-016～018 尚不能 done。P02：保留失败，确定性竞争负例、稳定最终回收、当前冻结全回归和独立审计。
- E03：p5-completeness-history.json 固定 75 条历史 Partial/S11 raw path/hash/lanes/packing。F03：P5-C ranking non-regression 不等于 completeness，通过 P5-D 也不自动解除 G5/M2。P03：分别修 lane coverage、输出预算 facet 保留、no-answer 行为，完整 V19/V20 再验。
- E04：04-PHASES.md、06-VALIDATION.md、07-ROLLOUT.md、09-BENCHMARK.md 定义 gate/milestone/conditional。F04：live 受授权预算限制，P9 允许真实收益决策后 deferred，但“决策完成”不能冒充“功能实现”。P04：依原条件分别结项并明确发布影响。

## 当前关键路径与最低证明

| 范围 | 需要完成的实现/验收 | 不可替代的证明 |
|---|---|---|
| P5-016 | 能力状态真实、dense disabled/配置状态贯通 | V18：真实 stdio/schema/sanitize/dispatch/status 和无虚假 ready |
| P5-017 | 合法 inflight 不被 LRU/idle 中断，租约最后释放可回收 | V11/V20：read/write/build 竞争时 sweep 非阻塞；最终每唯一实例回收；slow fake + 18 LRU + weak DB/runtime 消失 |
| P5-018 | 14 工具旧调用和新增 optional 参数契约 | 旧新实际 binary 的 tools/list 逐属性/required/mode对照、真实 stdio调用和文档事实一致 |
| P5-019 | 查询质量/预算/成本/并发消融 | 同输入、gold不改、逐题 required facets/span、no-answer/partial完整性、raw可重算、所有samples、尾延迟/CPU/资源与排队拒绝分报 |
| P5-020 / G5 / M2 | 完整本地增强版 | V11/V12/V18/V19/V20 按当前 source closure 验证；局部non-regression不是完整gate |
| P6 / G6 | 可选cc-semantic、编码空间、typed effects/schema、outbox/lease/cache/exact/publish/epoch/worker/rebuild/recovery/GC | fake确定性两进程CAS、过期lease/删除竞争、每crash点恢复、GC/publish竞态、空间隔离、默认不开第二库 |
| P7 / G7 / M3-engineering | HTTP provider、输入/响应/批次/并发/重试/隐私/费用、query编码/dense接线/状态/故障降级 | 真实HTTP stub与完整fake故障矩阵；默认无网络无key；无锁网络、有界deadline、无错误完整缓存；fake不算模型效果 |
| P8 / G8 / M4-local | 候选锁、真实多仓native、外部兼容、family holdout、1k–100k、增量/fanout、C1/4/8/16、冷热分层、内存/磁盘账、soak、恢复、平台冷构建、退出码、回滚、债务/文档/工件 | isolated release实际测量与原始可复算报告；明确平台范围、same work/no-best-of/sample N/CI/inconclusive、负例必nonzero、选择发布范围blocker=0 |
| P7-018 / P8-015 / M4-semantic | 授权真实provider小集与公开holdout真实效果 | 明确endpoint/model/允许外发数据/预算和live费用，heldout收益CI下界>0且exact无实质退化；否则deferred/blocked，不声称M4-semantic通过 |
| P8-014 | 可选LLM旁证 | 需独立授权预算；不可替代确定性gold，未执行不声称LLM复核 |
| P9 / G9 | ANN/LSP/rerank/查询分解逐项收益决策与选择后的工程 | 实测压力和exact filtered recall；LSP snapshot冲突与完整外部进程成本；rerank盲测收益/成本/deadline；未选择分支明确deferred及证据，不算实现done |

## P5-017 修复独立裁决

已审阅 project_session.rs、query_handle.rs、service_factory.rs、engine.close/is_closed、实际 P5-D lifecycle 测试和 owner 测试 diff。close_idle_instances 生产明确 try_write / build gate try_lock，不排队等待合法查询或构建；query pin 的捕获受同 runtime read lock 保护，回收在 write guard下检查 pin。watcher config probe可以短暂持B读锁，因此单次sweep返回1不证明泄漏。

接受 owner 的测试调整进入冻结验证：原 A/B 测试在5秒内累计每次sweep（不是best-of），最终 exact2、A/B都closed、原A透明同实例重开不变；新增确定性held-B read lock first exact1且500ms内返回、release后second exact1、third exact0。active和LRU重复Arc由于write guard下!is_closed不会重复计数。累计期间没有正式路由重开，故累计exact2有效；单此patch仍不足完成全P5-017，要保留最后租约和DB弱引用释放证据。不能将try_write改成等待来迎合旧测试。

## P5 完整性历史反例

这些只是历史诊断，不是当前失败重跑：source R01～R14全部有 lexical candidate_limit，大量graph_source_unmapped/graph_expansion_limit，R04还有path_token_limit；最终装配省略5～9hits。intents I03/I07/I11/I15有graph_expansion_limit，I07零hits。新6题I05/I06/I09/I10/I13/I14在lane全complete时因packing省略1～2hit。S11无lane失败却no-answer错误。逐题raw固定于p5-completeness-history.json。

应修通用召回/身份映射/意图选择、去除冗余metadata保护必需facet、通用no-answer置信与精确约束；不能删partial、无界扩大limit、改gold/输入预算或放宽scorer交换绿灯。片段正确和结构JSON预算始终是硬红线。

## 并行 ownership

implementation owner 持有生产入口、共享model/schema/manifest/tasks/gate和串行集成。审计 agent 只写本目录。额外 agent 可独占未来 HTTP stub/fixtures 或 P8 dataset/gold清单；依赖未冻结只能准备不能声称接线完成。P6 outbox/publish/GC/cache/rebuild属于同一致性事务边界，不能各agent定义不同状态机。性能测量时禁止同时运行其他编译/大负载。

## 全 192 项要求索引

完整机器可审阅矩阵见 round-01.json；以下列出每项状态、Vxx、里程碑和条件属性。历史 done 全部保留为“待当前行为重验”，不在本报告改写状态。

| ID | 标题 | 记录状态 | 验证 | 里程碑 | 关闭条件 |
|---|---|---|---|---|---|
| P0-001 | 冻结源码与工作区基线 | done | V01 | M0 | 实现与当前行为证据 |
| P0-002 | 建立隔离冷构建环境 | done | V01 | M0 | 实现与当前行为证据 |
| P0-003 | 运行现有回归并归档 | done | V01, V04 | M0 | 实现与当前行为证据 |
| P0-004 | 固定缺陷夹具与已知失败登记 | done | V05, V06, V07 | M0 | 实现与当前行为证据 |
| P0-005 | 定义版本化benchmark schema | done | V02 | M0 | 实现与当前行为证据 |
| P0-006 | 设计外部题库导入与许可记录 | done | V02, V03 | M0 | 实现与当前行为证据 |
| P0-007 | 实现OCE兼容评分goldens | done | V03 | M0 | 实现与当前行为证据 |
| P0-008 | 实现原生证据评分骨架 | done | V03 | M0 | 实现与当前行为证据 |
| P0-009 | 实现严格输入锁 | done | V02 | M0 | 实现与当前行为证据 |
| P0-010 | 冻结后端适配接口 | done | V04 | M0 | 实现与当前行为证据 |
| P0-011 | 实现真实MCP子进程适配 | done | V04, V18 | M0 | 实现与当前行为证据 |
| P0-012 | 实现可选OCE HTTP适配 | done | V04 | M0 | 实现与当前行为证据 |
| P0-013 | 实现readiness与失败分母 | done | V04 | M0 | 实现与当前行为证据 |
| P0-014 | 实现原始工件和报告一致性 | done | V04 | M0 | 实现与当前行为证据 |
| P0-015 | 建立资源与延迟采样原语 | done | V20 | M0 | 实现与当前行为证据 |
| P0-016 | 建立增量对全量oracle骨架 | done | V07 | M0 | 实现与当前行为证据 |
| P0-017 | 落第一批真实查询与强干扰样本 | done | V02, V19 | M0 | 实现与当前行为证据 |
| P0-018 | 冻结数据治理和holdout规则 | done | V02, V19 | M0 | 实现与当前行为证据 |
| P0-019 | 生成首份基线与门槛配置 | done | V01, V03, V04, V19, V20 | M0 | 实现与当前行为证据 |
| P0-020 | P0验收与任务视图维护 | done | V01, V02, V03, V04 | M0 | 实现与当前行为证据 |
| P1-001 | 补BM25端到端排序回归 | done | V05 | M1 | 实现与当前行为证据 |
| P1-002 | 就地修正BM25单调映射 | done | V05, V19 | M1 | 实现与当前行为证据 |
| P1-003 | 定义HardScope与SoftHints | done | V05, V18 | M1 | 实现与当前行为证据 |
| P1-004 | 移除预选到硬范围的隐式写回 | done | V05, V19 | M1 | 实现与当前行为证据 |
| P1-005 | 统一DSL与参数范围交集 | done | V05, V18 | M1 | 实现与当前行为证据 |
| P1-006 | 完善路径规范与越界测试 | done | V05 | M1 | 实现与当前行为证据 |
| P1-007 | 词法lane独立作用域 | done | V05, V19 | M1 | 实现与当前行为证据 |
| P1-008 | grep分段扫描与诊断 | done | V05, V20 | M1 | 实现与当前行为证据 |
| P1-009 | graph lane与最终过滤对齐 | done | V05 | M1 | 实现与当前行为证据 |
| P1-010 | 精确符号和路径结果保底测试 | done | V05, V19 | M1 | 实现与当前行为证据 |
| P1-011 | 范围与排序缓存键更新 | done | V05, V11 | M1 | 实现与当前行为证据 |
| P1-012 | 可观测scope解释 | done | V05, V12 | M1 | 实现与当前行为证据 |
| P1-013 | MCP契约与unknown参数回归 | done | V18 | M1 | 实现与当前行为证据 |
| P1-014 | 小型中文/英文意图回归 | done | V19 | M1 | 实现与当前行为证据 |
| P1-015 | 召回质量配对消融 | done | V19 | M1 | 实现与当前行为证据 |
| P1-016 | 检索解压与SQL成本对照 | done | V20 | M1 | 实现与当前行为证据 |
| P1-017 | 并发与读池退化测试 | done | V11, V20 | M1 | 实现与当前行为证据 |
| P1-018 | 删除重复过滤与旧分支 | done | V05, V18 | M1 | 实现与当前行为证据 |
| P1-019 | 同步搜索文档与行为说明 | done | V18, V21 | M1 | 实现与当前行为证据 |
| P1-020 | P1验收与known-failure关闭 | done | V05, V18, V19, V20 | M1 | 实现与当前行为证据 |
| P2-001 | 冻结PublicSurface模型 | done | V06 | M1 | 实现与当前行为证据 |
| P2-002 | 实现规范指纹与存储 | done | V06, V13 | M1 | 实现与当前行为证据 |
| P2-003 | JS/TS公共表面补全 | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-004 | Rust公共接口与可见性 | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-005 | Python模块表面 | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-006 | Go package公共接口 | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-007 | 其余语言保守能力表 | done | V06 | M1 | 实现与当前行为证据 |
| P2-008 | 定义ResolutionOutcome与歧义 | done | V07 | M1 | 实现与当前行为证据 |
| P2-009 | 修正warm/cold消歧差异 | done | V07 | M1 | 实现与当前行为证据 |
| P2-010 | 加入正向与负向解析依赖 | done | V07, V13 | M1 | 实现与当前行为证据 |
| P2-011 | 接入变化分类与DirtyPlanner | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-012 | 处理重导出环与多步收敛 | done | V07 | M1 | 实现与当前行为证据 |
| P2-013 | 完整清理脏重载目标字段 | done | V07 | M1 | 实现与当前行为证据 |
| P2-014 | 持久化未完成闭包frontier | done | V07, V13 | M1 | 实现与当前行为证据 |
| P2-015 | 将freshness传到查询与MCP | done | V07, V18 | M1 | 实现与当前行为证据 |
| P2-016 | 多语言mutation差分扩充 | done | V07 | M1 | 实现与当前行为证据 |
| P2-017 | 缓存压实和依赖存储性能 | done | V20 | M1 | 实现与当前行为证据 |
| P2-018 | 删除JS专属dirty入口假设 | done | V06, V07 | M1 | 实现与当前行为证据 |
| P2-019 | 文档与支持范围对齐 | done | V21 | M1 | 实现与当前行为证据 |
| P2-020 | P2验收与跨语言基线关闭 | done | V06, V07, V19, V20 | M1 | 实现与当前行为证据 |
| P3-001 | 定义不可变ProjectModel | done | V08 | M1 | 实现与当前行为证据 |
| P3-002 | 共享目录与配置发现 | done | V08, V20 | M1 | 实现与当前行为证据 |
| P3-003 | 配置缓存与继承DAG | done | V08 | M1 | 实现与当前行为证据 |
| P3-004 | TS最近配置与JSONC | done | V08 | M1 | 实现与当前行为证据 |
| P3-005 | TS paths/baseUrl与条件 | done | V08 | M1 | 实现与当前行为证据 |
| P3-006 | TS workspace与package入口 | done | V08 | M1 | 实现与当前行为证据 |
| P3-007 | TS扩展名/目录/ESM映射 | done | V08 | M1 | 实现与当前行为证据 |
| P3-008 | Rust workspace旧能力迁移 | done | V08 | M1 | 实现与当前行为证据 |
| P3-009 | Rust模块路径与cfg | done | V08 | M1 | 实现与当前行为证据 |
| P3-010 | Python包与src layout | done | V08 | M1 | 实现与当前行为证据 |
| P3-011 | Go module/workspace | done | V08 | M1 | 实现与当前行为证据 |
| P3-012 | 统一模块解析返回值 | done | V08 | M1 | 实现与当前行为证据 |
| P3-013 | 配置-only变更进入dirty | done | V07, V08 | M1 | 实现与当前行为证据 |
| P3-014 | watcher重命名与溢出对账 | done | V07, V08 | M1 | 实现与当前行为证据 |
| P3-015 | 模块解析夹具与外部oracle | done | V08 | M1 | 实现与当前行为证据 |
| P3-016 | 配置深度与文件访问防护 | done | V08 | M1 | 实现与当前行为证据 |
| P3-017 | import规模与配置缓存成本 | done | V20 | M1 | 实现与当前行为证据 |
| P3-018 | 清理旧路径解析重复逻辑 | done | V08, V21 | M1 | 实现与当前行为证据 |
| P3-019 | 更新语言矩阵和配置契约 | done | V18, V21 | M1 | 实现与当前行为证据 |
| P3-020 | P3模块解析验收 | done | V07, V08, V19, V20 | M1 | 实现与当前行为证据 |
| P4-001 | 定义SourceSnapshot与byte span | done | V09 | M2 | 实现与当前行为证据 |
| P4-002 | 提取紧凑AST边界 | done | V09, V20 | M2 | 实现与当前行为证据 |
| P4-003 | 方法/类的层级切块 | done | V09 | M2 | 实现与当前行为证据 |
| P4-004 | 长函数语句块分割 | done | V09 | M2 | 实现与当前行为证据 |
| P4-005 | 注释签名关联 | done | V09 | M2 | 实现与当前行为证据 |
| P4-006 | 小碎片合并与重复区间消除 | done | V09, V12 | M2 | 实现与当前行为证据 |
| P4-007 | 统一gap与尾部预算 | done | V09 | M2 | 实现与当前行为证据 |
| P4-008 | 长行与不可解析文件fallback | done | V09 | M2 | 实现与当前行为证据 |
| P4-009 | token与字符/字节预算 | done | V09 | M2 | 实现与当前行为证据 |
| P4-010 | 分离源码文本与embedding渲染 | done | V09, V10 | M2 | 实现与当前行为证据 |
| P4-011 | 定义DocKey与DocVersion | done | V10 | M2 | 实现与当前行为证据 |
| P4-012 | 文档清单存储与delta | done | V10, V13 | M2 | 实现与当前行为证据 |
| P4-013 | 接入构建与staging | done | V09, V10 | M2 | 实现与当前行为证据 |
| P4-014 | FTS与chunk兼容迁移 | done | V10, V18 | M2 | 实现与当前行为证据 |
| P4-015 | 源码hydration一致性 | done | V09, V12 | M2 | 实现与当前行为证据 |
| P4-016 | 身份扰动mutation tests | done | V10 | M2 | 实现与当前行为证据 |
| P4-017 | 切块检索质量消融 | done | V19 | M2 | 实现与当前行为证据 |
| P4-018 | 切块内存和构建成本 | done | V20 | M2 | 实现与当前行为证据 |
| P4-019 | 移除旧切块双实现并更新文档 | done | V09, V21 | M2 | 实现与当前行为证据 |
| P4-020 | P4文档与源码证据验收 | done | V09, V10, V19, V20 | M2 | 实现与当前行为证据 |
| P5-001 | 统一Candidate与LaneOutcome | done | V11, V18 | M2 | 实现与当前行为证据 |
| P5-002 | 收拢本地lane注册 | done | V05, V11 | M2 | 实现与当前行为证据 |
| P5-003 | 独立exact-symbol召回 | done | V05, V19 | M2 | 实现与当前行为证据 |
| P5-004 | 独立path召回 | done | V05, V19 | M2 | 实现与当前行为证据 |
| P5-005 | RRF与trace通用化 | done | V03, V11 | M2 | 实现与当前行为证据 |
| P5-006 | QueryPolicy与预算规划 | done | V11, V18 | M2 | 实现与当前行为证据 |
| P5-007 | 提取无锁QueryHandle | done | V11 | M2 | 实现与当前行为证据 |
| P5-008 | 建立有界执行器 | done | V11, V20 | M2 | 实现与当前行为证据 |
| P5-009 | 统一deadline与取消 | done | V11 | M2 | 实现与当前行为证据 |
| P5-010 | 可选语义端口和fake适配 | done | V11, V18 | M2 | 实现与当前行为证据 |
| P5-011 | generation读面与缓存守卫 | done | V11, V13 | M2 | 实现与当前行为证据 |
| P5-012 | selector任务覆盖模型 | done | V12, V19 | M2 | 实现与当前行为证据 |
| P5-013 | 重复/重叠证据抑制 | done | V12 | M2 | 实现与当前行为证据 |
| P5-014 | 结构化BudgetPacker | done | V12, V18 | M2 | 实现与当前行为证据 |
| P5-015 | EvidenceHydrator与最终验证 | done | V09, V11, V12 | M2 | 实现与当前行为证据 |
| P5-016 | 诊断与能力状态收口 | in_progress | V18 | M2 | 实现与当前行为证据 |
| P5-017 | 项目驱逐与查询资源生命周期 | in_progress | V11, V20 | M2 | 实现与当前行为证据 |
| P5-018 | MCP旧新契约和文档一体迁移 | in_progress | V18, V21 | M2 | 实现与当前行为证据 |
| P5-019 | 查询质量/成本/并发消融 | todo | V19, V20 | M2 | 实现与当前行为证据 |
| P5-020 | P5本地增强版验收 | todo | V11, V12, V18, V19, V20 | M2 | 实现与当前行为证据 |
| P6-001 | 正式确定单库边界修订ADR | todo | V21 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-002 | 新增可选cc-semantic骨架 | todo | V18, V21 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-003 | 冻结编码空间与输入规范 | todo | V10, V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-004 | 扩展类型化write effects | todo | V13 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-005 | 新表与schema初始化 | todo | V13, V21 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-006 | 源码事务原子写outbox | todo | V13, V14 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-007 | 实现claim与lease fencing | todo | V14 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-008 | 构建内容寻址artifact cache | todo | V10, V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-009 | 实现deterministic fake provider | todo | V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-010 | 实现filtered exact向量backend | todo | V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-011 | artifact到manifest发布CAS | todo | V14 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-012 | 覆盖率与semantic epoch | todo | V13, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-013 | worker资源与连续编辑合并 | todo | V14, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-014 | 换库incarnation与缓存重用 | todo | V13, V17 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-015 | 崩溃恢复扫描 | todo | V17 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-016 | GC与发布协调 | todo | V17 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-017 | model space切换规划 | todo | V16, V17 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-018 | cache缺失/损坏降级 | todo | V17, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-019 | 更新存储/恢复/配置文档 | todo | V21 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P6-020 | P6无网络语义底座验收 | todo | V13, V14, V16, V17, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-001 | 实现OpenAI-compatible provider适配 | todo | V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-002 | 模型参数与能力验证 | todo | V15, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-003 | 真实输入尺寸与批次规划 | todo | V09, V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-004 | 响应强校验 | todo | V15, V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-005 | 全局与项目并发限流 | todo | V15, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-006 | 有界重试与断路器 | todo | V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-007 | 代码外发与凭据政策 | todo | V15, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-008 | 费用与不确定尝试收据 | todo | V15, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-009 | 查询编码与缓存 | todo | V11, V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-010 | dense召回端口接线 | todo | V11, V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-011 | dense范围与hydrate守卫 | todo | V05, V16 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-012 | 融合与部分覆盖语义 | todo | V11, V19 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-013 | 查询总deadline和模型故障退化 | todo | V11, V15 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-014 | 配置/status/MCP全链贯通 | todo | V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-015 | 后台回填与前台查询竞争测试 | todo | V14, V17, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-016 | fake全故障矩阵回归 | todo | V14, V15, V17, V18 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-017 | 离线默认包和未启用测试 | todo | V18, V21 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-018 | 受授权的真实provider小集认证 | todo | V15, V19, V20 | M4-semantic | 明确条件决策或授权真实证明 |
| P7-019 | 本地加dense的质量/成本消融 | todo | V19, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P7-020 | P7语义闭环与发布范围验收 | todo | V15, V16, V17, V18, V19, V20 | M3-engineering, M4-semantic | 实现与当前行为证据 |
| P8-001 | 锁定release候选与证据输入 | todo | V01, V02 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-002 | 完成真实多仓native语料认证 | todo | V02, V19 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-003 | 运行外部兼容套件 | todo | V03, V04, V19 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-004 | 封存holdout与反过拟合检查 | todo | V19 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-005 | 完整规模1k到100k | todo | V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-006 | 增量规模与fanout曲线 | todo | V07, V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-007 | 多并发与混合负载 | todo | V11, V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-008 | 冷建/重开/热查分层 | todo | V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-009 | 内存/磁盘/费用总账 | todo | V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-010 | 长时soak与连续修改 | todo | V07, V17, V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-011 | 端到端故障与恢复认证 | todo | V14, V17, V18 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-012 | MSRV与平台冷构建矩阵 | todo | V01, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-013 | 指标/门槛与失败退出最终认证 | todo | V03, V04, V20 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-014 | 可选LLM评审旁证流程 | todo | V19 | optional-judge | 明确条件决策或授权真实证明 |
| P8-015 | 真实语义效果发布认证 | todo | V15, V19, V20 | M4-semantic | 明确条件决策或授权真实证明 |
| P8-016 | 数据库/配置/包回滚演练 | todo | V13, V17, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-017 | 删除临时兼容和重复模块 | todo | V18, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-018 | 文档事实与安装契约同步 | todo | V18, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-019 | 发布工件与完整报告归档 | todo | V01, V04, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P8-020 | P8发布评审与遗留关闭 | todo | V18, V19, V20, V21 | M4-local, M4-semantic | 实现与当前行为证据 |
| P9-001 | ANN收益与选型决策 | todo | V22 | optional | 明确条件决策或授权真实证明 |
| P9-002 | ANN适配与过滤更新闭环 | todo | V16, V22 | optional | 明确条件决策或授权真实证明 |
| P9-003 | ANN量化和规模认证 | todo | V20, V22 | optional | 明确条件决策或授权真实证明 |
| P9-004 | ANN决策收口 | todo | V21, V22 | optional | 明确条件决策或授权真实证明 |
| P9-005 | LSP按需生命周期 | todo | V22 | optional | 明确条件决策或授权真实证明 |
| P9-006 | LSP源码快照和冲突表达 | todo | V09, V22 | optional | 明确条件决策或授权真实证明 |
| P9-007 | LSP精度/内存收益认证 | todo | V20, V22 | optional | 明确条件决策或授权真实证明 |
| P9-008 | LSP工具面和安装边界收口 | todo | V18, V21, V22 | optional | 明确条件决策或授权真实证明 |
| P9-009 | 可选rerank端口与降级 | todo | V11, V22 | optional | 明确条件决策或授权真实证明 |
| P9-010 | rerank盲测与费用消融 | todo | V19, V20, V22 | optional | 明确条件决策或授权真实证明 |
| P9-011 | 按需查询分解收益决策 | todo | V19, V22 | optional | 明确条件决策或授权真实证明 |
| P9-012 | P9可选增强逐项结项 | todo | V21, V22 | optional | 明确条件决策或授权真实证明 |
