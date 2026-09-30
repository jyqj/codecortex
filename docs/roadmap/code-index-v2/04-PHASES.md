# 04｜分阶段实施、批次与里程碑

> 115done、77todo；P5为15/20，P5-C五项完成本地实现/契约验收，下一P5-016（P5-D）。最新见[P5-C-COMPLETION.md](P5-C-COMPLETION.md)与[P5-C-GATE.json](P5-C-GATE.json)，旧P5-C-IMPLEMENTATION/PARTIAL-GATE/PROGRESS保留历史范围与失败。固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。

## 1. 主线及交付边界

```text
P0 基线/benchmark
  -> P1 排序和范围
  -> P2 公共表面/dirty 正确性
  -> P3 项目模块规则
  -> P4 源码切块/文档版本
  -> P5 查询执行/证据装配        [M2：完整本地增强版]
  -> P6 语义存储/任务/publish
  -> P7 provider/dense          [M3：可选语义工程闭环]
  -> P8 全量认证                [M4-local / M4-semantic 分别声明]
  -> P9 可选 ANN/LSP/rerank      [各自收益门，不阻塞主线发布]
```

这是默认集成顺序，不意味着所有工作只能串行。P0后可以先准备任何阶段的只读设计、gold和独立fixtures；真实引擎修改必须满足 tasks.json 中的数据/契约依赖。P8的本地认证profile可在P5之后提前运行，但P8整合gate在主线完成后检查实际选择的发布范围。无key/未批准付费不能阻止M2或M4-local；没有live证据不能发布“已证明真实模型收益”的结论。条件任务未执行必须明确blocked/deferred以及影响，不算成功。

## 2. 每批标准产物

每批只解决一组可评审的行为；提交业务代码、必要schema/配置、回归夹具、文档与run-id报告摘要。原始大工件在独立artifacts/benchmarks目录，Git只纳入小型金样/锁文件和需长期维护的报告索引。每批开头固定基线并检查工作区；收尾更新任务证据、已知失败、下一批入口。没有用户授权不推送/PR/合并；收到后续实施指令也不自动获得额外发布权限。

## 3. 阶段卡片

### P0｜基线与 benchmark 底座（20项）

入口：当前main@4514630基线复核，保留本轮规划文档与无关改动。目标：先建尺子、看见已知缺陷，不在本阶段修所有业务错误。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P0-A | 001–005 | 冷构建/SDK证据、旧测试、关键缺陷夹具、benchmark schema |
| P0-B | 006–010 | OCE importer、兼容/原生scorer、严格输入锁、适配接口 |
| P0-C | 011–015 | 真实MCP子进程、可选HTTP适配、readiness、raw报告、资源采样 |
| P0-D | 016–020 | 增量oracle、首批gold/holdout规则、CLI/基线、G0与任务维护 |

退出：V01–V04基础可执行；首份有效小基线和known-failure表；scoring version和输入锁冻结。SDK仍阻塞时不能标冷构建通过，但独立scorer/schema工作继续。Benchmark是生产修复的前置，不推迟到P8。

### P1｜检索正确性和范围（20项）

入口：G0；需要P0已建立的BM25/scope失败夹具。目标：不接模型先修已确认的排序方向与召回边界。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P1-A | 001–005 | BM25红绿回归、单调修复、HardScope/SoftHints与DSL交集 |
| P1-B | 006–010 | 路径安全、lexical/grep/graph范围、精确结果回归 |
| P1-C | 011–015 | 缓存key/解释/MCP兼容、中英质量和两项修复消融 |
| P1-D | 016–020 | 扫描成本/并发、删除旧过滤分支、文档与G1 |

退出：V05全过；旧mode不变；预选外合法结果可见、hard scope越界为0；质量收益与扫描成本配对报告。不能通过删除范围限制来修漏召回。

### P2｜公共表面与增量正确性（20项）

入口：G1、增量oracle骨架。目标：结构事实不因语言导出字段缺失或目录缓存历史而失真。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P2-A | 001–005 | PublicSurface/指纹/存储，JS/TS、Rust、Python表面 |
| P2-B | 006–010 | Go和其他语言保守能力、ResolutionOutcome、稳定消歧、负向依赖 |
| P2-C | 011–015 | DirtyPlanner、环收敛、完整reload、durable frontier、freshness |
| P2-D | 016–020 | mutation扩充、目录/依赖成本、旧JS专属路径清理、G2 |

退出：V06/V07；支持范围内incremental/full一致，缓存warm/cold不改变结果；预算欠账能恢复；未知不等于空接口。Java/C++等未覆盖语义如实列capability，不能用几十种语言标识符掩盖支持深度。

### P3｜项目模型和模块解析（20项）

入口：G2，可复用新的依赖失效。目标：配置和包规则成为版本化索引输入，消除逐import重复磁盘探测。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P3-A | 001–005 | ProjectModel、共享目录、配置DAG/缓存、TS作用域和别名 |
| P3-B | 006–010 | TS workspace/入口/ESM、Rust迁移与模块、Python布局 |
| P3-C | 011–015 | Go、统一结果、config-only invalidation、watcher对账、独立oracle |
| P3-D | 016–020 | 配置安全边界、规模profile、旧resolver清理、文档和G3 |

退出：V08和config-only V07；支持模式有清晰矩阵；Astrolabe中的启发式不伪装成编译器真理；旧Rust workspace能力无回退。

### P4｜源码切块、身份和文档版本（20项）

入口：G3，模块/源码模型稳定。目标：原文正确、边界有意义、身份足以承载异步语义产物。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P4-A | 001–005 | SourceSnapshot/byte span、单次AST边界、类/方法/长函数与注释 |
| P4-B | 006–010 | 小块合并、gap/长行fallback、预算和source/embed text分离 |
| P4-C | 011–015 | DocKey/Version、清单delta、full/inc接线、FTS兼容、证据hydrate |
| P4-D | 016–020 | 身份扰动、切块质量/资源消融、旧路径清理和G4 |

退出：V09/V10；顶部插入/rename不会错绑新旧向量；每个片段可由原始bytes核验；不用新chunk_id作gold以避免自证。原文/行号正确性优先于token省一点。当前G4已在本地macOS arm64声明范围完成：身份扰动、固定检索器消融、局部资源成本和单一生产chunk核心通过；公开holdout、100k/peak RSS/tail、跨平台及发行仍留P8。

### P5｜查询执行与证据装配（20项）

入口：G4。目标：把现有同步lane演进为可安全接入异步能力的边界，同时交付更完整本地上下文。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P5-A | 001–005 | Candidate/LaneOutcome、本地lane迁移、exact/path独立召回、RRF |
| P5-B | 006–010 | QueryPolicy、无锁QueryHandle、有界执行、取消/deadline、semantic fake port |
| P5-C | 011–015 | generation缓存、facet selector、重复抑制、BudgetPacker和hydrate |
| P5-D | 016–020 | 能力状态、资源驱逐、wire迁移、质量/并发消融和G5 |

退出：V11/V12/V18；没有网络等待持锁；输出仍是完整结构化对象；首块/metadata也受预算；M2可独立作为默认离线交付版。

### P6｜语义持久化与发布底座（20项）

入口：G5。目标：先用fake证明版本、两存储协调、恢复和exact向量正确性，而非先碰云API。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P6-A | 001–005 | 正式ADR、可选crate、编码规格、write effects与schema |
| P6-B | 006–010 | outbox、lease fencing、artifact cache、fake provider、filtered exact |
| P6-C | 011–015 | publish CAS、coverage/epoch、worker、公用cache换库、crash恢复 |
| P6-D | 016–020 | GC协调、空间切换、损坏降级、存储文档和G6 |

退出：V13/V14/V16/V17；删除不复活、过期lease不发布、索引重建不清掉昂贵缓存；默认不开启仍无第二库。不能假装两库和远端请求有跨系统原子性。

### P7｜provider 与 dense 端到端（20项）

入口：G6；远程调用需独立opt-in及预算。目标：实现可选语义实际能力，允许故障时本地继续。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P7-A | 001–005 | HTTP provider、模型参数、输入批次、响应校验、共享限流 |
| P7-B | 006–010 | 重试/断路器、隐私/凭据、费用、query编码、dense port接线 |
| P7-C | 011–015 | dense过滤/融合/部分覆盖、deadline/status/MCP、竞争资源测试 |
| P7-D | 016–020 | fake故障、默认离线、条件live测试、质量/费用消融、G7 |

退出：工程闭环必须fake全过；live效果认证另栏，未授权不执行、不称已验证。M3-engineering与M4-semantic效果声明明确区分；auto降级不缓存成完整语义搜索。

### P8｜规模、质量与发行认证（20项）

入口：主线相应能力冻结；本地子profile可从M2提前准备。目标：稳定证据和可回滚发行，不是首次建立benchmark。

| Batch | Tasks | 集成产物 |
|---|---|---|
| P8-A | 001–005 | release锁、多仓gold、兼容套件、holdout、1k–100k |
| P8-B | 006–010 | 增量曲线、并发、冷热分层、资源费用、soak |
| P8-C | 011–015 | 故障恢复、MSRV/平台、gate退出、条件LLM旁证/真实语义认证 |
| P8-D | 016–020 | rollback、技术债清理、文档事实、发行工件、G8 |

退出：选择发布的profile blocker=0；raw可复算、CI非零失败可靠；不拿旧暖缓存bench证明当前p95，不用不同输入/预算对外比较。未启用的P9不阻塞G8。

### P9｜独立收益门的可选增强（12项）

入口：主线证据足够识别瓶颈，不因功能清单焦虑引入新依赖。

| Batch | Tasks | 决策/集成产物 |
|---|---|---|
| P9-A | 001–004 | ANN选型→实现→对exact/过滤质量认证→留一种backend或deferred |
| P9-B | 005–008 | 按需LSP生命周期→快照/冲突→精度资源认证→工具面收口 |
| P9-C | 009–012 | rerank接口→holdout费用消融→查询分解决策→逐项结项 |

退出：每项有enable或deferred理由；决策完成≠实现完成。ANN未通过过滤recall门不能替换exact；LSP不替代默认静态索引；LLM不生成未经验证事实。

## 4. 并行规则

P0的gold复核、平台环境诊断和schema设计可独立推进；scorer依赖schema，正式runner依赖scorer/锁/适配契约。P2不同语言surface在共享模型冻结后可开独立工作树，但集成者负责同一model/schema变更。P3各语言模块在ProjectModel端口固定后同理。P4解析器边界与chunk算法先统一SourceSpan；P5 query锁重构和selector不得同时大改同一个engine文件。

P6的持久化/publish/GC是同一一致性事务边界，不能由多个无协调分支各自定义状态机。P7 provider与bench HTTP stub可并行，但生产组装必须由一处维护。P8 dataset审阅与性能采样要隔离环境，不能在性能run期间重编译或跑其他大任务。

跨批可先写fixture或文档，不代表可越过gate部署。共享文件冲突以单一owner/小patch整合，不通过复制模块各做一套解决。

## 5. 状态、阻塞与停止条件

状态仅 todo/in_progress/blocked/done/deferred。只有current SHA证据、验收、评审和回滚齐备才done。conditional live/可选增强如不具备授权条件可deferred，但相关live发布scope继续不满足；报告不能隐藏。

发现source identity、scope、schema或publish设计出现根本冲突时，停下对应依赖链，记录精确反例并修正规格；与之无关的gold/schema/只读对照可以继续。不能为了维持排期把正确性红线改成performance例外。
