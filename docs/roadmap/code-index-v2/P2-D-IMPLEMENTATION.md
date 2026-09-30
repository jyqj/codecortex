# P2-D｜连续变更、成本与 G2 本地收口

日期：2026-09-28。工作区基线 main@4514630dcd26481cf6dbc2aff38824ed71ef06da，保留所有先前未提交成果。P2-C 的504个覆盖文件在入口无漂移；本批入口归档532个文件（包含路线图）。最终验证按统一清单冻结512个源码/测试/配置/文档/批次脚本，路线图状态另行检查。两个清单的覆盖范围不同，不以文件数量差推断删除。

本批 P2-016–020 完成；累计60 done、132 todo，P2 20/20。**G2 在已声明静态分析子集与本地 macOS arm64 范围通过，不是完整编译器、全部平台、语义检索或产品发行认证。** 下一批 P3-A（P3-001–005）。

最终摘要：`26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6`。原始证据均在 `artifacts/benchmarks/p2d-20260928-ja0e/`。

## 1. 实施结果

### P2-016：独立事实与可重放失败

新增 `benchmark/mutation_case.rs` 和 `cc-eval mutation-case`。一个 case 自带初始文件、写入/删除/重命名阶段、重启标记、预算续跑要求以及手写关系谓词。两份隔离项目分别实际增量和全量构建，比较十四表，保留目标ID/UID、策略、置信度、歧义、semantic_edges、dispatch_sites与frontier；不执行被索引源码。

Python、TS、Rust、Go的12条固定变更序列覆盖108个检查点；另有交错未完成预算、rebase/重启、Unknown与输入错误测试。冻结后的同一CLI追加4份可见性变更输入，11个检查点全部通过，case hash、命令和结果保存在 `visibility/summary.json`。这些案例检查声明可见性证据、关系和full/inc一致性，不声称Rust/TS编译器访问控制已经实现。

有效失败自动做有预算的阶段删除缩减，只有相同首个失败签名能接受；非法候选、解析/基础设施错误不冒充原失败。最终最小候选再次独立重放，报告是否达到阶段删除最小性；不声称全局最小源码。原case、结果与失败不覆盖。故意设置count=777的自测只认证缩减器，不是产品缺陷。

新增真实MCP子进程用例，四语言分别重命名提供者并重启进程，再用公开graph_query断言实际目标；同时断言fake.ts没有来自声明/字符串的CALLS。既有P2-A/B/C与P1/P0协议回归保留。

### 独立真值发现并修复的缺陷

**P2D-JSTS-CALL-01**：JS/TS旧正则补充路径会为声明、注释、字符串生成调用/引用。删除该路径，调用来自真实AST，引用由真实调用和语法节点投影，目标交给既有resolver，不能仅凭同名第一项标parser_exact。AST节点归属避免重复访问；保留await参数、模板插值、嵌套函数及链式接收者调用。成员调用锚点改为实际callee token，动态可调用表达式明确Unsupported。

**P2D-WAL-01**：1k无关文件背景下连续24次增量后再全量重建，使用自定义数据库文件名会报database disk image is malformed。原因是旧WAL/SHM路径通过替换扩展名生成。改为向完整数据库文件名追加后缀；同一失败回归已通过。没有将此修复扩展宣称为独立外部写进程的安全换库证明。

红测试及开发修正全保留，分类见 `evidence-classification.json`。最初的同名碰撞夹具用了不同参数数量，fuzzy_arg_count可区分它们；改成同等签名才测试歧义。这是夹具作者修正，不伪报resolver缺陷。测试过程中引入并修复的AST重复访问也不包装为历史产品问题。

### P2-017：有界依赖输出与真实工作量

正向绑定和解析依赖读取都有预算窗口、去重及溢出见证；原始失效原因仍保存在frontier，不将窗口误当完整剩余集合。`dirty_plan.dependency_sql`记录相关SQL语句、VM步数、返回行、扫描/排序计数，而非虚构总体I/O成本。

600个相关依赖，背景无关文件100/1000/5000时：未处理前缀下名称依赖查询为71 VM steps，正向查询为3717；三种背景规模相同。排除598个已完成文件时分别为12026和15672。说明**无关文件没有增加这些语句工作量，但相关/已完成前缀仍有成本**，不是O(1)或无限规模证明。32轮替换后依赖行数保持600，删除相关文件后归零。

保留单一catalog压实策略。160轮、每轮64符号的名字/UID桶回归触发两次压实，最大4224槽位；另一个真实1k背景/24次单文件构建测试确认每次只parse一个文件并复用健康的目录缓存，不以单元数据结构测试冒充真实构建。

### P2-018/019：单一指纹与文档

删除cc-index和SQL中两套旧export_name散列公式。兼容方法get_export_fingerprint(s)统一委托PublicSurface：KnownEmpty具有版本化指纹，Unknown/缺失不具有可比较指纹。语言展示元数据保留，不另建失效规则。

更新INDEXING、STORAGE、LANGUAGES、PUBLIC_SURFACE、RESOLUTION_DEPENDENCIES、恢复说明及benchmark入口；移除“同分可随缓存历史选不同赢家”与类型“最后存活者”的旧文档承诺。新增 `docs/internals/INCREMENTAL_VERIFICATION.md` 说明测试、成本、恢复与语言边界。

schema14必须隔离重建旧缓存以清除旧JS/TS事实和调用地址；没有打开或清空开发者日常索引。历史二进制、原始输入及先前阶段报告保留。

## 2. 冻结验证

`final/validation.json`中的24条命令均exit0；源码文件内容与集合、查询文件及历史冻结输入复核不变。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 严格Clippy：workspace/all-targets/HTTP特性 | 通过 | 通过 |
| Workspace含doctest | 1531 passed / 33 ignored | 1531 passed / 33 ignored |
| eval-http | 154 passed / 27 ignored | 154 passed / 27 ignored |
| 显式真实MCP | 14 passed | 14 passed |
| watcher | 11 passed | 11 passed |
| 查询generation及两次并发重复 | 各通过 | 各通过 |

stable另外显式执行release增量成本与release依赖工作量，各1项通过；ignored不计通过，不同命令有重叠不能相加。复用target目录的执行不是全新冷target或跨平台认证；CI配置已接入新协议用例，但未运行远端Actions。

## 3. Release机制成本

170个实际计时观测，见 `cost-summary.json` 和 `observations/final-release-cost/p2d-release-cost.json`。每个规模为无关文件数，另加25消费者和1提供者。表中为本轮中位毫秒，非尾延迟：

| 无关文件 | full（1样本） | no-op（12样本） | body（12样本） | API续跑单tick（60样本） |
|---:|---:|---:|---:|---:|
| 1000 | 200.342 | 25.094 | 26.471 | 29.504 |
| 5000 | 742.186 | 33.823 | 37.292 | 41.590 |

每次API更新每tick最多5个依赖，有限扇出可闭合；body不额外提升依赖；no-op没有反向依赖SQL。恢复基线的构建也实际执行，但不偷偷计入170条计时样本。没有此前同口径release配对，不宣称速度提升；没有认证RSS、100k、并发尾延迟。frontier payload重写和完成集排除仍随相关状态大小增长，保留200k组合条目/16MiB失败关闭上限。

## 4. 固定输入检索与工具契约

P2-C与P2-D分别对4套、合计51道固定题执行三次，共306请求；每题Top-1/nDCG变化均0，8组metrics/slices/cost回放一致。源码子集0.857143/0.937924，精确定位1/1，双语意图0.625/0.65625，smoke正例0.7/0.7。S11无答案在两版仍exit1；三份正式比较inconclusive，不伪称质量全面通过或更快。

两版原Python/Rust签名mutation都以各自14表oracle通过；14工具输入契约一致。新增CLI独立case9检查点通过。

配对启动脚本末项把fixture写成final-workspace而非final-stable-workspace，首次进程exit2；此前306请求、4个原mutation和旧契约均已完成。冻结脚本及原失败不改，用正确路径另行重放并从原收据逐项复核；详见 `paired/recovery.json`。`paired/summary.json`明确标completed_with_recorded_cli_path_recovery，而非把首次脚本失败改成成功。

当前源码rolling题库中的历史R09查询提到已删除helper；本批只刷新其source lock，不改gold，也不以它认证当前源码质量。正式配对使用历史冻结源码（其中该helper存在），所以配对有效；后续更新rolling数据集时应重新审核此题，而非直接修改答案求过关。

## 5. G2结论与后续边界

B03：跨语言声明接口、统一指纹和原签名回归闭合。B04：缓存/冷加载规范事实一致，同证据保持歧义，连续重启/增删及缓存命中回归保持。B14：名称桶、缺失路径、包/配置事件、删除与同名竞争变化可使未改消费者重判；预算溢出的剩余工作可持久恢复。G2证据由当前冻结目标重跑而非只拼历史报告。

V06/V07及本阶段V13/V18适用范围通过；V19是固定题库无退化而非600题公开多仓/holdout认证，V20是依赖/catalog/1k与5k机制验证而非整个规模矩阵，V21是本地schema/文档及既有回归而非发布包认证。Unknown保留未知，未闭合阶段明确不认证，不将这些状态计入完整正确性。

P3-A继续不可变ProjectModel、共享目录与配置发现、配置digest/继承DAG、最近TS配置/JSONC与paths/baseUrl/条件；不重造索引/依赖引擎。P5继续无答案和全查询快照；P8继续公开质量、100k、RSS/尾延迟/跨平台。无commit/push/PR/merge/模型费用，工作区全部源码、测试与证据保留。

收口面板保留开发期失败及原脚本路径错误，聚合显示mixed/unknown；当前判断依据不可变final命令、源码摘要及独立回放，不修改历史失败来使面板变绿。hygiene的27项为26个有意保留的未跟踪测试文件/目录和已有dirty工作区，无冲突或secret-like路径。详见批次 `closeout.json`。
