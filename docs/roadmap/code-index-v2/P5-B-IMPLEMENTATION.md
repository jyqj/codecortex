# P5-B 实施与验收：查询策略、拥有资源的句柄与有界执行

日期：2026-09-29。P5-006～010在本地声明范围完成；P5-C/D、G5/M2、完整检索门禁及发行认证未完成。

Git HEAD仍为`4514630dcd26481cf6dbc2aff38824ed71ef06da`；当前接受工作树为`a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9`，603文件、6497809字节。未提交、推送、创建PR或合并。

## 1. 五项任务的实际接线

| 任务 | 实际成果 |
|---|---|
| P5-006 | QueryPolicy统一local/auto/semantic、intent召回职责与总/子预算；配置和intent进入缓存域，未配置auto等价local，显式semantic报不可用。既有Partial/S11不隐藏，无答案及selector完整校准仍由P5后续承担。 |
| P5-007 | Arc-owned QueryHandle接入MCP search/context，短锁只复制捕获的项目/DB/engine/services；慢fake期间index/status及单读连接可用。纯检索保活idle close；附加旧图源码扩展仍短锁并校验实例，未声称所有图工具无锁。 |
| P5-008 | 进程共享CPU4/queue32、async8/queue32，固定4线程本地lane池；先准入再spawn，运行闭包持permit至真实退出。3轮64请求各36准入/28拒绝，peak<=4且恢复零占用；不称全进程线程或吞吐认证。 |
| P5-009 | 查询准入起总deadline贯穿等待、端口、三次epoch重试、CPU组装及JSON转换；MCP取消和Future Drop下传控制，结果/正文缓存发布受短栅栏保护。不可中断SQL不伪称立即终止；冷项目路由阶段不纳入此声明。 |
| P5-010 | 无provider依赖SemanticRecall端口/fake接入现有rank-only融合；scope/version/span/NaN/重复/假exact和过多候选拒收，timeout/partial/error/panic显式降级，语义结果不进普通结果缓存。无真实模型/vector认证。 |

详细配置、资源所有权和非承诺边界见[QUERY_EXECUTION.md](../../internals/QUERY_EXECUTION.md)。没有引入第二索引引擎或cc-semantic crate；原SearchOps委托同一QueryHandle实现，原文证据和rank-only融合保持单一来源。

## 2. 本轮纠正的问题

接线复查补回了旧resolution_freshness投影，异步前后观测固定使用捕获的数据库，避免await后指向另一个活动项目。项目切换隔离可选端口；附加符号源码扩展校验实例，失效明确报错。

取消不再等价于丢弃JoinHandle：CPU工作持有准入/执行permit直到真实退出；等待队列取消立即归还准入。取消与普通结果/正文提示缓存插入线性化，过期查询不能通过暖缓存返回。future构造或poll panic均保留error诊断，真实坏文档不变成空成功。

final-v1因JSON转换仍位于async线程而显式停止，原记录保留为未验收。final-v2纳入有界CPU转换后，全仓发现context_graph_enriched旧断言失败：过长策略说明挤掉图诊断。新增独立红测复现后，裁剪优先压缩policy详情并标明省略，不改原断言或放宽预算。final-v3在这些修复后重新冻结；旧失败与停止记录不覆盖。

## 3. 当前冻结验证

| 工具链 | Workspace/doctest | HTTP | P5-B/P5-A边界与scorer | MCP取消传输 | 真实stdio | watcher |
|---|---|---|---|---|---|
| stable | 1735 passed / 55 ignored | 230 passed / 48 ignored | 47 passed / 1 ignored | 1 passed / 0 ignored | 23 passed / 0 ignored | 17 passed / 0 ignored |
| 1.95.0 | 1735 passed / 55 ignored | 230 passed / 48 ignored | 47 passed / 1 ignored | 1 passed / 0 ignored | 23 passed / 0 ignored | 17 passed / 0 ignored |

两工具链严格Clippy、格式、两项架构守卫均通过。39条最终命令日志哈希逐项核验；所有测试组内部失败为0，ignored不算通过。产品和评测器在构建后复制到attempt专属binaries目录，后续Cargo特性切换不会替换实测对象。

协议取消测试通过真实rmcp双工传输发送notifications/cancelled，慢fake时status仍能返回，search/context请求取消后fake Future被释放；实际stdio测试另启动构建出的codecortex子进程。fake不是线上provider或模型测试。

## 4. 固定回归及资源观察

从逐文件校验的P5-A接受归档重建对照程序，与P5-B使用同一public-v5 runner、同一51题、每题3次，总306请求。逐题Top-1/nDCG无负差分，源码无invalid hit，回放一致；每套数据的完整性失败原因计数和状态计数与P5-A一致。原Partial及S11门禁仍未通过，没有修改gold或评分公式。

| 数据集 | 版本 | Top-1 | nDCG@10 | 请求 | 状态 |
|---|---|---:|---:|---:|---|
| source | p5a | 1.000000 | 1.000000 | 42 | {"partial": 42} |
| source | p5b | 1.000000 | 1.000000 | 42 | {"partial": 42} |
| smoke | p5a | 0.700000 | 0.700000 | 33 | {"success": 24, "no_match": 9} |
| smoke | p5b | 0.700000 | 0.700000 | 33 | {"success": 24, "no_match": 9} |
| exact | p5a | 1.000000 | 1.000000 | 24 | {"success": 24} |
| exact | p5b | 1.000000 | 1.000000 | 24 | {"success": 24} |
| intents | p5a | 0.625000 | 0.656250 | 54 | {"success": 24, "no_match": 18, "partial": 12} |
| intents | p5b | 0.625000 | 0.656250 | 54 | {"success": 24, "no_match": 18, "partial": 12} |

Release使用生产相同额度做3轮受控64请求，总192请求。每轮36准入、28容量拒绝，实际CPU峰值不超过4；工作退出后占用回到0且再次准入成功。另复跑60次旧局部搜索成本；所有原始样本保留，没有将这些同步屏障样本表述为开放负载吞吐、峰值RSS或p95/p99。

## 5. 交付、回滚与后续

完整证据目录：`artifacts/benchmarks/p5b-20260929-query-execution/final-v3`。源码归档SHA-256：`251c846f67024d422fd0c6128d90392e000e32158f8f312c8341b44876641bb8`。

数据库schema21不变；local配置和未注入端口不触网。源码恢复依据是保留的P5-A归档和本次文件清单；没有对日常索引执行回滚演练或重建。既有巨大未提交工作树和历史失败工件完整保留。

任务清单110done/82todo，P5为10/20。下一批P5-C从P5-011开始，复用现有控制/句柄/有限重试，完善持久化incarnation缓存读面、任务覆盖selector、同版本跨度去重、结构化BudgetPacker及EvidenceHydrator。完整性与无答案校准仍须进入P5-019/020整体验收，不因P5-B完成而消失。

## 6. 接续审阅补充边界

代码复核和冻结成员核对见source-review.json。没有外部传入QueryControl时，取得项目配置之前使用600秒临时准入上限；配置可读后才从最初起点收紧为项目预算。因此不能声称在尚未知晓项目配置、又被已有CodeIndex锁阻塞时，也能按更短项目期限准时返回。已有查询控制则从准入前直接生效。此边界与冷项目定位、不可中断SQL的限制分别记录，不以有限重试或Future取消掩盖。
